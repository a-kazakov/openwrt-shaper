package engine

import (
	"testing"
)

func TestNewDeviceBucket(t *testing.T) {
	// 50 Mbps = 6250000 bytes/sec, 300s duration
	b := NewDeviceBucket(6250000, 300)

	if b.Capacity() != 6250000*300 {
		t.Errorf("capacity = %d, want %d", b.Capacity(), 6250000*300)
	}
	if b.Tokens() != b.Capacity() {
		t.Errorf("tokens = %d, want capacity %d", b.Tokens(), b.Capacity())
	}
	if b.Mode() != ModeBurst {
		t.Errorf("mode = %v, want ModeBurst", b.Mode())
	}
	if !b.IsFull() {
		t.Error("expected IsFull() = true")
	}
}

func TestBucketDrainAndRefill(t *testing.T) {
	b := NewDeviceBucket(6250000, 300)
	cap := b.Capacity()

	// Drain some bytes
	drained := b.Drain(1000000)
	if drained != 1000000 {
		t.Errorf("drained = %d, want 1000000", drained)
	}
	if b.Tokens() != cap-1000000 {
		t.Errorf("tokens after drain = %d, want %d", b.Tokens(), cap-1000000)
	}

	// Drain more than available
	b2 := NewDeviceBucket(100, 1) // 100 byte capacity
	b2.Drain(50)
	drained = b2.Drain(100) // only 50 left
	if drained != 50 {
		t.Errorf("over-drain = %d, want 50", drained)
	}
	if b2.Tokens() != 0 {
		t.Errorf("tokens after over-drain = %d, want 0", b2.Tokens())
	}

	// Refill
	b2.Refill(30)
	if b2.Tokens() != 30 {
		t.Errorf("tokens after refill = %d, want 30", b2.Tokens())
	}

	// Refill beyond capacity
	b2.Refill(200)
	if b2.Tokens() != b2.Capacity() {
		t.Errorf("tokens after over-refill = %d, want capacity %d", b2.Tokens(), b2.Capacity())
	}
}

func TestBucketCapacityShrink(t *testing.T) {
	// Start with high curve rate
	b := NewDeviceBucket(6250000, 300) // ~1875 MB
	initialCap := b.Capacity()

	// Simulate curve rate dropping (quota depleting)
	// New curve rate = 1 Mbps = 125000 bytes/sec
	b.Update(125000, 300, 2, 0.10) // capacity = 37.5 MB

	newCap := b.Capacity()
	if newCap >= initialCap {
		t.Errorf("capacity should shrink: %d >= %d", newCap, initialCap)
	}
	if newCap != 125000*300 {
		t.Errorf("capacity = %d, want %d", newCap, 125000*300)
	}

	// Tokens should be clamped to new capacity
	if b.Tokens() > newCap {
		t.Errorf("tokens %d > capacity %d after shrink", b.Tokens(), newCap)
	}
}

func TestBucketHysteresis(t *testing.T) {
	// 50 Mbps = 6250000 bytes/sec, 300s, tick=2s
	b := NewDeviceBucket(6250000, 300)
	b.Update(6250000, 300, 2, 0.10)

	// Start in BURST mode
	if b.Mode() != ModeBurst {
		t.Fatalf("initial mode = %v, want ModeBurst", b.Mode())
	}

	// Drain to below shape_threshold
	// shape_threshold = min(cap/4, 20*1048576*2) = min(468MB, 40MB) = 40MB
	// Drain almost everything
	b.Drain(b.Tokens())
	b.Update(6250000, 300, 2, 0.10)

	if b.Mode() != ModeSustained {
		t.Errorf("after drain mode = %v, want ModeSustained", b.Mode())
	}

	// Refill to dead zone (between shape and unshape thresholds)
	// unshape_threshold = 40MB * 3 = 120MB
	b.Refill(80 * 1048576) // 80 MB - in dead zone
	b.Update(6250000, 300, 2, 0.10)

	if b.Mode() != ModeSustained {
		t.Errorf("in dead zone mode = %v, want ModeSustained (no change)", b.Mode())
	}

	// Refill above unshape_threshold
	b.Refill(200 * 1048576) // well above 120 MB
	b.Update(6250000, 300, 2, 0.10)

	if b.Mode() != ModeBurst {
		t.Errorf("after refill mode = %v, want ModeBurst", b.Mode())
	}
}

func TestBucketBurstCeil(t *testing.T) {
	// 50 Mbps = 6250000 bytes/sec, 300s, tick=2s, drain_ratio=0.10
	b := NewDeviceBucket(6250000, 300)
	b.Update(6250000, 300, 2, 0.10)

	ceil := b.BurstCeilKbit()
	// At full: tokens=1875MB, burst = 1875MB * 0.10 / 2 = 93.75 MB/s = 750 Mbps = 750000 kbit
	if ceil < 700000 || ceil > 800000 {
		t.Errorf("full bucket burst ceil = %d kbit, want ~750000", ceil)
	}

	// Drain to small bucket
	b.Drain(b.Tokens())
	b.Refill(56 * 1048576) // 56 MB (like 99% quota used)
	b.Update(6250000, 300, 2, 0.10)

	ceil = b.BurstCeilKbit()
	// 56MB * 0.10 / 2 = 2.8 MB/s = ~22.4 Mbps = ~22400 kbit
	if ceil < 20000 || ceil > 25000 {
		t.Errorf("small bucket burst ceil = %d kbit, want ~22400", ceil)
	}
}

func TestBucketBurstCeilFloor(t *testing.T) {
	b := NewDeviceBucket(100, 1) // tiny bucket
	b.Drain(b.Tokens())         // empty
	b.Update(100, 1, 2, 0.10)

	ceil := b.BurstCeilKbit()
	if ceil < 1000 {
		t.Errorf("burst ceil floor = %d, want >= 1000", ceil)
	}
}

func TestBucketTurboMode(t *testing.T) {
	b := NewDeviceBucket(6250000, 300)
	b.SetMode(ModeTurbo)

	// Turbo should not be changed by Update
	b.Drain(b.Tokens())
	b.Update(6250000, 300, 2, 0.10)

	if b.Mode() != ModeTurbo {
		t.Errorf("turbo mode changed to %v after update", b.Mode())
	}

	// Cancel turbo
	b.SetMode(ModeBurst)
	b.Update(6250000, 300, 2, 0.10)
	// With 0 tokens, should transition to SUSTAINED
	if b.Mode() != ModeSustained {
		t.Errorf("after turbo cancel with empty bucket, mode = %v, want ModeSustained", b.Mode())
	}
}

func TestBucketSetTokens(t *testing.T) {
	b := NewDeviceBucket(6250000, 300)

	b.SetTokens(500 * 1048576) // 500 MB
	if b.Tokens() != 500*1048576 {
		t.Errorf("tokens = %d, want %d", b.Tokens(), 500*1048576)
	}

	// Over capacity
	b.SetTokens(99999999999)
	if b.Tokens() != b.Capacity() {
		t.Errorf("tokens = %d, want capped at capacity %d", b.Tokens(), b.Capacity())
	}

	// Negative
	b.SetTokens(-100)
	if b.Tokens() != 0 {
		t.Errorf("tokens = %d, want 0 for negative input", b.Tokens())
	}
}
