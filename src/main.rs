pub mod config;
pub mod engine;
pub mod model;
pub mod netctl;
pub mod store;

fn main() {
    let cfg = config::Config::default_config();
    let snap = cfg.snapshot();
    println!("slqm v0.1.0 — Starlink Quota Manager (Rust)");
    println!(
        "config: quota={}GB, shape={:.2}, rate={}-{}kbit",
        snap.monthly_quota_gb, snap.curve_shape, snap.min_rate_kbit, snap.max_rate_kbit
    );
}
