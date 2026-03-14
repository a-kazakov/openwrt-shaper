package store

import (
	"path/filepath"
	"testing"
	"time"
)

func testStore(t *testing.T) *Store {
	t.Helper()
	path := filepath.Join(t.TempDir(), "test.db")
	s, err := Open(path)
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	t.Cleanup(func() { s.Close() })
	return s
}

func TestQuotaRoundTrip(t *testing.T) {
	s := testStore(t)

	if err := s.SaveQuota(1234567890, 400000000, 834567890, "2026-03"); err != nil {
		t.Fatalf("save: %v", err)
	}

	used, up, down, month, err := s.LoadQuota()
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if used != 1234567890 {
		t.Errorf("used = %d, want 1234567890", used)
	}
	if up != 400000000 {
		t.Errorf("upload = %d, want 400000000", up)
	}
	if down != 834567890 {
		t.Errorf("download = %d, want 834567890", down)
	}
	if month != "2026-03" {
		t.Errorf("month = %s, want 2026-03", month)
	}
}

func TestDeviceCycleBytes(t *testing.T) {
	s := testStore(t)

	mac := "aa:bb:cc:dd:ee:ff"
	if err := s.SaveDeviceCycleBytes(mac, 5000000); err != nil {
		t.Fatalf("save: %v", err)
	}

	got, err := s.LoadDeviceCycleBytes(mac)
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if got != 5000000 {
		t.Errorf("cycle bytes = %d, want 5000000", got)
	}

	// Unknown device
	got, err = s.LoadDeviceCycleBytes("00:00:00:00:00:00")
	if err != nil {
		t.Fatalf("load unknown: %v", err)
	}
	if got != 0 {
		t.Errorf("unknown device bytes = %d, want 0", got)
	}
}

func TestConfigPersistence(t *testing.T) {
	s := testStore(t)

	cfg := []byte(`{"monthly_quota_gb": 50}`)
	if err := s.SaveConfig(cfg); err != nil {
		t.Fatalf("save: %v", err)
	}

	got, err := s.LoadConfig()
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if string(got) != string(cfg) {
		t.Errorf("config = %s, want %s", got, cfg)
	}
}

func TestHistorySnapshot(t *testing.T) {
	s := testStore(t)

	now := time.Now()
	snap1 := []byte(`{"ts":1}`)
	snap2 := []byte(`{"ts":2}`)

	s.SaveHistorySnapshot(now.Add(-2*time.Hour), snap1)
	s.SaveHistorySnapshot(now.Add(-1*time.Hour), snap2)

	results, err := s.LoadHistory(now.Add(-3*time.Hour), now)
	if err != nil {
		t.Fatalf("load history: %v", err)
	}
	if len(results) != 2 {
		t.Errorf("history count = %d, want 2", len(results))
	}
}

func TestPruneHistory(t *testing.T) {
	s := testStore(t)

	now := time.Now()
	s.SaveHistorySnapshot(now.Add(-48*time.Hour), []byte(`{"old":true}`))
	s.SaveHistorySnapshot(now.Add(-1*time.Hour), []byte(`{"new":true}`))

	s.PruneHistory(now.Add(-24 * time.Hour))

	results, _ := s.LoadHistory(now.Add(-72*time.Hour), now)
	if len(results) != 1 {
		t.Errorf("after prune count = %d, want 1", len(results))
	}
}

func TestClearDevices(t *testing.T) {
	s := testStore(t)

	s.SaveDeviceCycleBytes("aa:bb:cc:dd:ee:ff", 1000)
	s.ClearDevices()

	got, _ := s.LoadDeviceCycleBytes("aa:bb:cc:dd:ee:ff")
	if got != 0 {
		t.Errorf("after clear = %d, want 0", got)
	}
}

func TestEmptyLoad(t *testing.T) {
	s := testStore(t)

	used, up, down, month, err := s.LoadQuota()
	if err != nil {
		t.Fatalf("load empty: %v", err)
	}
	if used != 0 || up != 0 || down != 0 || month != "" {
		t.Errorf("empty load = %d/%d/%d/%s, want 0/0/0/empty", used, up, down, month)
	}
}
