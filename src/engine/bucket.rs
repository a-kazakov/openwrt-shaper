use crate::model::DeviceMode;

/// Per-device byte token bucket with dynamic capacity and hysteresis-based
/// mode transitions.
///
/// Modes:
/// - Burst: tokens available → ceiling proportional to bucket size
/// - Sustained: tokens depleted → fair share (80/20 down/up split)
/// - Turbo: manual override → uncapped, time-limited
pub struct DeviceBucket {
    tokens: i64,
    capacity: i64,
    mode: DeviceMode,
    burst_ceil_kbit: i32,
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
        }
    }

    /// Recalculate capacity from current curve rate, clamp tokens,
    /// compute burst ceiling, and apply hysteresis for mode transitions.
    pub fn update(
        &mut self,
        curve_rate_bytes_per_sec: i64,
        duration_sec: i32,
        tick_sec: i32,
        burst_drain_ratio: f64,
    ) {
        self.capacity = curve_rate_bytes_per_sec * duration_sec as i64;

        if self.tokens > self.capacity {
            self.tokens = self.capacity;
        }

        let burst_bytes_per_sec =
            self.tokens as f64 * burst_drain_ratio / tick_sec as f64;
        self.burst_ceil_kbit = (burst_bytes_per_sec * 8.0 / 1000.0) as i32;
        if self.burst_ceil_kbit < 1000 {
            self.burst_ceil_kbit = 1000;
        }

        let shape_at = (self.capacity / 4).min(20 * 1_048_576 * tick_sec as i64);
        let unshape_at = shape_at * 3;

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
        b.update(125_000, 300, 2, 0.10); // capacity = 37.5 MB

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
        // 50 Mbps = 6250000 bytes/sec, 300s, tick=2s
        let mut b = full_bucket(6_250_000, 300);
        b.update(6_250_000, 300, 2, 0.10);

        // Start in BURST mode (full bucket)
        assert_eq!(b.mode(), DeviceMode::Burst);

        // Drain to below shape_threshold
        // shape_threshold = min(cap/4, 20*1048576*2) = min(468MB, 40MB) = 40MB
        // Drain almost everything
        b.drain(b.tokens());
        b.update(6_250_000, 300, 2, 0.10);

        assert_eq!(b.mode(), DeviceMode::Sustained);

        // Refill to dead zone (between shape and unshape thresholds)
        // unshape_threshold = 40MB * 3 = 120MB
        b.refill(80 * 1_048_576); // 80 MB - in dead zone
        b.update(6_250_000, 300, 2, 0.10);

        assert_eq!(
            b.mode(),
            DeviceMode::Sustained,
            "in dead zone should stay Sustained"
        );

        // Refill above unshape_threshold
        b.refill(200 * 1_048_576); // well above 120 MB
        b.update(6_250_000, 300, 2, 0.10);

        assert_eq!(b.mode(), DeviceMode::Burst);
    }

    #[test]
    fn burst_ceil() {
        // 50 Mbps = 6250000 bytes/sec, 300s, tick=2s, drain_ratio=0.10
        let mut b = full_bucket(6_250_000, 300);
        b.update(6_250_000, 300, 2, 0.10);

        let ceil = b.burst_ceil_kbit();
        // At full: tokens=1875MB, burst = 1875MB * 0.10 / 2 = 93.75 MB/s = 750 Mbps = 750000 kbit
        assert!(
            ceil >= 700_000 && ceil <= 800_000,
            "full bucket burst ceil = {ceil} kbit, want ~750000"
        );

        // Drain to small bucket
        b.drain(b.tokens());
        b.refill(56 * 1_048_576); // 56 MB
        b.update(6_250_000, 300, 2, 0.10);

        let ceil = b.burst_ceil_kbit();
        // 56MB * 0.10 / 2 = 2.8 MB/s = ~22.4 Mbps = ~22400 kbit
        assert!(
            ceil >= 20_000 && ceil <= 25_000,
            "small bucket burst ceil = {ceil} kbit, want ~22400"
        );
    }

    #[test]
    fn burst_ceil_floor() {
        let mut b = DeviceBucket::new(100, 1); // tiny bucket, starts empty
        b.update(100, 1, 2, 0.10);

        let ceil = b.burst_ceil_kbit();
        assert!(ceil >= 1000, "burst ceil floor = {ceil}, want >= 1000");
    }

    #[test]
    fn turbo_mode() {
        let mut b = full_bucket(6_250_000, 300);
        b.set_mode(DeviceMode::Turbo);

        // Turbo should not be changed by Update
        b.drain(b.tokens());
        b.update(6_250_000, 300, 2, 0.10);

        assert_eq!(b.mode(), DeviceMode::Turbo);

        // Cancel turbo
        b.set_mode(DeviceMode::Burst);
        b.update(6_250_000, 300, 2, 0.10);
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
