use serde::{Deserialize, Serialize};

/// Device shaping mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceMode {
    Burst,
    Sustained,
    Turbo,
}

impl std::fmt::Display for DeviceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceMode::Burst => write!(f, "burst"),
            DeviceMode::Sustained => write!(f, "sustained"),
            DeviceMode::Turbo => write!(f, "turbo"),
        }
    }
}

/// Per-device turbo mode state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurboState {
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub bytes_used: i64,
}

/// A discovered LAN device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub source: String,
}

/// Full runtime state for a device (internal, not sent over WebSocket).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    // Embedded Device fields
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub source: String,
    pub slot: i32,
    pub mark: i32,
    pub mode: DeviceMode,
    pub bucket_bytes: i64,
    pub bucket_capacity: i64,
    pub burst_ceil_kbit: i32,
    pub fair_share_kbit: i32,
    pub rate_down_bps: i64,
    pub rate_up_bps: i64,
    pub session_bytes: i64,
    pub session_up: i64,
    pub session_down: i64,
    pub cycle_bytes: i64,
    pub turbo: TurboState,
    #[serde(skip)]
    pub prev_counter_up: i64,
    #[serde(skip)]
    pub prev_counter_down: i64,
    #[serde(skip)]
    pub delta_up: i64,
    #[serde(skip)]
    pub delta_down: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaped_down_kbit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaped_up_kbit: Option<i32>,
}

/// Quota tracking state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaState {
    pub used: i64,
    pub remaining: i64,
    pub total: i64,
    pub used_upload: i64,
    pub used_download: i64,
    pub billing_month: String,
    pub pct: i32,
}

/// Current curve parameters for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveState {
    pub rate_kbit: i32,
    pub shape: f64,
    pub down_up_ratio: f64,
}

/// A single throughput measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputSample {
    pub ts: i64,
    pub down_bps: i64,
    pub up_bps: i64,
}

/// Aggregate throughput data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputState {
    pub current_down_bps: i64,
    pub current_up_bps: i64,
    pub samples_1h: Vec<ThroughputSample>,
}

/// Starlink dish status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DishStatus {
    pub connected: bool,
    pub uptime: i64,
    pub downlink_bps: f64,
    pub uplink_bps: f64,
    pub pop_ping_latency_ms: f64,
    pub signal_quality: f64,
    pub obstructed: bool,
    pub fraction_obstructed: f64,
    pub software_version: String,
    pub reachable: bool,
    pub usage_down: i64,
    pub usage_up: i64,
}

/// Full state snapshot pushed over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub ts: i64,
    pub quota: QuotaState,
    pub curve: CurveState,
    pub devices: Vec<DeviceSnapshot>,
    pub throughput: ThroughputState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dish: Option<DishStatus>,
}

/// Per-device data in the state snapshot (sent over WebSocket).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub mode: DeviceMode,
    pub bucket_bytes: i64,
    pub bucket_capacity: i64,
    pub bucket_pct: i32,
    pub burst_ceil_kbit: i32,
    pub rate_down_bps: i64,
    pub rate_up_bps: i64,
    pub session_bytes: i64,
    pub session_up: i64,
    pub session_down: i64,
    pub cycle_bytes: i64,
    pub turbo: bool,
    pub turbo_expires: Option<i64>,
    pub turbo_bytes: i64,
    pub bucket_refill_bps: i64,
    pub shaped_down_kbit: Option<i32>,
    pub shaped_up_kbit: Option<i32>,
    pub bucket_shape_at: i64,
    pub bucket_unshape_at: i64,
}

/// Request body for POST /api/v1/sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub starlink_used_gb: f64,
    pub source: String,
}

/// Request body for POST /api/v1/quota/adjust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaAdjustRequest {
    #[serde(default)]
    pub delta_bytes: Option<i64>,
    #[serde(default)]
    pub set_bytes: Option<i64>,
}

/// Request body for POST /api/v1/device/{mac}/turbo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurboRequest {
    pub duration_min: i32,
}

/// Request body for POST /api/v1/device/{mac}/bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSetRequest {
    pub tokens_mb: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_mode_display() {
        assert_eq!(DeviceMode::Burst.to_string(), "burst");
        assert_eq!(DeviceMode::Sustained.to_string(), "sustained");
        assert_eq!(DeviceMode::Turbo.to_string(), "turbo");
    }

    #[test]
    fn device_mode_serde_roundtrip() {
        let mode = DeviceMode::Burst;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"burst\"");
        let back: DeviceMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }

    #[test]
    fn turbo_state_default() {
        let ts = TurboState::default();
        assert!(!ts.active);
        assert_eq!(ts.bytes_used, 0);
        assert!(ts.expires_at.is_none());
    }

    #[test]
    fn state_snapshot_json_field_names() {
        let snap = StateSnapshot {
            ts: 1234567890,
            quota: QuotaState {
                used: 100,
                remaining: 900,
                total: 1000,
                used_upload: 30,
                used_download: 70,
                billing_month: "2026-03".to_string(),
                pct: 10,
            },
            curve: CurveState {
                rate_kbit: 50000,
                shape: 0.40,
                down_up_ratio: 0.80,
            },
            devices: vec![],
            throughput: ThroughputState {
                current_down_bps: 0,
                current_up_bps: 0,
                samples_1h: vec![],
            },
            dish: None,
        };

        let json = serde_json::to_value(&snap).unwrap();
        // Verify Go-compatible JSON field names
        assert!(json.get("ts").is_some());
        assert!(json.get("quota").is_some());
        assert!(json.get("curve").is_some());
        assert!(json.get("devices").is_some());
        assert!(json.get("throughput").is_some());

        let quota = json.get("quota").unwrap();
        assert!(quota.get("used").is_some());
        assert!(quota.get("remaining").is_some());
        assert!(quota.get("billing_month").is_some());
        assert!(quota.get("used_upload").is_some());
        assert!(quota.get("used_download").is_some());

        let curve = json.get("curve").unwrap();
        assert!(curve.get("rate_kbit").is_some());
        assert!(curve.get("down_up_ratio").is_some());

        let throughput = json.get("throughput").unwrap();
        assert!(throughput.get("current_down_bps").is_some());
        assert!(throughput.get("samples_1h").is_some());
    }

    #[test]
    fn device_snapshot_json_field_names() {
        let ds = DeviceSnapshot {
            mac: "aa:bb:cc:dd:ee:ff".to_string(),
            ip: "192.168.1.100".to_string(),
            hostname: "laptop".to_string(),
            mode: DeviceMode::Burst,
            bucket_bytes: 1000,
            bucket_capacity: 5000,
            bucket_pct: 20,
            burst_ceil_kbit: 50000,
            rate_down_bps: 1000000,
            rate_up_bps: 200000,
            session_bytes: 5000000,
            session_up: 1000000,
            session_down: 4000000,
            cycle_bytes: 10000000,
            turbo: false,
            turbo_expires: None,
            turbo_bytes: 0,
            bucket_refill_bps: 6250000,
            shaped_down_kbit: None,
            shaped_up_kbit: None,
            bucket_shape_at: 1250,
            bucket_unshape_at: 3750,
        };

        let json = serde_json::to_value(&ds).unwrap();
        assert!(json.get("mac").is_some());
        assert!(json.get("bucket_bytes").is_some());
        assert!(json.get("bucket_refill_bps").is_some());
        assert!(json.get("bucket_capacity").is_some());
        assert!(json.get("bucket_pct").is_some());
        assert!(json.get("burst_ceil_kbit").is_some());
        assert!(json.get("rate_down_bps").is_some());
        assert!(json.get("rate_up_bps").is_some());
        assert!(json.get("turbo_expires").is_some());
        assert!(json.get("turbo_bytes").is_some());
        assert!(json.get("shaped_down_kbit").is_some());
    }

    #[test]
    fn throughput_sample_json() {
        let sample = ThroughputSample {
            ts: 1234567890,
            down_bps: 50000000,
            up_bps: 10000000,
        };
        let json = serde_json::to_value(&sample).unwrap();
        assert_eq!(json.get("ts").unwrap(), 1234567890);
        assert_eq!(json.get("down_bps").unwrap(), 50000000);
        assert_eq!(json.get("up_bps").unwrap(), 10000000);
    }

    #[test]
    fn quota_adjust_request_optional_fields() {
        // Only delta_bytes
        let json = r#"{"delta_bytes": 1000}"#;
        let req: QuotaAdjustRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.delta_bytes, Some(1000));
        assert_eq!(req.set_bytes, None);

        // Only set_bytes
        let json = r#"{"set_bytes": 5000}"#;
        let req: QuotaAdjustRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.delta_bytes, None);
        assert_eq!(req.set_bytes, Some(5000));
    }

    #[test]
    fn dish_status_omitted_when_none() {
        let snap = StateSnapshot {
            ts: 0,
            quota: QuotaState {
                used: 0,
                remaining: 0,
                total: 0,
                used_upload: 0,
                used_download: 0,
                billing_month: String::new(),
                pct: 0,
            },
            curve: CurveState {
                rate_kbit: 0,
                shape: 0.0,
                down_up_ratio: 0.0,
            },
            devices: vec![],
            throughput: ThroughputState {
                current_down_bps: 0,
                current_up_bps: 0,
                samples_1h: vec![],
            },
            dish: None,
        };

        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains("\"dish\""));
    }
}
