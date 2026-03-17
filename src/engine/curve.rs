/// Power curve parameters for sustained rate calculation.
///
/// Maps remaining quota bytes to a sustained rate using:
///   rate = min_rate + (max_rate - min_rate) * (remaining / total)^shape
pub struct CurveParams {
    pub max_rate_kbit: i32,
    pub min_rate_kbit: i32,
    pub shape: f64,
    pub total_bytes: i64,
}

impl CurveParams {
    /// Returns the sustained rate in kbit/s for the given remaining bytes.
    pub fn rate(&self, remaining_bytes: i64) -> i32 {
        let ratio = (remaining_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0);

        let curved = ratio.powf(self.shape);
        let rate =
            self.min_rate_kbit as f64 + (self.max_rate_kbit - self.min_rate_kbit) as f64 * curved;
        rate as i32
    }

    /// Returns the sustained rate in bytes/sec.
    pub fn rate_bytes_per_sec(&self, remaining_bytes: i64) -> i64 {
        let kbit = self.rate(remaining_bytes);
        kbit as i64 * 1000 / 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_curve() -> CurveParams {
        CurveParams {
            max_rate_kbit: 50000,
            min_rate_kbit: 1000,
            shape: 0.40,
            total_bytes: 20 * 1_000_000_000, // 20 GB
        }
    }

    #[test]
    fn curve_rate() {
        let c = test_curve();

        let tests = [
            ("100% remaining", 20i64 * 1_000_000_000, 49000, 50001),
            ("0% remaining", 0, 1000, 1001),
            ("50% remaining", 10 * 1_000_000_000, 30000, 42000),
            ("10% remaining", 2 * 1_000_000_000, 15000, 25000),
            ("1% remaining", 200_000_000, 5000, 12000),
            ("negative remaining", -1_000_000_000, 1000, 1001),
            ("over 100% remaining", 25 * 1_000_000_000, 49000, 50001),
        ];

        for (name, remaining, want_min, want_max) in &tests {
            let rate = c.rate(*remaining);
            assert!(
                rate >= *want_min && rate <= *want_max,
                "{name}: Rate({remaining}) = {rate}, want between {want_min} and {want_max}"
            );
        }
    }

    #[test]
    fn curve_rate_full_range() {
        let c = test_curve();

        // Rate should monotonically decrease as remaining decreases
        let mut prev_rate = c.rate(c.total_bytes);
        for pct in (0..100).rev() {
            let remaining = c.total_bytes * pct / 100;
            let rate = c.rate(remaining);
            assert!(
                rate <= prev_rate,
                "Rate not monotonically decreasing: at {pct}%, rate={rate} > prev={prev_rate}"
            );
            prev_rate = rate;
        }
    }

    #[test]
    fn curve_shape_exponents() {
        let total = 20i64 * 1_000_000_000;
        let half = total / 2;

        // Lower shape = higher rate at 50% (more aggressive curve)
        let shapes = [0.20, 0.40, 0.60, 1.00];
        let mut prev_rate = 0;
        for (i, &shape) in shapes.iter().enumerate() {
            let c = CurveParams {
                max_rate_kbit: 50000,
                min_rate_kbit: 1000,
                shape,
                total_bytes: total,
            };
            let rate = c.rate(half);
            if i > 0 {
                assert!(
                    rate < prev_rate,
                    "Shape {shape:.2} rate={rate} should be less than previous rate={prev_rate} at 50%"
                );
            }
            prev_rate = rate;
        }
    }

    #[test]
    fn curve_rate_bytes_per_sec() {
        let c = test_curve();

        let bps = c.rate_bytes_per_sec(c.total_bytes);
        let expected_bps = 50000i64 * 1000 / 8;
        assert_eq!(bps, expected_bps);
    }
}
