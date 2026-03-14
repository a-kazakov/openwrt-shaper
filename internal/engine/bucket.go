package engine

import "sync"

// DeviceMode represents the shaping mode of a device.
type DeviceMode int

const (
	ModeBurst     DeviceMode = iota
	ModeSustained
	ModeTurbo
)

func (m DeviceMode) String() string {
	switch m {
	case ModeBurst:
		return "burst"
	case ModeSustained:
		return "sustained"
	case ModeTurbo:
		return "turbo"
	default:
		return "unknown"
	}
}

// DeviceBucket implements a per-device byte token bucket with dynamic capacity
// and hysteresis-based mode transitions.
type DeviceBucket struct {
	mu            sync.Mutex
	tokens        int64
	capacity      int64
	mode          DeviceMode
	burstCeilKbit int
}

// NewDeviceBucket creates a bucket starting in BURST mode with full tokens.
func NewDeviceBucket(curveRateBytesPerSec int64, durationSec int) *DeviceBucket {
	cap := curveRateBytesPerSec * int64(durationSec)
	return &DeviceBucket{
		tokens:   cap,
		capacity: cap,
		mode:     ModeBurst,
	}
}

// Update recalculates capacity from current curve rate, clamps tokens,
// computes burst ceiling, and applies hysteresis for mode transitions.
func (b *DeviceBucket) Update(curveRateBytesPerSec int64, durationSec int, tickSec int, burstDrainRatio float64) {
	b.mu.Lock()
	defer b.mu.Unlock()

	b.capacity = curveRateBytesPerSec * int64(durationSec)

	if b.tokens > b.capacity {
		b.tokens = b.capacity
	}

	burstBytesPerSec := float64(b.tokens) * burstDrainRatio / float64(tickSec)
	b.burstCeilKbit = int(burstBytesPerSec * 8 / 1000)
	if b.burstCeilKbit < 1000 {
		b.burstCeilKbit = 1000
	}

	shapeAt := min64(b.capacity/4, 20*1048576*int64(tickSec))
	unshapeAt := shapeAt * 3

	if b.mode == ModeTurbo {
		return
	}
	if b.mode == ModeBurst && b.tokens < shapeAt {
		b.mode = ModeSustained
	} else if b.mode == ModeSustained && b.tokens > unshapeAt {
		b.mode = ModeBurst
	}
}

// Drain removes bytes from the bucket. Returns actual drained amount.
func (b *DeviceBucket) Drain(bytes int64) int64 {
	b.mu.Lock()
	defer b.mu.Unlock()
	if bytes > b.tokens {
		drained := b.tokens
		b.tokens = 0
		return drained
	}
	b.tokens -= bytes
	return bytes
}

// Refill adds bytes to the bucket, capped at current dynamic capacity.
func (b *DeviceBucket) Refill(bytes int64) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.tokens += bytes
	if b.tokens > b.capacity {
		b.tokens = b.capacity
	}
}

// Mode returns the current mode (respects hysteresis).
func (b *DeviceBucket) Mode() DeviceMode {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.mode
}

// SetMode forces a mode (used for turbo management).
func (b *DeviceBucket) SetMode(m DeviceMode) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.mode = m
}

// Tokens returns current token count.
func (b *DeviceBucket) Tokens() int64 {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.tokens
}

// SetTokens sets the token count directly (used for manual bucket set).
func (b *DeviceBucket) SetTokens(t int64) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.tokens = t
	if b.tokens > b.capacity {
		b.tokens = b.capacity
	}
	if b.tokens < 0 {
		b.tokens = 0
	}
}

// Capacity returns current dynamic capacity.
func (b *DeviceBucket) Capacity() int64 {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.capacity
}

// BurstCeilKbit returns the current burst ceiling in kbit/s.
func (b *DeviceBucket) BurstCeilKbit() int {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.burstCeilKbit
}

// IsFull returns true if bucket is at capacity.
func (b *DeviceBucket) IsFull() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.tokens >= b.capacity
}

func min64(a, b int64) int64 {
	if a < b {
		return a
	}
	return b
}
