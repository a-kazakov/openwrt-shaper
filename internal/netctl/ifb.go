package netctl

import (
	"fmt"
	"os/exec"
)

// SetupIFB creates the IFB device and redirects WAN ingress through it.
func SetupIFB(wanIface, ifbIface string) error {
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

	// Redirect all WAN ingress to IFB
	if err := run("tc", "filter", "add", "dev", wanIface, "parent", "ffff:", "protocol", "ip",
		"u32", "match", "u32", "0", "0",
		"action", "mirred", "egress", "redirect", "dev", ifbIface); err != nil {
		return fmt.Errorf("add mirred redirect: %w", err)
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
