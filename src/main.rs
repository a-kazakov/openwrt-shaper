pub mod api;
pub mod config;
pub mod dish;
pub mod engine;
pub mod model;
pub mod netctl;
pub mod store;
pub mod web;

use clap::Parser;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check if a network interface exists in sysfs.
fn iface_exists(name: &str) -> bool {
    std::path::Path::new(&format!("/sys/class/net/{name}")).exists()
}

/// Resolve an interface name: if explicit and exists, use it; if explicit but missing,
/// warn and fall back to auto-detection; if "auto", auto-detect.
/// Returns (resolved_name, fallback_warning_message_or_none).
fn resolve_interface<F>(configured: &str, role: &str, detect: F) -> (String, Option<String>)
where
    F: FnOnce() -> Result<String, String>,
{
    if configured != "auto" && iface_exists(configured) {
        info!("{role} interface: {configured} (configured)");
        return (configured.to_string(), None);
    }

    if configured != "auto" {
        warn!(
            "{role} interface {configured} does not exist, falling back to auto-detection"
        );
    }

    match detect() {
        Ok(iface) => {
            info!("{role} interface: {iface} (auto-detected)");
            let warning = if configured != "auto" {
                Some(format!(
                    "{role} interface '{configured}' does not exist, using auto-detected '{iface}'"
                ))
            } else {
                None
            };
            (iface, warning)
        }
        Err(e) => {
            if configured != "auto" {
                // Explicit interface doesn't exist and auto-detection failed too.
                // Use the configured name anyway — it may appear later (e.g. repeater
                // connecting) and check_interface_change() will pick it up.
                warn!(
                    "{role} auto-detection failed ({e}), using configured {configured} (may not exist yet)"
                );
                let warning = format!(
                    "{role} interface '{configured}' does not exist and auto-detection failed"
                );
                (configured.to_string(), Some(warning))
            } else {
                error!("failed to detect {role} interface: {e}");
                std::process::exit(1);
            }
        }
    }
}

#[derive(Parser)]
#[command(name = "slqm", about = "Starlink Quota Manager")]
struct Args {
    /// Path to config file
    #[arg(long, default_value = "/etc/slqm/config.json")]
    config: String,

    /// Path to database file (must be on persistent storage, not tmpfs)
    #[arg(long, default_value = "/etc/slqm/state.db")]
    db: String,

    /// Print version and exit
    #[arg(long)]
    version: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    if args.version {
        println!("slqm v{VERSION}");
        return;
    }

    info!("slqm v{VERSION} starting");

    // Load config
    let cfg = match config::Config::load(&args.config) {
        Ok(c) => c,
        Err(e) => {
            error!("failed to load config: {e}");
            std::process::exit(1);
        }
    };
    cfg.set_file_path(&args.config);

    let snap = cfg.snapshot();
    info!(
        "config: quota={}GB, shape={:.2}, rate={}-{}kbit, listen={}",
        snap.monthly_quota_gb, snap.curve_shape, snap.min_rate_kbit, snap.max_rate_kbit,
        snap.listen_addr
    );

    // Detect interfaces.
    // If an explicit interface is configured but doesn't exist (e.g. GL-Inet "sta"
    // interface only present when repeater is active), fall back to auto-detection
    // instead of crashing.
    let (wan, wan_warning) = resolve_interface(
        &snap.wan_iface,
        "WAN",
        netctl::devices::detect_wan_iface,
    );
    let (lan, lan_warning) = resolve_interface(
        &snap.lan_iface,
        "LAN",
        || netctl::devices::detect_lan_iface(&wan),
    );
    cfg.resolve_ifaces(&wan, &lan);

    // Open database
    let db_path = Path::new(&args.db);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let store = match store::Store::open(db_path) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            warn!("database corrupt or incompatible, recreating: {e}");
            if let Err(rm_err) = std::fs::remove_file(db_path) {
                error!("failed to remove corrupt database: {rm_err}");
                std::process::exit(1);
            }
            match store::Store::open(db_path) {
                Ok(s) => Arc::new(s),
                Err(e2) => {
                    error!("failed to create new database: {e2}");
                    std::process::exit(1);
                }
            }
        }
    };

    // Create engine
    let engine = engine::Engine::new(cfg.clone(), store);

    // Set interface fallback warnings from startup
    if let Some(msg) = wan_warning {
        engine.add_interface_warning(msg);
    }
    if let Some(msg) = lan_warning {
        engine.add_interface_warning(msg);
    }

    // Setup network (nftables + tc)
    if let Err(e) = engine.setup() {
        error!("engine setup failed: {e}");
        std::process::exit(1);
    }

    // Open firewall port
    let port = netctl::firewall::extract_port(&snap.listen_addr);
    netctl::firewall::open_firewall_port(port);

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Start dish poller
    let dish_client = dish::DishClient::new(&snap.dish_addr, &wan);
    let dish_engine = engine.clone();
    let dish_shutdown = shutdown_rx.clone();
    let dish_interval = std::time::Duration::from_secs(snap.dish_poll_interval_sec as u64);
    tokio::spawn(async move {
        let poller_shutdown = dish_shutdown.clone();
        let dish_client_ref = &dish_client;
        let engine_ref = &dish_engine;

        // Run poller in background, updating engine with dish status
        let mut timer = tokio::time::interval(dish_interval);
        let mut shutdown = poller_shutdown;

        dish_client_ref.ensure_route();
        dish_client_ref.poll();

        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = timer.tick() => {
                    dish_client_ref.poll();
                    engine_ref.set_dish_status(dish_client_ref.status());
                }
            }
        }
    });

    // Start engine loop
    let engine_shutdown = shutdown_rx.clone();
    let engine_clone = engine.clone();
    tokio::spawn(async move {
        engine_clone.run(engine_shutdown).await;
    });

    // Build axum router
    let app = api::router(engine);

    // Parse listen address
    let listen_addr = if snap.listen_addr.starts_with(':') {
        format!("0.0.0.0{}", snap.listen_addr)
    } else {
        snap.listen_addr.clone()
    };

    // Start HTTP server (IPv4 only)
    info!("starting HTTP server on {listen_addr}");

    let server = tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&listen_addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("failed to bind {listen_addr}: {e}");
                return;
            }
        };
        info!("listening on {listen_addr}");
        if let Err(e) = axum::serve(listener, app).await {
            error!("server error: {e}");
        }
    });

    // Wait for shutdown signal (SIGINT or SIGTERM)
    // OpenWrt's procd sends SIGTERM on service stop / reboot.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received SIGINT, shutting down");
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received SIGINT, shutting down");
            }
        }
    }

    // Signal shutdown to all tasks
    let _ = shutdown_tx.send(true);

    // Close firewall port
    netctl::firewall::close_firewall_port(port);

    info!("slqm shutdown complete");

    server.abort();
}
