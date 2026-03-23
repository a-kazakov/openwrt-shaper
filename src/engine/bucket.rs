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
    /// Called for ALL devices every tick (including overridden) so that
    /// capacity and thresholds stay current when devices join/leave.
    ///
    /// Does NOT evaluate mode transitions — call `evaluate_mode` separately
    /// for devices that should participate in hysteresis.
    pub fn update_params(
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
    }

    /// Evaluate hysteresis mode transitions based on current token level.
    /// Only call for devices not under a user override (turbo/throttled/disabled
    /// are managed externally).
    pub fn evaluate_mode(&mut self) {
        if self.mode == DeviceMode::Turbo {
            return;
        }
        if self.mode == DeviceMode::Burst && self.tokens < self.shape_at {
            self.mode = DeviceMode::Sustained;
        } else if self.mode == DeviceMode::Sustained && self.tokens > self.unshape_at {
            self.mode = DeviceMode::Burst;
        }
    }

    /// Combined update_params + evaluate_mode (convenience for callers
    /// that always want both, e.g. tests and initial setup).
    pub fn update(
        &mut self,
        curve_rate_bytes_per_sec: i64,
        duration_sec: i32,
        tick_sec: i32,
        max_burst_kbit: i32,
    ) {
        self.update_params(curve_rate_bytes_per_sec, duration_sec, tick_sec, max_burst_kbit);
        self.evaluate_mode();
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

    /// Bytes of space remaining before capacity is reached.
    pub fn space_remaining(&self) -> i64 {
        (self.capacity - self.tokens).max(0)
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

    // === Tests for update_params / evaluate_mode split ===

    /// update_params updates capacity and thresholds without touching mode.
    /// This is critical for overridden devices that need current thresholds
    /// but must NOT have their mode changed by hysteresis.
    #[test]
    fn update_params_does_not_change_mode() {
        let mut b = full_bucket(6_250_000, 300);
        assert_eq!(b.mode(), DeviceMode::Burst);

        // Drain all tokens → would transition to Sustained if mode were evaluated
        b.drain(b.tokens());
        assert_eq!(b.tokens(), 0);

        // update_params alone must NOT change mode
        b.update_params(6_250_000, 300, 2, 300_000);
        assert_eq!(b.mode(), DeviceMode::Burst, "update_params must not change mode");

        // Capacity and thresholds should still be updated
        assert_eq!(b.capacity(), 6_250_000 * 300);
        let (shape_at, unshape_at) = b.thresholds();
        assert!(shape_at > 0, "shape_at should be computed");
        assert!(unshape_at > shape_at, "unshape_at > shape_at");
        assert!(b.burst_ceil_kbit() > 0, "burst_ceil should be computed");
    }

    /// evaluate_mode applies hysteresis transitions based on current tokens.
    #[test]
    fn evaluate_mode_applies_transitions() {
        let mut b = full_bucket(6_250_000, 300);
        b.update_params(6_250_000, 300, 2, 300_000);
        assert_eq!(b.mode(), DeviceMode::Burst);

        // Drain below shape_at → should transition
        b.drain(b.tokens());
        b.evaluate_mode();
        assert_eq!(b.mode(), DeviceMode::Sustained);

        // Refill above unshape_at → should transition back
        b.refill(b.capacity());
        b.evaluate_mode();
        assert_eq!(b.mode(), DeviceMode::Burst);
    }

    /// update_params recalculates capacity when curve rate changes.
    /// Verifying overridden devices get correct capacity even without
    /// mode evaluation (important when devices connect/disconnect and
    /// the curve rate shifts).
    #[test]
    fn update_params_tracks_capacity_changes() {
        let mut b = full_bucket(6_250_000, 300);
        let cap_high = b.capacity();

        // Curve rate drops to 1 Mbps
        b.update_params(125_000, 300, 2, 300_000);
        let cap_low = b.capacity();
        assert!(cap_low < cap_high, "capacity should shrink with lower curve rate");
        assert_eq!(cap_low, 125_000 * 300);

        // Tokens clamped to new lower capacity
        assert!(b.tokens() <= cap_low);

        // Curve rate rises back
        b.update_params(6_250_000, 300, 2, 300_000);
        assert_eq!(b.capacity(), cap_high, "capacity should restore");
        // Tokens don't magically increase (were clamped down)
        assert!(b.tokens() <= cap_low, "tokens should not grow on capacity increase");
    }

    /// Combined update() behaves identically to update_params + evaluate_mode.
    #[test]
    fn update_equals_params_plus_evaluate() {
        let mut b1 = full_bucket(6_250_000, 300);
        let mut b2 = full_bucket(6_250_000, 300);

        b1.drain(b1.tokens());
        b2.drain(b2.tokens());

        b1.update(6_250_000, 300, 2, 300_000);

        b2.update_params(6_250_000, 300, 2, 300_000);
        b2.evaluate_mode();

        assert_eq!(b1.mode(), b2.mode());
        assert_eq!(b1.tokens(), b2.tokens());
        assert_eq!(b1.capacity(), b2.capacity());
        assert_eq!(b1.burst_ceil_kbit(), b2.burst_ceil_kbit());
        assert_eq!(b1.thresholds(), b2.thresholds());
    }
}
