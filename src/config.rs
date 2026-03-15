use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// A preconfigured device entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticDevice {
    pub mac: String,
    pub name: String,
}

/// Optional basic auth settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UIAuth {
    pub enabled: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password_hash: String,
}

/// All SLQM configuration values (safe to clone/copy around).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Values {
    pub network_mode: String,
    pub wan_iface: String,
    pub lan_iface: String,
    pub ifb_iface: String,
    pub dish_addr: String,
    pub dish_poll_interval_sec: i32,
    pub listen_addr: String,
    pub billing_reset_day: i32,
    pub monthly_quota_gb: i32,
    pub curve_shape: f64,
    pub max_rate_kbit: i32,
    pub min_rate_kbit: i32,
    pub down_up_ratio: f64,
    pub bucket_duration_sec: i32,
    pub burst_drain_ratio: f64,
    pub max_burst_kbit: i32,
    pub tick_interval_sec: i32,
    pub save_interval_sec: i32,
    pub device_scan_interval_sec: i32,
    pub overage_cost_per_gb: f64,
    pub plan_cost_monthly: f64,
    pub ui_auth: UIAuth,
    #[serde(default)]
    pub static_devices: Vec<StaticDevice>,
}

impl Default for Values {
    fn default() -> Self {
        Self {
            network_mode: "router".to_string(),
            wan_iface: "auto".to_string(),
            lan_iface: "auto".to_string(),
            ifb_iface: "ifb0".to_string(),
            dish_addr: "192.168.100.1:9200".to_string(),
            dish_poll_interval_sec: 30,
            listen_addr: ":8275".to_string(),
            billing_reset_day: 1,
            monthly_quota_gb: 20,
            curve_shape: 0.40,
            max_rate_kbit: 50000,
            min_rate_kbit: 1000,
            down_up_ratio: 0.80,
            bucket_duration_sec: 300,
            burst_drain_ratio: 0.10,
            max_burst_kbit: 300000,
            tick_interval_sec: 2,
            save_interval_sec: 60,
            device_scan_interval_sec: 15,
            overage_cost_per_gb: 10.0,
            plan_cost_monthly: 250.0,
            ui_auth: UIAuth::default(),
            static_devices: Vec::new(),
        }
    }
}

impl Values {
    /// Validate all config values are within allowed ranges.
    pub fn validate(&self) -> Result<(), String> {
        if self.billing_reset_day < 1 || self.billing_reset_day > 28 {
            return Err(format!(
                "billing_reset_day must be 1-28, got {}",
                self.billing_reset_day
            ));
        }
        if self.monthly_quota_gb < 1 || self.monthly_quota_gb > 500 {
            return Err(format!(
                "monthly_quota_gb must be 1-500, got {}",
                self.monthly_quota_gb
            ));
        }
        if self.curve_shape < 0.10 || self.curve_shape > 2.00 {
            return Err(format!(
                "curve_shape must be 0.10-2.00, got {:.2}",
                self.curve_shape
            ));
        }
        if self.max_rate_kbit < 1 || self.max_rate_kbit > 500000 {
            return Err(format!(
                "max_rate_kbit must be 1-500000, got {}",
                self.max_rate_kbit
            ));
        }
        if self.min_rate_kbit < 64 || self.min_rate_kbit > 50000 {
            return Err(format!(
                "min_rate_kbit must be 64-50000, got {}",
                self.min_rate_kbit
            ));
        }
        if self.down_up_ratio < 0.50 || self.down_up_ratio > 0.95 {
            return Err(format!(
                "down_up_ratio must be 0.50-0.95, got {:.2}",
                self.down_up_ratio
            ));
        }
        if self.bucket_duration_sec < 30 || self.bucket_duration_sec > 900 {
            return Err(format!(
                "bucket_duration_sec must be 30-900, got {}",
                self.bucket_duration_sec
            ));
        }
        if self.burst_drain_ratio < 0.01 || self.burst_drain_ratio > 0.50 {
            return Err(format!(
                "burst_drain_ratio must be 0.01-0.50, got {:.2}",
                self.burst_drain_ratio
            ));
        }
        if self.tick_interval_sec < 1 || self.tick_interval_sec > 10 {
            return Err(format!(
                "tick_interval_sec must be 1-10, got {}",
                self.tick_interval_sec
            ));
        }
        Ok(())
    }

    /// Returns total quota in bytes. 1 GB = 1073741824 bytes.
    pub fn monthly_quota_bytes(&self) -> i64 {
        self.monthly_quota_gb as i64 * 1_073_741_824
    }
}

/// Thread-safe config manager with mutex protection.
#[derive(Clone)]
pub struct Config {
    inner: Arc<RwLock<ConfigInner>>,
}

struct ConfigInner {
    values: Values,
    file_path: Option<PathBuf>,
    resolved_wan: Option<String>,
    resolved_lan: Option<String>,
}

impl Config {
    /// Create a Config with all default values.
    pub fn default_config() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ConfigInner {
                values: Values::default(),
                file_path: None,
                resolved_wan: None,
                resolved_lan: None,
            })),
        }
    }

    /// Load config from a JSON file, falling back to defaults for missing fields.
    pub fn load(path: &str) -> Result<Self, String> {
        let cfg = Self::default_config();
        {
            let mut inner = cfg.inner.write().unwrap();
            inner.file_path = Some(PathBuf::from(path));
        }

        let data = match std::fs::read_to_string(path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(cfg),
            Err(e) => return Err(format!("read config: {e}")),
        };

        // Merge file JSON on top of defaults so missing fields keep default values
        let file_map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&data).map_err(|e| format!("parse config: {e}"))?;

        {
            let mut inner = cfg.inner.write().unwrap();
            let mut default_json = serde_json::to_value(&inner.values)
                .map_err(|e| format!("serialize defaults: {e}"))?;
            let default_map = default_json.as_object_mut().unwrap();
            for (k, v) in file_map {
                default_map.insert(k, v);
            }
            inner.values = serde_json::from_value(serde_json::Value::Object(default_map.clone()))
                .map_err(|e| format!("merge config: {e}"))?;
        }

        {
            let inner = cfg.inner.read().unwrap();
            inner.values.validate().map_err(|e| format!("validate config: {e}"))?;
        }

        Ok(cfg)
    }

    /// Save current config to disk.
    pub fn save(&self) -> Result<(), String> {
        let inner = self.inner.read().unwrap();
        let path = inner
            .file_path
            .as_ref()
            .ok_or_else(|| "no config file path set".to_string())?;

        let data = serde_json::to_string_pretty(&inner.values)
            .map_err(|e| format!("marshal config: {e}"))?;

        std::fs::write(path, data).map_err(|e| format!("write config: {e}"))
    }

    /// Apply a partial JSON update to the config.
    pub fn update(&self, data: &[u8]) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        // Deserialize on top of existing values (partial update)
        let updated: Values = {
            let current_json = serde_json::to_value(&inner.values)
                .map_err(|e| format!("serialize current: {e}"))?;
            let mut current_map: serde_json::Map<String, serde_json::Value> =
                serde_json::from_value(current_json)
                    .map_err(|e| format!("convert to map: {e}"))?;
            let update_map: serde_json::Map<String, serde_json::Value> =
                serde_json::from_slice(data).map_err(|e| format!("parse update: {e}"))?;
            for (k, v) in update_map {
                current_map.insert(k, v);
            }
            serde_json::from_value(serde_json::Value::Object(current_map))
                .map_err(|e| format!("merge config: {e}"))?
        };
        updated.validate()?;
        inner.values = updated;
        Ok(())
    }

    /// Return a read-only copy of config values with effective interfaces.
    /// WAN/LAN fields contain the resolved interface names, not "auto".
    pub fn snapshot(&self) -> Values {
        let inner = self.inner.read().unwrap();
        let mut vals = inner.values.clone();
        if vals.wan_iface == "auto" {
            if let Some(ref resolved) = inner.resolved_wan {
                vals.wan_iface = resolved.clone();
            }
        }
        if vals.lan_iface == "auto" {
            if let Some(ref resolved) = inner.resolved_lan {
                vals.lan_iface = resolved.clone();
            }
        }
        vals
    }

    /// Return total monthly quota in bytes.
    pub fn monthly_quota_bytes(&self) -> i64 {
        self.inner.read().unwrap().values.monthly_quota_bytes()
    }

    /// Set the file path for saving config.
    pub fn set_file_path(&self, path: &str) {
        self.inner.write().unwrap().file_path = Some(PathBuf::from(path));
    }

    /// Store detected WAN/LAN interfaces. Does not modify the config values
    /// (which may be "auto"). Use `effective_wan`/`effective_lan` to get the
    /// actual interface in use.
    pub fn resolve_ifaces(&self, wan: &str, lan: &str) {
        let mut inner = self.inner.write().unwrap();
        if !wan.is_empty() {
            inner.resolved_wan = Some(wan.to_string());
        }
        if !lan.is_empty() {
            inner.resolved_lan = Some(lan.to_string());
        }
    }

    /// Return the effective WAN interface (resolved or configured).
    pub fn effective_wan(&self) -> String {
        let inner = self.inner.read().unwrap();
        if inner.values.wan_iface == "auto" {
            inner.resolved_wan.clone().unwrap_or_else(|| "auto".to_string())
        } else {
            inner.values.wan_iface.clone()
        }
    }

    /// Return the effective LAN interface (resolved or configured).
    pub fn effective_lan(&self) -> String {
        let inner = self.inner.read().unwrap();
        if inner.values.lan_iface == "auto" {
            inner.resolved_lan.clone().unwrap_or_else(|| "auto".to_string())
        } else {
            inner.values.lan_iface.clone()
        }
    }

    /// Return the config values as a JSON Value (for API responses).
    /// Includes `resolved_wan` and `resolved_lan` fields showing
    /// the actual interfaces in use.
    pub fn to_json(&self) -> serde_json::Value {
        let inner = self.inner.read().unwrap();
        let mut json = serde_json::to_value(&inner.values).unwrap();
        if let Some(obj) = json.as_object_mut() {
            if let Some(ref wan) = inner.resolved_wan {
                obj.insert("resolved_wan".to_string(), serde_json::Value::String(wan.clone()));
            }
            if let Some(ref lan) = inner.resolved_lan {
                obj.insert("resolved_lan".to_string(), serde_json::Value::String(lan.clone()));
            }
        }
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let cfg = Config::default_config();
        let snap = cfg.snapshot();

        assert_eq!(snap.monthly_quota_gb, 20);
        assert!((snap.curve_shape - 0.40).abs() < f64::EPSILON);
        assert_eq!(snap.max_rate_kbit, 50000);
        assert_eq!(snap.wan_iface, "auto");
    }

    #[test]
    fn test_load_missing() {
        let cfg = Config::load("/nonexistent/config.json").unwrap();
        let snap = cfg.snapshot();
        assert_eq!(snap.monthly_quota_gb, 20);
    }

    #[test]
    fn test_load_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let path_str = path.to_str().unwrap();

        let cfg = Config::default_config();
        cfg.set_file_path(path_str);
        cfg.save().unwrap();

        let cfg2 = Config::load(path_str).unwrap();
        let snap = cfg2.snapshot();
        assert_eq!(snap.monthly_quota_gb, 20);
    }

    #[test]
    fn test_update() {
        let cfg = Config::default_config();

        let update = r#"{"monthly_quota_gb": 50, "curve_shape": 0.60}"#;
        cfg.update(update.as_bytes()).unwrap();

        let snap = cfg.snapshot();
        assert_eq!(snap.monthly_quota_gb, 50);
        assert!((snap.curve_shape - 0.60).abs() < f64::EPSILON);
        // Other values should remain default
        assert_eq!(snap.max_rate_kbit, 50000);
    }

    #[test]
    fn test_validation() {
        let cfg = Config::default_config();

        // Invalid billing reset day = 0
        let result = cfg.update(r#"{"billing_reset_day": 0}"#.as_bytes());
        assert!(result.is_err());

        // Reset to valid state
        let cfg = Config::default_config();
        let result = cfg.update(r#"{"billing_reset_day": 29}"#.as_bytes());
        assert!(result.is_err());

        // Invalid curve shape
        let cfg = Config::default_config();
        let result = cfg.update(r#"{"curve_shape": 0.05}"#.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn test_monthly_quota_bytes() {
        let cfg = Config::default_config();
        let got = cfg.monthly_quota_bytes();
        let want = 20i64 * 1_073_741_824;
        assert_eq!(got, want);
    }

    #[test]
    fn test_load_custom_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let path_str = path.to_str().unwrap();

        let custom = serde_json::json!({
            "monthly_quota_gb": 100,
            "curve_shape": 0.50,
            "max_rate_kbit": 100000,
            "min_rate_kbit": 500
        });
        std::fs::write(&path, serde_json::to_string(&custom).unwrap()).unwrap();

        let cfg = Config::load(path_str).unwrap();
        let snap = cfg.snapshot();
        assert_eq!(snap.monthly_quota_gb, 100);
        assert_eq!(snap.min_rate_kbit, 500);
    }

    #[test]
    fn test_resolve_ifaces() {
        let cfg = Config::default_config();
        // Before resolving, snapshot returns "auto" (no resolved value yet)
        assert_eq!(cfg.snapshot().wan_iface, "auto");
        assert_eq!(cfg.snapshot().lan_iface, "auto");

        cfg.resolve_ifaces("eth0", "br-lan");
        // Snapshot returns resolved values
        assert_eq!(cfg.snapshot().wan_iface, "eth0");
        assert_eq!(cfg.snapshot().lan_iface, "br-lan");
        // Effective accessors also return resolved values
        assert_eq!(cfg.effective_wan(), "eth0");
        assert_eq!(cfg.effective_lan(), "br-lan");

        // Stored config still has "auto"
        let json = cfg.to_json();
        assert_eq!(json.get("wan_iface").unwrap(), "auto");
        assert_eq!(json.get("lan_iface").unwrap(), "auto");
        // Resolved values included in API response
        assert_eq!(json.get("resolved_wan").unwrap(), "eth0");
        assert_eq!(json.get("resolved_lan").unwrap(), "br-lan");

        // Updating resolved values works
        cfg.resolve_ifaces("eth1", "br-guest");
        assert_eq!(cfg.snapshot().wan_iface, "eth1");
        assert_eq!(cfg.snapshot().lan_iface, "br-guest");

        // Non-auto config values are not affected by resolve
        cfg.update(r#"{"wan_iface": "eth2"}"#.as_bytes()).unwrap();
        cfg.resolve_ifaces("eth99", "");
        assert_eq!(cfg.snapshot().wan_iface, "eth2"); // uses config, not resolved
        assert_eq!(cfg.effective_wan(), "eth2");
    }

    #[test]
    fn test_json_field_names_match_go() {
        let vals = Values::default();
        let json = serde_json::to_value(&vals).unwrap();

        // Verify Go-compatible JSON field names
        assert!(json.get("network_mode").is_some());
        assert!(json.get("wan_iface").is_some());
        assert!(json.get("lan_iface").is_some());
        assert!(json.get("ifb_iface").is_some());
        assert!(json.get("dish_addr").is_some());
        assert!(json.get("dish_poll_interval_sec").is_some());
        assert!(json.get("listen_addr").is_some());
        assert!(json.get("billing_reset_day").is_some());
        assert!(json.get("monthly_quota_gb").is_some());
        assert!(json.get("curve_shape").is_some());
        assert!(json.get("max_rate_kbit").is_some());
        assert!(json.get("min_rate_kbit").is_some());
        assert!(json.get("down_up_ratio").is_some());
        assert!(json.get("bucket_duration_sec").is_some());
        assert!(json.get("burst_drain_ratio").is_some());
        assert!(json.get("tick_interval_sec").is_some());
        assert!(json.get("save_interval_sec").is_some());
        assert!(json.get("device_scan_interval_sec").is_some());
        assert!(json.get("overage_cost_per_gb").is_some());
        assert!(json.get("plan_cost_monthly").is_some());
        assert!(json.get("ui_auth").is_some());
        assert!(json.get("static_devices").is_some());
    }

    #[test]
    fn test_save_without_path() {
        let cfg = Config::default_config();
        let result = cfg.save();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no config file path set"));
    }
}
