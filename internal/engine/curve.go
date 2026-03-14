package engine

import "math"

// CurveParams defines the power curve for sustained rate calculation.
type CurveParams struct {
	MaxRateKbit int
	MinRateKbit int
	Shape       float64
	TotalBytes  int64
}

// Rate returns the sustained rate in kbit/s for the given remaining bytes.
func (c *CurveParams) Rate(remainingBytes int64) int {
	ratio := float64(remainingBytes) / float64(c.TotalBytes)
	if ratio < 0 {
		ratio = 0
	}
	if ratio > 1 {
		ratio = 1
	}

	curved := math.Pow(ratio, c.Shape)
	rate := float64(c.MinRateKbit) + float64(c.MaxRateKbit-c.MinRateKbit)*curved
	return int(rate)
}

// RateBytesPerSec returns the sustained rate in bytes/sec.
func (c *CurveParams) RateBytesPerSec(remainingBytes int64) int64 {
	kbit := c.Rate(remainingBytes)
	return int64(kbit) * 1000 / 8
}
