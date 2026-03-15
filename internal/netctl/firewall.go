package netctl

import (
	"fmt"
	"log"
	"os/exec"
)

// OpenFirewallPort adds an iptables INPUT ACCEPT rule for the given TCP port.
// This is needed on OpenWrt where fw3/iptables blocks non-standard ports.
func OpenFirewallPort(port string) {
	log.Printf("firewall: opening TCP port %s", port)
	exec.Command("iptables", "-I", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT").Run()
	exec.Command("ip6tables", "-I", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT").Run()
}

// CloseFirewallPort removes the iptables INPUT ACCEPT rule for the given TCP port.
func CloseFirewallPort(port string) {
	log.Printf("firewall: closing TCP port %s", port)
	exec.Command("iptables", "-D", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT").Run()
	exec.Command("ip6tables", "-D", "INPUT", "-p", "tcp", "--dport", port, "-j", "ACCEPT").Run()
}

// ExtractPort returns the port portion of a host:port address.
func ExtractPort(addr string) string {
	// addr is like "0.0.0.0:8275" or ":8275"
	for i := len(addr) - 1; i >= 0; i-- {
		if addr[i] == ':' {
			return addr[i+1:]
		}
	}
	return fmt.Sprintf("%s", addr)
}
