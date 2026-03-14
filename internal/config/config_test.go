package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestDefault(t *testing.T) {
	cfg := Default()
	snap := cfg.Snapshot()

	if snap.MonthlyQuotaGB != 20 {
		t.Errorf("default quota = %d, want 20", snap.MonthlyQuotaGB)
	}
	if snap.CurveShape != 0.40 {
		t.Errorf("default shape = %f, want 0.40", snap.CurveShape)
	}
	if snap.MaxRateKbit != 50000 {
		t.Errorf("default max rate = %d, want 50000", snap.MaxRateKbit)
	}
	if snap.WANIface != "eth0" {
		t.Errorf("default wan = %s, want eth0", snap.WANIface)
	}
}

func TestLoadMissing(t *testing.T) {
	cfg, err := Load("/nonexistent/config.json")
	if err != nil {
		t.Fatalf("Load missing file should not error: %v", err)
	}
	snap := cfg.Snapshot()
	if snap.MonthlyQuotaGB != 20 {
		t.Errorf("fallback quota = %d, want 20", snap.MonthlyQuotaGB)
	}
}

func TestLoadAndSave(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")

	// Save a config
	cfg := Default()
	cfg.SetFilePath(path)
	if err := cfg.Save(); err != nil {
		t.Fatalf("save: %v", err)
	}

	// Load it back
	cfg2, err := Load(path)
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	snap := cfg2.Snapshot()
	if snap.MonthlyQuotaGB != 20 {
		t.Errorf("loaded quota = %d, want 20", snap.MonthlyQuotaGB)
	}
}

func TestUpdate(t *testing.T) {
	cfg := Default()

	update := map[string]interface{}{
		"monthly_quota_gb": 50,
		"curve_shape":      0.60,
	}
	data, _ := json.Marshal(update)

	if err := cfg.Update(data); err != nil {
		t.Fatalf("update: %v", err)
	}

	snap := cfg.Snapshot()
	if snap.MonthlyQuotaGB != 50 {
		t.Errorf("updated quota = %d, want 50", snap.MonthlyQuotaGB)
	}
	if snap.CurveShape != 0.60 {
		t.Errorf("updated shape = %f, want 0.60", snap.CurveShape)
	}
	// Other values should remain default
	if snap.MaxRateKbit != 50000 {
		t.Errorf("max rate should stay default: %d", snap.MaxRateKbit)
	}
}

func TestValidation(t *testing.T) {
	cfg := Default()

	// Invalid billing reset day
	data, _ := json.Marshal(map[string]int{"billing_reset_day": 0})
	if err := cfg.Update(data); err == nil {
		t.Error("expected error for billing_reset_day=0")
	}

	// Reset to valid
	cfg = Default()
	data, _ = json.Marshal(map[string]int{"billing_reset_day": 29})
	if err := cfg.Update(data); err == nil {
		t.Error("expected error for billing_reset_day=29")
	}

	// Invalid curve shape
	cfg = Default()
	data, _ = json.Marshal(map[string]float64{"curve_shape": 0.05})
	if err := cfg.Update(data); err == nil {
		t.Error("expected error for curve_shape=0.05")
	}
}

func TestMonthlyQuotaBytes(t *testing.T) {
	cfg := Default()
	got := cfg.MonthlyQuotaBytes()
	want := int64(20) * 1073741824
	if got != want {
		t.Errorf("MonthlyQuotaBytes = %d, want %d", got, want)
	}
}

func TestLoadCustomConfig(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")

	custom := map[string]interface{}{
		"monthly_quota_gb": 100,
		"curve_shape":      0.50,
		"max_rate_kbit":    100000,
		"min_rate_kbit":    500,
	}
	data, _ := json.Marshal(custom)
	os.WriteFile(path, data, 0644)

	cfg, err := Load(path)
	if err != nil {
		t.Fatalf("load custom: %v", err)
	}
	snap := cfg.Snapshot()
	if snap.MonthlyQuotaGB != 100 {
		t.Errorf("custom quota = %d, want 100", snap.MonthlyQuotaGB)
	}
	// min_rate_kbit=500 is valid (>= 64)
	if snap.MinRateKbit != 500 {
		t.Errorf("custom min rate = %d, want 500", snap.MinRateKbit)
	}
}
