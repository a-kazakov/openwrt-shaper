use super::run_cmd_ignore;
use tracing::info;

/// Add iptables INPUT ACCEPT rules for the given TCP port.
/// Needed on OpenWrt where fw3/iptables blocks non-standard ports.
pub fn open_firewall_port(port: &str) {
    info!("firewall: opening TCP port {port}");
    run_cmd_ignore(
        "iptables",
        &["-I", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT"],
    );
    run_cmd_ignore(
        "ip6tables",
        &["-I", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT"],
    );
}

/// Remove the iptables INPUT ACCEPT rules for the given TCP port.
pub fn close_firewall_port(port: &str) {
    info!("firewall: closing TCP port {port}");
    run_cmd_ignore(
        "iptables",
        &["-D", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT"],
    );
    run_cmd_ignore(
        "ip6tables",
        &["-D", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT"],
    );
}

/// Extract the port portion of a host:port address.
pub fn extract_port(addr: &str) -> &str {
    match addr.rfind(':') {
        Some(i) => &addr[i + 1..],
        None => addr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_port() {
        assert_eq!(extract_port(":8275"), "8275");
        assert_eq!(extract_port("0.0.0.0:8275"), "8275");
        assert_eq!(extract_port("8275"), "8275");
    }
}
