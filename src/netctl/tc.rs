use super::{run_cmd, run_cmd_ignore};
use crate::model::DeviceMode;
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

    pub fn wan_iface(&self) -> &str {
        &self.wan_iface
    }

    pub fn lan_iface(&self) -> &str {
        &self.lan_iface
    }

    /// Initialize the HTB qdisc trees on WAN and LAN interfaces.
    pub fn setup_htb(&self, root_rate_kbit: i32) -> Result<(), String> {
        self.setup_wan(root_rate_kbit)?;
        self.setup_lan(root_rate_kbit)
    }

    /// WAN tree structure:
    ///   1: (root HTB, default 2)
    ///   └── 1:1 (uncapped parent — per-device classes handle shaping)
    ///       ├── 1:2 (default catch-all)
    ///       └── 1:10+ (per-device classes)
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
    ///   └── 1:1 (uncapped)
    ///       ├── 1:2 (unmatched/local, uncapped)
    ///       └── 1:3 (download parent, uncapped — per-device classes handle shaping)
    ///           ├── 1:4 (default shaped)
    ///           └── 1:10+ (per-device classes)
    fn setup_lan(&self, _download_rate_kbit: i32) -> Result<(), String> {
        let iface = &self.lan_iface;
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
            "rate", &format!("{UNCAPPED}kbit"),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
        if let Err(e) = self.tc(&["qdisc", "add", "dev", iface, "parent", "1:2", "fq_codel"]) {
            warn!("fq_codel on {iface} default: {e}");
        }
        self.tc(&[
            "class", "add", "dev", iface, "parent", "1:1", "classid", "1:3", "htb",
            "rate", &format!("{UNCAPPED}kbit"),
            "ceil", &format!("{UNCAPPED}kbit"),
        ])?;
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

    /// No-op — parent classes are always uncapped.
    /// Per-device classes handle all rate limiting.
    pub fn update_root_rate(&self, _rate_kbit: i32, _has_turbo: bool) -> Result<(), String> {
        Ok(())
    }

    /// Check if the HTB root qdisc exists on the WAN interface.
    pub fn htb_exists(&self) -> bool {
        if let Ok(output) = run_cmd("tc", &["qdisc", "show", "dev", &self.wan_iface]) {
            output.contains("htb")
        } else {
            false
        }
    }

    /// Re-create HTB trees only if they are missing.
    pub fn ensure_htb(&self, rate_kbit: i32) -> Result<(), String> {
        if !self.htb_exists() {
            info!("HTB trees missing, re-creating");
            self.setup_htb(rate_kbit)?;
        }
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

        // rate == ceil: Rust state machine controls mode, HTB enforces hard cap
        let rate = ceil_kbit.max(rate_kbit).max(1);
        // WAN: device class under 1:1
        self.add_class_on_iface(&self.wan_iface, "1:1", &class_id, &handle, &mark, rate, rate)?;
        // LAN: device class under 1:3 (download parent)
        self.add_class_on_iface(&self.lan_iface, "1:3", &class_id, &handle, &mark, rate, rate)?;
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
    /// rate == ceil everywhere — Rust state machine controls mode transitions,
    /// HTB just enforces the current hard cap.
    pub fn set_device_mode(
        &self,
        slot: i32,
        mode: DeviceMode,
        fair_share_kbit: i32,
        burst_ceil_kbit: i32,
        down_up_ratio: f64,
    ) {
        match mode {
            DeviceMode::Turbo => {
                self.set_class(&self.wan_iface, slot, "1:1", UNCAPPED, UNCAPPED);
                self.set_class(&self.lan_iface, slot, "1:3", UNCAPPED, UNCAPPED);
            }
            DeviceMode::Burst => {
                let down = (burst_ceil_kbit as f64 * down_up_ratio) as i32;
                let up = burst_ceil_kbit - down;
                self.set_class(&self.wan_iface, slot, "1:1", up.max(1), up.max(1));
                self.set_class(&self.lan_iface, slot, "1:3", down.max(1), down.max(1));
            }
            DeviceMode::Sustained => {
                let down = (fair_share_kbit as f64 * down_up_ratio) as i32;
                let up = fair_share_kbit - down;
                self.set_class(&self.wan_iface, slot, "1:1", up.max(1), up.max(1));
                self.set_class(&self.lan_iface, slot, "1:3", down.max(1), down.max(1));
            }
        }
    }

    fn set_class(&self, iface: &str, slot: i32, parent: &str, rate_kbit: i32, ceil_kbit: i32) {
        let class_id = format!("1:{}", 10 + slot);
        let rate = format!("{rate_kbit}kbit");
        let ceil = format!("{ceil_kbit}kbit");

        // Try "change" first; if the class was lost (e.g., qdisc reset),
        // fall back to "add" so the device doesn't run unshaped.
        if self
            .tc(&[
                "class", "change", "dev", iface, "parent", parent, "classid", &class_id, "htb",
                "rate", &rate, "ceil", &ceil,
            ])
            .is_err()
        {
            warn!(
                "tc class change failed for {class_id} on {iface}, re-adding"
            );
            if let Err(e) = self.tc(&[
                "class", "add", "dev", iface, "parent", parent, "classid", &class_id, "htb",
                "rate", &rate, "ceil", &ceil,
            ]) {
                warn!("tc class add also failed for {class_id} on {iface}: {e}");
                return;
            }
            // Re-add fq_codel and filter for the recreated class
            let handle = format!("{}:", 10 + slot);
            let mark = (100 + slot).to_string();
            let _ = self.tc(&[
                "qdisc", "add", "dev", iface, "parent", &class_id, "handle", &handle, "fq_codel",
            ]);
            let _ = self.tc(&[
                "filter", "add", "dev", iface, "parent", "1:", "protocol", "ip", "prio", "1",
                "handle", &mark, "fw", "classid", &class_id,
            ]);
        }
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
