use crate::model::DeviceMode;

const BURST_CEIL_FLOOR_KBIT: i32 = 1000;

/// Per-device byte token bucket with hysteresis-based mode transitions.
/// See `DeviceMode` for mode descriptions.
pub struct DeviceBucket {
    tokens: i64,
    capacity: i64,
    mode: DeviceMode,
    burst_ceil_kbit: i32,
    shape_at: i64,
    unshape_at: i64,
}

impl DeviceBucket {
    /// Create a bucket starting in SUSTAINED mode with empty tokens.
    /// New devices must earn their burst capacity through the refill cycle.
    pub fn new(curve_rate_bytes_per_sec: i64, duration_sec: i32) -> Self {
        let cap = curve_rate_bytes_per_sec * duration_sec as i64;
        Self {
            tokens: 0,
            capacity: cap,
            mode: DeviceMode::Sustained,
            burst_ceil_kbit: 0,
            shape_at: 0,
            unshape_at: 0,
        }
    }

    /// Recalculate capacity, burst ceiling, and hysteresis thresholds.
    ///
    /// Hysteresis creates a dead zone between shape_at and unshape_at to prevent
    /// mode flapping. shape_at = bytes drained in one tick at max burst speed
    /// (capped at 25% capacity to prevent oscillation in small buckets).
    /// unshape_at = 3× shape_at, giving a wide dead zone for stability.
    pub fn update(
        &mut self,
        curve_rate_bytes_per_sec: i64,
        duration_sec: i32,
        tick_sec: i32,
        max_burst_kbit: i32,
    ) {
        self.capacity = curve_rate_bytes_per_sec * duration_sec as i64;

        if self.tokens > self.capacity {
            self.tokens = self.capacity;
        }

        let max_burst_bytes_per_tick =
            max_burst_kbit as i64 * 1000 / 8 * tick_sec as i64;
        self.shape_at = max_burst_bytes_per_tick;

        // Cap at 25% of capacity to prevent oscillation in small buckets
        let cap_quarter = self.capacity / 4;
        if self.shape_at > cap_quarter {
            self.shape_at = cap_quarter;
        }
        if self.shape_at < 1 {
            self.shape_at = 1;
        }

        self.unshape_at = self.shape_at * 3;

        // Derive effective burst ceiling from (possibly capped) shape_at
        let burst_bytes_per_sec = self.shape_at as f64 / tick_sec as f64;
        self.burst_ceil_kbit = (burst_bytes_per_sec * 8.0 / 1000.0) as i32;
        // Floor prevents tc from rejecting sub-kbit rates
        if self.burst_ceil_kbit < BURST_CEIL_FLOOR_KBIT {
            self.burst_ceil_kbit = BURST_CEIL_FLOOR_KBIT;
        }
        let shape_at = self.shape_at;
        let unshape_at = self.unshape_at;

        if self.mode == DeviceMode::Turbo {
            return;
        }
        if self.mode == DeviceMode::Burst && self.tokens < shape_at {
            self.mode = DeviceMode::Sustained;
        } else if self.mode == DeviceMode::Sustained && self.tokens > unshape_at {
            self.mode = DeviceMode::Burst;
        }
    }

    /// Remove bytes from the bucket. Returns actual drained amount.
    pub fn drain(&mut self, bytes: i64) -> i64 {
        if bytes > self.tokens {
            let drained = self.tokens;
            self.tokens = 0;
            drained
        } else {
            self.tokens -= bytes;
            bytes
        }
    }

    /// Add bytes to the bucket, capped at current dynamic capacity.
    pub fn refill(&mut self, bytes: i64) {
        self.tokens += bytes;
        if self.tokens > self.capacity {
            self.tokens = self.capacity;
        }
    }

    /// Current mode (respects hysteresis).
    pub fn mode(&self) -> DeviceMode {
        self.mode
    }

    /// Force a mode (used for turbo management).
    pub fn set_mode(&mut self, m: DeviceMode) {
        self.mode = m;
    }

    /// Current token count.
    pub fn tokens(&self) -> i64 {
        self.tokens
    }

    /// Set the token count directly (used for manual bucket set).
    pub fn set_tokens(&mut self, t: i64) {
        self.tokens = t;
        if self.tokens > self.capacity {
            self.tokens = self.capacity;
        }
        if self.tokens < 0 {
            self.tokens = 0;
        }
    }

    /// Current dynamic capacity.
    pub fn capacity(&self) -> i64 {
        self.capacity
    }

    /// Current burst ceiling in kbit/s.
    pub fn burst_ceil_kbit(&self) -> i32 {
        self.burst_ceil_kbit
    }

    /// Hysteresis thresholds: (shape_at, unshape_at) in bytes.
    pub fn thresholds(&self) -> (i64, i64) {
        (self.shape_at, self.unshape_at)
    }

    /// True if bucket is at capacity.
    pub fn is_full(&self) -> bool {
        self.tokens >= self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a full bucket for tests that need one.
    fn full_bucket(rate: i64, dur: i32) -> DeviceBucket {
        let mut b = DeviceBucket::new(rate, dur);
        b.refill(b.capacity()); // fill to capacity
        b.set_mode(DeviceMode::Burst);
        b
    }

    #[test]
    fn new_device_bucket() {
        // 50 Mbps = 6250000 bytes/sec, 300s duration
        let b = DeviceBucket::new(6_250_000, 300);

        assert_eq!(b.capacity(), 6_250_000 * 300);
        assert_eq!(b.tokens(), 0, "new devices start empty");
        assert_eq!(b.mode(), DeviceMode::Sustained, "new devices start sustained");
        assert!(!b.is_full());
    }

    #[test]
    fn drain_and_refill() {
        let mut b = full_bucket(6_250_000, 300);
        let cap = b.capacity();

        // Drain some bytes
        let drained = b.drain(1_000_000);
        assert_eq!(drained, 1_000_000);
        assert_eq!(b.tokens(), cap - 1_000_000);

        // Drain more than available
        let mut b2 = full_bucket(100, 1); // 100 byte capacity
        b2.drain(50);
        let drained = b2.drain(100); // only 50 left
        assert_eq!(drained, 50);
        assert_eq!(b2.tokens(), 0);

        // Refill
        b2.refill(30);
        assert_eq!(b2.tokens(), 30);

        // Refill beyond capacity
        b2.refill(200);
        assert_eq!(b2.tokens(), b2.capacity());
    }

    #[test]
    fn capacity_shrink() {
        // Start with high curve rate
        let mut b = full_bucket(6_250_000, 300); // ~1875 MB
        let initial_cap = b.capacity();

        // Simulate curve rate dropping (quota depleting)
        // New curve rate = 1 Mbps = 125000 bytes/sec
        b.update(125_000, 300, 2, 300_000); // capacity = 37.5 MB

        let new_cap = b.capacity();
        assert!(
            new_cap < initial_cap,
            "capacity should shrink: {new_cap} >= {initial_cap}"
        );
        assert_eq!(new_cap, 125_000 * 300);

        // Tokens should be clamped to new capacity
        assert!(
            b.tokens() <= new_cap,
            "tokens {} > capacity {} after shrink",
            b.tokens(),
            new_cap
        );
    }

    #[test]
    fn hysteresis() {
        // 50 Mbps = 6250000 bytes/sec, 300s, tick=2s, max_burst=300Mbps
        // capacity = 1875 MB
        // max_burst_bytes_per_tick = 300000 * 1000 / 8 * 2 = 75 MB
        // cap_quarter = 468.75 MB
        // shape_at = min(75MB, 468.75MB) = 75 MB
        // unshape_at = 75 * 3 = 225 MB
        let mut b = full_bucket(6_250_000, 300);
        b.update(6_250_000, 300, 2, 300_000);

        assert_eq!(b.mode(), DeviceMode::Burst);

        // Drain everything → below shape_at
        b.drain(b.tokens());
        b.update(6_250_000, 300, 2, 300_000);
        assert_eq!(b.mode(), DeviceMode::Sustained);

        // Refill to dead zone (between shape_at=75MB and unshape_at=225MB)
        b.refill(150 * 1_048_576); // 150 MB
        b.update(6_250_000, 300, 2, 300_000);
        assert_eq!(
            b.mode(),
            DeviceMode::Sustained,
            "in dead zone should stay Sustained"
        );

        // Refill above unshape_at
        b.refill(300 * 1_048_576); // well above 225 MB
        b.update(6_250_000, 300, 2, 300_000);
        assert_eq!(b.mode(), DeviceMode::Burst);
    }

    #[test]
    fn burst_ceil() {
        // 50 Mbps = 6250000 bytes/sec, 300s, tick=2s, max_burst=300Mbps
        // shape_at = 75 MB (not capped, < cap/4)
        // burst_ceil = shape_at / tick × 8 / 1000 = 75MB/2 × 8/1000 = 300000 kbit
        let mut b = full_bucket(6_250_000, 300);
        b.update(6_250_000, 300, 2, 300_000);

        let ceil = b.burst_ceil_kbit();
        assert_eq!(ceil, 300_000, "burst ceil should be 300 Mbps");

        // Burst ceil doesn't depend on tokens
        b.drain(b.tokens());
        b.refill(56 * 1_048_576);
        b.update(6_250_000, 300, 2, 300_000);

        let ceil = b.burst_ceil_kbit();
        assert_eq!(ceil, 300_000, "burst ceil should still be 300 Mbps");
    }

    #[test]
    fn burst_ceil_capped() {
        // Low capacity: 1 Mbps = 125000 bytes/sec, 60s, tick=2s, max_burst=300Mbps
        // capacity = 125000 * 60 = 7,500,000 bytes (7.5 MB)
        // max_burst_bytes_per_tick = 300000 * 1000 / 8 * 2 = 75 MB
        // cap_quarter = 7.5 / 4 = 1.875 MB
        // shape_at = min(75MB, 1.875MB) = 1.875 MB (capped at 25%)
        // burst_ceil = 1,875,000 / 2 × 8/1000 = 7500 kbit
        let mut b = full_bucket(125_000, 60);
        b.update(125_000, 60, 2, 300_000);

        let ceil = b.burst_ceil_kbit();
        assert!(
            ceil >= 7000 && ceil <= 8000,
            "capped burst ceil = {ceil} kbit, want ~7500"
        );
    }

    #[test]
    fn burst_ceil_floor() {
        let mut b = DeviceBucket::new(100, 1); // tiny bucket, starts empty
        b.update(100, 1, 2, 300_000);

        let ceil = b.burst_ceil_kbit();
        assert!(ceil >= BURST_CEIL_FLOOR_KBIT, "burst ceil floor = {ceil}, want >= {BURST_CEIL_FLOOR_KBIT}");
    }

    #[test]
    fn turbo_mode() {
        let mut b = full_bucket(6_250_000, 300);
        b.set_mode(DeviceMode::Turbo);

        // Turbo should not be changed by Update
        b.drain(b.tokens());
        b.update(6_250_000, 300, 2, 300_000);

        assert_eq!(b.mode(), DeviceMode::Turbo);

        // Cancel turbo
        b.set_mode(DeviceMode::Burst);
        b.update(6_250_000, 300, 2, 300_000);
        // With 0 tokens, should transition to SUSTAINED
        assert_eq!(b.mode(), DeviceMode::Sustained);
    }

    #[test]
    fn set_tokens() {
        let mut b = DeviceBucket::new(6_250_000, 300);

        b.set_tokens(500 * 1_048_576); // 500 MB
        assert_eq!(b.tokens(), 500 * 1_048_576);

        // Over capacity
        b.set_tokens(99_999_999_999);
        assert_eq!(b.tokens(), b.capacity());

        // Negative
        b.set_tokens(-100);
        assert_eq!(b.tokens(), 0);
    }
}
