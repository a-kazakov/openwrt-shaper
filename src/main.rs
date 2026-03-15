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
use tracing::{error, info};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "slqm", about = "Starlink Quota Manager")]
struct Args {
    /// Path to config file
    #[arg(long, default_value = "/etc/slqm/config.json")]
    config: String,

    /// Path to database file
    #[arg(long, default_value = "/var/lib/slqm/state.db")]
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

    // Auto-detect interfaces
    let mut wan = snap.wan_iface.clone();
    let mut lan = snap.lan_iface.clone();

    if wan == "auto" {
        match netctl::devices::detect_wan_iface() {
            Ok(iface) => {
                info!("detected WAN interface: {iface}");
                wan = iface;
            }
            Err(e) => {
                error!("failed to detect WAN interface: {e}");
                std::process::exit(1);
            }
        }
    }
    if lan == "auto" {
        match netctl::devices::detect_lan_iface(&wan) {
            Ok(iface) => {
                info!("detected LAN interface: {iface}");
                lan = iface;
            }
            Err(e) => {
                error!("failed to detect LAN interface: {e}");
                std::process::exit(1);
            }
        }
    }
    cfg.resolve_ifaces(&wan, &lan);

    // Open database
    let db_path = Path::new(&args.db);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let store = match store::Store::open(db_path) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!("failed to open database: {e}");
            std::process::exit(1);
        }
    };

    // Create engine
    let engine = engine::Engine::new(cfg.clone(), store);

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

    // Start HTTP servers (separate IPv4 and IPv6)
    let ipv4_addr = listen_addr.clone();
    let ipv6_addr = listen_addr.replace("0.0.0.0", "[::]");

    info!("starting HTTP server on {ipv4_addr} and {ipv6_addr}");

    let app4 = app.clone();
    let ipv4_server = tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&ipv4_addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("failed to bind IPv4 {ipv4_addr}: {e}");
                return;
            }
        };
        info!("IPv4 listener ready on {ipv4_addr}");
        if let Err(e) = axum::serve(listener, app4).await {
            error!("IPv4 server error: {e}");
        }
    });

    let ipv6_server = tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&ipv6_addr).await {
            Ok(l) => l,
            Err(e) => {
                // IPv6 may not be available, don't fail
                info!("IPv6 bind failed (non-fatal): {e}");
                return;
            }
        };
        info!("IPv6 listener ready on {ipv6_addr}");
        if let Err(e) = axum::serve(listener, app).await {
            error!("IPv6 server error: {e}");
        }
    });

    // Wait for shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("received SIGINT, shutting down");
        }
    }

    // Signal shutdown to all tasks
    let _ = shutdown_tx.send(true);

    // Close firewall port
    netctl::firewall::close_firewall_port(port);

    info!("slqm shutdown complete");

    // Abort server tasks
    ipv4_server.abort();
    ipv6_server.abort();
}
