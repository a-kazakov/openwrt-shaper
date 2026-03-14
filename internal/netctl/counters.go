package netctl

import (
	"fmt"
	"os/exec"
	"regexp"
	"strconv"
	"strings"
)

var counterRegex = regexp.MustCompile(`counter packets (\d+) bytes (\d+)`)

// ReadDeviceBytes returns upload and download byte counts for a device by mark.
func (n *NFTController) ReadDeviceBytes(mark int) (upload, download int64, err error) {
	upload, err = n.readChainBytes("upload", mark)
	if err != nil {
		return 0, 0, fmt.Errorf("read upload counters: %w", err)
	}
	download, err = n.readChainBytes("download", mark)
	if err != nil {
		return 0, 0, fmt.Errorf("read download counters: %w", err)
	}
	return upload, download, nil
}

func (n *NFTController) readChainBytes(chain string, mark int) (int64, error) {
	out, err := exec.Command("nft", "list", "chain", "inet", n.tableName, chain).CombinedOutput()
	if err != nil {
		return 0, fmt.Errorf("list chain %s: %w", chain, err)
	}

	markStr := fmt.Sprintf("mark set %d", mark)
	for _, line := range strings.Split(string(out), "\n") {
		if !strings.Contains(line, markStr) {
			continue
		}
		matches := counterRegex.FindStringSubmatch(line)
		if len(matches) >= 3 {
			bytes, err := strconv.ParseInt(matches[2], 10, 64)
			if err != nil {
				return 0, err
			}
			return bytes, nil
		}
	}
	return 0, nil
}

// ReadAllCounters reads byte counters for all devices in both chains.
// Returns map[mark] -> (upload, download).
func (n *NFTController) ReadAllCounters() (map[int][2]int64, error) {
	result := make(map[int][2]int64)

	for i, chain := range []string{"upload", "download"} {
		out, err := exec.Command("nft", "list", "chain", "inet", n.tableName, chain).CombinedOutput()
		if err != nil {
			return nil, fmt.Errorf("list chain %s: %w", chain, err)
		}

		markRegex := regexp.MustCompile(`mark set (\d+)`)

		for _, line := range strings.Split(string(out), "\n") {
			markMatch := markRegex.FindStringSubmatch(line)
			if len(markMatch) < 2 {
				continue
			}
			mark, err := strconv.Atoi(markMatch[1])
			if err != nil {
				continue
			}

			counterMatch := counterRegex.FindStringSubmatch(line)
			if len(counterMatch) < 3 {
				continue
			}
			bytes, err := strconv.ParseInt(counterMatch[2], 10, 64)
			if err != nil {
				continue
			}

			entry := result[mark]
			entry[i] = bytes
			result[mark] = entry
		}
	}

	return result, nil
}
