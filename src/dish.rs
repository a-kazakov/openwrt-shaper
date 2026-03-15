use crate::model::DishStatus;
use crate::netctl::run_cmd;
use std::net::TcpStream;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::warn;

/// Starlink dish client — probes TCP connectivity.
pub struct DishClient {
    addr: String,
    wan_iface: String,
    status: Arc<RwLock<Option<DishStatus>>>,
}

impl DishClient {
    pub fn new(addr: &str, wan_iface: &str) -> Self {
        Self {
            addr: addr.to_string(),
            wan_iface: wan_iface.to_string(),
            status: Arc::new(RwLock::new(None)),
        }
    }

    /// Add a static route to the dish's subnet via the WAN interface.
    pub fn ensure_route(&self) {
        let host = self
            .addr
            .split(':')
            .next()
            .unwrap_or(&self.addr);

        let octets: Vec<&str> = host.split('.').collect();
        if octets.len() != 4 {
            warn!("dish: invalid IP: {host}");
            return;
        }
        let subnet = format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]);

        if let Err(e) =
            run_cmd("ip", &["route", "replace", &subnet, "dev", &self.wan_iface])
        {
            warn!("dish: route setup: {e}");
        }
    }

    /// Test TCP connectivity to the dish.
    pub fn poll(&self) -> Option<DishStatus> {
        let addr = if self.addr.contains(':') {
            self.addr.clone()
        } else {
            format!("{}:9200", self.addr)
        };

        match TcpStream::connect_timeout(
            &addr.parse().unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
            Duration::from_secs(3),
        ) {
            Ok(_) => {
                let status = DishStatus {
                    reachable: true,
                    connected: true,
                    uptime: 0,
                    downlink_bps: 0.0,
                    uplink_bps: 0.0,
                    pop_ping_latency_ms: 0.0,
                    signal_quality: 0.0,
                    obstructed: false,
                    fraction_obstructed: 0.0,
                    software_version: String::new(),
                    usage_down: 0,
                    usage_up: 0,
                };
                *self.status.write().unwrap() = Some(status.clone());
                Some(status)
            }
            Err(_) => {
                let status = DishStatus {
                    reachable: false,
                    connected: false,
                    uptime: 0,
                    downlink_bps: 0.0,
                    uplink_bps: 0.0,
                    pop_ping_latency_ms: 0.0,
                    signal_quality: 0.0,
                    obstructed: false,
                    fraction_obstructed: 0.0,
                    software_version: String::new(),
                    usage_down: 0,
                    usage_up: 0,
                };
                *self.status.write().unwrap() = Some(status);
                None
            }
        }
    }

    /// Get the last known dish status.
    pub fn status(&self) -> Option<DishStatus> {
        self.status.read().unwrap().clone()
    }

}
