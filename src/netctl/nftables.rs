use super::{run_cmd, run_cmd_ignore};
use tracing::info;

/// Manages the nftables table for SLQM packet marking.
pub struct NFTController {
    wan_iface: String,
    table_name: String,
}

impl NFTController {
    pub fn new(wan_iface: &str) -> Self {
        Self {
            wan_iface: wan_iface.to_string(),
            table_name: "slqm".to_string(),
        }
    }

    /// Create the nftables table and chains for bidirectional marking.
    pub fn setup(&self) -> Result<(), String> {
        // Remove existing table (ignore error if not exists)
        run_cmd_ignore("nft", &["delete", "table", "inet", &self.table_name]);

        let cmds = [
            format!("add table inet {}", self.table_name),
            format!(
                "add chain inet {} upload {{ type filter hook forward priority mangle ; }}",
                self.table_name
            ),
            format!(
                "add chain inet {} download {{ type filter hook forward priority mangle ; }}",
                self.table_name
            ),
        ];

        for cmd in &cmds {
            self.nft(cmd)?;
        }
        Ok(())
    }

    /// Add bidirectional marking rules for a device.
    pub fn add_device(&self, ip: &str, mark: i32) -> Result<(), String> {
        let upload_rule = format!(
            "add rule inet {} upload oifname \"{}\" ip saddr {} counter meta mark set {}",
            self.table_name, self.wan_iface, ip, mark
        );
        let download_rule = format!(
            "add rule inet {} download iifname \"{}\" ip daddr {} counter meta mark set {}",
            self.table_name, self.wan_iface, ip, mark
        );

        info!(
            "nft: adding rules for {} mark={} (wan={})",
            ip, mark, self.wan_iface
        );
        self.nft(&upload_rule)
            .map_err(|e| format!("add upload rule for {ip}: {e}"))?;
        self.nft(&download_rule)
            .map_err(|e| format!("add download rule for {ip}: {e}"))?;
        Ok(())
    }

    /// Remove all rules for a device IP.
    pub fn remove_device(&self, ip: &str) {
        for chain in ["upload", "download"] {
            if let Ok(handles) = self.find_rule_handles(chain, ip) {
                for handle in handles {
                    let _ = self.nft(&format!(
                        "delete rule inet {} {} handle {}",
                        self.table_name, chain, handle
                    ));
                }
            }
        }
    }

    /// Remove the entire SLQM nftables table.
    pub fn teardown(&self) {
        run_cmd_ignore("nft", &["delete", "table", "inet", &self.table_name]);
    }

    /// Get the table name (for counter reading).
    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    fn find_rule_handles(&self, chain: &str, ip: &str) -> Result<Vec<String>, String> {
        let output = run_cmd(
            "nft",
            &["-a", "list", "chain", "inet", &self.table_name, chain],
        )?;

        let mut handles = Vec::new();
        for line in output.lines() {
            if line.contains(ip) && line.contains("handle") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for (i, p) in parts.iter().enumerate() {
                    if *p == "handle" && i + 1 < parts.len() {
                        handles.push(parts[i + 1].to_string());
                    }
                }
            }
        }
        Ok(handles)
    }

    fn nft(&self, rule: &str) -> Result<(), String> {
        let args: Vec<&str> = rule.split_whitespace().collect();
        run_cmd("nft", &args).map(|_| ())
    }
}
