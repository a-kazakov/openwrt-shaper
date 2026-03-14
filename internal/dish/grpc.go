package dish

import (
	"context"
	"fmt"
	"log"
	"net"
	"os/exec"
	"strings"
	"sync"
	"time"

	"github.com/akazakov/openwrt-shaper/internal/model"
)

// Client handles communication with the Starlink dish.
type Client struct {
	addr     string
	wanIface string
	mu       sync.RWMutex
	status   *model.DishStatus
	lastPoll time.Time
}

// NewClient creates a new dish client.
func NewClient(addr, wanIface string) *Client {
	return &Client{
		addr:     addr,
		wanIface: wanIface,
	}
}

// EnsureRoute adds a static route to the dish's subnet via the WAN interface.
// Required in bypass mode where WAN gets a CGNAT IP.
func (c *Client) EnsureRoute() error {
	host, _, err := net.SplitHostPort(c.addr)
	if err != nil {
		host = c.addr
	}

	ip := net.ParseIP(host)
	if ip == nil {
		return fmt.Errorf("invalid dish IP: %s", host)
	}

	// Determine subnet (assume /24)
	ip4 := ip.To4()
	if ip4 == nil {
		return fmt.Errorf("dish IP must be IPv4: %s", host)
	}
	subnet := fmt.Sprintf("%d.%d.%d.0/24", ip4[0], ip4[1], ip4[2])

	// ip route replace is idempotent
	out, err := exec.Command("ip", "route", "replace", subnet, "dev", c.wanIface).CombinedOutput()
	if err != nil {
		return fmt.Errorf("add route to %s: %s: %w", subnet, strings.TrimSpace(string(out)), err)
	}
	return nil
}

// Poll fetches current dish status. Returns nil status if unreachable.
func (c *Client) Poll(ctx context.Context) (*model.DishStatus, error) {
	// Try to reach the dish with a TCP connection test
	host, port, err := net.SplitHostPort(c.addr)
	if err != nil {
		host = c.addr
		port = "9200"
	}

	dialer := net.Dialer{Timeout: 3 * time.Second}
	conn, err := dialer.DialContext(ctx, "tcp", net.JoinHostPort(host, port))
	if err != nil {
		c.mu.Lock()
		c.status = &model.DishStatus{Reachable: false}
		c.lastPoll = time.Now()
		c.mu.Unlock()
		return nil, fmt.Errorf("dish unreachable at %s: %w", c.addr, err)
	}
	conn.Close()

	// Dish is reachable but we don't have the gRPC proto definitions.
	// In a production build, this would use grpc reflection or compiled stubs.
	// For now, mark as reachable and return basic status.
	status := &model.DishStatus{
		Reachable: true,
		Connected: true,
	}

	c.mu.Lock()
	c.status = status
	c.lastPoll = time.Now()
	c.mu.Unlock()

	return status, nil
}

// Status returns the last known dish status.
func (c *Client) Status() *model.DishStatus {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if c.status == nil {
		return &model.DishStatus{Reachable: false}
	}
	cp := *c.status
	return &cp
}

// RunPoller starts a background goroutine that polls the dish periodically.
func (c *Client) RunPoller(ctx context.Context, interval time.Duration) {
	// Ensure route on startup
	if err := c.EnsureRoute(); err != nil {
		log.Printf("dish: route setup: %v", err)
	}

	ticker := time.NewTicker(interval)
	defer ticker.Stop()

	// Initial poll
	if _, err := c.Poll(ctx); err != nil {
		log.Printf("dish: initial poll: %v", err)
	}

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if _, err := c.Poll(ctx); err != nil {
				log.Printf("dish: poll: %v", err)
			}
		}
	}
}
