# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
make build              # Build native binary to dist/slqm
make test               # Run all tests with race detector
make lint               # Run go vet
make build-all          # Cross-compile for arm64, armv7, mipsle
make package            # Cross-compile + generate .ipk packages
make test-cover         # Generate HTML coverage report
```

Run a single test:
```bash
go test -race -run TestBucketHysteresis ./internal/engine/
```

All builds use `CGO_ENABLED=0` for static binaries. MIPS targets use `GOMIPS=softfloat`.

## Architecture

SLQM (Starlink Quota Manager) is a traffic shaper for OpenWrt routers that maps remaining monthly data quota to a sustained bandwidth limit using a power curve, with per-device token buckets for burst/fairness control.

### Core Data Flow (every 2-second tick)

```
nftables counters → compute per-device byte deltas (up+down combined)
  → drain device token buckets
  → recompute curve rate from remaining quota
  → refill buckets (shared pool / non-full device count)
  → evaluate mode per device (hysteresis: burst ↔ sustained)
  → update tc HTB classes on WAN (upload) + IFB (download)
  → broadcast state snapshot via WebSocket
```

### Dual-Tree Shaping

Linux tc only shapes egress. Download shaping uses an IFB (Intermediate Functional Block) device: WAN ingress is redirected to IFB egress via mirred, then shaped by a second HTB tree. Both trees use the same slot/mark numbering. nftables marks packets by IP in two chains (upload by src IP, download by dst IP) with the same mark value, routing to mirrored tc classes.

### Device Modes

Each device has a token bucket with **dynamic capacity** (`curve_rate_bps × bucket_duration`). Mode transitions use hysteresis (dead zone between shape/unshape thresholds) to prevent flapping:

- **Burst**: tokens available → ceiling = `tokens × burst_drain_ratio / tick` (proportional to bucket size)
- **Sustained**: tokens depleted → ceiling = fair share split 80/20 down/up
- **Turbo**: manual override → uncapped, time-limited, auto-expires

### Key Interfaces

The `api.Engine` interface decouples HTTP handlers from the engine. All engine public methods are mutex-protected. Config uses a snapshot pattern: `cfg.Snapshot()` returns a `Values` struct (no mutex) safe to pass around.

### Persistence

bbolt stores quota state (`month_used`, `billing_month`), per-device cycle bytes, and history snapshots. Saved every 60s and on shutdown. On startup, if the billing cycle has rolled over, usage resets to zero.

### Web UI

Vanilla JS, single HTML file, dark theme, all embedded via `//go:embed`. No frameworks, no CDN dependencies. WebSocket receives full state snapshot every second.

## Conventions

- Network commands (`tc`, `nft`, `ip`) are called via `os/exec`, not netlink libraries
- WAN/LAN interfaces default to `"auto"` and are detected at startup from the default route and bridge devices
- Device marks start at 100 (mark = 100 + slot); tc class IDs start at 1:10
- Negative counter deltas (counter reset) are treated as 0
- Config validation enforces ranges (e.g., `curve_shape` 0.10–2.00, `billing_reset_day` 1–28)
- The `down_up_ratio` (default 0.80) splits bandwidth asymmetrically only when shaping is active
