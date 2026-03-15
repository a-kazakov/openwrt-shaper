#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION=$(git describe --tags --always 2>/dev/null || echo "1.0.0")
PKG_DIR="dist/pkg"

declare -A ARCH_MAP=(
    ["arm64"]="aarch64_cortex-a53"
    ["armv7"]="arm_cortex-a7_neon-vfpv4"
    ["mipsle"]="mipsel_24kc"
)

for suffix in arm64 armv7 mipsle; do
    binary="dist/slqm-${suffix}"
    [ -f "$binary" ] || continue

    arch="${ARCH_MAP[$suffix]}"
    pkg_name="slqm_${VERSION}_${arch}"
    work="${PKG_DIR}/${pkg_name}"

    echo "Packaging ${pkg_name}..."

    rm -rf "$work"
    mkdir -p "$work/data/usr/bin"
    mkdir -p "$work/data/etc/slqm"
    mkdir -p "$work/data/etc/init.d"
    mkdir -p "$work/control"

    cp "$binary" "$work/data/usr/bin/slqm"
    chmod 755 "$work/data/usr/bin/slqm"

    # Default config
    cat > "$work/data/etc/slqm/config.json" << 'CFGEOF'
{
  "network_mode": "router",
  "wan_iface": "auto",
  "lan_iface": "auto",
  "ifb_iface": "ifb0",
  "dish_addr": "192.168.100.1:9200",
  "dish_poll_interval_sec": 30,
  "listen_addr": "0.0.0.0:8275",
  "billing_reset_day": 1,
  "monthly_quota_gb": 20,
  "curve_shape": 0.40,
  "max_rate_kbit": 50000,
  "min_rate_kbit": 1000,
  "down_up_ratio": 0.80,
  "bucket_duration_sec": 300,
  "burst_drain_ratio": 0.10,
  "tick_interval_sec": 2,
  "save_interval_sec": 60,
  "device_scan_interval_sec": 15,
  "overage_cost_per_gb": 10.0,
  "plan_cost_monthly": 250.0
}
CFGEOF

    # Init script
    cat > "$work/data/etc/init.d/slqm" << 'INITEOF'
#!/bin/sh /etc/rc.common
START=99
STOP=10
USE_PROCD=1

start_service() {
    procd_open_instance
    procd_set_param command /usr/bin/slqm -config /etc/slqm/config.json
    procd_set_param respawn 3600 5 5
    procd_set_param term_timeout 5
    procd_set_param stdout 1
    procd_set_param stderr 1
    procd_set_param file /etc/slqm/config.json
    procd_close_instance
}
INITEOF
    chmod 755 "$work/data/etc/init.d/slqm"

    # Control file
    cat > "$work/control/control" << CTLEOF
Package: slqm
Version: ${VERSION}
Architecture: ${arch}
Maintainer: Artem Kazakov <opensource@akazakov.net>
Section: net
Description: Starlink Quota Manager - smart traffic shaping with per-device byte buckets
Depends: nftables
CTLEOF

    # Conffiles
    echo "/etc/slqm/config.json" > "$work/control/conffiles"

    # Post-install
    cat > "$work/control/postinst" << 'POSTEOF'
#!/bin/sh
mkdir -p /var/lib/slqm
/etc/init.d/slqm enable
/etc/init.d/slqm start
exit 0
POSTEOF
    chmod 755 "$work/control/postinst"

    # Pre-remove
    cat > "$work/control/prerm" << 'PRERMEOF'
#!/bin/sh
/etc/init.d/slqm stop 2>/dev/null
# Wait for Go signal handler to clean up
sleep 1
# Fallback cleanup in case the process didn't clean up properly
WAN=$(ip -o route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | head -1)
[ -z "$WAN" ] && WAN="eth0"
# Clean up WAN (upload) shaping
tc qdisc del dev "$WAN" root 2>/dev/null
tc qdisc del dev "$WAN" ingress 2>/dev/null
# Clean up LAN (download) shaping — try common LAN interfaces
for LAN in br-lan br0 lan0; do
    tc qdisc del dev "$LAN" root 2>/dev/null
done
# Clean up legacy IFB from older versions
tc qdisc del dev ifb0 root 2>/dev/null
ip link del ifb0 2>/dev/null
# Clean up nftables
nft delete table inet slqm 2>/dev/null
# Clean up firewall (fallback + legacy includes)
PORT=$(grep listen_addr /etc/slqm/config.json 2>/dev/null | grep -o '[0-9]*"' | tr -d '"')
[ -n "$PORT" ] && iptables -D INPUT -p tcp --dport "$PORT" -j ACCEPT 2>/dev/null
rm -f /etc/firewall.slqm
uci -q delete firewall.slqm_include 2>/dev/null
uci -q delete firewall.slqm_web 2>/dev/null
uci commit firewall 2>/dev/null
exit 0
PRERMEOF
    chmod 755 "$work/control/prerm"

    # Build .ipk (tar.gz based)
    (cd "$work/data" && tar czf ../data.tar.gz .)
    (cd "$work/control" && tar czf ../control.tar.gz .)
    echo "2.0" > "$work/debian-binary"
    (cd "$work" && tar czf "../${pkg_name}.ipk" debian-binary control.tar.gz data.tar.gz)

    echo "  → dist/pkg/${pkg_name}.ipk"
done

echo "Packaging complete."
