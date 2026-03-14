package config

import (
	"encoding/json"
	"fmt"
	"os"
	"sync"
)

// StaticDevice is a preconfigured device entry.
type StaticDevice struct {
	MAC  string `json:"mac"`
	Name string `json:"name"`
}

// UIAuth holds optional basic auth settings.
type UIAuth struct {
	Enabled      bool   `json:"enabled"`
	Username     string `json:"username"`
	PasswordHash string `json:"password_hash"`
}

// Values holds all SLQM configuration values (safe to copy).
type Values struct {
	NetworkMode           string         `json:"network_mode"`
	WANIface              string         `json:"wan_iface"`
	LANIface              string         `json:"lan_iface"`
	IFBIface              string         `json:"ifb_iface"`
	DishAddr              string         `json:"dish_addr"`
	DishPollIntervalSec   int            `json:"dish_poll_interval_sec"`
	ListenAddr            string         `json:"listen_addr"`
	BillingResetDay       int            `json:"billing_reset_day"`
	MonthlyQuotaGB        int            `json:"monthly_quota_gb"`
	CurveShape            float64        `json:"curve_shape"`
	MaxRateKbit           int            `json:"max_rate_kbit"`
	MinRateKbit           int            `json:"min_rate_kbit"`
	DownUpRatio           float64        `json:"down_up_ratio"`
	BucketDurationSec     int            `json:"bucket_duration_sec"`
	BurstDrainRatio       float64        `json:"burst_drain_ratio"`
	TickIntervalSec       int            `json:"tick_interval_sec"`
	SaveIntervalSec       int            `json:"save_interval_sec"`
	DeviceScanIntervalSec int            `json:"device_scan_interval_sec"`
	OverageCostPerGB      float64        `json:"overage_cost_per_gb"`
	PlanCostMonthly       float64        `json:"plan_cost_monthly"`
	UIAuth                UIAuth         `json:"ui_auth"`
	StaticDevices         []StaticDevice `json:"static_devices"`
}

func (v *Values) validate() error {
	if v.BillingResetDay < 1 || v.BillingResetDay > 28 {
		return fmt.Errorf("billing_reset_day must be 1-28, got %d", v.BillingResetDay)
	}
	if v.MonthlyQuotaGB < 1 || v.MonthlyQuotaGB > 500 {
		return fmt.Errorf("monthly_quota_gb must be 1-500, got %d", v.MonthlyQuotaGB)
	}
	if v.CurveShape < 0.10 || v.CurveShape > 2.00 {
		return fmt.Errorf("curve_shape must be 0.10-2.00, got %.2f", v.CurveShape)
	}
	if v.MaxRateKbit < 1 || v.MaxRateKbit > 500000 {
		return fmt.Errorf("max_rate_kbit must be 1-500000, got %d", v.MaxRateKbit)
	}
	if v.MinRateKbit < 64 || v.MinRateKbit > 50000 {
		return fmt.Errorf("min_rate_kbit must be 64-50000, got %d", v.MinRateKbit)
	}
	if v.DownUpRatio < 0.50 || v.DownUpRatio > 0.95 {
		return fmt.Errorf("down_up_ratio must be 0.50-0.95, got %.2f", v.DownUpRatio)
	}
	if v.BucketDurationSec < 30 || v.BucketDurationSec > 900 {
		return fmt.Errorf("bucket_duration_sec must be 30-900, got %d", v.BucketDurationSec)
	}
	if v.BurstDrainRatio < 0.01 || v.BurstDrainRatio > 0.50 {
		return fmt.Errorf("burst_drain_ratio must be 0.01-0.50, got %.2f", v.BurstDrainRatio)
	}
	if v.TickIntervalSec < 1 || v.TickIntervalSec > 10 {
		return fmt.Errorf("tick_interval_sec must be 1-10, got %d", v.TickIntervalSec)
	}
	return nil
}

// Config holds SLQM configuration with mutex protection.
type Config struct {
	mu       sync.RWMutex
	values   Values
	filePath string
}

func defaultValues() Values {
	return Values{
		NetworkMode:           "router",
		WANIface:              "eth0",
		LANIface:              "br-lan",
		IFBIface:              "ifb0",
		DishAddr:              "192.168.100.1:9200",
		DishPollIntervalSec:   30,
		ListenAddr:            ":8275",
		BillingResetDay:       1,
		MonthlyQuotaGB:        20,
		CurveShape:            0.40,
		MaxRateKbit:           50000,
		MinRateKbit:           1000,
		DownUpRatio:           0.80,
		BucketDurationSec:     300,
		BurstDrainRatio:       0.10,
		TickIntervalSec:       2,
		SaveIntervalSec:       60,
		DeviceScanIntervalSec: 15,
		OverageCostPerGB:      10.0,
		PlanCostMonthly:       250.0,
	}
}

// Default returns a Config with all default values.
func Default() *Config {
	return &Config{values: defaultValues()}
}

// Load reads config from a JSON file, falling back to defaults for missing fields.
func Load(path string) (*Config, error) {
	cfg := Default()
	cfg.filePath = path

	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return cfg, nil
		}
		return nil, fmt.Errorf("read config: %w", err)
	}

	if err := json.Unmarshal(data, &cfg.values); err != nil {
		return nil, fmt.Errorf("parse config: %w", err)
	}

	if err := cfg.values.validate(); err != nil {
		return nil, fmt.Errorf("validate config: %w", err)
	}

	return cfg, nil
}

// Save writes the current config to disk.
func (c *Config) Save() error {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if c.filePath == "" {
		return fmt.Errorf("no config file path set")
	}

	data, err := json.MarshalIndent(c.values, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal config: %w", err)
	}

	return os.WriteFile(c.filePath, data, 0644)
}

// Update applies a partial JSON update to the config.
func (c *Config) Update(data []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if err := json.Unmarshal(data, &c.values); err != nil {
		return fmt.Errorf("parse update: %w", err)
	}

	return c.values.validate()
}

// Snapshot returns a read-only copy of config values.
func (c *Config) Snapshot() Values {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.values
}

// MonthlyQuotaBytes returns total quota in bytes.
func (c *Config) MonthlyQuotaBytes() int64 {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return int64(c.values.MonthlyQuotaGB) * 1073741824
}

// Validate checks all config values are within allowed ranges.
func (c *Config) Validate() error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.values.validate()
}

// SetFilePath sets the path for saving config.
func (c *Config) SetFilePath(path string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.filePath = path
}
