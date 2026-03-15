# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
make build              # Build native binary to dist/slqm
make test               # Run all tests (cargo test)
make lint               # Run cargo clippy
make build-all          # Cross-compile for arm64, armv7, mipsle (requires `cross`)
make package            # Cross-compile + generate .ipk packages
```

Run a single test:
```bash
cargo test --bin slqm bucket::tests::hysteresis
```

All cross-compiled builds produce static musl binaries. Release profile uses `opt-level = "z"`, LTO, and strip for size optimization.

## Architecture

SLQM (Starlink Quota Manager) is a traffic shaper for OpenWrt routers that maps remaining monthly data quota to a sustained bandwidth limit using a power curve, with per-device token buckets for burst/fairness control. Written in Rust with tokio (single-threaded runtime).

### Crate Structure

```
src/
├── main.rs              # Entry point, signal handling, dual listeners
├── config.rs            # JSON config, validation, snapshot pattern
├── model.rs             # Shared types (serde-serializable)
├── store.rs             # redb persistence
├── engine/
│   ├── mod.rs           # Engine struct, tick loop, device management
│   ├── curve.rs         # Power curve calculation
│   ├── billing.rs       # Billing cycle logic
│   └── bucket.rs        # Token bucket with hysteresis
├── netctl/
│   ├── mod.rs           # Re-exports, run_cmd helpers
│   ├── tc.rs            # HTB qdisc management via tc CLI
│   ├── nftables.rs      # nftables table/chain/rule management
│   ├── counters.rs      # nftables counter parsing
│   ├── devices.rs       # ARP + DHCP device discovery
│   └── firewall.rs      # iptables port management
├── api/
│   ├── mod.rs           # axum router setup
│   ├── handlers.rs      # REST endpoint handlers
│   └── websocket.rs     # WebSocket with watch channel broadcast
├── dish.rs              # Starlink dish client (TCP probe)
└── web.rs               # rust-embed static file serving
```

### Core Data Flow (every 2-second tick)

```
nftables counters → compute per-device byte deltas (up+down combined)
  → drain device token buckets
  → recompute curve rate from remaining quota
  → refill buckets (shared pool / non-full device count)
  → evaluate mode per device (hysteresis: burst ↔ sustained)
  → update tc HTB classes on WAN (upload) + LAN (download)
  → broadcast state snapshot via tokio::sync::watch
```

### Dual-Tree Shaping

Linux tc only shapes egress. Upload shaping uses an HTB tree on the WAN interface. Download shaping uses an HTB tree on the LAN interface (br-lan), where forwarded packets have already been marked by nftables. The LAN tree has a high-rate root (1 Gbps) with an unmatched default class for local/inter-LAN traffic, and a rate-limited download parent class (1:3) at the curve rate containing per-device classes. nftables marks packets by IP in two chains (upload by src IP, download by dst IP) with the same mark value, routing to the corresponding tc classes.

### Device Modes

Each device has a token bucket with **dynamic capacity** (`curve_rate_bps × bucket_duration`). Mode transitions use hysteresis (dead zone between shape/unshape thresholds) to prevent flapping:

- **Burst**: tokens available → ceiling = `tokens × burst_drain_ratio / tick` (proportional to bucket size)
- **Sustained**: tokens depleted → ceiling = fair share split 80/20 down/up
- **Turbo**: manual override → uncapped, time-limited, auto-expires

### State Management

`Engine` uses `Arc<RwLock<EngineInner>>` shared between the engine tick loop and axum API handlers. The engine loop takes write locks during tick/scan. API handlers take read locks for snapshots, write locks for mutations. Config uses a snapshot pattern: `cfg.snapshot()` returns a `Values` clone safe to pass around.

### Persistence

redb (pure Rust, no C deps) stores quota state (`month_used`, `billing_month`), per-device cycle bytes, and history snapshots. Saved every 60s and on shutdown. On startup, if the billing cycle has rolled over, usage resets to zero.

### Web UI

Vanilla JS, single HTML file, dark theme, all embedded via `rust-embed`. No frameworks, no CDN dependencies. WebSocket receives full state snapshot on each engine tick via `tokio::sync::watch`.

## Conventions

- Network commands (`tc`, `nft`, `ip`) are called via `std::process::Command`, not netlink libraries
- WAN/LAN interfaces default to `"auto"` and are detected at startup from the default route and bridge devices
- Device marks start at 100 (mark = 100 + slot); tc class IDs start at 1:10
- Negative counter deltas (counter reset) are treated as 0
- Config validation enforces ranges (e.g., `curve_shape` 0.10–2.00, `billing_reset_day` 1–28)
- The `down_up_ratio` (default 0.80) splits bandwidth asymmetrically only when shaping is active
- CLI flags: `--config`, `--db`, `--version`
- Default paths: `/etc/slqm/config.json`, `/var/lib/slqm/state.db`
