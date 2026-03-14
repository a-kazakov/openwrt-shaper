package netctl

import (
	"fmt"
	"os/exec"
)

// SetupIFB creates the IFB device and redirects WAN ingress through it.
// lanSubnet scopes the redirect to only packets destined for LAN clients
// (e.g. "192.168.8.0/24"), preventing router-local traffic from being shaped.
// If lanSubnet is empty, falls back to matching all RFC1918 ranges.
func SetupIFB(wanIface, ifbIface, lanSubnet string) error {
	// Tear down any existing IFB first (idempotent)
	TeardownIFB(wanIface, ifbIface)

	// Create IFB device
	if err := run("ip", "link", "add", ifbIface, "type", "ifb"); err != nil {
		return fmt.Errorf("create ifb: %w", err)
	}
	if err := run("ip", "link", "set", ifbIface, "up"); err != nil {
		return fmt.Errorf("set ifb up: %w", err)
	}

	// Add ingress qdisc on WAN
	if err := run("tc", "qdisc", "add", "dev", wanIface, "handle", "ffff:", "ingress"); err != nil {
		return fmt.Errorf("add ingress qdisc: %w", err)
	}

	// Redirect only LAN-destined WAN ingress to IFB.
	// Using "u32 match u32 0 0" (match-all) breaks router-local services
	// because it also redirects packets destined for the router itself.
	subnets := []string{lanSubnet}
	if lanSubnet == "" {
		// Fallback: match all RFC1918 private ranges
		subnets = []string{"10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"}
	}
	for _, subnet := range subnets {
		if err := run("tc", "filter", "add", "dev", wanIface, "parent", "ffff:", "protocol", "ip",
			"u32", "match", "ip", "dst", subnet,
			"action", "mirred", "egress", "redirect", "dev", ifbIface); err != nil {
			return fmt.Errorf("add mirred redirect for %s: %w", subnet, err)
		}
	}

	return nil
}

// TeardownIFB removes the IFB device and ingress redirect.
func TeardownIFB(wanIface, ifbIface string) {
	exec.Command("tc", "qdisc", "del", "dev", wanIface, "ingress").Run()
	exec.Command("ip", "link", "del", ifbIface).Run()
}

func run(name string, args ...string) error {
	out, err := exec.Command(name, args...).CombinedOutput()
	if err != nil {
		return fmt.Errorf("%s: %s", string(out), err)
	}
	return nil
}
