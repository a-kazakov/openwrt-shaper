package model

import "time"

// DeviceMode represents the shaping mode of a device.
type DeviceMode int

const (
	ModeBurst     DeviceMode = iota // Tokens available → fast (ceiling from bucket size)
	ModeSustained                    // Tokens depleted → fair share 80/20
	ModeTurbo                        // Manual override → truly uncapped, time-limited
)

func (m DeviceMode) String() string {
	switch m {
	case ModeBurst:
		return "burst"
	case ModeSustained:
		return "sustained"
	case ModeTurbo:
		return "turbo"
	default:
		return "unknown"
	}
}

// TurboState tracks per-device turbo mode.
type TurboState struct {
	Active    bool      `json:"active"`
	ExpiresAt time.Time `json:"expires_at,omitempty"`
	StartedAt time.Time `json:"started_at,omitempty"`
	BytesUsed int64     `json:"bytes_used"`
}

// Device represents a discovered LAN device.
type Device struct {
	MAC      string `json:"mac"`
	IP       string `json:"ip"`
	Hostname string `json:"hostname"`
	Source   string `json:"source"` // "arp", "dhcp", "static"
}

// DeviceState holds full runtime state for a device.
type DeviceState struct {
	Device
	Slot           int        `json:"slot"`
	Mark           int        `json:"mark"`
	Mode           DeviceMode `json:"mode"`
	BucketTokens   int64      `json:"bucket_bytes"`
	BucketCapacity int64      `json:"bucket_capacity"`
	BurstCeilKbit  int        `json:"burst_ceil_kbit"`
	FairShareKbit  int        `json:"fair_share_kbit"`
	RateDownBps    int64      `json:"rate_down_bps"`
	RateUpBps      int64      `json:"rate_up_bps"`
	SessionBytes   int64      `json:"session_bytes"`
	SessionUp      int64      `json:"session_up"`
	SessionDown    int64      `json:"session_down"`
	CycleBytes     int64      `json:"cycle_bytes"`
	Turbo          TurboState `json:"turbo"`
	PrevCounterUp   int64     `json:"-"`
	PrevCounterDown int64     `json:"-"`
	DeltaUp        int64      `json:"-"`
	DeltaDown      int64      `json:"-"`
	ShapedDownKbit *int       `json:"shaped_down_kbit,omitempty"`
	ShapedUpKbit   *int       `json:"shaped_up_kbit,omitempty"`
}

// QuotaState holds the quota tracking state.
type QuotaState struct {
	Used         int64  `json:"used"`
	Remaining    int64  `json:"remaining"`
	Total        int64  `json:"total"`
	UsedUpload   int64  `json:"used_upload"`
	UsedDownload int64  `json:"used_download"`
	BillingMonth string `json:"billing_month"`
	Pct          int    `json:"pct"`
}

// CurveState holds current curve parameters for the UI.
type CurveState struct {
	RateKbit    int     `json:"rate_kbit"`
	Shape       float64 `json:"shape"`
	DownUpRatio float64 `json:"down_up_ratio"`
}

// ThroughputSample is a single throughput measurement.
type ThroughputSample struct {
	Timestamp int64 `json:"ts"`
	DownBps   int64 `json:"down_bps"`
	UpBps     int64 `json:"up_bps"`
}

// DishStatus holds Starlink dish gRPC status.
type DishStatus struct {
	Connected          bool    `json:"connected"`
	Uptime             int64   `json:"uptime"`
	DownlinkBps        float64 `json:"downlink_bps"`
	UplinkBps          float64 `json:"uplink_bps"`
	PopPingLatencyMs   float64 `json:"pop_ping_latency_ms"`
	SignalQuality      float64 `json:"signal_quality"`
	Obstructed         bool    `json:"obstructed"`
	FractionObstructed float64 `json:"fraction_obstructed"`
	SoftwareVersion    string  `json:"software_version"`
	Reachable          bool    `json:"reachable"`
	UsageDown          int64   `json:"usage_down"`
	UsageUp            int64   `json:"usage_up"`
}

// StateSnapshot is the full state pushed over WebSocket.
type StateSnapshot struct {
	Timestamp  int64              `json:"ts"`
	Quota      QuotaState         `json:"quota"`
	Curve      CurveState         `json:"curve"`
	Devices    []DeviceSnapshot   `json:"devices"`
	Throughput ThroughputState    `json:"throughput"`
	Dish       *DishStatus        `json:"dish,omitempty"`
}

// DeviceSnapshot is the per-device data in the state snapshot.
type DeviceSnapshot struct {
	MAC            string  `json:"mac"`
	IP             string  `json:"ip"`
	Hostname       string  `json:"hostname"`
	Mode           string  `json:"mode"`
	BucketBytes    int64   `json:"bucket_bytes"`
	BucketCapacity int64   `json:"bucket_capacity"`
	BucketPct      int     `json:"bucket_pct"`
	BurstCeilKbit  int     `json:"burst_ceil_kbit"`
	RateDownBps    int64   `json:"rate_down_bps"`
	RateUpBps      int64   `json:"rate_up_bps"`
	SessionBytes   int64   `json:"session_bytes"`
	SessionUp      int64   `json:"session_up"`
	SessionDown    int64   `json:"session_down"`
	CycleBytes     int64   `json:"cycle_bytes"`
	Turbo          bool    `json:"turbo"`
	TurboExpires   *int64  `json:"turbo_expires"`
	TurboBytes     int64   `json:"turbo_bytes"`
	ShapedDownKbit *int    `json:"shaped_down_kbit"`
	ShapedUpKbit   *int    `json:"shaped_up_kbit"`
}

// ThroughputState holds aggregate throughput data.
type ThroughputState struct {
	CurrentDownBps int64              `json:"current_down_bps"`
	CurrentUpBps   int64              `json:"current_up_bps"`
	Samples1m      []ThroughputSample `json:"samples_1m"`
}

// SyncRequest is the request body for POST /api/v1/sync.
type SyncRequest struct {
	StarlinkUsedGB float64 `json:"starlink_used_gb"`
	Source         string  `json:"source"`
}

// QuotaAdjustRequest is the request body for POST /api/v1/quota/adjust.
type QuotaAdjustRequest struct {
	DeltaBytes *int64 `json:"delta_bytes,omitempty"`
	SetBytes   *int64 `json:"set_bytes,omitempty"`
}

// TurboRequest is the request body for POST /api/v1/device/{mac}/turbo.
type TurboRequest struct {
	DurationMin int `json:"duration_min"`
}

// BucketSetRequest is the request body for POST /api/v1/device/{mac}/bucket.
type BucketSetRequest struct {
	TokensMB int64 `json:"tokens_mb"`
}
