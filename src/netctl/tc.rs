use super::{run_cmd, run_cmd_ignore};
use tracing::{info, warn};

const UNCAPPED: i32 = 1_000_000; // 1 Gbps in kbit

/// Manages dual HTB qdisc trees:
///   - WAN interface egress: upload shaping (root class = curve rate)
///   - LAN interface egress: download shaping (uses an intermediate parent class
///     at the curve rate so unmatched local/inter-LAN traffic is not throttled)
pub struct TCController {
    wan_iface: String,
    lan_iface: String,
    min_rate: i32,
}

impl TCController {
    pub fn new(wan_iface: &str, lan_iface: &str, min_rate_kbit: i32) -> Self {
        Self {
            wan_iface: wan_iface.to_string(),
            lan_iface: lan_iface.to_string(),
            min_rate: min_rate_kbit,
        }
    }

    /// Initialize the HTB qdisc trees on WAN and LAN interfaces.
    pub fn setup_htb(&self, root_rate_kbit: i32) -> Result<(), String> {
        self.setup_wan(root_rate_kbit)?;
        self.setup_lan(root_rate_kbit)
    }

    /// WAN tree structure:
    ///   1: (root HTB, default 2)
    ///   └── 1:1 (rate=uncapped, ceil=uncapped — no parent restriction)
    ///       ├── 1:2 (default catch-all, rate=minRate, ceil=uncapped)
    ///       └── 1:10+ (per-device classes do the actual shaping)
    fn setup_wan(&self, _root_rate_kbit: i32) -> Result<(), String> {
        let iface = &self.wan_iface;
        run_cmd_ignore("tc", &["qdisc", "del", "dev", iface, "root"]);

        self.tc(&[
            "qdisc", "add", "dev", iface, "root", "handle", "1:", "htb", "default", "2",
        ])?;
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:", "classid", "1:1", "htb",
            "rate", &format!("{UNCAPPED}kbit"),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:1", "classid", "1:2", "htb",
            "rate", &format!("{}kbit", self.min_rate),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        if let Err(e) = self.tc(&["qdisc", "add", "dev", iface, "parent", "1:2", "fq_codel"]) {
            warn!("fq_codel on {iface} default: {e}");
        }
        Ok(())
    }

    /// LAN tree structure:
    ///   1: (root HTB, default 2)
    ///   └── 1:1 (rate=uncapped, ceil=uncapped)
    ///       ├── 1:2 (unmatched/local, rate=uncapped, ceil=uncapped)
    ///       └── 1:3 (download parent, rate=uncapped, ceil=uncapped — no restriction)
    ///           ├── 1:4 (default shaped, rate=minRate, ceil=uncapped)
    ///           └── 1:10+ (per-device classes do the actual shaping)
    fn setup_lan(&self, _download_rate_kbit: i32) -> Result<(), String> {
        let iface = &self.lan_iface;
        run_cmd_ignore("tc", &["qdisc", "del", "dev", iface, "root"]);

        self.tc(&[
            "qdisc", "add", "dev", iface, "root", "handle", "1:", "htb", "default", "2",
        ])?;
        // Root class: uncapped
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:", "classid", "1:1", "htb",
            "rate", &format!("{UNCAPPED}kbit"),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        // Default class for unmatched/local traffic
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:1", "classid", "1:2", "htb",
            "rate", &format!("{UNCAPPED}kbit"),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        if let Err(e) = self.tc(&["qdisc", "add", "dev", iface, "parent", "1:2", "fq_codel"]) {
            warn!("fq_codel on {iface} default: {e}");
        }
        // Download shaping parent — uncapped, per-device classes handle limits
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:1", "classid", "1:3", "htb",
            "rate", &format!("{UNCAPPED}kbit"),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        // Default shaped class
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:3", "classid", "1:4", "htb",
            "rate", &format!("{}kbit", self.min_rate),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        if let Err(e) = self.tc(&[
            "qdisc", "add", "dev", iface, "parent", "1:4", "fq_codel",
        ]) {
            warn!("fq_codel on {iface} default shaped: {e}");
        }
        Ok(())
    }

    /// Parent classes are always uncapped — no-op.
    /// Per-device classes handle all rate limiting.
    pub fn update_root_rate(&self, _rate_kbit: i32, _has_turbo: bool) -> Result<(), String> {
        Ok(())
    }

    /// Create a class for a device in both HTB trees.
    pub fn add_device_class(
        &self,
        slot: i32,
        rate_kbit: i32,
        ceil_kbit: i32,
    ) -> Result<(), String> {
        let class_id = format!("1:{}", 10 + slot);
        let handle = format!("{}:", 10 + slot);
        let mark = (100 + slot).to_string();

        // WAN: device class under 1:1
        self.add_class_on_iface(&self.wan_iface, "1:1", &class_id, &handle, &mark, rate_kbit, ceil_kbit)?;
        // LAN: device class under 1:3 (download parent)
        self.add_class_on_iface(&self.lan_iface, "1:3", &class_id, &handle, &mark, rate_kbit, ceil_kbit)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_class_on_iface(
        &self,
        iface: &str,
        parent: &str,
        class_id: &str,
        handle: &str,
        mark: &str,
        rate_kbit: i32,
        ceil_kbit: i32,
    ) -> Result<(), String> {
        self.tc(&[
            "class", "add", "dev", iface, "parent", parent, "classid", class_id, "htb", "rate",
            &format!("{rate_kbit}kbit"),
            "ceil",
            &format!("{ceil_kbit}kbit"),
        ])?;
        if let Err(e) = self.tc(&[
            "qdisc", "add", "dev", iface, "parent", class_id, "handle", handle, "fq_codel",
        ]) {
            warn!("fq_codel on {iface} {class_id}: {e}");
        }
        self.tc(&[
            "filter", "add", "dev", iface, "parent", "1:", "protocol", "ip", "prio", "1",
            "handle", mark, "fw", "classid", class_id,
        ])?;
        Ok(())
    }

    /// Remove a device's class from both HTB trees.
    pub fn remove_device_class(&self, slot: i32) {
        let class_id = format!("1:{}", 10 + slot);
        for iface in [&self.wan_iface, &self.lan_iface] {
            run_cmd_ignore("tc", &["class", "del", "dev", iface, "classid", &class_id]);
        }
    }

    /// Update both HTB trees based on device mode.
    pub fn set_device_mode(
        &self,
        slot: i32,
        mode: &str,
        fair_share_kbit: i32,
        burst_ceil_kbit: i32,
        down_up_ratio: f64,
    ) {
        match mode {
            "turbo" => {
                self.set_class(&self.wan_iface, slot, "1:1", fair_share_kbit, UNCAPPED);
                self.set_class(&self.lan_iface, slot, "1:3", fair_share_kbit, UNCAPPED);
            }
            "burst" => {
                let mut down_ceil = (burst_ceil_kbit as f64 * down_up_ratio) as i32;
                let mut up_ceil = burst_ceil_kbit - down_ceil;
                if down_ceil < self.min_rate {
                    down_ceil = self.min_rate;
                }
                if up_ceil < self.min_rate {
                    up_ceil = self.min_rate;
                }
                self.set_class(&self.wan_iface, slot, "1:1", fair_share_kbit, up_ceil);
                self.set_class(&self.lan_iface, slot, "1:3", fair_share_kbit, down_ceil);
            }
            "sustained" => {
                let mut down_ceil = (fair_share_kbit as f64 * down_up_ratio) as i32;
                let mut up_ceil = fair_share_kbit - down_ceil;
                if down_ceil < self.min_rate {
                    down_ceil = self.min_rate;
                }
                if up_ceil < self.min_rate {
                    up_ceil = self.min_rate;
                }
                self.set_class(&self.wan_iface, slot, "1:1", up_ceil, up_ceil);
                self.set_class(&self.lan_iface, slot, "1:3", down_ceil, down_ceil);
            }
            _ => {
                info!("unknown mode: {mode}");
            }
        }
    }

    fn set_class(&self, iface: &str, slot: i32, parent: &str, rate_kbit: i32, ceil_kbit: i32) {
        let class_id = format!("1:{}", 10 + slot);
        let _ = self.tc(&[
            "class", "change", "dev", iface, "parent", parent, "classid", &class_id, "htb",
            "rate",
            &format!("{rate_kbit}kbit"),
            "ceil",
            &format!("{ceil_kbit}kbit"),
        ]);
    }

    /// Remove all tc qdiscs from both interfaces.
    pub fn teardown(&self) {
        for iface in [&self.wan_iface, &self.lan_iface] {
            run_cmd_ignore("tc", &["qdisc", "del", "dev", iface, "root"]);
        }
    }

    fn tc(&self, args: &[&str]) -> Result<(), String> {
        run_cmd("tc", args).map(|_| ())
    }
}
