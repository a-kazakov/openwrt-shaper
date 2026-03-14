package netctl

import (
	"fmt"
	"log"
	"os/exec"
	"strconv"
	"strings"
)

const uncapped = 1000000 // 1 Gbps in kbit

// TCController manages dual HTB qdisc trees on WAN and IFB interfaces.
type TCController struct {
	wanIface string
	ifbIface string
	minRate  int // minimum rate in kbit
}

// NewTCController creates a tc controller for the given interfaces.
func NewTCController(wanIface, ifbIface string, minRateKbit int) *TCController {
	return &TCController{
		wanIface: wanIface,
		ifbIface: ifbIface,
		minRate:  minRateKbit,
	}
}

// SetupHTB initializes the HTB qdisc trees on both WAN and IFB interfaces.
func (tc *TCController) SetupHTB(rootRateKbit int) error {
	for _, iface := range []string{tc.wanIface, tc.ifbIface} {
		// Remove existing qdisc (ignore errors if none exists)
		exec.Command("tc", "qdisc", "del", "dev", iface, "root").Run()

		// Add root HTB qdisc
		if err := tc.run("tc", "qdisc", "add", "dev", iface, "root", "handle", "1:", "htb",
			"default", "2"); err != nil {
			return fmt.Errorf("add htb qdisc on %s: %w", iface, err)
		}

		// Add root class with curve rate
		if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:", "classid", "1:1",
			"htb", "rate", fmt.Sprintf("%dkbit", rootRateKbit), "ceil", fmt.Sprintf("%dkbit", rootRateKbit)); err != nil {
			return fmt.Errorf("add root class on %s: %w", iface, err)
		}

		// Add default catch-all class
		if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:1", "classid", "1:2",
			"htb", "rate", fmt.Sprintf("%dkbit", tc.minRate), "ceil", fmt.Sprintf("%dkbit", rootRateKbit)); err != nil {
			return fmt.Errorf("add default class on %s: %w", iface, err)
		}

		// Add fq_codel to default class
		if err := tc.run("tc", "qdisc", "add", "dev", iface, "parent", "1:2", "fq_codel"); err != nil {
			log.Printf("warning: fq_codel on %s default: %v", iface, err)
		}
	}
	return nil
}

// UpdateRootRate changes the root class rate on both trees.
func (tc *TCController) UpdateRootRate(rateKbit int) error {
	for _, iface := range []string{tc.wanIface, tc.ifbIface} {
		if err := tc.run("tc", "class", "change", "dev", iface, "parent", "1:", "classid", "1:1",
			"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", rateKbit)); err != nil {
			return fmt.Errorf("update root rate on %s: %w", iface, err)
		}
	}
	return nil
}

// AddDeviceClass creates a class for a device in both HTB trees.
func (tc *TCController) AddDeviceClass(slot int, rateKbit, ceilKbit int) error {
	classID := fmt.Sprintf("1:%d", 10+slot)
	handle := fmt.Sprintf("%d:", 10+slot)
	for _, iface := range []string{tc.wanIface, tc.ifbIface} {
		if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:1", "classid", classID,
			"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", ceilKbit)); err != nil {
			return fmt.Errorf("add device class %s on %s: %w", classID, iface, err)
		}

		// Add fq_codel leaf qdisc
		if err := tc.run("tc", "qdisc", "add", "dev", iface, "parent", classID, "handle", handle, "fq_codel"); err != nil {
			log.Printf("warning: fq_codel on %s %s: %v", iface, classID, err)
		}

		// Add filter to match fw mark to this class
		mark := 100 + slot
		if err := tc.run("tc", "filter", "add", "dev", iface, "parent", "1:", "protocol", "ip",
			"prio", "1", "handle", strconv.Itoa(mark), "fw", "classid", classID); err != nil {
			return fmt.Errorf("add filter for mark %d on %s: %w", mark, iface, err)
		}
	}
	return nil
}

// RemoveDeviceClass removes a device's class from both HTB trees.
func (tc *TCController) RemoveDeviceClass(slot int) error {
	classID := fmt.Sprintf("1:%d", 10+slot)
	for _, iface := range []string{tc.wanIface, tc.ifbIface} {
		exec.Command("tc", "class", "del", "dev", iface, "classid", classID).Run()
	}
	return nil
}

// SetDeviceMode updates both HTB trees based on device mode.
func (tc *TCController) SetDeviceMode(slot int, mode string, fairShareKbit int, burstCeilKbit int, downUpRatio float64) error {
	switch mode {
	case "turbo":
		tc.setClass(tc.wanIface, slot, fairShareKbit, uncapped)
		tc.setClass(tc.ifbIface, slot, fairShareKbit, uncapped)
	case "burst":
		downCeil := int(float64(burstCeilKbit) * downUpRatio)
		upCeil := burstCeilKbit - downCeil
		if downCeil < tc.minRate {
			downCeil = tc.minRate
		}
		if upCeil < tc.minRate {
			upCeil = tc.minRate
		}
		tc.setClass(tc.wanIface, slot, fairShareKbit, upCeil)
		tc.setClass(tc.ifbIface, slot, fairShareKbit, downCeil)
	case "sustained":
		downCeil := int(float64(fairShareKbit) * downUpRatio)
		upCeil := fairShareKbit - downCeil
		if downCeil < tc.minRate {
			downCeil = tc.minRate
		}
		if upCeil < tc.minRate {
			upCeil = tc.minRate
		}
		tc.setClass(tc.wanIface, slot, upCeil, upCeil)
		tc.setClass(tc.ifbIface, slot, downCeil, downCeil)
	}
	return nil
}

func (tc *TCController) setClass(iface string, slot int, rateKbit, ceilKbit int) {
	classID := fmt.Sprintf("1:%d", 10+slot)
	tc.run("tc", "class", "change", "dev", iface, "parent", "1:1", "classid", classID,
		"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", ceilKbit))
}

// Teardown removes all tc qdiscs from both interfaces.
func (tc *TCController) Teardown() {
	for _, iface := range []string{tc.wanIface, tc.ifbIface} {
		exec.Command("tc", "qdisc", "del", "dev", iface, "root").Run()
	}
}

func (tc *TCController) run(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("%s %s: %s: %w", name, strings.Join(args, " "), string(out), err)
	}
	return nil
}
