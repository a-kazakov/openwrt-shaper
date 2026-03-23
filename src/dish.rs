use crate::model::DishStatus;
use crate::netctl::run_cmd;
use starlink::proto::space_x::api::device::{
    device_client::DeviceClient, request, DishState, GetStatusRequest, Request,
};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::{info, warn};

/// Starlink dish client — queries gRPC API for real telemetry.
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
        let host = self.addr.split(':').next().unwrap_or(&self.addr);

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

    /// Query dish via gRPC for real telemetry. Falls back to unreachable status on failure.
    pub async fn poll(&self) {
        let host = self.addr.split(':').next().unwrap_or(&self.addr);
        let grpc_url = format!("http://{host}:9200");

        let was_reachable = self
            .status
            .read()
            .unwrap()
            .as_ref()
            .map(|s| s.reachable);

        // Connect with timeout to avoid blocking the poller
        let connect_result = tokio::time::timeout(
            Duration::from_secs(5),
            DeviceClient::connect(grpc_url.clone()),
        )
        .await;

        let mut client = match connect_result {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                if was_reachable != Some(false) {
                    warn!("dish: gRPC connect failed ({grpc_url}): {e}");
                }
                self.set_unreachable();
                return;
            }
            Err(_) => {
                if was_reachable != Some(false) {
                    warn!("dish: gRPC connect timeout ({grpc_url})");
                }
                self.set_unreachable();
                return;
            }
        };

        let request = tonic::Request::new(Request {
            id: None,
            epoch_id: None,
            target_id: None,
            request: Some(request::Request::GetStatus(GetStatusRequest {})),
        });

        let response = match tokio::time::timeout(
            Duration::from_secs(5),
            client.handle(request),
        )
        .await
        {
            Ok(Ok(r)) => r.into_inner(),
            Ok(Err(e)) => {
                if was_reachable != Some(false) {
                    warn!("dish: gRPC request failed: {e}");
                }
                self.set_unreachable();
                return;
            }
            Err(_) => {
                if was_reachable != Some(false) {
                    warn!("dish: gRPC request timeout");
                }
                self.set_unreachable();
                return;
            }
        };

        // Extract dish status from gRPC response
        let dish_status = response.response.and_then(|r| {
            if let starlink::proto::space_x::api::device::response::Response::DishGetStatus(s) = r
            {
                Some(s)
            } else {
                None
            }
        });

        let Some(ds) = dish_status else {
            warn!("dish: unexpected response (no dish_get_status)");
            self.set_unreachable();
            return;
        };

        if was_reachable != Some(true) {
            info!("dish: connected via gRPC at {grpc_url}");
        }

        let uptime = ds
            .device_state
            .as_ref()
            .and_then(|s| s.uptime_s)
            .unwrap_or(0) as i64;

        let software_version = ds
            .device_info
            .as_ref()
            .and_then(|i| i.software_version.clone())
            .unwrap_or_default();

        let connected = ds
            .state
            .map(|s| s == DishState::Connected as i32)
            .unwrap_or(false);

        let snr = ds.snr.unwrap_or(0.0);
        let obstruction_stats = ds.obstruction_stats;
        let (obstructed, fraction_obstructed) = obstruction_stats
            .as_ref()
            .map(|o| {
                (
                    o.currently_obstructed.unwrap_or(false),
                    o.fraction_obstructed.unwrap_or(0.0),
                )
            })
            .unwrap_or((false, 0.0));

        let status = DishStatus {
            reachable: true,
            connected,
            uptime,
            downlink_bps: ds.downlink_throughput_bps.unwrap_or(0.0) as f64,
            uplink_bps: ds.uplink_throughput_bps.unwrap_or(0.0) as f64,
            pop_ping_latency_ms: ds.pop_ping_latency_ms.unwrap_or(0.0) as f64,
            signal_quality: snr as f64,
            obstructed,
            fraction_obstructed: fraction_obstructed as f64,
            software_version,
        };

        *self.status.write().unwrap() = Some(status);
    }

    fn set_unreachable(&self) {
        let mut guard = self.status.write().unwrap();
        // Preserve previous data but mark as unreachable
        if let Some(ref mut s) = *guard {
            s.reachable = false;
            s.connected = false;
        } else {
            *guard = Some(DishStatus {
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
            });
        }
    }

    /// Get the last known dish status.
    pub fn status(&self) -> Option<DishStatus> {
        self.status.read().unwrap().clone()
    }
}
