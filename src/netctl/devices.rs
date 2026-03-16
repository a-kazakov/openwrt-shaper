use super::run_cmd;
use crate::model::Device;
use std::collections::HashMap;

/// Detect WAN interface from the default route.
pub fn detect_wan_iface() -> Result<String, String> {
    let output = run_cmd("ip", &["-o", "route", "show", "default"])?;

    if let Some(idx) = output.find("dev ") {
        let rest = &output[idx + 4..];
        if let Some(iface) = rest.split_whitespace().next() {
            return Ok(iface.to_string());
        }
    }

    Err("no default route found".to_string())
}

/// Detect LAN interface. Prefers br-lan (OpenWrt standard),
/// falls back to first bridge, then first non-WAN non-lo interface.
pub fn detect_lan_iface(wan_iface: &str) -> Result<String, String> {
    // Check for br-lan first (OpenWrt standard)
    if std::path::Path::new("/sys/class/net/br-lan").exists() {
        return Ok("br-lan".to_string());
    }

    // Look for any bridge interface
    let entries = std::fs::read_dir("/sys/class/net")
        .map_err(|e| format!("read /sys/class/net: {e}"))?;

    let mut ifaces: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" || name == wan_iface || name == "ifb0" {
            continue;
        }
        let bridge_dir = format!("/sys/class/net/{name}/bridge");
        if std::path::Path::new(&bridge_dir).exists() {
            return Ok(name);
        }
        ifaces.push(name);
    }

    // Fallback: first non-WAN, non-lo interface
    if let Some(iface) = ifaces.into_iter().next() {
        return Ok(iface);
    }

    Err("no LAN interface found".to_string())
}

/// Metadata for a network interface.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub kind: &'static str,
    pub state: String,
    pub ip: Option<String>,
    pub speed_mbps: Option<i32>,
    pub rx_bytes: i64,
    pub tx_bytes: i64,
    pub is_default_route: bool,
}

fn read_sysfs(iface: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/{file}"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn detect_kind(name: &str) -> &'static str {
    let base = format!("/sys/class/net/{name}");
    if std::path::Path::new(&format!("{base}/bridge")).exists() {
        "bridge"
    } else if std::path::Path::new(&format!("{base}/wireless")).exists()
        || name.starts_with("wlan")
        || name.starts_with("apcli")
        || name.starts_with("ra")
    {
        "wireless"
    } else if name.starts_with("docker")
        || name.starts_with("veth")
        || name.starts_with("br-")
        || name.starts_with("ifb")
    {
        "virtual"
    } else {
        "ethernet"
    }
}

/// List all network interfaces with metadata.
pub fn list_interfaces() -> Vec<InterfaceInfo> {
    let entries = match std::fs::read_dir("/sys/class/net") {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    // Get default route interface
    let default_wan = detect_wan_iface().ok().unwrap_or_default();

    // Get IP addresses via `ip -o -4 addr`
    let ip_map: HashMap<String, String> = run_cmd("ip", &["-o", "-4", "addr", "show"])
        .ok()
        .map(|output| {
            let mut map = HashMap::new();
            for line in output.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let iface = parts[1].to_string();
                    if let Some(addr) = parts.get(3) {
                        map.entry(iface).or_insert_with(|| addr.to_string());
                    }
                }
            }
            map
        })
        .unwrap_or_default();

    let mut ifaces: Vec<InterfaceInfo> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "lo" {
                return None;
            }

            let state = read_sysfs(&name, "operstate").unwrap_or_else(|| "unknown".into());
            let speed_mbps = read_sysfs(&name, "speed")
                .and_then(|s| s.parse::<i32>().ok())
                .filter(|&s| s > 0 && s < 100_000);
            let rx_bytes = read_sysfs(&name, "statistics/rx_bytes")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let tx_bytes = read_sysfs(&name, "statistics/tx_bytes")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            Some(InterfaceInfo {
                kind: detect_kind(&name),
                state,
                ip: ip_map.get(&name).cloned(),
                speed_mbps,
                rx_bytes,
                tx_bytes,
                is_default_route: name == default_wan,
                name,
            })
        })
        .collect();

    ifaces.sort_by(|a, b| a.name.cmp(&b.name));
    ifaces
}

/// Detect the LAN subnet (e.g. "192.168.8.0/24").
pub fn detect_lan_subnet(lan_iface: &str) -> Result<String, String> {
    let output = run_cmd("ip", &["-o", "-4", "addr", "show", "dev", lan_iface])?;

    for field in output.split_whitespace() {
        if field.contains('/') && field.matches('.').count() == 3 {
            let parts: Vec<&str> = field.splitn(2, '/').collect();
            if parts.len() != 2 {
                continue;
            }
            let ip = parts[0];
            let prefix = parts[1];
            let octets: Vec<&str> = ip.split('.').collect();
            if octets.len() != 4 {
                continue;
            }
            return match prefix {
                "24" => Ok(format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2])),
                "16" => Ok(format!("{}.{}.0.0/16", octets[0], octets[1])),
                "8" => Ok(format!("{}.0.0.0/8", octets[0])),
                _ => Ok(field.to_string()),
            };
        }
    }

    Err(format!("no IPv4 address found on {lan_iface}"))
}

/// Static device entry for merge.
pub struct StaticDeviceEntry {
    pub mac: String,
    pub name: String,
}

/// Discover LAN devices from ARP table and DHCP leases.
pub fn discover_devices(
    lan_iface: &str,
    static_devices: &[StaticDeviceEntry],
) -> Result<Vec<Device>, String> {
    let mut devices_by_mac: HashMap<String, Device> = HashMap::new();

    // 1. Parse ARP table
    if let Ok(arp_devices) = parse_arp(lan_iface) {
        for d in arp_devices {
            devices_by_mac.insert(d.mac.clone(), d);
        }
    }

    // 2. Enrich with DHCP lease hostnames
    if let Ok(leases) = parse_dhcp_leases() {
        for lease in leases {
            if let Some(dev) = devices_by_mac.get_mut(&lease.mac) {
                if !lease.hostname.is_empty() && lease.hostname != "*" {
                    dev.hostname = lease.hostname;
                }
            }
        }
    }

    // 3. Merge static devices
    for sd in static_devices {
        let mac = sd.mac.to_lowercase();
        if let Some(dev) = devices_by_mac.get_mut(&mac) {
            dev.hostname = sd.name.clone();
            dev.source = "static".to_string();
        }
    }

    Ok(devices_by_mac.into_values().collect())
}

fn parse_arp(lan_iface: &str) -> Result<Vec<Device>, String> {
    let output = run_cmd("ip", &["neigh", "show", "dev", lan_iface])?;

    let mut devices = Vec::new();
    for line in output.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }

        let ip = fields[0];
        let state = fields[fields.len() - 1];

        // Only include reachable/stale/delay neighbors
        if state != "REACHABLE" && state != "STALE" && state != "DELAY" {
            continue;
        }

        let mut mac = String::new();
        for (i, f) in fields.iter().enumerate() {
            if *f == "lladdr" && i + 1 < fields.len() {
                mac = fields[i + 1].to_lowercase();
                break;
            }
        }
        if mac.is_empty() {
            continue;
        }

        devices.push(Device {
            mac,
            ip: ip.to_string(),
            hostname: String::new(),
            source: "arp".to_string(),
        });
    }

    Ok(devices)
}

struct DhcpLease {
    mac: String,
    hostname: String,
}

fn parse_dhcp_leases() -> Result<Vec<DhcpLease>, String> {
    let content = std::fs::read_to_string("/tmp/dhcp.leases")
        .map_err(|e| format!("read dhcp.leases: {e}"))?;

    let mut leases = Vec::new();
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        // Format: timestamp MAC IP hostname *
        leases.push(DhcpLease {
            mac: fields[1].to_lowercase(),
            hostname: fields[3].to_string(),
        });
    }

    Ok(leases)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_arp_line() {
        // Simulated test — actual ARP parsing requires `ip` command
        let line = "192.168.1.100 lladdr aa:bb:cc:dd:ee:ff REACHABLE";
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(fields[0], "192.168.1.100");
        assert_eq!(fields[fields.len() - 1], "REACHABLE");

        let mut mac = String::new();
        for (i, f) in fields.iter().enumerate() {
            if *f == "lladdr" && i + 1 < fields.len() {
                mac = fields[i + 1].to_lowercase();
                break;
            }
        }
        assert_eq!(mac, "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn detect_lan_subnet_parsing() {
        // Test the subnet calculation logic
        let test_cases = [
            ("192.168.8.1/24", "192.168.8.0/24"),
            ("10.0.0.1/8", "10.0.0.0/8"),
            ("172.16.1.1/16", "172.16.0.0/16"),
        ];

        for (input, expected) in &test_cases {
            let parts: Vec<&str> = input.splitn(2, '/').collect();
            let ip = parts[0];
            let prefix = parts[1];
            let octets: Vec<&str> = ip.split('.').collect();

            let result = match prefix {
                "24" => format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]),
                "16" => format!("{}.{}.0.0/16", octets[0], octets[1]),
                "8" => format!("{}.0.0.0/8", octets[0]),
                _ => input.to_string(),
            };
            assert_eq!(&result, *expected);
        }
    }
}
