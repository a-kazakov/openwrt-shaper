/** Matches Rust model.rs StateSnapshot */
export interface StateSnapshot {
  ts: number;
  quota: QuotaState;
  curve: CurveState;
  devices: DeviceSnapshot[];
  throughput: ThroughputState;
  dish?: DishStatus;
}

export interface QuotaState {
  used: number;
  remaining: number;
  total: number;
  used_upload: number;
  used_download: number;
  billing_month: string;
  pct: number;
}

export interface CurveState {
  rate_kbit: number;
  shape: number;
  down_up_ratio: number;
}

export interface DeviceSnapshot {
  mac: string;
  ip: string;
  hostname: string;
  mode: string;
  bucket_bytes: number;
  bucket_capacity: number;
  bucket_pct: number;
  burst_ceil_kbit: number;
  rate_down_bps: number;
  rate_up_bps: number;
  session_bytes: number;
  session_up: number;
  session_down: number;
  cycle_bytes: number;
  turbo: boolean;
  turbo_expires: number | null;
  turbo_bytes: number;
  bucket_refill_bps: number;
  shaped_down_kbit: number | null;
  shaped_up_kbit: number | null;
  bucket_shape_at: number;
  bucket_unshape_at: number;
}

export interface ThroughputSample {
  ts: number;
  down_bps: number;
  up_bps: number;
}

export interface ThroughputState {
  current_down_bps: number;
  current_up_bps: number;
  samples_1m: ThroughputSample[];
}

export interface DishStatus {
  connected: boolean;
  uptime: number;
  downlink_bps: number;
  uplink_bps: number;
  pop_ping_latency_ms: number;
  signal_quality: number;
  obstructed: boolean;
  fraction_obstructed: number;
  software_version: string;
  reachable: boolean;
  usage_down: number;
  usage_up: number;
}

/** Matches Rust config.rs Values */
export interface ConfigValues {
  network_mode: string;
  wan_iface: string;
  lan_iface: string;
  ifb_iface: string;
  dish_addr: string;
  dish_poll_interval_sec: number;
  listen_addr: string;
  billing_reset_day: number;
  monthly_quota_gb: number;
  curve_shape: number;
  max_rate_kbit: number;
  min_rate_kbit: number;
  down_up_ratio: number;
  bucket_duration_sec: number;
  burst_drain_ratio: number;
  tick_interval_sec: number;
  save_interval_sec: number;
  device_scan_interval_sec: number;
  overage_cost_per_gb: number;
  plan_cost_monthly: number;
}
