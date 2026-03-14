package engine

import (
	"testing"
)

func TestCurveRate(t *testing.T) {
	c := CurveParams{
		MaxRateKbit: 50000,
		MinRateKbit: 1000,
		Shape:       0.40,
		TotalBytes:  20 * 1073741824, // 20 GB
	}

	tests := []struct {
		name      string
		remaining int64
		wantMin   int
		wantMax   int
	}{
		{"100% remaining", 20 * 1073741824, 49000, 50001},
		{"0% remaining", 0, 1000, 1001},
		{"50% remaining", 10 * 1073741824, 30000, 42000},
		{"10% remaining", 2 * 1073741824, 15000, 25000},
		{"1% remaining", 214748364, 5000, 12000},
		{"negative remaining", -1073741824, 1000, 1001},
		{"over 100% remaining", 25 * 1073741824, 49000, 50001},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rate := c.Rate(tt.remaining)
			if rate < tt.wantMin || rate > tt.wantMax {
				t.Errorf("Rate(%d) = %d, want between %d and %d", tt.remaining, rate, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestCurveRateFullRange(t *testing.T) {
	c := CurveParams{
		MaxRateKbit: 50000,
		MinRateKbit: 1000,
		Shape:       0.40,
		TotalBytes:  20 * 1073741824,
	}

	// Rate should monotonically decrease as remaining decreases
	prevRate := c.Rate(c.TotalBytes)
	for pct := 99; pct >= 0; pct-- {
		remaining := c.TotalBytes * int64(pct) / 100
		rate := c.Rate(remaining)
		if rate > prevRate {
			t.Errorf("Rate not monotonically decreasing: at %d%%, rate=%d > prev=%d", pct, rate, prevRate)
		}
		prevRate = rate
	}
}

func TestCurveShapeExponents(t *testing.T) {
	total := int64(20 * 1073741824)
	half := total / 2

	// Lower shape = higher rate at 50% (more aggressive curve)
	shapes := []float64{0.20, 0.40, 0.60, 1.00}
	var prevRate int
	for _, shape := range shapes {
		c := CurveParams{
			MaxRateKbit: 50000,
			MinRateKbit: 1000,
			Shape:       shape,
			TotalBytes:  total,
		}
		rate := c.Rate(half)
		if prevRate > 0 && rate >= prevRate {
			t.Errorf("Shape %.2f rate=%d should be less than shape %.2f rate=%d at 50%%",
				shape, rate, shapes[0], prevRate)
		}
		prevRate = rate
	}
}

func TestCurveRateBytesPerSec(t *testing.T) {
	c := CurveParams{
		MaxRateKbit: 50000,
		MinRateKbit: 1000,
		Shape:       0.40,
		TotalBytes:  20 * 1073741824,
	}

	bps := c.RateBytesPerSec(c.TotalBytes)
	expectedBps := int64(50000) * 1000 / 8
	if bps != expectedBps {
		t.Errorf("RateBytesPerSec at full = %d, want %d", bps, expectedBps)
	}
}
