pub mod billing;
pub mod bucket;
pub mod curve;

use crate::config::Config;
use crate::model::{
    CurveState, DeviceSnapshot, DishStatus, QuotaState, StateSnapshot, ThroughputSample,
    ThroughputState, TurboState,
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
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::watch;
use tracing::{error, info, warn};

/// Internal per-device state held by the engine.
struct DeviceState {
    mac: String,
    ip: String,
    hostname: String,
    #[allow(dead_code)]
    source: String,
    slot: i32,
    mark: i32,
    bucket: DeviceBucket,
    turbo: TurboState,
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
    throughput_samples: Vec<ThroughputSample>,
    last_tick_down: i64,
    last_tick_up: i64,

    // Snapshot cache
    last_snapshot: Option<StateSnapshot>,

    // Dish status (set externally)
    dish_status: Option<DishStatus>,
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
        curve.total_bytes = snap.monthly_quota_gb as i64 * 1_073_741_824;

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
                throughput_samples: Vec::new(),
                last_tick_down: 0,
                last_tick_up: 0,
                last_snapshot: None,
                dish_status: None,
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
        let snap = {
            let inner = self.inner.read().unwrap();
            inner.cfg.snapshot()
        };

        let tick_interval =
            tokio::time::Duration::from_secs(snap.tick_interval_sec as u64);
        let save_interval =
            tokio::time::Duration::from_secs(snap.save_interval_sec as u64);
        let scan_interval =
            tokio::time::Duration::from_secs(snap.device_scan_interval_sec as u64);

        let mut tick_timer = tokio::time::interval(tick_interval);
        let mut save_timer = tokio::time::interval(save_interval);
        let mut scan_timer = tokio::time::interval(scan_interval);

        // Initial device scan
        self.scan_devices();

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    self.shutdown();
                    return;
                }
                _ = tick_timer.tick() => {
                    self.tick();
                }
                _ = save_timer.tick() => {
                    self.persist();
                }
                _ = scan_timer.tick() => {
                    self.scan_devices();
                }
            }
        }
    }

    fn tick(&self) {
        let mut inner = self.inner.write().unwrap();
        let snap = inner.cfg.snapshot();
        let now = Utc::now();

        // Check billing cycle
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
        inner.curve.total_bytes = snap.monthly_quota_gb as i64 * 1_073_741_824;

        // Compute curve rate
        let mut remaining = inner.curve.total_bytes - inner.month_used;
        if remaining < 0 {
            remaining = 0;
        }
        let curve_rate_kbit = inner.curve.rate(remaining);
        let curve_rate_bps = curve_rate_kbit as i64 * 1000 / 8;

        // Check if any device is in turbo mode
        let has_turbo = inner.devices.values().any(|d| d.turbo.active);

        // Update root class rate on both HTB trees
        let _ = inner.tc.update_root_rate(curve_rate_kbit, has_turbo);

        // Read all nftables counters
        let counters_result = counters::read_all_counters(inner.nft.table_name());

        // Count active devices for fair share
        let mut active_devices = 0i32;
        for dev in inner.devices.values() {
            if !dev.bucket.is_full() || dev.turbo.active {
                active_devices += 1;
            }
        }
        if active_devices == 0 {
            active_devices = 1;
        }

        let mut tick_down_total: i64 = 0;
        let mut tick_up_total: i64 = 0;
        let mut quota_delta: i64 = 0;
        let mut upload_delta: i64 = 0;
        let mut download_delta: i64 = 0;

        // Process each device
        for dev in inner.devices.values_mut() {
            // Update bucket capacity and thresholds
            dev.bucket.update(
                curve_rate_bps,
                snap.bucket_duration_sec,
                snap.tick_interval_sec,
                snap.burst_drain_ratio,
            );

            // Read counters
            if let Ok(ref counter_map) = counters_result {
                if let Some(c) = counter_map.get(&dev.mark) {
                    let new_up = c[0];
                    let new_down = c[1];

                    // Compute deltas (handle counter reset)
                    dev.delta_up = new_up - dev.prev_counter_up;
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

            // Combined delta
            let delta = dev.delta_up + dev.delta_down;
            tick_up_total += dev.delta_up;
            tick_down_total += dev.delta_down;

            // Drain bucket
            dev.bucket.drain(delta);

            // Update session and cycle bytes
            dev.session_up += dev.delta_up;
            dev.session_down += dev.delta_down;
            dev.cycle_bytes += delta;

            // Accumulate quota updates (applied after loop)
            quota_delta += delta;
            upload_delta += dev.delta_up;
            download_delta += dev.delta_down;

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

        // Apply accumulated quota updates
        inner.month_used += quota_delta;
        inner.used_upload += upload_delta;
        inner.used_download += download_delta;

        // Compute refill
        let mut non_full_count = 0i64;
        for dev in inner.devices.values() {
            if !dev.bucket.is_full() {
                non_full_count += 1;
            }
        }
        if non_full_count > 0 {
            let refill_per_device =
                curve_rate_bps * snap.tick_interval_sec as i64 / non_full_count;
            for dev in inner.devices.values_mut() {
                if !dev.bucket.is_full() {
                    dev.bucket.refill(refill_per_device);
                }
            }
        }

        // Apply device modes and update tc
        let mut fair_share_kbit = curve_rate_kbit / active_devices;
        if fair_share_kbit < snap.min_rate_kbit {
            fair_share_kbit = snap.min_rate_kbit;
        }

        // Collect tc updates to avoid borrowing inner.tc while iterating inner.devices
        struct TcUpdate {
            slot: i32,
            mode: String,
            fair_share: i32,
            burst_ceil: i32,
        }
        let mut tc_updates: Vec<TcUpdate> = Vec::new();

        for dev in inner.devices.values_mut() {
            dev.fair_share_kbit = fair_share_kbit;
            let mode = dev.bucket.mode();
            let burst_ceil = dev.bucket.burst_ceil_kbit();

            if dev.turbo.active {
                if dev.last_mode != crate::model::DeviceMode::Turbo {
                    tc_updates.push(TcUpdate {
                        slot: dev.slot,
                        mode: "turbo".to_string(),
                        fair_share: fair_share_kbit,
                        burst_ceil: 0,
                    });
                    dev.last_mode = crate::model::DeviceMode::Turbo;
                }
                continue;
            }

            // Only update tc if mode or burst ceil changed meaningfully
            let mode_changed = mode != dev.last_mode;
            let ceil_changed = dev.last_burst_ceil > 0
                && (burst_ceil - dev.last_burst_ceil).unsigned_abs() * 100
                    / dev.last_burst_ceil.unsigned_abs().max(1)
                    > 5;

            if mode_changed || ceil_changed {
                tc_updates.push(TcUpdate {
                    slot: dev.slot,
                    mode: mode.to_string(),
                    fair_share: fair_share_kbit,
                    burst_ceil,
                });
                dev.last_mode = mode;
                dev.last_burst_ceil = burst_ceil;
            }
        }

        // Apply tc updates
        for update in tc_updates {
            inner.tc.set_device_mode(
                update.slot,
                &update.mode,
                update.fair_share,
                update.burst_ceil,
                snap.down_up_ratio,
            );
        }

        // Track throughput
        inner.last_tick_down = tick_down_total;
        inner.last_tick_up = tick_up_total;
        let sample = ThroughputSample {
            ts: now.timestamp(),
            down_bps: tick_down_total * 8 / snap.tick_interval_sec as i64,
            up_bps: tick_up_total * 8 / snap.tick_interval_sec as i64,
        };
        inner.throughput_samples.push(sample);
        // Keep last 5 minutes of samples
        let max_samples = 300 / snap.tick_interval_sec as usize;
        if inner.throughput_samples.len() > max_samples {
            let start = inner.throughput_samples.len() - max_samples;
            inner.throughput_samples = inner.throughput_samples[start..].to_vec();
        }

        // Update snapshot cache and broadcast
        let snapshot = Self::build_snapshot(&inner);
        inner.last_snapshot = Some(snapshot.clone());
        let _ = self.snapshot_tx.send(Some(snapshot));
    }

    fn scan_devices(&self) {
        // Check for WAN interface change
        if let Ok(current_wan) = devices::detect_wan_iface() {
            let mut inner = self.inner.write().unwrap();
            let snap = inner.cfg.snapshot();
            if current_wan != snap.wan_iface {
                info!(
                    "engine: WAN interface changed from {} to {}, rebuilding",
                    snap.wan_iface, current_wan
                );

                // Teardown old tc + nft
                inner.tc.teardown();
                inner.nft.teardown();

                // Update config with new WAN
                inner.cfg.resolve_ifaces(&current_wan, "");

                // Create new controllers
                inner.tc = TCController::new(&current_wan, &snap.lan_iface, snap.min_rate_kbit);
                inner.nft = NFTController::new(&current_wan);

                // Re-setup nft + tc
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

                // Collect device info for re-adding rules
                let dev_info: Vec<(String, i32, i32, i32)> = inner
                    .devices
                    .values()
                    .map(|d| (d.ip.clone(), d.mark, d.slot, d.bucket.burst_ceil_kbit()))
                    .collect();

                // Re-add all device rules
                for (ip, mark, slot, burst_ceil) in &dev_info {
                    let _ = inner.nft.add_device(ip, *mark);
                    let _ = inner.tc.add_device_class(*slot, snap.min_rate_kbit, *burst_ceil);
                }

                // Reset counters
                for dev in inner.devices.values_mut() {
                    dev.prev_counter_up = 0;
                    dev.prev_counter_down = 0;
                    dev.last_mode = crate::model::DeviceMode::Burst;
                    dev.last_burst_ceil = 0;
                }

                info!("engine: WAN switchover to {} complete", current_wan);
            }
            drop(inner);
        }

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
                // New device
                let slot = inner.slot_alloc;
                inner.slot_alloc += 1;
                let mark = 100 + slot;

                let bucket = DeviceBucket::new(curve_rate_bps, snap.bucket_duration_sec);

                let mut dev = DeviceState {
                    mac: d.mac.clone(),
                    ip: d.ip.clone(),
                    hostname: d.hostname.clone(),
                    source: d.source.clone(),
                    slot,
                    mark,
                    bucket,
                    turbo: TurboState::default(),
                    fair_share_kbit: 0,
                    prev_counter_up: 0,
                    prev_counter_down: 0,
                    delta_up: 0,
                    delta_down: 0,
                    session_up: 0,
                    session_down: 0,
                    cycle_bytes: 0,
                    last_mode: crate::model::DeviceMode::Sustained,
                    last_burst_ceil: 0,
                };

                // Load persisted cycle bytes
                if let Ok(cb) = inner.store.load_device_cycle_bytes(&d.mac) {
                    dev.cycle_bytes = cb;
                }

                // Add tc classes and nftables rules
                let device_count = (inner.devices.len() + 1).max(1) as i32;
                let mut fair_share = inner.curve.rate(remaining) / device_count;
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

        // Compute per-device refill rate
        let curve_rate_bps = inner.curve.rate_bytes_per_sec(remaining);
        let non_full_count = inner
            .devices
            .values()
            .filter(|d| !d.bucket.is_full())
            .count()
            .max(1) as i64;
        let refill_bps_per_device = curve_rate_bps * 8 / non_full_count;

        let mut devices = Vec::with_capacity(inner.devices.len());
        for dev in inner.devices.values() {
            let bucket_cap = dev.bucket.capacity();
            let bucket_tokens = dev.bucket.tokens();
            let bucket_pct = if bucket_cap > 0 {
                (bucket_tokens * 100 / bucket_cap) as i32
            } else {
                0
            };

            // Refill rate: if this device's bucket is full, it gets 0
            let bucket_refill_bps = if dev.bucket.is_full() {
                0
            } else {
                refill_bps_per_device
            };

            let mut ds = DeviceSnapshot {
                mac: dev.mac.clone(),
                ip: dev.ip.clone(),
                hostname: dev.hostname.clone(),
                mode: dev.bucket.mode().to_string(),
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
            };
            if dev.turbo.active {
                ds.mode = "turbo".to_string();
                if let Some(expires) = dev.turbo.expires_at {
                    ds.turbo_expires = Some(expires.timestamp());
                }
            }
            devices.push(ds);
        }

        let curve_rate = inner.curve.rate(remaining);
        let count = 60 / snap.tick_interval_sec as usize;
        let samples_start = if inner.throughput_samples.len() > count {
            inner.throughput_samples.len() - count
        } else {
            0
        };

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
                samples_1m: inner.throughput_samples[samples_start..].to_vec(),
            },
            dish: inner.dish_status.clone(),
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

    /// Enable turbo mode for a device.
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

    /// Set a device's bucket tokens.
    pub fn set_device_bucket(&self, mac: &str, tokens_mb: i64) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        let dev = inner
            .devices
            .get_mut(mac)
            .ok_or_else(|| format!("device {mac} not found"))?;

        dev.bucket.set_tokens(tokens_mb * 1_048_576);
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
}
