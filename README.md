# slqm — Starlink Quota Manager

Smart traffic shaper for OpenWrt routers that makes your Starlink aviation quota last. Maps remaining monthly data to a sustained bandwidth limit using a power curve, with per-device token buckets for burst allowance and fairness.

**Problem:** Starlink aviation plans provide 20 GB/month at $250 with $10/GB overage. There's no built-in traffic management — once you exceed the cap, overage charges accrue silently.

**Solution:** slqm shapes traffic so the quota lasts the full billing cycle. Devices get full link speed for bursty workloads (weather radar, chart downloads) and fair-shared bandwidth when sustained. A web dashboard shows quota status, per-device usage, and lets you adjust everything in real time.

## How It Works

The shaper uses a power curve to compute a sustained aggregate rate from remaining quota:

```
sustained_rate = min_rate + (max_rate - min_rate) × (remaining / total) ^ shape
```

Each device gets a token bucket whose capacity shrinks with the curve. When a device has tokens, it can burst (up to a ceiling proportional to its remaining tokens). When tokens are depleted, it gets a fair share of the curve rate. This happens independently per device — one passenger streaming doesn't starve the pilot's EFB.

Both upload and download count toward the quota (matching Starlink's metering). Upload is shaped on the WAN interface egress, download is shaped on the LAN interface egress (where nftables marks are already applied).

## Quick Start

### Install on a GL.iNet router

```bash
# Copy the .ipk to the router
scp -O slqm_*.ipk root@192.168.8.1:/tmp/

# SSH in and install
ssh root@192.168.8.1
opkg update
opkg install nftables
opkg install /tmp/slqm_*.ipk
```

slqm auto-detects WAN and LAN interfaces on startup. Open `http://<router-ip>:8275` to access the dashboard.

### Build from source

```bash
git clone https://github.com/a-kazakov/openwrt-shaper.git
cd openwrt-shaper

make build          # Native binary → dist/slqm
make build-all      # Cross-compile arm64, armv7, mipsle
make package        # Generate .ipk packages
make test           # Run tests with race detector
```

Requires Go 1.22+. All builds are static (`CGO_ENABLED=0`).

## Supported Routers

| Router | Architecture | Binary |
|---|---|---|
| GL-MT3000 (Beryl AX) | arm64 | `slqm-arm64` |
| GL-MT2500 (Brume 2) | arm64 | `slqm-arm64` |
| GL-MT1300 (Beryl) | armv7 | `slqm-armv7` |
| GL-AR750S (Slate) | mipsle | `slqm-mipsle` |
| GL-A1300 (Slate Plus) | armv7 | `slqm-armv7` |

## Configuration

Config lives at `/etc/slqm/config.json`. All parameters are also editable from the web UI and take effect immediately.

| Parameter | Default | Description |
|---|---|---|
| `monthly_quota_gb` | 20 | Monthly data cap in GB |
| `billing_reset_day` | 1 | Day of month quota resets (1–28) |
| `curve_shape` | 0.40 | Power exponent — lower keeps speed high longer |
| `max_rate_kbit` | 50000 | Max sustained rate (50 Mbps) |
| `min_rate_kbit` | 1000 | Floor rate (1 Mbps) |
| `down_up_ratio` | 0.80 | Download share when shaped (80/20 split) |
| `bucket_duration_sec` | 300 | Burst budget window (5 minutes) |
| `burst_drain_ratio` | 0.10 | Max bucket fraction consumable per tick |
| `wan_iface` | auto | WAN interface (auto-detected from default route) |
| `lan_iface` | auto | LAN interface (auto-detected, prefers br-lan) |
| `dish_addr` | 192.168.100.1:9200 | Starlink dish gRPC address |
| `listen_addr` | :8275 | Web UI listen address |

## Web UI

The dashboard runs at `http://<router-ip>:8275` with live updates via WebSocket:

- **Quota overview** — used/remaining GB, billing cycle, estimated time remaining
- **Rate curve chart** — interactive SVG showing current position on the power curve
- **Device table** — per-device speed, bucket fill, mode (burst/sustained/turbo), session bytes
- **Throughput sparkline** — real-time aggregate bandwidth (upload/download split)
- **Turbo mode** — per-device toggle to bypass all shaping for a configurable duration
- **Manual adjustments** — sync with Starlink app, +/- quota, reset billing cycle

The UI is embedded in the binary — no internet connectivity required.

## API

All endpoints under `/api/v1/`:

| Method | Endpoint | Description |
|---|---|---|
| `GET` | `/api/v1/state` | Full state snapshot |
| `GET` | `/api/v1/config` | Current configuration |
| `PUT` | `/api/v1/config` | Update configuration |
| `POST` | `/api/v1/sync` | Sync quota with Starlink `{"starlink_used_gb": 12.5}` |
| `POST` | `/api/v1/quota/adjust` | Adjust quota `{"delta_bytes": 1073741824}` |
| `POST` | `/api/v1/quota/reset` | Reset billing cycle |
| `POST` | `/api/v1/device/{mac}/turbo` | Enable turbo `{"duration_min": 15}` |
| `DELETE` | `/api/v1/device/{mac}/turbo` | Cancel turbo |
| `GET` | `/ws` | WebSocket for live state (1-second push) |

## Network Setup

**Recommended: Starlink bypass mode.** Put the dish in bypass/passthrough so the GL.iNet handles all routing with a single NAT hop. slqm automatically adds a static route to the dish subnet for gRPC access.

**Standard router mode** also works — the GL.iNet runs its own DHCP/NAT behind the dish (double NAT). Fine for web browsing, weather data, EFB updates, and messaging.

## Kernel Dependencies

These are installed automatically by the `.ipk` package:

- `nftables` — packet marking and per-device byte counters

HTB qdiscs are typically built into the kernel on GL.iNet firmware.

## License

MIT
