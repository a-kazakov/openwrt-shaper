package engine

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"

	"github.com/akazakov/openwrt-shaper/internal/config"
	"github.com/akazakov/openwrt-shaper/internal/dish"
	"github.com/akazakov/openwrt-shaper/internal/model"
	"github.com/akazakov/openwrt-shaper/internal/netctl"
	"github.com/akazakov/openwrt-shaper/internal/store"
)

// Engine is the main SLQM orchestrator.
type Engine struct {
	mu sync.RWMutex

	cfg   *config.Config
	store *store.Store
	tc    *netctl.TCController
	nft   *netctl.NFTController
	dish  *dish.Client

	curve   CurveParams
	billing BillingCycle

	// Quota state
	monthUsed    int64
	usedUpload   int64
	usedDownload int64
	billingMonth string

	// Device state
	devices   map[string]*DeviceState // key: MAC
	slotAlloc int                     // next available slot

	// Throughput tracking
	throughputSamples []model.ThroughputSample
	lastTickDown      int64
	lastTickUp        int64

	// Snapshot cache
	lastSnapshot *model.StateSnapshot
}

// DeviceState holds per-device runtime state within the engine.
type DeviceState struct {
	model.Device
	Slot           int
	Mark           int
	Bucket         *DeviceBucket
	Turbo          model.TurboState
	FairShareKbit  int
	PrevCounterUp   int64
	PrevCounterDown int64
	DeltaUp        int64
	DeltaDown      int64
	SessionUp      int64
	SessionDown    int64
	CycleBytes     int64
	LastMode       DeviceMode
	LastBurstCeil  int
}

// New creates a new Engine.
func New(cfg *config.Config, st *store.Store, dishClient *dish.Client) *Engine {
	snap := cfg.Snapshot()
	e := &Engine{
		cfg:   cfg,
		store: st,
		dish:  dishClient,
		curve: CurveParams{
			MaxRateKbit: snap.MaxRateKbit,
			MinRateKbit: snap.MinRateKbit,
			Shape:       snap.CurveShape,
			TotalBytes:  cfg.MonthlyQuotaBytes(),
		},
		billing: BillingCycle{ResetDay: snap.BillingResetDay},
		devices: make(map[string]*DeviceState),
		tc:      netctl.NewTCController(snap.WANIface, snap.LANIface, snap.MinRateKbit),
		nft:     netctl.NewNFTController(snap.WANIface),
	}

	// Load persisted quota state
	monthUsed, usedUp, usedDown, billingMonth, err := st.LoadQuota()
	if err != nil {
		log.Printf("engine: load quota: %v", err)
	}

	now := time.Now()
	currentMonth := e.billing.CurrentMonth(now)
	if billingMonth != currentMonth {
		log.Printf("engine: billing cycle rolled over from %s to %s, resetting", billingMonth, currentMonth)
		monthUsed = 0
		usedUp = 0
		usedDown = 0
	}

	e.monthUsed = monthUsed
	e.usedUpload = usedUp
	e.usedDownload = usedDown
	e.billingMonth = currentMonth

	return e
}

// Setup initializes nftables and tc trees on WAN (upload) and LAN (download).
// No IFB device is used — download shaping happens on the LAN interface egress
// where nftables marks are already set by the forward hook.
func (e *Engine) Setup() error {
	snap := e.cfg.Snapshot()

	log.Printf("engine: wan=%s lan=%s", snap.WANIface, snap.LANIface)

	// Clean up any leftover IFB from previous versions
	netctl.TeardownIFB(snap.WANIface, snap.IFBIface)

	// Setup nftables
	if err := e.nft.Setup(); err != nil {
		return fmt.Errorf("setup nftables: %w", err)
	}
	log.Printf("engine: nftables table inet slqm created")

	// Compute initial curve rate
	remaining := e.curve.TotalBytes - e.monthUsed
	rateKbit := e.curve.Rate(remaining)

	// Setup HTB trees: WAN egress for upload, LAN egress for download
	if err := e.tc.SetupHTB(rateKbit); err != nil {
		return fmt.Errorf("setup htb: %w", err)
	}
	log.Printf("engine: HTB trees created on %s (upload) and %s (download)", snap.WANIface, snap.LANIface)

	log.Printf("engine: setup complete, curve rate=%d kbit/s, used=%d bytes", rateKbit, e.monthUsed)
	return nil
}

// Run starts the main engine loop.
func (e *Engine) Run(ctx context.Context) error {
	snap := e.cfg.Snapshot()

	tickInterval := time.Duration(snap.TickIntervalSec) * time.Second
	saveInterval := time.Duration(snap.SaveIntervalSec) * time.Second
	scanInterval := time.Duration(snap.DeviceScanIntervalSec) * time.Second

	ticker := time.NewTicker(tickInterval)
	defer ticker.Stop()

	saveTicker := time.NewTicker(saveInterval)
	defer saveTicker.Stop()

	scanTicker := time.NewTicker(scanInterval)
	defer scanTicker.Stop()

	// Initial device scan
	e.scanDevices()

	for {
		select {
		case <-ctx.Done():
			e.shutdown()
			return nil
		case <-ticker.C:
			e.tick()
		case <-saveTicker.C:
			e.persist()
		case <-scanTicker.C:
			e.scanDevices()
		}
	}
}

func (e *Engine) tick() {
	e.mu.Lock()
	defer e.mu.Unlock()

	snap := e.cfg.Snapshot()
	now := time.Now()

	// Check billing cycle
	if e.billing.ShouldReset(e.billingMonth, now) {
		log.Printf("engine: billing cycle reset")
		e.monthUsed = 0
		e.usedUpload = 0
		e.usedDownload = 0
		e.billingMonth = e.billing.CurrentMonth(now)
		for _, dev := range e.devices {
			dev.CycleBytes = 0
		}
	}

	// Update curve params from config
	e.curve.MaxRateKbit = snap.MaxRateKbit
	e.curve.MinRateKbit = snap.MinRateKbit
	e.curve.Shape = snap.CurveShape
	e.curve.TotalBytes = int64(snap.MonthlyQuotaGB) * 1073741824

	// Compute curve rate
	remaining := e.curve.TotalBytes - e.monthUsed
	if remaining < 0 {
		remaining = 0
	}
	curveRateKbit := e.curve.Rate(remaining)
	curveRateBps := int64(curveRateKbit) * 1000 / 8

	// Update root class rate on both HTB trees
	e.tc.UpdateRootRate(curveRateKbit)

	// Read all nftables counters
	counters, err := e.nft.ReadAllCounters()
	if err != nil {
		log.Printf("engine: read counters: %v", err)
	}

	// Count active devices (non-full buckets) for fair share
	activeDevices := 0
	for _, dev := range e.devices {
		if !dev.Bucket.IsFull() || dev.Turbo.Active {
			activeDevices++
		}
	}
	if activeDevices == 0 {
		activeDevices = 1
	}

	var tickDownTotal, tickUpTotal int64

	// Process each device
	for _, dev := range e.devices {
		// Update bucket capacity and thresholds
		dev.Bucket.Update(curveRateBps, snap.BucketDurationSec, snap.TickIntervalSec, snap.BurstDrainRatio)

		// Read counters
		if counters != nil {
			if c, ok := counters[dev.Mark]; ok {
				newUp := c[0]
				newDown := c[1]

				// Compute deltas (handle counter reset)
				dev.DeltaUp = newUp - dev.PrevCounterUp
				if dev.DeltaUp < 0 {
					dev.DeltaUp = 0
				}
				dev.DeltaDown = newDown - dev.PrevCounterDown
				if dev.DeltaDown < 0 {
					dev.DeltaDown = 0
				}

				dev.PrevCounterUp = newUp
				dev.PrevCounterDown = newDown
			}
		}

		// Combined delta
		delta := dev.DeltaUp + dev.DeltaDown
		tickUpTotal += dev.DeltaUp
		tickDownTotal += dev.DeltaDown

		// Drain bucket
		dev.Bucket.Drain(delta)

		// Update session and cycle bytes
		dev.SessionUp += dev.DeltaUp
		dev.SessionDown += dev.DeltaDown
		dev.CycleBytes += delta

		// Update quota
		e.monthUsed += delta
		e.usedUpload += dev.DeltaUp
		e.usedDownload += dev.DeltaDown

		// Handle turbo
		if dev.Turbo.Active {
			if now.After(dev.Turbo.ExpiresAt) {
				dev.Turbo.Active = false
				dev.Bucket.SetMode(ModeBurst) // Will be re-evaluated by hysteresis
				log.Printf("engine: turbo expired for %s", dev.Hostname)
			} else {
				dev.Turbo.BytesUsed += delta
			}
		}
	}

	// Compute refill
	nonFullCount := 0
	for _, dev := range e.devices {
		if !dev.Bucket.IsFull() {
			nonFullCount++
		}
	}
	if nonFullCount > 0 {
		refillPerDevice := curveRateBps * int64(snap.TickIntervalSec) / int64(nonFullCount)
		for _, dev := range e.devices {
			if !dev.Bucket.IsFull() {
				dev.Bucket.Refill(refillPerDevice)
			}
		}
	}

	// Apply device modes and update tc
	fairShareKbit := curveRateKbit / activeDevices
	if fairShareKbit < snap.MinRateKbit {
		fairShareKbit = snap.MinRateKbit
	}

	for _, dev := range e.devices {
		dev.FairShareKbit = fairShareKbit
		mode := dev.Bucket.Mode()
		burstCeil := dev.Bucket.BurstCeilKbit()

		if dev.Turbo.Active {
			if dev.LastMode != ModeTurbo {
				e.tc.SetDeviceMode(dev.Slot, "turbo", fairShareKbit, 0, snap.DownUpRatio)
				dev.LastMode = ModeTurbo
			}
			continue
		}

		// Only update tc if mode or burst ceil changed meaningfully
		modeChanged := mode != dev.LastMode
		ceilChanged := dev.LastBurstCeil > 0 && abs(burstCeil-dev.LastBurstCeil)*100/dev.LastBurstCeil > 5

		if modeChanged || ceilChanged {
			e.tc.SetDeviceMode(dev.Slot, mode.String(), fairShareKbit, burstCeil, snap.DownUpRatio)
			dev.LastMode = mode
			dev.LastBurstCeil = burstCeil
		}
	}

	// Track throughput
	e.lastTickDown = tickDownTotal
	e.lastTickUp = tickUpTotal
	sample := model.ThroughputSample{
		Timestamp: now.Unix(),
		DownBps:   tickDownTotal * 8 / int64(snap.TickIntervalSec),
		UpBps:     tickUpTotal * 8 / int64(snap.TickIntervalSec),
	}
	e.throughputSamples = append(e.throughputSamples, sample)
	// Keep last 5 minutes of samples
	maxSamples := 300 / snap.TickIntervalSec
	if len(e.throughputSamples) > maxSamples {
		e.throughputSamples = e.throughputSamples[len(e.throughputSamples)-maxSamples:]
	}

	// Update snapshot cache
	e.updateSnapshot()
}

func (e *Engine) scanDevices() {
	e.mu.Lock()
	defer e.mu.Unlock()

	snap := e.cfg.Snapshot()
	staticDevs := make([]struct{ MAC, Name string }, len(snap.StaticDevices))
	for i, sd := range snap.StaticDevices {
		staticDevs[i] = struct{ MAC, Name string }{sd.MAC, sd.Name}
	}

	discovered, err := netctl.DiscoverDevices(snap.LANIface, staticDevs)
	if err != nil {
		log.Printf("engine: discover devices: %v", err)
		return
	}

	// Track which MACs are still present
	seen := make(map[string]bool)
	curveRateBps := e.curve.RateBytesPerSec(e.curve.TotalBytes - e.monthUsed)

	for _, d := range discovered {
		seen[d.MAC] = true
		if _, exists := e.devices[d.MAC]; !exists {
			// New device
			slot := e.slotAlloc
			e.slotAlloc++
			mark := 100 + slot

			bucket := NewDeviceBucket(curveRateBps, snap.BucketDurationSec)

			dev := &DeviceState{
				Device:   d,
				Slot:     slot,
				Mark:     mark,
				Bucket:   bucket,
				LastMode: ModeBurst,
			}

			// Load persisted cycle bytes
			if cb, err := e.store.LoadDeviceCycleBytes(d.MAC); err == nil {
				dev.CycleBytes = cb
			}

			// Add tc classes and nftables rules
			fairShare := e.curve.Rate(e.curve.TotalBytes-e.monthUsed) / max(len(e.devices)+1, 1)
			if fairShare < snap.MinRateKbit {
				fairShare = snap.MinRateKbit
			}
			if err := e.tc.AddDeviceClass(slot, fairShare, bucket.BurstCeilKbit()); err != nil {
				log.Printf("engine: add tc class for %s: %v", d.MAC, err)
				continue
			}
			if err := e.nft.AddDevice(d.IP, mark); err != nil {
				log.Printf("engine: add nft rules for %s: %v", d.MAC, err)
				continue
			}

			e.devices[d.MAC] = dev
			log.Printf("engine: new device %s (%s) slot=%d", d.Hostname, d.IP, slot)
		} else {
			// Update existing device info
			existing := e.devices[d.MAC]
			if d.IP != existing.IP {
				// IP changed — update nftables rules
				e.nft.RemoveDevice(existing.IP)
				e.nft.AddDevice(d.IP, existing.Mark)
				existing.PrevCounterUp = 0
				existing.PrevCounterDown = 0
			}
			existing.Device = d
		}
	}

	// Remove departed devices
	for mac, dev := range e.devices {
		if !seen[mac] {
			e.tc.RemoveDeviceClass(dev.Slot)
			e.nft.RemoveDevice(dev.IP)
			e.store.SaveDeviceCycleBytes(mac, dev.CycleBytes)
			delete(e.devices, mac)
			log.Printf("engine: removed device %s (%s)", dev.Hostname, dev.IP)
		}
	}
}

func (e *Engine) persist() {
	e.mu.RLock()
	defer e.mu.RUnlock()

	if err := e.store.SaveQuota(e.monthUsed, e.usedUpload, e.usedDownload, e.billingMonth); err != nil {
		log.Printf("engine: persist quota: %v", err)
	}
	for mac, dev := range e.devices {
		if err := e.store.SaveDeviceCycleBytes(mac, dev.CycleBytes); err != nil {
			log.Printf("engine: persist device %s: %v", mac, err)
		}
	}
}

func (e *Engine) shutdown() {
	log.Println("engine: shutting down")
	e.persist()

	snap := e.cfg.Snapshot()
	log.Println("engine: tearing down tc qdiscs")
	e.tc.Teardown()
	log.Println("engine: tearing down nftables")
	e.nft.Teardown()
	// Clean up IFB in case of leftover from previous version
	netctl.TeardownIFB(snap.WANIface, snap.IFBIface)
	log.Println("engine: cleanup complete")
}

func (e *Engine) updateSnapshot() {
	snap := e.cfg.Snapshot()
	remaining := e.curve.TotalBytes - e.monthUsed
	if remaining < 0 {
		remaining = 0
	}

	pct := 0
	if e.curve.TotalBytes > 0 {
		pct = int(e.monthUsed * 100 / e.curve.TotalBytes)
	}

	devices := make([]model.DeviceSnapshot, 0, len(e.devices))
	for _, dev := range e.devices {
		bucketCap := dev.Bucket.Capacity()
		bucketTokens := dev.Bucket.Tokens()
		bucketPct := 0
		if bucketCap > 0 {
			bucketPct = int(bucketTokens * 100 / bucketCap)
		}

		ds := model.DeviceSnapshot{
			MAC:            dev.MAC,
			IP:             dev.IP,
			Hostname:       dev.Hostname,
			Mode:           dev.Bucket.Mode().String(),
			BucketBytes:    bucketTokens,
			BucketCapacity: bucketCap,
			BucketPct:      bucketPct,
			BurstCeilKbit:  dev.Bucket.BurstCeilKbit(),
			RateDownBps:    dev.DeltaDown * 8 / int64(snap.TickIntervalSec),
			RateUpBps:      dev.DeltaUp * 8 / int64(snap.TickIntervalSec),
			SessionBytes:   dev.SessionUp + dev.SessionDown,
			SessionUp:      dev.SessionUp,
			SessionDown:    dev.SessionDown,
			CycleBytes:     dev.CycleBytes,
			Turbo:          dev.Turbo.Active,
			TurboBytes:     dev.Turbo.BytesUsed,
		}
		if dev.Turbo.Active {
			ds.Mode = "turbo"
			expires := dev.Turbo.ExpiresAt.Unix()
			ds.TurboExpires = &expires
		}
		devices = append(devices, ds)
	}

	curveRate := e.curve.Rate(remaining)
	e.lastSnapshot = &model.StateSnapshot{
		Timestamp: time.Now().Unix(),
		Quota: model.QuotaState{
			Used:         e.monthUsed,
			Remaining:    remaining,
			Total:        e.curve.TotalBytes,
			UsedUpload:   e.usedUpload,
			UsedDownload: e.usedDownload,
			BillingMonth: e.billingMonth,
			Pct:          pct,
		},
		Curve: model.CurveState{
			RateKbit:    curveRate,
			Shape:       e.curve.Shape,
			DownUpRatio: snap.DownUpRatio,
		},
		Devices: devices,
		Throughput: model.ThroughputState{
			CurrentDownBps: e.lastTickDown * 8 / int64(snap.TickIntervalSec),
			CurrentUpBps:   e.lastTickUp * 8 / int64(snap.TickIntervalSec),
			Samples1m:      e.recentSamples(60),
		},
	}

	if e.dish != nil {
		e.lastSnapshot.Dish = e.dish.Status()
	}
}

func (e *Engine) recentSamples(seconds int) []model.ThroughputSample {
	snap := e.cfg.Snapshot()
	count := seconds / snap.TickIntervalSec
	if count > len(e.throughputSamples) {
		count = len(e.throughputSamples)
	}
	if count == 0 {
		return nil
	}
	return e.throughputSamples[len(e.throughputSamples)-count:]
}

// --- Public API methods (called from api package) ---

// Snapshot returns the current state snapshot.
func (e *Engine) Snapshot() interface{} {
	e.mu.RLock()
	defer e.mu.RUnlock()
	if e.lastSnapshot == nil {
		e.updateSnapshot()
	}
	return e.lastSnapshot
}

// MonthUsed returns the current month's usage in bytes.
func (e *Engine) MonthUsed() int64 {
	e.mu.RLock()
	defer e.mu.RUnlock()
	return e.monthUsed
}

// AdjustQuota adds delta bytes to the monthly usage counter.
func (e *Engine) AdjustQuota(delta int64) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.monthUsed += delta
	if e.monthUsed < 0 {
		e.monthUsed = 0
	}
}

// SetQuota sets the monthly usage counter to an absolute value.
func (e *Engine) SetQuota(total int64) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.monthUsed = total
	if e.monthUsed < 0 {
		e.monthUsed = 0
	}
}

// ResetBillingCycle resets usage to zero and starts a new billing month.
func (e *Engine) ResetBillingCycle() {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.monthUsed = 0
	e.usedUpload = 0
	e.usedDownload = 0
	e.billingMonth = e.billing.CurrentMonth(time.Now())
	for _, dev := range e.devices {
		dev.CycleBytes = 0
		dev.SessionUp = 0
		dev.SessionDown = 0
	}
	e.store.ClearDevices()
}

// SetDeviceTurbo enables turbo mode for a device.
func (e *Engine) SetDeviceTurbo(mac string, duration time.Duration) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	dev, ok := e.devices[mac]
	if !ok {
		return fmt.Errorf("device %s not found", mac)
	}
	now := time.Now()
	dev.Turbo = model.TurboState{
		Active:    true,
		StartedAt: now,
		ExpiresAt: now.Add(duration),
		BytesUsed: 0,
	}
	dev.Bucket.SetMode(ModeTurbo)
	return nil
}

// CancelDeviceTurbo cancels turbo mode for a device.
func (e *Engine) CancelDeviceTurbo(mac string) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	dev, ok := e.devices[mac]
	if !ok {
		return fmt.Errorf("device %s not found", mac)
	}
	dev.Turbo.Active = false
	dev.Bucket.SetMode(ModeBurst) // Will be re-evaluated by hysteresis
	return nil
}

// SetDeviceBucket sets a device's bucket tokens.
func (e *Engine) SetDeviceBucket(mac string, tokensMB int64) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	dev, ok := e.devices[mac]
	if !ok {
		return fmt.Errorf("device %s not found", mac)
	}
	dev.Bucket.SetTokens(tokensMB * 1048576)
	return nil
}

// Config returns the current configuration.
func (e *Engine) Config() interface{} {
	return e.cfg.Snapshot()
}

// UpdateConfig applies a partial config update.
func (e *Engine) UpdateConfig(data []byte) error {
	if err := e.cfg.Update(data); err != nil {
		return err
	}
	return e.cfg.Save()
}

func abs(x int) int {
	if x < 0 {
		return -x
	}
	return x
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
