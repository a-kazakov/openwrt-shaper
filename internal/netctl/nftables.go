package netctl

import (
	"fmt"
	"os/exec"
	"strings"
)

// NFTController manages the nftables table for SLQM packet marking.
type NFTController struct {
	wanIface  string
	tableName string
}

// NewNFTController creates a new nftables controller.
func NewNFTController(wanIface string) *NFTController {
	return &NFTController{
		wanIface:  wanIface,
		tableName: "slqm",
	}
}

// Setup creates the nftables table and chains for bidirectional marking.
func (n *NFTController) Setup() error {
	// Remove existing table (ignore error if not exists)
	exec.Command("nft", "delete", "table", "inet", n.tableName).Run()

	cmds := []string{
		fmt.Sprintf("add table inet %s", n.tableName),
		fmt.Sprintf("add chain inet %s upload { type filter hook forward priority mangle ; }", n.tableName),
		fmt.Sprintf("add chain inet %s download { type filter hook forward priority mangle ; }", n.tableName),
	}

	for _, cmd := range cmds {
		if err := n.nft(cmd); err != nil {
			return fmt.Errorf("nft setup %q: %w", cmd, err)
		}
	}
	return nil
}

// AddDevice adds bidirectional marking rules for a device.
func (n *NFTController) AddDevice(ip string, mark int) error {
	uploadRule := fmt.Sprintf("add rule inet %s upload oifname %q ip saddr %s counter meta mark set %d",
		n.tableName, n.wanIface, ip, mark)
	downloadRule := fmt.Sprintf("add rule inet %s download iifname %q ip daddr %s counter meta mark set %d",
		n.tableName, n.wanIface, ip, mark)

	if err := n.nft(uploadRule); err != nil {
		return fmt.Errorf("add upload rule for %s: %w", ip, err)
	}
	if err := n.nft(downloadRule); err != nil {
		return fmt.Errorf("add download rule for %s: %w", ip, err)
	}
	return nil
}

// RemoveDevice removes all rules for a device IP.
func (n *NFTController) RemoveDevice(ip string) error {
	// List rules and find handles for this IP, then delete by handle
	for _, chain := range []string{"upload", "download"} {
		handles, err := n.findRuleHandles(chain, ip)
		if err != nil {
			continue
		}
		for _, handle := range handles {
			n.nft(fmt.Sprintf("delete rule inet %s %s handle %s", n.tableName, chain, handle))
		}
	}
	return nil
}

// Teardown removes the entire SLQM nftables table.
func (n *NFTController) Teardown() {
	exec.Command("nft", "delete", "table", "inet", n.tableName).Run()
}

func (n *NFTController) findRuleHandles(chain, ip string) ([]string, error) {
	out, err := exec.Command("nft", "-a", "list", "chain", "inet", n.tableName, chain).CombinedOutput()
	if err != nil {
		return nil, err
	}
	var handles []string
	for _, line := range strings.Split(string(out), "\n") {
		if strings.Contains(line, ip) && strings.Contains(line, "handle") {
			parts := strings.Fields(line)
			for i, p := range parts {
				if p == "handle" && i+1 < len(parts) {
					handles = append(handles, parts[i+1])
				}
			}
		}
	}
	return handles, nil
}

func (n *NFTController) nft(rule string) error {
	args := strings.Fields(rule)
	cmd := exec.Command("nft", args...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("nft %s: %s: %w", rule, strings.TrimSpace(string(out)), err)
	}
	return nil
}
