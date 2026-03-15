use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;

const QUOTA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("quota");
const DEVICES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("devices");
const CONFIG_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("config");
const HISTORY_TABLE: TableDefinition<i64, &[u8]> = TableDefinition::new("history");

/// Persistent store backed by redb (replaces bbolt from Go version).
pub struct Store {
    db: Database,
}

impl Store {
    /// Open or create the database at the given path.
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = Database::create(path).map_err(|e| format!("open store: {e}"))?;

        // Initialize tables by doing a write transaction
        let write_txn = db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let _ = write_txn.open_table(QUOTA_TABLE).map_err(|e| format!("init quota: {e}"))?;
            let _ = write_txn.open_table(DEVICES_TABLE).map_err(|e| format!("init devices: {e}"))?;
            let _ = write_txn.open_table(CONFIG_TABLE).map_err(|e| format!("init config: {e}"))?;
            let _ = write_txn.open_table(HISTORY_TABLE).map_err(|e| format!("init history: {e}"))?;
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit init: {e}"))?;

        Ok(Self { db })
    }

    /// Persist the current monthly usage and billing month.
    pub fn save_quota(
        &self,
        month_used: i64,
        used_upload: i64,
        used_download: i64,
        billing_month: &str,
    ) -> Result<(), String> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(QUOTA_TABLE)
                .map_err(|e| format!("open quota: {e}"))?;
            table
                .insert("month_used", &i64_to_bytes(month_used)[..])
                .map_err(|e| format!("put month_used: {e}"))?;
            table
                .insert("used_upload", &i64_to_bytes(used_upload)[..])
                .map_err(|e| format!("put used_upload: {e}"))?;
            table
                .insert("used_download", &i64_to_bytes(used_download)[..])
                .map_err(|e| format!("put used_download: {e}"))?;
            table
                .insert("billing_month", billing_month.as_bytes())
                .map_err(|e| format!("put billing_month: {e}"))?;
            let now = chrono::Utc::now().timestamp();
            table
                .insert("last_save", &i64_to_bytes(now)[..])
                .map_err(|e| format!("put last_save: {e}"))?;
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit quota: {e}"))?;
        Ok(())
    }

    /// Read the persisted quota state.
    pub fn load_quota(&self) -> Result<(i64, i64, i64, String), String> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| format!("begin read: {e}"))?;
        let table = read_txn
            .open_table(QUOTA_TABLE)
            .map_err(|e| format!("open quota: {e}"))?;

        let month_used = table
            .get("month_used")
            .map_err(|e| format!("get month_used: {e}"))?
            .map(|v| bytes_to_i64(v.value()))
            .unwrap_or(0);

        let used_upload = table
            .get("used_upload")
            .map_err(|e| format!("get used_upload: {e}"))?
            .map(|v| bytes_to_i64(v.value()))
            .unwrap_or(0);

        let used_download = table
            .get("used_download")
            .map_err(|e| format!("get used_download: {e}"))?
            .map(|v| bytes_to_i64(v.value()))
            .unwrap_or(0);

        let billing_month = table
            .get("billing_month")
            .map_err(|e| format!("get billing_month: {e}"))?
            .map(|v| String::from_utf8_lossy(v.value()).to_string())
            .unwrap_or_default();

        Ok((month_used, used_upload, used_download, billing_month))
    }

    /// Persist a device's cumulative cycle bytes.
    pub fn save_device_cycle_bytes(&self, mac: &str, cycle_bytes: i64) -> Result<(), String> {
        let key = format!("{mac}_cycle");
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(DEVICES_TABLE)
                .map_err(|e| format!("open devices: {e}"))?;
            table
                .insert(key.as_str(), &i64_to_bytes(cycle_bytes)[..])
                .map_err(|e| format!("put device: {e}"))?;
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit device: {e}"))?;
        Ok(())
    }

    /// Read a device's persisted cycle bytes.
    pub fn load_device_cycle_bytes(&self, mac: &str) -> Result<i64, String> {
        let key = format!("{mac}_cycle");
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| format!("begin read: {e}"))?;
        let table = read_txn
            .open_table(DEVICES_TABLE)
            .map_err(|e| format!("open devices: {e}"))?;

        let value = table
            .get(key.as_str())
            .map_err(|e| format!("get device: {e}"))?
            .map(|v| bytes_to_i64(v.value()))
            .unwrap_or(0);

        Ok(value)
    }

    /// Persist the config as JSON bytes.
    pub fn save_config(&self, data: &[u8]) -> Result<(), String> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(CONFIG_TABLE)
                .map_err(|e| format!("open config: {e}"))?;
            table
                .insert("config_json", data)
                .map_err(|e| format!("put config: {e}"))?;
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit config: {e}"))?;
        Ok(())
    }

    /// Read the persisted config JSON.
    pub fn load_config(&self) -> Result<Option<Vec<u8>>, String> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| format!("begin read: {e}"))?;
        let table = read_txn
            .open_table(CONFIG_TABLE)
            .map_err(|e| format!("open config: {e}"))?;

        let data = table
            .get("config_json")
            .map_err(|e| format!("get config: {e}"))?
            .map(|v| v.value().to_vec());

        Ok(data)
    }

    /// Save a timestamped state snapshot for historical charting.
    pub fn save_history_snapshot(&self, ts: i64, snapshot: &[u8]) -> Result<(), String> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| format!("open history: {e}"))?;
            table
                .insert(ts, snapshot)
                .map_err(|e| format!("put history: {e}"))?;
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit history: {e}"))?;
        Ok(())
    }

    /// Read history snapshots within a time range (inclusive).
    pub fn load_history(&self, from: i64, to: i64) -> Result<Vec<Vec<u8>>, String> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| format!("begin read: {e}"))?;
        let table = read_txn
            .open_table(HISTORY_TABLE)
            .map_err(|e| format!("open history: {e}"))?;

        let mut results = Vec::new();
        let range = table
            .range(from..=to)
            .map_err(|e| format!("range history: {e}"))?;
        for entry in range {
            let (_, v) = entry.map_err(|e| format!("read entry: {e}"))?;
            results.push(v.value().to_vec());
        }

        Ok(results)
    }

    /// Remove history entries older than the given timestamp.
    pub fn prune_history(&self, before: i64) -> Result<(), String> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| format!("open history: {e}"))?;

            // Collect keys to delete
            let keys: Vec<i64> = {
                let range = table
                    .range(..before)
                    .map_err(|e| format!("range history: {e}"))?;
                let mut keys = Vec::new();
                for entry in range {
                    let (k, _) = entry.map_err(|e| format!("read entry: {e}"))?;
                    keys.push(k.value());
                }
                keys
            };

            for key in keys {
                table
                    .remove(key)
                    .map_err(|e| format!("delete entry: {e}"))?;
            }
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit prune: {e}"))?;
        Ok(())
    }

    /// Remove all device data (used on billing cycle reset).
    pub fn clear_devices(&self) -> Result<(), String> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| format!("begin write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(DEVICES_TABLE)
                .map_err(|e| format!("open devices: {e}"))?;

            // Drain the entire table
            while let Some(entry) = table
                .pop_first()
                .map_err(|e| format!("pop device: {e}"))?
            {
                let _ = entry;
            }
        }
        write_txn
            .commit()
            .map_err(|e| format!("commit clear: {e}"))?;
        Ok(())
    }
}

fn i64_to_bytes(v: i64) -> [u8; 8] {
    (v as u64).to_be_bytes()
}

fn bytes_to_i64(b: &[u8]) -> i64 {
    if b.len() < 8 {
        return 0;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&b[..8]);
    u64::from_be_bytes(arr) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let store = Store::open(&path).unwrap();
        (store, dir)
    }

    #[test]
    fn quota_round_trip() {
        let (s, _dir) = test_store();

        s.save_quota(1234567890, 400000000, 834567890, "2026-03")
            .unwrap();

        let (used, up, down, month) = s.load_quota().unwrap();
        assert_eq!(used, 1234567890);
        assert_eq!(up, 400000000);
        assert_eq!(down, 834567890);
        assert_eq!(month, "2026-03");
    }

    #[test]
    fn device_cycle_bytes() {
        let (s, _dir) = test_store();

        let mac = "aa:bb:cc:dd:ee:ff";
        s.save_device_cycle_bytes(mac, 5000000).unwrap();

        let got = s.load_device_cycle_bytes(mac).unwrap();
        assert_eq!(got, 5000000);

        // Unknown device
        let got = s.load_device_cycle_bytes("00:00:00:00:00:00").unwrap();
        assert_eq!(got, 0);
    }

    #[test]
    fn config_persistence() {
        let (s, _dir) = test_store();

        let cfg = br#"{"monthly_quota_gb": 50}"#;
        s.save_config(cfg).unwrap();

        let got = s.load_config().unwrap().unwrap();
        assert_eq!(got, cfg);
    }

    #[test]
    fn history_snapshot() {
        let (s, _dir) = test_store();

        let now = chrono::Utc::now().timestamp();
        let snap1 = br#"{"ts":1}"#;
        let snap2 = br#"{"ts":2}"#;

        s.save_history_snapshot(now - 7200, snap1).unwrap();
        s.save_history_snapshot(now - 3600, snap2).unwrap();

        let results = s.load_history(now - 10800, now).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn prune_history() {
        let (s, _dir) = test_store();

        let now = chrono::Utc::now().timestamp();
        s.save_history_snapshot(now - 172800, br#"{"old":true}"#)
            .unwrap();
        s.save_history_snapshot(now - 3600, br#"{"new":true}"#)
            .unwrap();

        s.prune_history(now - 86400).unwrap();

        let results = s.load_history(now - 259200, now).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn clear_devices() {
        let (s, _dir) = test_store();

        s.save_device_cycle_bytes("aa:bb:cc:dd:ee:ff", 1000)
            .unwrap();
        s.clear_devices().unwrap();

        let got = s.load_device_cycle_bytes("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(got, 0);
    }

    #[test]
    fn empty_load() {
        let (s, _dir) = test_store();

        let (used, up, down, month) = s.load_quota().unwrap();
        assert_eq!(used, 0);
        assert_eq!(up, 0);
        assert_eq!(down, 0);
        assert_eq!(month, "");
    }

    #[test]
    fn empty_config_load() {
        let (s, _dir) = test_store();
        let got = s.load_config().unwrap();
        assert!(got.is_none());
    }
}
