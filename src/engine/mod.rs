pub mod billing;
pub mod bucket;
pub mod curve;

use crate::config::Config;
use crate::model::{
    CurveState, DeviceSnapshot, DishStatus, QuotaState, StateSnapshot, ThroughputSample,
    ThroughputState, TurboState, Warning,
};
use crate::netctl::counters;
use crate::netctl::devices::{self, StaticDeviceEntry};
use crate::netctl::nftables::NFTController;
use crate::netctl::tc::TCController;
use crate::store::Store;

use billing::BillingCycle;
use bucket::DeviceBucket;
use curve::CurveParams;

use chrono::Utc;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use tokio::sync::watch;
use tracing::{error, info, warn};

const THROUGHPUT_WINDOW_SEC: i32 = 30;
const THROUGHPUT_HISTORY_SAMPLES: usize = 120;
const DEVICE_MARK_BASE: i32 = 100;

/// Internal per-device state held by the engine.
struct DeviceState {
    mac: String,
    ip: String,
    hostname: String,
    source: String,
    slot: i32,
    mark: i32,
    bucket: DeviceBucket,
    turbo: TurboState,
    /// User-set mode override: Throttled or Disabled. None = automatic (bucket-driven).
    override_mode: Option<crate::model::DeviceMode>,
    fair_share_kbit: i32,
    prev_counter_up: i64,
    prev_counter_down: i64,
    delta_up: i64,
    delta_down: i64,
    session_up: i64,
    session_down: i64,
    cycle_bytes: i64,
    last_mode: crate::model::DeviceMode,
    last_burst_ceil: i32,
    last_fair_share: i32,
}

/// Shared engine state behind Arc<RwLock<>>.
struct EngineInner {
    cfg: Config,
    store: Arc<Store>,
    tc: TCController,
    nft: NFTController,

    curve: CurveParams,
    billing: BillingCycle,

    // Quota state
    month_used: i64,
    used_upload: i64,
    used_download: i64,
    billing_month: String,

    // Device state
    devices: HashMap<String, DeviceState>,
    slot_alloc: i32,

    // Throughput tracking
    throughput_samples: VecDeque<ThroughputSample>,
    last_tick_down: i64,
    last_tick_up: i64,
    // 30-second accumulator for coarse samples
    sample_accum_down: i64,
    sample_accum_up: i64,
    sample_accum_ticks: i32,

    // Snapshot cache
    last_snapshot: Option<StateSnapshot>,

    // WAN interface sysfs counters for quota tracking (includes router traffic)
    prev_wan_rx: i64,
    prev_wan_tx: i64,

    // Dish status (set externally)
    dish_status: Option<DishStatus>,

    // Persistent warnings
    interface_warnings: Vec<Warning>,
}

/// Thread-safe engine handle.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<RwLock<EngineInner>>,
    snapshot_tx: watch::Sender<Option<StateSnapshot>>,
    snapshot_rx: watch::Receiver<Option<StateSnapshot>>,
}

impl Engine {
    /// Create a new Engine.
    pub fn new(cfg: Config, store: Arc<Store>) -> Self {
        let snap = cfg.snapshot();

        let mut curve = CurveParams {
            max_rate_kbit: snap.max_rate_kbit,
            min_rate_kbit: snap.min_rate_kbit,
            shape: snap.curve_shape,
            total_bytes: cfg.monthly_quota_bytes(),
        };

        let billing = BillingCycle {
            reset_day: snap.billing_reset_day as u32,
        };

        let tc = TCController::new(&snap.wan_iface, &snap.lan_iface, snap.min_rate_kbit);
        let nft = NFTController::new(&snap.wan_iface);

        // Load persisted quota state
        let (mut month_used, mut used_up, mut used_down, billing_month) =
            match store.load_quota() {
                Ok(q) => q,
                Err(e) => {
                    warn!("engine: load quota: {e}");
                    (0, 0, 0, String::new())
                }
            };

        let now = Utc::now();
        let current_month = billing.current_month(now);
        if billing_month != current_month {
            info!(
                "engine: billing cycle rolled over from {} to {}, resetting",
                billing_month, current_month
            );
            month_used = 0;
            used_up = 0;
            used_down = 0;
        }

        // Update curve total in case config changed
        curve.total_bytes = snap.monthly_quota_gb as i64 * 1_000_000_000;

        let (snapshot_tx, snapshot_rx) = watch::channel(None);

        Self {
            inner: Arc::new(RwLock::new(EngineInner {
                cfg,
                store,
                tc,
                nft,
                curve,
                billing,
                month_used,
                used_upload: used_up,
                used_download: used_down,
                billing_month: current_month,
                devices: HashMap::new(),
                slot_alloc: 0,
                throughput_samples: VecDeque::new(),
                last_tick_down: 0,
                last_tick_up: 0,
                sample_accum_down: 0,
                sample_accum_up: 0,
                sample_accum_ticks: 0,
                last_snapshot: None,
                prev_wan_rx: 0,
                prev_wan_tx: 0,
                dish_status: None,
                interface_warnings: Vec::new(),
            })),
            snapshot_tx,
            snapshot_rx,
        }
    }

    /// Initialize nftables and tc trees.
    pub fn setup(&self) -> Result<(), String> {
        let inner = self.inner.write().unwrap();
        let snap = inner.cfg.snapshot();

        info!("engine: wan={} lan={}", snap.wan_iface, snap.lan_iface);

        // Setup nftables
        inner.nft.setup()?;
        info!("engine: nftables table inet slqm created");

        // Compute initial curve rate
        let remaining = inner.curve.total_bytes - inner.month_used;
        let rate_kbit = inner.curve.rate(remaining);

        // Setup HTB trees
        inner.tc.setup_htb(rate_kbit)?;
        info!(
            "engine: HTB trees created on {} (upload) and {} (download)",
            snap.wan_iface, snap.lan_iface
        );

        info!(
            "engine: setup complete, curve rate={} kbit/s, used={} bytes",
            rate_kbit, inner.month_used
        );
        Ok(())
    }

    /// Run the engine loop with tick, save, and device scan intervals.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        // Initial device scan
        self.scan_devices();

        let mut last_save = tokio::time::Instant::now();
        let mut last_scan = tokio::time::Instant::now();

        loop {
            // Re-read intervals from config each iteration (hot-reloadable)
            let snap = {
                let inner = self.inner.read().unwrap();
                inner.cfg.snapshot()
            };
            let tick_dur =
                tokio::time::Duration::from_millis(snap.tick_interval_sec as u64 * 1000);
            let save_dur =
                tokio::time::Duration::from_secs(snap.save_interval_sec as u64);
            let scan_dur =
                tokio::time::Duration::from_secs(snap.device_scan_interval_sec as u64);

            tokio::select! {
                _ = shutdown.changed() => {
                    self.shutdown();
                    return;
                }
                _ = tokio::time::sleep(tick_dur) => {
                    self.tick();
                    if last_save.elapsed() >= save_dur {
                        self.persist();
                        last_save = tokio::time::Instant::now();
                    }
                    if last_scan.elapsed() >= scan_dur {
                        self.scan_devices();
                        last_scan = tokio::time::Instant::now();
                    }
                }
            }
        }
    }

    /// Engine tick: four-phase update ensuring state machine consistency.
    /// Order matters: drain → hysteresis → tc update → refill.
    /// Draining before hysteresis ensures mode transitions reflect this tick's usage.
    /// Updating tc before refill prevents a device from bursting with freshly-added tokens.
    fn tick(&self) {
        let mut inner = self.inner.write().unwrap();
        let snap = inner.cfg.snapshot();
        let now = Utc::now();

        if inner.billing.should_reset(&inner.billing_month, now) {
            info!("engine: billing cycle reset");
            inner.month_used = 0;
            inner.used_upload = 0;
            inner.used_download = 0;
            inner.billing_month = inner.billing.current_month(now);
            for dev in inner.devices.values_mut() {
                dev.cycle_bytes = 0;
            }
        }

        // Update curve params from config
        inner.curve.max_rate_kbit = snap.max_rate_kbit;
        inner.curve.min_rate_kbit = snap.min_rate_kbit;
        inner.curve.shape = snap.curve_shape;
        inner.curve.total_bytes = snap.monthly_quota_gb as i64 * 1_000_000_000;

        // Compute curve rate
        let mut remaining = inner.curve.total_bytes - inner.month_used;
        if remaining < 0 {
            remaining = 0;
        }
        let curve_rate_kbit = inner.curve.rate(remaining);
        let curve_rate_bps = curve_rate_kbit as i64 * 1000 / 8;

        // Parent classes are always uncapped; per-device classes handle shaping
        let _ = inner.tc.update_root_rate(curve_rate_kbit, false);

        // Read all nftables counters
        let counters_result = counters::read_all_counters(inner.nft.table_name());

        // Count active devices for fair share (exclude disabled devices)
        let mut active_devices = 0i32;
        for dev in inner.devices.values() {
            if dev.override_mode == Some(crate::model::DeviceMode::Disabled) {
                continue;
            }
            if !dev.bucket.is_full() || dev.turbo.active
                || dev.override_mode == Some(crate::model::DeviceMode::Throttled)
            {
                active_devices += 1;
            }
        }
        if active_devices == 0 {
            active_devices = 1; // prevent division-by-zero in fair-share calculation
        }

        // Read WAN interface counters for quota tracking.
        // WAN rx = download (from internet), tx = upload (to internet).
        // This captures ALL traffic including router-originated (DNS, NTP,
        // firmware updates) that per-device nft counters miss.
        let wan_iface = inner.tc.wan_iface().to_string();
        let wan_rx = read_iface_counter(&wan_iface, "rx_bytes");
        let wan_tx = read_iface_counter(&wan_iface, "tx_bytes");

        let mut wan_download_delta = wan_rx - inner.prev_wan_rx;
        let mut wan_upload_delta = wan_tx - inner.prev_wan_tx;
        // Negative delta means counter wrapped or interface was reset
        if wan_download_delta < 0 {
            wan_download_delta = 0;
        }
        if wan_upload_delta < 0 {
            wan_upload_delta = 0;
        }
        // Skip the first tick (prev was 0 → delta would be the entire lifetime counter)
        let wan_initialized = inner.prev_wan_rx > 0 || inner.prev_wan_tx > 0;
        inner.prev_wan_rx = wan_rx;
        inner.prev_wan_tx = wan_tx;

        let mut tick_down_total: i64 = 0;
        let mut tick_up_total: i64 = 0;

        // Phase 1: Read per-device nft counters and drain buckets.
        // Per-device counters are still used for bucket drain and device stats.
        for dev in inner.devices.values_mut() {
            if let Ok(ref counter_map) = counters_result {
                if let Some(c) = counter_map.get(&dev.mark) {
                    let new_up = c.upload;
                    let new_down = c.download;

                    dev.delta_up = new_up - dev.prev_counter_up;
                    // Negative delta means nft counters were reset (e.g., table recreated).
                    if dev.delta_up < 0 {
                        dev.delta_up = 0;
                    }
                    dev.delta_down = new_down - dev.prev_counter_down;
                    if dev.delta_down < 0 {
                        dev.delta_down = 0;
                    }

                    dev.prev_counter_up = new_up;
                    dev.prev_counter_down = new_down;
                }
            }

            let delta = dev.delta_up + dev.delta_down;
            tick_up_total += dev.delta_up;
            tick_down_total += dev.delta_down;

            // Disabled devices don't drain their bucket (they're shaped to near-zero)
            if dev.override_mode != Some(crate::model::DeviceMode::Disabled) {
                dev.bucket.drain(delta);
            }

            dev.session_up += dev.delta_up;
            dev.session_down += dev.delta_down;
            dev.cycle_bytes += delta;

            // Handle turbo expiration
            if dev.turbo.active {
                if let Some(expires) = dev.turbo.expires_at {
                    if now > expires {
                        dev.turbo.active = false;
                        dev.bucket.set_mode(crate::model::DeviceMode::Burst);
                        info!("engine: turbo expired for {}", dev.hostname);
                    } else {
                        dev.turbo.bytes_used += delta;
                    }
                }
            }
        }

        // Quota is tracked from WAN interface counters (includes router traffic).
        // Per-device nft counters are only used for bucket drain and device stats.
        if wan_initialized {
            inner.month_used += wan_download_delta + wan_upload_delta;
            inner.used_upload += wan_upload_delta;
            inner.used_download += wan_download_delta;
        }

        // Update bucket capacity and thresholds for ALL devices (including overridden),
        // but only evaluate mode transitions for non-overridden devices.
        for dev in inner.devices.values_mut() {
            dev.bucket.update_params(
                curve_rate_bps,
                snap.bucket_duration_sec,
                snap.tick_interval_sec,
                snap.max_burst_kbit,
            );
            if dev.override_mode.is_none() {
                dev.bucket.evaluate_mode();
            }
        }

        // Apply mode changes to tc before refill to prevent burst with fresh tokens
        let mut fair_share_kbit = curve_rate_kbit / active_devices;
        if fair_share_kbit < snap.min_rate_kbit {
            fair_share_kbit = snap.min_rate_kbit;
        }

        // Collect tc updates to avoid borrowing inner.tc while iterating inner.devices
        struct TcUpdate {
            slot: i32,
            mode: crate::model::DeviceMode,
            fair_share: i32,
            burst_ceil: i32,
        }
        let mut tc_updates: Vec<TcUpdate> = Vec::new();

        for dev in inner.devices.values_mut() {
            dev.fair_share_kbit = fair_share_kbit;
            let burst_ceil = dev.bucket.burst_ceil_kbit();

            // Determine effective mode: override > turbo > bucket hysteresis
            let effective_mode = if dev.turbo.active {
                crate::model::DeviceMode::Turbo
            } else if let Some(om) = dev.override_mode {
                om
            } else {
                dev.bucket.mode()
            };

            // Re-apply tc shaping when mode, burst ceil (>1%), or fair share changed.
            // Detects device connect/disconnect (fair_share changes) and external resets.
            let mode_changed = effective_mode != dev.last_mode;
            let ceil_changed = dev.last_burst_ceil > 0
                && (burst_ceil - dev.last_burst_ceil).unsigned_abs() * 100
                    / dev.last_burst_ceil.unsigned_abs().max(1)
                    > 1;
            let share_changed = fair_share_kbit != dev.last_fair_share;

            if mode_changed || ceil_changed || share_changed {
                tc_updates.push(TcUpdate {
                    slot: dev.slot,
                    mode: effective_mode,
                    fair_share: fair_share_kbit,
                    burst_ceil,
                });
                dev.last_mode = effective_mode;
                dev.last_burst_ceil = burst_ceil;
                dev.last_fair_share = fair_share_kbit;
            }
        }

        // Apply tc updates
        for update in tc_updates {
            inner.tc.set_device_mode(
                update.slot,
                update.mode,
                update.fair_share,
                update.burst_ceil,
                snap.down_up_ratio,
            );
        }

        // Water-filling refill: distribute budget fairly without exceeding curve rate.
        // Devices closest to full get served first so their overflow is redistributed
        // to hungrier devices, ensuring no tokens are wasted.
        // Throttled and disabled devices do not receive refill; their actual consumption
        // is subtracted from the budget so normal devices get the leftover.
        // Turbo devices are NOT subtracted — they participate in normal refill.
        let throttled_disabled_bytes: i64 = inner
            .devices
            .values()
            .filter(|dev| dev.override_mode.is_some() && !dev.turbo.active)
            .map(|dev| dev.delta_up + dev.delta_down)
            .sum();
        let total_budget =
            (curve_rate_bps * snap.tick_interval_sec as i64 - throttled_disabled_bytes).max(0);
        let mut spaces: Vec<(String, i64)> = inner
            .devices
            .iter()
            .filter(|(_, dev)| !dev.bucket.is_full() && dev.override_mode.is_none())
            .map(|(mac, dev)| (mac.clone(), dev.bucket.space_remaining()))
            .collect();

        let allocs = water_fill_refill(total_budget, &mut spaces);

        for (mac, alloc) in &allocs {
            if let Some(dev) = inner.devices.get_mut(mac) {
                dev.bucket.refill(*alloc);
            }
        }

        // Track throughput from WAN counters (includes router traffic)
        if wan_initialized {
            inner.last_tick_down = wan_download_delta;
            inner.last_tick_up = wan_upload_delta;
        } else {
            inner.last_tick_down = tick_down_total;
            inner.last_tick_up = tick_up_total;
        }

        // Accumulate into 30-second buckets
        inner.sample_accum_down += inner.last_tick_down;
        inner.sample_accum_up += inner.last_tick_up;
        inner.sample_accum_ticks += 1;
        let ticks_per_sample = THROUGHPUT_WINDOW_SEC / snap.tick_interval_sec;
        if inner.sample_accum_ticks >= ticks_per_sample {
            let window_sec = inner.sample_accum_ticks as i64 * snap.tick_interval_sec as i64;
            let sample = ThroughputSample {
                ts: now.timestamp(),
                down_bps: inner.sample_accum_down * 8 / window_sec,
                up_bps: inner.sample_accum_up * 8 / window_sec,
            };
            inner.throughput_samples.push_back(sample);
            inner.sample_accum_down = 0;
            inner.sample_accum_up = 0;
            inner.sample_accum_ticks = 0;
            if inner.throughput_samples.len() > THROUGHPUT_HISTORY_SAMPLES {
                inner.throughput_samples.pop_front();
            }
        }

        // Update snapshot cache and broadcast
        let snapshot = Self::build_snapshot(&inner);
        inner.last_snapshot = Some(snapshot.clone());
        let _ = self.snapshot_tx.send(Some(snapshot));
    }

    /// Compare desired interfaces (from config + auto-detection) against what
    /// tc/nft controllers are currently using. Rebuild if they differ.
    /// Also handles interfaces that appear/disappear dynamically (e.g. GL-Inet
    /// "sta" interface only exists when repeater is active).
    fn check_interface_change(&self) {
        let mut inner = self.inner.write().unwrap();
        let snap = inner.cfg.snapshot();

        // Determine desired WAN: re-detect if "auto" or if configured interface
        // doesn't exist (may have disappeared, e.g. wifi repeater disconnected)
        let wan_missing = !inner.cfg.is_wan_auto() && !iface_exists(&snap.wan_iface);
        let desired_wan = if inner.cfg.is_wan_auto() || wan_missing {
            devices::detect_wan_iface().unwrap_or_else(|_| snap.wan_iface.clone())
        } else {
            snap.wan_iface.clone()
        };

        // Determine desired LAN: same logic
        let lan_missing = !inner.cfg.is_lan_auto() && !iface_exists(&snap.lan_iface);
        let desired_lan = if inner.cfg.is_lan_auto() || lan_missing {
            devices::detect_lan_iface(&desired_wan)
                .unwrap_or_else(|_| snap.lan_iface.clone())
        } else {
            snap.lan_iface.clone()
        };

        // Update interface warnings
        inner
            .interface_warnings
            .retain(|w| !w.id.starts_with("iface_fallback"));
        if wan_missing {
            inner.interface_warnings.push(Warning {
                id: "iface_fallback_wan".to_string(),
                level: "warning".to_string(),
                message: format!(
                    "WAN interface '{}' does not exist, using auto-detected '{}'",
                    snap.wan_iface, desired_wan
                ),
            });
        }
        if lan_missing {
            inner.interface_warnings.push(Warning {
                id: "iface_fallback_lan".to_string(),
                level: "warning".to_string(),
                message: format!(
                    "LAN interface '{}' does not exist, using auto-detected '{}'",
                    snap.lan_iface, desired_lan
                ),
            });
        }

        let current_wan = inner.tc.wan_iface().to_string();
        let current_lan = inner.tc.lan_iface().to_string();

        if desired_wan == current_wan && desired_lan == current_lan {
            return;
        }

        info!(
            "engine: interface change detected (WAN: {} → {}, LAN: {} → {}), rebuilding",
            current_wan, desired_wan, current_lan, desired_lan
        );

        inner.tc.teardown();
        inner.nft.teardown();

        inner.cfg.resolve_ifaces(&desired_wan, &desired_lan);
        inner.tc = TCController::new(&desired_wan, &desired_lan, snap.min_rate_kbit);
        inner.nft = NFTController::new(&desired_wan);

        if let Err(e) = inner.nft.setup() {
            error!("engine: nft re-setup failed: {e}");
        }
        let mut remaining = inner.curve.total_bytes - inner.month_used;
        if remaining < 0 {
            remaining = 0;
        }
        let rate_kbit = inner.curve.rate(remaining);
        if let Err(e) = inner.tc.setup_htb(rate_kbit) {
            error!("engine: tc re-setup failed: {e}");
        }

        // Re-add all device rules on new interfaces
        let dev_info: Vec<(String, i32, i32, i32)> = inner
            .devices
            .values()
            .map(|d| (d.ip.clone(), d.mark, d.slot, d.bucket.burst_ceil_kbit()))
            .collect();

        for (ip, mark, slot, burst_ceil) in &dev_info {
            let _ = inner.nft.add_device(ip, *mark);
            let _ = inner.tc.add_device_class(*slot, snap.min_rate_kbit, *burst_ceil);
        }

        // Reset counters since old nft table was destroyed
        for dev in inner.devices.values_mut() {
            dev.prev_counter_up = 0;
            dev.prev_counter_down = 0;
            dev.last_mode = crate::model::DeviceMode::Burst;
            dev.last_burst_ceil = 0;
            dev.last_fair_share = 0;
        }

        // Reset WAN sysfs counters (new interface has different counters)
        inner.prev_wan_rx = 0;
        inner.prev_wan_tx = 0;

        info!(
            "engine: interface switchover complete (WAN: {}, LAN: {})",
            desired_wan, desired_lan
        );
    }

    fn scan_devices(&self) {
        // Check if interfaces need rebuilding (explicit config change or auto re-detection)
        self.check_interface_change();

        let mut inner = self.inner.write().unwrap();
        let snap = inner.cfg.snapshot();

        let static_devs: Vec<StaticDeviceEntry> = snap
            .static_devices
            .iter()
            .map(|sd| StaticDeviceEntry {
                mac: sd.mac.clone(),
                name: sd.name.clone(),
            })
            .collect();

        let discovered = match devices::discover_devices(&snap.lan_iface, &static_devs) {
            Ok(d) => d,
            Err(e) => {
                warn!("engine: discover devices: {e}");
                return;
            }
        };

        let mut remaining = inner.curve.total_bytes - inner.month_used;
        if remaining < 0 {
            remaining = 0;
        }
        let curve_rate_bps = inner.curve.rate_bytes_per_sec(remaining);

        let mut seen: HashMap<String, bool> = HashMap::new();

        for d in &discovered {
            seen.insert(d.mac.clone(), true);

            if !inner.devices.contains_key(&d.mac) {
                let slot = inner.slot_alloc;
                // Offset from DEVICE_MARK_BASE to avoid conflict with default nft/iptables marks
                let mark = DEVICE_MARK_BASE + slot;

                let mut bucket = DeviceBucket::new(curve_rate_bps, snap.bucket_duration_sec);
                // Compute burst ceiling before using it (otherwise it's 0)
                bucket.update(
                    curve_rate_bps,
                    snap.bucket_duration_sec,
                    snap.tick_interval_sec,
                    snap.max_burst_kbit,
                );

                let mut dev = DeviceState {
                    mac: d.mac.clone(),
                    ip: d.ip.clone(),
                    hostname: d.hostname.clone(),
                    source: d.source.clone(),
                    slot,
                    mark,
                    bucket,
                    turbo: TurboState::default(),
                    override_mode: None,
                    fair_share_kbit: 0,
                    prev_counter_up: 0,
                    prev_counter_down: 0,
                    delta_up: 0,
                    delta_down: 0,
                    session_up: 0,
                    session_down: 0,
                    cycle_bytes: 0,
                    // Set to Burst so the first tick detects a mode change
                    // and applies ratio-split rates via set_device_mode
                    last_mode: crate::model::DeviceMode::Burst,
                    last_burst_ceil: 0,
                    last_fair_share: 0,
                };

                // Load persisted cycle bytes
                if let Ok(cb) = inner.store.load_device_cycle_bytes(&d.mac) {
                    dev.cycle_bytes = cb;
                }

                // Ensure HTB trees exist (may have been reset by network restart)
                let rate_kbit = inner.curve.rate(remaining);
                if let Err(e) = inner.tc.ensure_htb(rate_kbit) {
                    error!("engine: re-setup HTB trees: {e}");
                }

                // Add tc classes and nftables rules
                let device_count = (inner.devices.len() + 1).max(1) as i32;
                let mut fair_share = rate_kbit / device_count;
                if fair_share < snap.min_rate_kbit {
                    fair_share = snap.min_rate_kbit;
                }
                if let Err(e) =
                    inner
                        .tc
                        .add_device_class(slot, fair_share, dev.bucket.burst_ceil_kbit())
                {
                    error!("engine: add tc class for {}: {e}", d.mac);
                    continue;
                }
                if let Err(e) = inner.nft.add_device(&d.ip, mark) {
                    error!("engine: add nft rules for {}: {e}", d.mac);
                    continue;
                }

                // Only allocate slot after successful setup
                inner.slot_alloc += 1;

                info!(
                    "engine: new device {} ({}) slot={}",
                    d.hostname, d.ip, slot
                );
                inner.devices.insert(d.mac.clone(), dev);
            } else {
                // Update existing device info — check if IP changed
                let existing = inner.devices.get(&d.mac).unwrap();
                let ip_changed = d.ip != existing.ip;
                let old_ip = existing.ip.clone();
                let mark = existing.mark;

                if ip_changed {
                    inner.nft.remove_device(&old_ip);
                    let _ = inner.nft.add_device(&d.ip, mark);
                }

                let existing = inner.devices.get_mut(&d.mac).unwrap();
                if ip_changed {
                    existing.prev_counter_up = 0;
                    existing.prev_counter_down = 0;
                }
                existing.ip = d.ip.clone();
                existing.hostname = d.hostname.clone();
                existing.source = d.source.clone();
            }
        }

        // Remove departed devices
        let departed: Vec<String> = inner
            .devices
            .keys()
            .filter(|mac| !seen.contains_key(*mac))
            .cloned()
            .collect();

        for mac in departed {
            if let Some(dev) = inner.devices.remove(&mac) {
                inner.tc.remove_device_class(dev.slot);
                inner.nft.remove_device(&dev.ip);
                let _ = inner.store.save_device_cycle_bytes(&mac, dev.cycle_bytes);
                info!("engine: removed device {} ({})", dev.hostname, dev.ip);
            }
        }
    }

    fn persist(&self) {
        let inner = self.inner.read().unwrap();

        if let Err(e) = inner.store.save_quota(
            inner.month_used,
            inner.used_upload,
            inner.used_download,
            &inner.billing_month,
        ) {
            error!("engine: persist quota: {e}");
        }
        for (mac, dev) in &inner.devices {
            if let Err(e) = inner.store.save_device_cycle_bytes(mac, dev.cycle_bytes) {
                error!("engine: persist device {mac}: {e}");
            }
        }
    }

    fn shutdown(&self) {
        info!("engine: shutting down");
        self.persist();

        let inner = self.inner.read().unwrap();
        info!("engine: tearing down tc qdiscs");
        inner.tc.teardown();
        info!("engine: tearing down nftables");
        inner.nft.teardown();
        info!("engine: cleanup complete");
    }

    fn build_snapshot(inner: &EngineInner) -> StateSnapshot {
        let snap = inner.cfg.snapshot();
        let mut remaining = inner.curve.total_bytes - inner.month_used;
        if remaining < 0 {
            remaining = 0;
        }

        let pct = if inner.curve.total_bytes > 0 {
            (inner.month_used * 100 / inner.curve.total_bytes) as i32
        } else {
            0
        };

        // Compute per-device refill rate (excludes overridden devices).
        // Subtract throttled/disabled devices' last-tick consumption from the budget.
        // Turbo devices are NOT subtracted — they participate in normal refill.
        let snap_cfg = inner.cfg.snapshot();
        let curve_rate_bps = inner.curve.rate_bytes_per_sec(remaining);
        let throttled_disabled_bps: i64 = inner
            .devices
            .values()
            .filter(|d| d.override_mode.is_some() && !d.turbo.active)
            .map(|d| (d.delta_up + d.delta_down) * 8 / snap_cfg.tick_interval_sec as i64)
            .sum();
        let effective_rate_bps = (curve_rate_bps * 8 - throttled_disabled_bps).max(0);
        let non_full_count = inner
            .devices
            .values()
            .filter(|d| !d.bucket.is_full() && d.override_mode.is_none())
            .count()
            .max(1) as i64;
        let refill_bps_per_device = effective_rate_bps / non_full_count;

        let mut devices = Vec::with_capacity(inner.devices.len());
        for dev in inner.devices.values() {
            let bucket_cap = dev.bucket.capacity();
            let bucket_tokens = dev.bucket.tokens();
            let bucket_pct = if bucket_cap > 0 {
                (bucket_tokens * 100 / bucket_cap) as i32
            } else {
                0
            };

            // Refill rate: overridden or full devices get 0
            let bucket_refill_bps = if dev.override_mode.is_some() || dev.bucket.is_full() {
                0
            } else {
                refill_bps_per_device
            };

            // Determine effective mode: override > turbo > bucket
            let effective_mode = if dev.turbo.active {
                crate::model::DeviceMode::Turbo
            } else if let Some(om) = dev.override_mode {
                om
            } else {
                dev.bucket.mode()
            };

            let mut ds = DeviceSnapshot {
                mac: dev.mac.clone(),
                ip: dev.ip.clone(),
                hostname: dev.hostname.clone(),
                mode: effective_mode,
                bucket_bytes: bucket_tokens,
                bucket_capacity: bucket_cap,
                bucket_pct,
                burst_ceil_kbit: dev.bucket.burst_ceil_kbit(),
                rate_down_bps: dev.delta_down * 8 / snap.tick_interval_sec as i64,
                rate_up_bps: dev.delta_up * 8 / snap.tick_interval_sec as i64,
                session_bytes: dev.session_up + dev.session_down,
                session_up: dev.session_up,
                session_down: dev.session_down,
                cycle_bytes: dev.cycle_bytes,
                turbo: dev.turbo.active,
                turbo_expires: None,
                turbo_bytes: dev.turbo.bytes_used,
                bucket_refill_bps,
                shaped_down_kbit: None,
                shaped_up_kbit: None,
                bucket_shape_at: dev.bucket.thresholds().0,
                bucket_unshape_at: dev.bucket.thresholds().1,
            };

            // Compute actual tc ceil values per effective mode
            let burst_ceil = dev.bucket.burst_ceil_kbit();
            let fair_share = dev.fair_share_kbit;
            if let Some(expires) = dev.turbo.expires_at {
                if dev.turbo.active {
                    ds.turbo_expires = Some(expires.timestamp());
                }
            }
            match effective_mode {
                crate::model::DeviceMode::Turbo => {
                    ds.shaped_down_kbit = None;
                    ds.shaped_up_kbit = None;
                }
                crate::model::DeviceMode::Burst => {
                    let down_ceil = (burst_ceil as f64 * snap.down_up_ratio) as i32;
                    let up_ceil = burst_ceil - down_ceil;
                    ds.shaped_down_kbit = Some(down_ceil.max(1));
                    ds.shaped_up_kbit = Some(up_ceil.max(1));
                }
                crate::model::DeviceMode::Sustained | crate::model::DeviceMode::Throttled => {
                    let down_ceil = (fair_share as f64 * snap.down_up_ratio) as i32;
                    let up_ceil = fair_share - down_ceil;
                    ds.shaped_down_kbit = Some(down_ceil.max(1));
                    ds.shaped_up_kbit = Some(up_ceil.max(1));
                }
                crate::model::DeviceMode::Disabled => {
                    ds.shaped_down_kbit = Some(1);
                    ds.shaped_up_kbit = Some(1);
                }
            }
            devices.push(ds);
        }

        // Build warnings list
        let mut warnings: Vec<Warning> = inner.interface_warnings.clone();

        // Dish unreachable
        if let Some(ref dish) = inner.dish_status {
            if !dish.reachable {
                warnings.push(Warning {
                    id: "dish_unreachable".to_string(),
                    level: "warning".to_string(),
                    message: "Starlink dish is unreachable. Quota tracking may be inaccurate if internet is not via Starlink.".to_string(),
                });
            }
        }

        let curve_rate = inner.curve.rate(remaining);
        StateSnapshot {
            ts: Utc::now().timestamp(),
            quota: QuotaState {
                used: inner.month_used,
                remaining,
                total: inner.curve.total_bytes,
                used_upload: inner.used_upload,
                used_download: inner.used_download,
                billing_month: inner.billing_month.clone(),
                pct,
            },
            curve: CurveState {
                rate_kbit: curve_rate,
                shape: inner.curve.shape,
                down_up_ratio: snap.down_up_ratio,
            },
            devices,
            throughput: ThroughputState {
                current_down_bps: inner.last_tick_down * 8 / snap.tick_interval_sec as i64,
                current_up_bps: inner.last_tick_up * 8 / snap.tick_interval_sec as i64,
                samples_1h: inner.throughput_samples.iter().cloned().collect(),
            },
            dish: inner.dish_status.clone(),
            warnings,
        }
    }

    // --- Public API methods ---

    /// Get the current state snapshot.
    pub fn snapshot(&self) -> StateSnapshot {
        let inner = self.inner.read().unwrap();
        match &inner.last_snapshot {
            Some(s) => s.clone(),
            None => Self::build_snapshot(&inner),
        }
    }

    /// Subscribe to snapshot updates (for WebSocket broadcast).
    pub fn subscribe(&self) -> watch::Receiver<Option<StateSnapshot>> {
        self.snapshot_rx.clone()
    }

    /// Get current month usage in bytes.
    pub fn month_used(&self) -> i64 {
        self.inner.read().unwrap().month_used
    }

    /// Add delta bytes to the monthly usage counter.
    pub fn adjust_quota(&self, delta: i64) {
        let mut inner = self.inner.write().unwrap();
        inner.month_used += delta;
        if inner.month_used < 0 {
            inner.month_used = 0;
        }
    }

    /// Set the monthly usage counter to an absolute value.
    pub fn set_quota(&self, total: i64) {
        let mut inner = self.inner.write().unwrap();
        inner.month_used = total;
        if inner.month_used < 0 {
            inner.month_used = 0;
        }
    }

    /// Reset usage to zero and start a new billing month.
    pub fn reset_billing_cycle(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.month_used = 0;
        inner.used_upload = 0;
        inner.used_download = 0;
        inner.billing_month = inner.billing.current_month(Utc::now());
        for dev in inner.devices.values_mut() {
            dev.cycle_bytes = 0;
            dev.session_up = 0;
            dev.session_down = 0;
        }
        let _ = inner.store.clear_devices();
    }

    /// Enable turbo mode for a device. Clears any override.
    pub fn set_device_turbo(
        &self,
        mac: &str,
        duration: std::time::Duration,
    ) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        let dev = inner
            .devices
            .get_mut(mac)
            .ok_or_else(|| format!("device {mac} not found"))?;

        let now = Utc::now();
        dev.override_mode = None;
        dev.turbo = TurboState {
            active: true,
            started_at: Some(now),
            expires_at: Some(now + chrono::Duration::from_std(duration).unwrap()),
            bytes_used: 0,
        };
        dev.bucket.set_mode(crate::model::DeviceMode::Turbo);
        Ok(())
    }

    /// Cancel turbo mode for a device.
    pub fn cancel_device_turbo(&self, mac: &str) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        let dev = inner
            .devices
            .get_mut(mac)
            .ok_or_else(|| format!("device {mac} not found"))?;

        dev.turbo.active = false;
        dev.bucket.set_mode(crate::model::DeviceMode::Burst);
        Ok(())
    }

    /// Set a device mode override. Normal clears the override and returns
    /// to automatic burst/sustained behavior.
    pub fn set_device_mode(
        &self,
        mac: &str,
        mode: crate::model::DeviceModeOverride,
    ) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        let dev = inner
            .devices
            .get_mut(mac)
            .ok_or_else(|| format!("device {mac} not found"))?;

        match mode {
            crate::model::DeviceModeOverride::Throttled => {
                dev.turbo.active = false;
                dev.override_mode = Some(crate::model::DeviceMode::Throttled);
                info!("engine: device {} set to throttled", dev.hostname);
            }
            crate::model::DeviceModeOverride::Disabled => {
                dev.turbo.active = false;
                dev.override_mode = Some(crate::model::DeviceMode::Disabled);
                info!("engine: device {} set to disabled", dev.hostname);
            }
            crate::model::DeviceModeOverride::Normal => {
                dev.turbo.active = false;
                dev.override_mode = None;
                dev.bucket.set_mode(crate::model::DeviceMode::Burst);
                info!("engine: device {} set to normal", dev.hostname);
            }
        }
        Ok(())
    }

    /// Set a device's bucket tokens.
    pub fn set_device_bucket(&self, mac: &str, tokens_mb: i64) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        let dev = inner
            .devices
            .get_mut(mac)
            .ok_or_else(|| format!("device {mac} not found"))?;

        dev.bucket.set_tokens(tokens_mb * 1_000_000);
        Ok(())
    }

    /// Get the current configuration as JSON.
    pub fn config_json(&self) -> serde_json::Value {
        let inner = self.inner.read().unwrap();
        inner.cfg.to_json()
    }

    /// Apply a partial config update and save.
    pub fn update_config(&self, data: &[u8]) -> Result<(), String> {
        let inner = self.inner.read().unwrap();
        inner.cfg.update(data)?;
        inner.cfg.save()
    }

    /// Set dish status (called from dish poller).
    pub fn set_dish_status(&self, status: Option<DishStatus>) {
        let mut inner = self.inner.write().unwrap();
        inner.dish_status = status;
    }

    /// Add an interface fallback warning (called at startup / interface change).
    pub fn add_interface_warning(&self, message: String) {
        let mut inner = self.inner.write().unwrap();
        // Replace existing interface warnings
        inner
            .interface_warnings
            .retain(|w| !w.id.starts_with("iface_fallback"));
        let idx = inner.interface_warnings.len();
        inner.interface_warnings.push(Warning {
            id: format!("iface_fallback_{idx}"),
            level: "warning".to_string(),
            message,
        });
    }

    /// Clear interface warnings (called when interfaces are successfully resolved).
    pub fn clear_interface_warnings(&self) {
        let mut inner = self.inner.write().unwrap();
        inner
            .interface_warnings
            .retain(|w| !w.id.starts_with("iface_fallback"));
    }

}

/// Read a sysfs counter for a network interface. Returns 0 on failure.
fn read_iface_counter(iface: &str, counter: &str) -> i64 {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{counter}"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Check if a network interface exists in sysfs.
fn iface_exists(name: &str) -> bool {
    std::path::Path::new(&format!("/sys/class/net/{name}")).exists()
}

/// Distribute `budget` bytes across devices using water-filling.
/// `spaces` is a list of (id, space_remaining) for each non-full bucket.
/// Returns a list of (id, allocation) pairs. Total allocation never exceeds budget.
/// Sorted ascending by space so nearly-full devices are filled first, with their
/// leftover budget redistributed to hungrier devices.
fn water_fill_refill(budget: i64, spaces: &mut [(String, i64)]) -> Vec<(String, i64)> {
    spaces.sort_by_key(|&(_, space)| space);
    let mut remaining = budget;
    let n = spaces.len();
    let mut result = Vec::with_capacity(n);

    for (i, (id, space)) in spaces.iter().enumerate() {
        if remaining <= 0 {
            break;
        }
        let devices_left = (n - i) as i64;
        let fair_share = remaining / devices_left;
        let alloc = fair_share.min(*space);
        if alloc > 0 {
            result.push((id.clone(), alloc));
            remaining -= alloc;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_fill_equal_space() {
        // 3 devices, each needing 100 bytes, budget = 300
        let mut spaces = vec![
            ("a".into(), 100),
            ("b".into(), 100),
            ("c".into(), 100),
        ];
        let result = water_fill_refill(300, &mut spaces);
        let total: i64 = result.iter().map(|(_, a)| a).sum();
        assert_eq!(total, 300);
        for (_, alloc) in &result {
            assert_eq!(*alloc, 100);
        }
    }

    #[test]
    fn water_fill_budget_less_than_space() {
        // 3 devices need 1000 each, budget = 300 → 100 each
        let mut spaces = vec![
            ("a".into(), 1000),
            ("b".into(), 1000),
            ("c".into(), 1000),
        ];
        let result = water_fill_refill(300, &mut spaces);
        let total: i64 = result.iter().map(|(_, a)| a).sum();
        assert_eq!(total, 300);
        for (_, alloc) in &result {
            assert_eq!(*alloc, 100);
        }
    }

    #[test]
    fn water_fill_overflow_redistribution() {
        // Device A needs only 10, B and C need 1000. Budget = 300.
        // A gets min(300/3=100, 10) = 10, remaining = 290
        // B gets min(290/2=145, 1000) = 145, remaining = 145
        // C gets min(145/1=145, 1000) = 145, remaining = 0
        let mut spaces = vec![
            ("a".into(), 10),
            ("b".into(), 1000),
            ("c".into(), 1000),
        ];
        let result = water_fill_refill(300, &mut spaces);
        let total: i64 = result.iter().map(|(_, a)| a).sum();
        assert_eq!(total, 300, "total must equal budget");

        let map: HashMap<String, i64> = result.into_iter().collect();
        assert_eq!(map["a"], 10);
        assert_eq!(map["b"], 145);
        assert_eq!(map["c"], 145);
    }

    #[test]
    fn water_fill_all_nearly_full() {
        // Two devices need only 5 bytes each. Budget = 1000.
        // A gets min(500, 5) = 5, remaining = 995
        // B gets min(995, 5) = 5, remaining = 990
        // Total = 10, not 1000 (excess budget is not forced)
        let mut spaces = vec![("a".into(), 5), ("b".into(), 5)];
        let result = water_fill_refill(1000, &mut spaces);
        let total: i64 = result.iter().map(|(_, a)| a).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn water_fill_zero_budget() {
        let mut spaces = vec![("a".into(), 100)];
        let result = water_fill_refill(0, &mut spaces);
        assert!(result.is_empty());
    }

    #[test]
    fn water_fill_no_devices() {
        let mut spaces: Vec<(String, i64)> = vec![];
        let result = water_fill_refill(1000, &mut spaces);
        assert!(result.is_empty());
    }

    #[test]
    fn water_fill_single_device() {
        let mut spaces = vec![("a".into(), 500)];
        // Budget exceeds space
        let result = water_fill_refill(1000, &mut spaces);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, 500);
    }

    #[test]
    fn water_fill_mixed_sizes() {
        // A needs 50, B needs 200, C needs 1000. Budget = 600.
        // Sorted: A(50), B(200), C(1000)
        // A: min(200, 50) = 50, remaining = 550
        // B: min(275, 200) = 200, remaining = 350
        // C: min(350, 1000) = 350, remaining = 0
        let mut spaces = vec![
            ("c".into(), 1000),
            ("a".into(), 50),
            ("b".into(), 200),
        ];
        let result = water_fill_refill(600, &mut spaces);
        let total: i64 = result.iter().map(|(_, a)| a).sum();
        assert_eq!(total, 600);

        let map: HashMap<String, i64> = result.into_iter().collect();
        assert_eq!(map["a"], 50);
        assert_eq!(map["b"], 200);
        assert_eq!(map["c"], 350);
    }

    /// Scenario: 100 kb/s curve rate, 5 devices, 4 always-throttled.
    /// Throttled devices consume 30 kb/s combined (below their fair share).
    /// The normal device should get refilled at 70 kb/s (100 - 30 consumed).
    #[test]
    fn refill_budget_subtracts_overridden_consumption() {
        // Curve rate = 100 kbit/s = 12500 bytes/sec.  Tick = 2s.
        let curve_rate_bps: i64 = 12500;
        let tick_sec: i64 = 2;
        let full_budget = curve_rate_bps * tick_sec; // 25000 bytes

        // 4 throttled devices consumed 30 kbit/s = 3750 bytes/sec combined → 7500 bytes/tick
        let overridden_bytes: i64 = 7500;

        // Effective budget for normal devices
        let total_budget = (full_budget - overridden_bytes).max(0);

        // Expected: 25000 - 7500 = 17500 bytes = 70 kbit/s * 2s
        assert_eq!(total_budget, 17500);

        // Normal device has plenty of space → gets all 17500
        let mut spaces = vec![("normal".into(), 1_000_000i64)];
        let allocs = water_fill_refill(total_budget, &mut spaces);
        assert_eq!(allocs.len(), 1);
        assert_eq!(allocs[0].1, 17500);

        // Verify: 17500 bytes / 2s * 8 = 70000 bps = 70 kbit/s ✓
        let refill_kbit = allocs[0].1 * 8 / tick_sec / 1000;
        assert_eq!(refill_kbit, 70);
    }

    /// When overridden devices consume MORE than the curve rate (unlikely but
    /// defensive), the budget clamps to 0 — normal devices get no refill.
    #[test]
    fn refill_budget_clamps_at_zero() {
        let curve_rate_bps: i64 = 12500;
        let tick_sec: i64 = 2;
        let full_budget = curve_rate_bps * tick_sec; // 25000

        // Overridden devices somehow consumed more than curve rate
        let overridden_bytes: i64 = 30000;
        let total_budget = (full_budget - overridden_bytes).max(0);
        assert_eq!(total_budget, 0);

        let mut spaces = vec![("normal".into(), 1_000_000i64)];
        let allocs = water_fill_refill(total_budget, &mut spaces);
        assert!(allocs.is_empty());
    }

    // =========================================================================
    //  Refill budget: throttled/disabled consumption subtracted, turbo is NOT
    // =========================================================================

    /// Helper: compute effective refill budget using the same formula as tick().
    /// `device_deltas` = list of (override_mode, turbo_active, bytes_consumed_this_tick).
    fn compute_refill_budget(
        curve_rate_bps: i64,
        tick_sec: i64,
        device_deltas: &[(Option<crate::model::DeviceMode>, bool, i64)],
    ) -> i64 {
        let throttled_disabled_bytes: i64 = device_deltas
            .iter()
            .filter(|(om, turbo, _)| om.is_some() && !turbo)
            .map(|(_, _, bytes)| bytes)
            .sum();
        (curve_rate_bps * tick_sec - throttled_disabled_bytes).max(0)
    }

    /// Turbo devices must NOT reduce the refill budget even though they consume
    /// significant bandwidth. Turbo is a user-granted privilege; penalizing
    /// other devices for it would be unfair.
    #[test]
    fn refill_budget_turbo_not_subtracted() {
        let curve_rate_bps: i64 = 12500; // 100 kbit/s
        let tick_sec: i64 = 2;

        // One turbo device consuming 80 kb/s = 20000 bytes/tick
        // One normal device
        let devices = vec![
            (None, true, 20000i64),  // turbo — must NOT reduce budget
            (None, false, 0i64),      // normal
        ];
        let budget = compute_refill_budget(curve_rate_bps, tick_sec, &devices);

        // Budget should be the full 25000 (turbo not subtracted)
        assert_eq!(budget, 25000, "turbo consumption must not reduce budget");
    }

    /// Throttled devices reduce the budget by their actual consumption.
    #[test]
    fn refill_budget_throttled_subtracted() {
        let curve_rate_bps: i64 = 12500;
        let tick_sec: i64 = 2;

        // Two throttled devices consuming 5000 bytes/tick each
        // One normal device
        let devices = vec![
            (Some(crate::model::DeviceMode::Throttled), false, 5000i64),
            (Some(crate::model::DeviceMode::Throttled), false, 5000i64),
            (None, false, 0i64),
        ];
        let budget = compute_refill_budget(curve_rate_bps, tick_sec, &devices);
        assert_eq!(budget, 25000 - 10000, "throttled consumption subtracted");
        assert_eq!(budget, 15000);
    }

    /// Disabled devices reduce the budget by their (tiny) actual consumption.
    #[test]
    fn refill_budget_disabled_subtracted() {
        let curve_rate_bps: i64 = 12500;
        let tick_sec: i64 = 2;

        // One disabled device consuming 10 bytes/tick (shaped to 1 kbit)
        let devices = vec![
            (Some(crate::model::DeviceMode::Disabled), false, 10i64),
            (None, false, 0i64),
        ];
        let budget = compute_refill_budget(curve_rate_bps, tick_sec, &devices);
        assert_eq!(budget, 25000 - 10);
    }

    /// Mixed scenario: turbo + throttled + disabled + normal.
    /// Only throttled and disabled consumption reduces the budget.
    #[test]
    fn refill_budget_mixed_modes() {
        let curve_rate_bps: i64 = 12500; // 100 kbit/s
        let tick_sec: i64 = 2;
        // full budget = 25000

        let devices = vec![
            (None, true, 15000i64),                                       // turbo: NOT subtracted
            (Some(crate::model::DeviceMode::Throttled), false, 4000i64),  // throttled: subtracted
            (Some(crate::model::DeviceMode::Disabled), false, 20i64),     // disabled: subtracted
            (None, false, 3000i64),                                       // normal: NOT subtracted
            (None, false, 1000i64),                                       // normal: NOT subtracted
        ];
        let budget = compute_refill_budget(curve_rate_bps, tick_sec, &devices);
        // Only throttled (4000) + disabled (20) = 4020 subtracted
        assert_eq!(budget, 25000 - 4020);
        assert_eq!(budget, 20980);
    }

    /// All devices throttled — budget goes to zero, no refill happens.
    #[test]
    fn refill_budget_all_throttled() {
        let curve_rate_bps: i64 = 12500;
        let tick_sec: i64 = 2;

        let devices = vec![
            (Some(crate::model::DeviceMode::Throttled), false, 8000i64),
            (Some(crate::model::DeviceMode::Throttled), false, 8000i64),
            (Some(crate::model::DeviceMode::Throttled), false, 9000i64),
        ];
        let budget = compute_refill_budget(curve_rate_bps, tick_sec, &devices);
        // 25000 - 25000 = 0
        assert_eq!(budget, 0);
    }

    /// No overridden devices — full curve rate goes to refill.
    #[test]
    fn refill_budget_no_overrides() {
        let curve_rate_bps: i64 = 12500;
        let tick_sec: i64 = 2;

        let devices = vec![
            (None, false, 5000i64),
            (None, false, 3000i64),
        ];
        let budget = compute_refill_budget(curve_rate_bps, tick_sec, &devices);
        assert_eq!(budget, 25000, "no overrides = full budget");
    }

    // =========================================================================
    //  Water-fill distribution with throttled budget reduction
    // =========================================================================

    /// End-to-end: 5 devices, 4 throttled at 30 kb/s combined, 1 normal.
    /// Normal device gets all 70 kb/s of remaining budget.
    #[test]
    fn water_fill_with_throttled_budget() {
        let curve_rate_bps: i64 = 12500; // 100 kbit/s
        let tick_sec: i64 = 2;

        // 4 throttled consume 7500 bytes/tick total (30 kbit/s)
        let throttled_bytes: i64 = 7500;
        let budget = (curve_rate_bps * tick_sec - throttled_bytes).max(0);

        // Only the normal device participates in refill
        let mut spaces = vec![("normal".into(), 1_000_000i64)];
        let allocs = water_fill_refill(budget, &mut spaces);

        assert_eq!(allocs.len(), 1);
        assert_eq!(allocs[0].1, 17500); // 70 kbit/s * 2s / 8 = 17500 bytes
    }

    /// Two normal devices sharing the leftover from one throttled device.
    /// Budget split fairly via water-fill.
    #[test]
    fn water_fill_two_normal_one_throttled() {
        let curve_rate_bps: i64 = 12500; // 100 kbit/s
        let tick_sec: i64 = 2;

        // 1 throttled consumes 5000 bytes/tick (20 kbit/s)
        let budget = (curve_rate_bps * tick_sec - 5000).max(0);
        assert_eq!(budget, 20000);

        // Two normal devices, both hungry
        let mut spaces = vec![
            ("dev_a".into(), 500_000i64),
            ("dev_b".into(), 500_000i64),
        ];
        let allocs = water_fill_refill(budget, &mut spaces);
        let total: i64 = allocs.iter().map(|(_, a)| a).sum();
        assert_eq!(total, 20000, "full budget distributed");

        let map: HashMap<String, i64> = allocs.into_iter().collect();
        assert_eq!(map["dev_a"], 10000, "equal split");
        assert_eq!(map["dev_b"], 10000, "equal split");
    }

    // =========================================================================
    //  Fair share and tc update detection
    // =========================================================================

    /// fair_share changes when a device connects. Simulates the logic:
    /// before = curve_rate / 2 devices, after = curve_rate / 3 devices.
    /// The change should be detected (share_changed = true).
    #[test]
    fn fair_share_changes_on_device_connect() {
        let curve_rate_kbit = 50000;
        let min_rate_kbit = 1000;

        let share_before = {
            let active = 2;
            let s = curve_rate_kbit / active;
            s.max(min_rate_kbit)
        };
        let share_after = {
            let active = 3;
            let s = curve_rate_kbit / active;
            s.max(min_rate_kbit)
        };

        assert_ne!(share_before, share_after, "fair share must change");
        assert_eq!(share_before, 25000);
        assert_eq!(share_after, 16666);

        // Simulates the tc update detection
        let last_fair_share = share_before;
        let share_changed = share_after != last_fair_share;
        assert!(share_changed, "tc update should be triggered");
    }

    /// fair_share change from 1→2 devices halves the share.
    /// From 2→1 devices doubles it. Both must trigger tc update.
    #[test]
    fn fair_share_changes_on_device_disconnect() {
        let curve_rate_kbit = 50000;

        let share_2dev = curve_rate_kbit / 2; // 25000
        let share_1dev = curve_rate_kbit / 1; // 50000

        assert_ne!(share_2dev, share_1dev);
        assert_eq!(share_1dev, 50000);
        assert_eq!(share_2dev, 25000);
    }

    /// Burst ceil within 1% tolerance does NOT trigger tc update.
    /// Burst ceil change >1% DOES trigger.
    /// Note: uses integer division, so 1500/100000*100 = 1 (truncated).
    #[test]
    fn ceil_change_threshold_one_percent() {
        let last_ceil: i32 = 100_000; // 100 Mbps

        // 0.5% change (500) → 500*100/100000 = 0 — should NOT trigger
        let new_ceil_small = 100_500;
        let pct_small = (new_ceil_small - last_ceil).unsigned_abs() * 100
            / last_ceil.unsigned_abs().max(1);
        assert_eq!(pct_small, 0);
        assert!(!(pct_small > 1), "0.5% change should not trigger");

        // 2% change (2000) → 2000*100/100000 = 2 — should trigger
        let new_ceil_big = 102_000;
        let pct_big = (new_ceil_big - last_ceil).unsigned_abs() * 100
            / last_ceil.unsigned_abs().max(1);
        assert_eq!(pct_big, 2);
        assert!(pct_big > 1, "2% change should trigger");

        // Exact 1% (1000) → 1000*100/100000 = 1 — should NOT trigger (> 1, not >=)
        let new_ceil_exact = 101_000;
        let pct_exact = (new_ceil_exact - last_ceil).unsigned_abs() * 100
            / last_ceil.unsigned_abs().max(1);
        assert_eq!(pct_exact, 1);
        assert!(!(pct_exact > 1), "exactly 1% should not trigger");
    }

    /// When last_burst_ceil is 0 (new device), ceil_changed is always false,
    /// but mode_changed is true (Burst != initial), so tc update still fires.
    #[test]
    fn new_device_always_gets_tc_update() {
        let last_burst_ceil: i32 = 0;
        let burst_ceil: i32 = 300_000;

        // ceil_changed formula: last > 0 && delta% > 1
        let ceil_changed = last_burst_ceil > 0
            && (burst_ceil - last_burst_ceil).unsigned_abs() * 100
                / last_burst_ceil.unsigned_abs().max(1)
                > 1;
        assert!(!ceil_changed, "ceil_changed false when last is 0");

        // But mode_changed is true (new device starts with last_mode=Burst,
        // bucket starts in Sustained → mismatch)
        let last_mode = crate::model::DeviceMode::Burst;
        let effective_mode = crate::model::DeviceMode::Sustained;
        let mode_changed = effective_mode != last_mode;
        assert!(mode_changed, "mode_changed true for new device");

        // At least one trigger fires
        assert!(mode_changed || ceil_changed);
    }

    // =========================================================================
    //  Effective mode priority: override > turbo > bucket hysteresis
    // =========================================================================

    /// Turbo takes priority over everything, even if override is set.
    /// (In practice turbo clears override, but test the priority logic.)
    #[test]
    fn effective_mode_turbo_wins() {
        let turbo_active = true;
        let override_mode = Some(crate::model::DeviceMode::Throttled);
        let bucket_mode = crate::model::DeviceMode::Burst;

        let effective = if turbo_active {
            crate::model::DeviceMode::Turbo
        } else if let Some(om) = override_mode {
            om
        } else {
            bucket_mode
        };
        assert_eq!(effective, crate::model::DeviceMode::Turbo);
    }

    /// Override takes priority over bucket hysteresis.
    #[test]
    fn effective_mode_override_over_bucket() {
        let turbo_active = false;
        let override_mode = Some(crate::model::DeviceMode::Disabled);
        let bucket_mode = crate::model::DeviceMode::Burst;

        let effective = if turbo_active {
            crate::model::DeviceMode::Turbo
        } else if let Some(om) = override_mode {
            om
        } else {
            bucket_mode
        };
        assert_eq!(effective, crate::model::DeviceMode::Disabled);
    }

    /// No turbo, no override — bucket mode is used.
    #[test]
    fn effective_mode_falls_through_to_bucket() {
        let turbo_active = false;
        let override_mode: Option<crate::model::DeviceMode> = None;
        let bucket_mode = crate::model::DeviceMode::Sustained;

        let effective = if turbo_active {
            crate::model::DeviceMode::Turbo
        } else if let Some(om) = override_mode {
            om
        } else {
            bucket_mode
        };
        assert_eq!(effective, crate::model::DeviceMode::Sustained);
    }

    // =========================================================================
    //  Active device counting
    // =========================================================================

    /// Helper: count active devices using the same logic as tick().
    fn count_active(
        devices: &[(Option<crate::model::DeviceMode>, bool, bool)], // (override, turbo, bucket_full)
    ) -> i32 {
        let mut active = 0i32;
        for &(override_mode, turbo, bucket_full) in devices {
            if override_mode == Some(crate::model::DeviceMode::Disabled) {
                continue;
            }
            if !bucket_full || turbo
                || override_mode == Some(crate::model::DeviceMode::Throttled)
            {
                active += 1;
            }
        }
        active.max(1)
    }

    /// Disabled devices are excluded from active count entirely.
    #[test]
    fn active_count_excludes_disabled() {
        let devices = vec![
            (Some(crate::model::DeviceMode::Disabled), false, false),
            (None, false, false),
        ];
        assert_eq!(count_active(&devices), 1);
    }

    /// Throttled devices count as active (they consume fair share bandwidth).
    #[test]
    fn active_count_includes_throttled() {
        let devices = vec![
            (Some(crate::model::DeviceMode::Throttled), false, true),
            (None, false, false),
        ];
        assert_eq!(count_active(&devices), 2);
    }

    /// Devices with full buckets and no override are NOT active
    /// (they don't need refill or special tc handling).
    #[test]
    fn active_count_excludes_full_normal() {
        let devices = vec![
            (None, false, true),  // full bucket, no override → inactive
            (None, false, false), // non-full → active
        ];
        assert_eq!(count_active(&devices), 1);
    }

    /// Turbo devices are active even with full buckets.
    #[test]
    fn active_count_includes_turbo_even_if_full() {
        let devices = vec![
            (None, true, true), // turbo + full → still active
        ];
        assert_eq!(count_active(&devices), 1);
    }

    /// Empty device list → clamped to 1 (prevents division by zero).
    #[test]
    fn active_count_minimum_one() {
        let devices: Vec<(Option<crate::model::DeviceMode>, bool, bool)> = vec![];
        assert_eq!(count_active(&devices), 1);
    }

    /// All disabled → clamped to 1.
    #[test]
    fn active_count_all_disabled_clamps() {
        let devices = vec![
            (Some(crate::model::DeviceMode::Disabled), false, false),
            (Some(crate::model::DeviceMode::Disabled), false, false),
        ];
        assert_eq!(count_active(&devices), 1);
    }

    /// Real scenario: 5 devices, 3 throttled, 1 disabled, 1 normal (non-full).
    #[test]
    fn active_count_mixed() {
        let devices = vec![
            (Some(crate::model::DeviceMode::Throttled), false, true),  // active
            (Some(crate::model::DeviceMode::Throttled), false, true),  // active
            (Some(crate::model::DeviceMode::Throttled), false, true),  // active
            (Some(crate::model::DeviceMode::Disabled), false, false),  // excluded
            (None, false, false),                                       // active
        ];
        assert_eq!(count_active(&devices), 4);
    }
}
