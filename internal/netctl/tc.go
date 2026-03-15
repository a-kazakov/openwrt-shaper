package netctl

import (
	"fmt"
	"log"
	"os/exec"
	"strconv"
	"strings"
)

const uncapped = 1000000 // 1 Gbps in kbit

// TCController manages dual HTB qdisc trees:
//   - WAN interface egress: upload shaping (root class = curve rate)
//   - LAN interface egress: download shaping (uses an intermediate parent class
//     at the curve rate so unmatched local/inter-LAN traffic is not throttled)
//
// This replaces the previous IFB-based approach which had two problems:
// 1. tc ingress redirect ran before nftables, so fw marks were never set on IFB
// 2. Redirecting all WAN ingress broke router-local services
type TCController struct {
	wanIface string
	lanIface string
	minRate  int // minimum rate in kbit
}

// NewTCController creates a tc controller for the given interfaces.
func NewTCController(wanIface, lanIface string, minRateKbit int) *TCController {
	return &TCController{
		wanIface: wanIface,
		lanIface: lanIface,
		minRate:  minRateKbit,
	}
}

// SetupHTB initializes the HTB qdisc trees on WAN and LAN interfaces.
func (tc *TCController) SetupHTB(rootRateKbit int) error {
	if err := tc.setupWAN(rootRateKbit); err != nil {
		return err
	}
	return tc.setupLAN(rootRateKbit)
}

// setupWAN creates the upload shaping tree on the WAN interface.
// Tree structure:
//
//	1: (root HTB, default 2)
//	└── 1:1 (rate=curveRate, ceil=curveRate)
//	    ├── 1:2 (default catch-all, rate=minRate, ceil=curveRate)
//	    └── 1:10+ (per-device classes)
func (tc *TCController) setupWAN(rootRateKbit int) error {
	iface := tc.wanIface
	exec.Command("tc", "qdisc", "del", "dev", iface, "root").Run()

	if err := tc.run("tc", "qdisc", "add", "dev", iface, "root", "handle", "1:", "htb",
		"default", "2"); err != nil {
		return fmt.Errorf("add htb qdisc on %s: %w", iface, err)
	}
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:", "classid", "1:1",
		"htb", "rate", fmt.Sprintf("%dkbit", rootRateKbit), "ceil", fmt.Sprintf("%dkbit", rootRateKbit)); err != nil {
		return fmt.Errorf("add root class on %s: %w", iface, err)
	}
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:1", "classid", "1:2",
		"htb", "rate", fmt.Sprintf("%dkbit", tc.minRate), "ceil", fmt.Sprintf("%dkbit", rootRateKbit)); err != nil {
		return fmt.Errorf("add default class on %s: %w", iface, err)
	}
	if err := tc.run("tc", "qdisc", "add", "dev", iface, "parent", "1:2", "fq_codel"); err != nil {
		log.Printf("warning: fq_codel on %s default: %v", iface, err)
	}
	return nil
}

// setupLAN creates the download shaping tree on the LAN interface.
// Unmatched traffic (router-local, inter-LAN) goes to class 1:2 which is
// effectively unlimited. Marked download traffic goes to device classes
// under 1:3 which is rate-limited to the curve rate.
//
// Tree structure:
//
//	1: (root HTB, default 2)
//	└── 1:1 (rate=1Gbps, ceil=1Gbps)
//	    ├── 1:2 (unmatched/local, rate=900Mbps, ceil=1Gbps)
//	    └── 1:3 (download parent, rate=curveRate, ceil=curveRate)
//	        ├── 1:4 (default shaped, rate=minRate, ceil=curveRate)
//	        └── 1:10+ (per-device classes)
func (tc *TCController) setupLAN(downloadRateKbit int) error {
	iface := tc.lanIface
	exec.Command("tc", "qdisc", "del", "dev", iface, "root").Run()

	if err := tc.run("tc", "qdisc", "add", "dev", iface, "root", "handle", "1:", "htb",
		"default", "2"); err != nil {
		return fmt.Errorf("add htb qdisc on %s: %w", iface, err)
	}
	// Root class: high rate to not throttle unmatched traffic
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:", "classid", "1:1",
		"htb", "rate", fmt.Sprintf("%dkbit", uncapped), "ceil", fmt.Sprintf("%dkbit", uncapped)); err != nil {
		return fmt.Errorf("add root class on %s: %w", iface, err)
	}
	// Default class for unmatched traffic (router-local, inter-LAN): essentially unlimited
	localRate := uncapped - downloadRateKbit
	if localRate < tc.minRate {
		localRate = tc.minRate
	}
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:1", "classid", "1:2",
		"htb", "rate", fmt.Sprintf("%dkbit", localRate), "ceil", fmt.Sprintf("%dkbit", uncapped)); err != nil {
		return fmt.Errorf("add default class on %s: %w", iface, err)
	}
	if err := tc.run("tc", "qdisc", "add", "dev", iface, "parent", "1:2", "fq_codel"); err != nil {
		log.Printf("warning: fq_codel on %s default: %v", iface, err)
	}
	// Download shaping parent: limits aggregate download to curve rate
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:1", "classid", "1:3",
		"htb", "rate", fmt.Sprintf("%dkbit", downloadRateKbit), "ceil", fmt.Sprintf("%dkbit", downloadRateKbit)); err != nil {
		return fmt.Errorf("add download parent on %s: %w", iface, err)
	}
	// Default shaped class (for marked traffic without a specific device class)
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", "1:3", "classid", "1:4",
		"htb", "rate", fmt.Sprintf("%dkbit", tc.minRate), "ceil", fmt.Sprintf("%dkbit", downloadRateKbit)); err != nil {
		return fmt.Errorf("add default shaped class on %s: %w", iface, err)
	}
	if err := tc.run("tc", "qdisc", "add", "dev", iface, "parent", "1:4", "fq_codel"); err != nil {
		log.Printf("warning: fq_codel on %s default shaped: %v", iface, err)
	}
	return nil
}

// UpdateRootRate changes the root class rate on both trees.
func (tc *TCController) UpdateRootRate(rateKbit int) error {
	// WAN: update root class 1:1
	if err := tc.run("tc", "class", "change", "dev", tc.wanIface, "parent", "1:", "classid", "1:1",
		"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", rateKbit)); err != nil {
		return fmt.Errorf("update root rate on %s: %w", tc.wanIface, err)
	}
	// LAN: update download parent class 1:3
	if err := tc.run("tc", "class", "change", "dev", tc.lanIface, "parent", "1:1", "classid", "1:3",
		"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", rateKbit)); err != nil {
		return fmt.Errorf("update download parent on %s: %w", tc.lanIface, err)
	}
	return nil
}

// AddDeviceClass creates a class for a device in both HTB trees.
func (tc *TCController) AddDeviceClass(slot int, rateKbit, ceilKbit int) error {
	classID := fmt.Sprintf("1:%d", 10+slot)
	handle := fmt.Sprintf("%d:", 10+slot)
	mark := 100 + slot

	// WAN: device class under 1:1
	if err := tc.addClassOnIface(tc.wanIface, "1:1", classID, handle, mark, rateKbit, ceilKbit); err != nil {
		return err
	}
	// LAN: device class under 1:3 (download parent)
	if err := tc.addClassOnIface(tc.lanIface, "1:3", classID, handle, mark, rateKbit, ceilKbit); err != nil {
		return err
	}
	return nil
}

func (tc *TCController) addClassOnIface(iface, parent, classID, handle string, mark, rateKbit, ceilKbit int) error {
	if err := tc.run("tc", "class", "add", "dev", iface, "parent", parent, "classid", classID,
		"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", ceilKbit)); err != nil {
		return fmt.Errorf("add device class %s on %s: %w", classID, iface, err)
	}
	if err := tc.run("tc", "qdisc", "add", "dev", iface, "parent", classID, "handle", handle, "fq_codel"); err != nil {
		log.Printf("warning: fq_codel on %s %s: %v", iface, classID, err)
	}
	if err := tc.run("tc", "filter", "add", "dev", iface, "parent", "1:", "protocol", "ip",
		"prio", "1", "handle", strconv.Itoa(mark), "fw", "classid", classID); err != nil {
		return fmt.Errorf("add filter for mark %d on %s: %w", mark, iface, err)
	}
	return nil
}

// RemoveDeviceClass removes a device's class from both HTB trees.
func (tc *TCController) RemoveDeviceClass(slot int) error {
	classID := fmt.Sprintf("1:%d", 10+slot)
	for _, iface := range []string{tc.wanIface, tc.lanIface} {
		exec.Command("tc", "class", "del", "dev", iface, "classid", classID).Run()
	}
	return nil
}

// SetDeviceMode updates both HTB trees based on device mode.
func (tc *TCController) SetDeviceMode(slot int, mode string, fairShareKbit int, burstCeilKbit int, downUpRatio float64) error {
	switch mode {
	case "turbo":
		tc.setClass(tc.wanIface, slot, "1:1", fairShareKbit, uncapped)
		tc.setClass(tc.lanIface, slot, "1:3", fairShareKbit, uncapped)
	case "burst":
		downCeil := int(float64(burstCeilKbit) * downUpRatio)
		upCeil := burstCeilKbit - downCeil
		if downCeil < tc.minRate {
			downCeil = tc.minRate
		}
		if upCeil < tc.minRate {
			upCeil = tc.minRate
		}
		tc.setClass(tc.wanIface, slot, "1:1", fairShareKbit, upCeil)
		tc.setClass(tc.lanIface, slot, "1:3", fairShareKbit, downCeil)
	case "sustained":
		downCeil := int(float64(fairShareKbit) * downUpRatio)
		upCeil := fairShareKbit - downCeil
		if downCeil < tc.minRate {
			downCeil = tc.minRate
		}
		if upCeil < tc.minRate {
			upCeil = tc.minRate
		}
		tc.setClass(tc.wanIface, slot, "1:1", upCeil, upCeil)
		tc.setClass(tc.lanIface, slot, "1:3", downCeil, downCeil)
	}
	return nil
}

func (tc *TCController) setClass(iface string, slot int, parent string, rateKbit, ceilKbit int) {
	classID := fmt.Sprintf("1:%d", 10+slot)
	tc.run("tc", "class", "change", "dev", iface, "parent", parent, "classid", classID,
		"htb", "rate", fmt.Sprintf("%dkbit", rateKbit), "ceil", fmt.Sprintf("%dkbit", ceilKbit))
}

// Teardown removes all tc qdiscs from both interfaces.
func (tc *TCController) Teardown() {
	for _, iface := range []string{tc.wanIface, tc.lanIface} {
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
