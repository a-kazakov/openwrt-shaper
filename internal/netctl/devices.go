package netctl

import (
	"bufio"
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/akazakov/openwrt-shaper/internal/model"
)

// DetectWANIface returns the WAN interface by finding the default route device.
func DetectWANIface() (string, error) {
	out, err := exec.Command("ip", "-o", "route", "show", "default").CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("ip route: %w", err)
	}
	// Format: default via 10.0.0.1 dev eth0 ...
	for _, field := range strings.Fields(string(out)) {
		if field == "dev" {
			continue
		}
		// The field after "dev" is the interface name
		idx := strings.Index(string(out), "dev ")
		if idx >= 0 {
			rest := strings.Fields(string(out)[idx+4:])
			if len(rest) > 0 {
				return rest[0], nil
			}
		}
		break
	}
	return "", fmt.Errorf("no default route found")
}

// DetectLANIface returns the LAN interface. Prefers br-lan (OpenWrt standard),
// falls back to the first bridge, then first non-WAN non-lo interface.
func DetectLANIface(wanIface string) (string, error) {
	// Check for br-lan first (OpenWrt standard)
	if _, err := os.Stat("/sys/class/net/br-lan"); err == nil {
		return "br-lan", nil
	}

	// Look for any bridge interface
	entries, err := os.ReadDir("/sys/class/net")
	if err != nil {
		return "", fmt.Errorf("read /sys/class/net: %w", err)
	}

	for _, e := range entries {
		name := e.Name()
		if name == "lo" || name == wanIface || name == "ifb0" {
			continue
		}
		bridgeDir := "/sys/class/net/" + name + "/bridge"
		if _, err := os.Stat(bridgeDir); err == nil {
			return name, nil
		}
	}

	// Fallback: first non-WAN, non-lo interface
	for _, e := range entries {
		name := e.Name()
		if name == "lo" || name == wanIface || name == "ifb0" {
			continue
		}
		return name, nil
	}

	return "", fmt.Errorf("no LAN interface found")
}

// DiscoverDevices finds LAN devices from ARP table and DHCP leases.
func DiscoverDevices(lanIface string, staticDevices []struct{ MAC, Name string }) ([]model.Device, error) {
	devicesByMAC := make(map[string]*model.Device)

	// 1. Parse ARP table
	arpDevices, err := parseARP(lanIface)
	if err == nil {
		for _, d := range arpDevices {
			devicesByMAC[d.MAC] = &d
		}
	}

	// 2. Enrich with DHCP lease hostnames
	leases, err := parseDHCPLeases()
	if err == nil {
		for _, l := range leases {
			if dev, ok := devicesByMAC[l.MAC]; ok {
				if l.Hostname != "" && l.Hostname != "*" {
					dev.Hostname = l.Hostname
				}
			} else {
				// Device in DHCP but not ARP — may be stale, skip
			}
		}
	}

	// 3. Merge static devices
	for _, sd := range staticDevices {
		mac := strings.ToLower(sd.MAC)
		if dev, ok := devicesByMAC[mac]; ok {
			dev.Hostname = sd.Name
			dev.Source = "static"
		}
	}

	result := make([]model.Device, 0, len(devicesByMAC))
	for _, d := range devicesByMAC {
		result = append(result, *d)
	}
	return result, nil
}

func parseARP(lanIface string) ([]model.Device, error) {
	out, err := exec.Command("ip", "neigh", "show", "dev", lanIface).CombinedOutput()
	if err != nil {
		return nil, err
	}

	var devices []model.Device
	scanner := bufio.NewScanner(strings.NewReader(string(out)))
	for scanner.Scan() {
		line := scanner.Text()
		fields := strings.Fields(line)
		if len(fields) < 4 {
			continue
		}

		// Format: IP lladdr MAC STATE
		ip := fields[0]
		state := fields[len(fields)-1]

		// Only include reachable/stale/delay neighbors
		if state != "REACHABLE" && state != "STALE" && state != "DELAY" {
			continue
		}

		var mac string
		for i, f := range fields {
			if f == "lladdr" && i+1 < len(fields) {
				mac = strings.ToLower(fields[i+1])
				break
			}
		}
		if mac == "" {
			continue
		}

		devices = append(devices, model.Device{
			MAC:    mac,
			IP:     ip,
			Source: "arp",
		})
	}
	return devices, nil
}

type dhcpLease struct {
	MAC      string
	IP       string
	Hostname string
}

func parseDHCPLeases() ([]dhcpLease, error) {
	f, err := os.Open("/tmp/dhcp.leases")
	if err != nil {
		return nil, err
	}
	defer f.Close()

	var leases []dhcpLease
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		fields := strings.Fields(scanner.Text())
		if len(fields) < 4 {
			continue
		}
		// Format: timestamp MAC IP hostname *
		leases = append(leases, dhcpLease{
			MAC:      strings.ToLower(fields[1]),
			IP:       fields[2],
			Hostname: fields[3],
		})
	}
	return leases, nil
}
