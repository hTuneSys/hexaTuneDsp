// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! PRNG-based event scheduler for random one-shot sounds.
//!
//! Uses a Xorshift64 pseudo-random number generator to determine when to
//! trigger the next event. All operations are real-time safe: no allocation,
//! no I/O, deterministic execution.

/// Maximum number of event slots.
pub const MAX_EVENT_SLOTS: usize = 5;

/// Xorshift64 pseudo-random number generator.
///
/// Lightweight, allocation-free PRNG suitable for audio-thread use.
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    /// Create a new PRNG with the given seed. Seed must not be zero.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Generate the next pseudo-random u64.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Generate a random f32 in the range [0.0, 1.0).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Generate a random u64 in the range [min, max] (inclusive).
    #[inline]
    pub fn range_u64(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let range = max - min + 1;
        min + (self.next_u64() % range)
    }

    /// Generate a random f32 in the range [min, max].
    #[inline]
    pub fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        if min >= max {
            return min;
        }
        min + self.next_f32() * (max - min)
    }
}

/// Scheduling parameters for a single event slot.
#[derive(Debug, Clone)]
pub struct EventTiming {
    /// Minimum interval between triggers in milliseconds.
    pub min_interval_ms: u32,
    /// Maximum interval between triggers in milliseconds.
    pub max_interval_ms: u32,
}

/// Sample-based event scheduler.
///
/// Counts down samples until the next event trigger, then selects which
/// event slot to play. After triggering, schedules the next event.
pub struct EventScheduler {
    rng: Xorshift64,
    /// Samples remaining until the next event trigger.
    samples_until_next: u64,
    sample_rate: f32,
    /// Whether the scheduler is actively scheduling events.
    enabled: bool,
    /// Index of the next slot to play (round-robin among available).
    next_slot: usize,
    /// Number of slots currently registered (set externally).
    slot_count: usize,
}

impl EventScheduler {
    /// Create a new scheduler.
    pub fn new(sample_rate: f32, seed: u64) -> Self {
        Self {
            rng: Xorshift64::new(seed),
            samples_until_next: 0,
            sample_rate,
            enabled: false,
            next_slot: 0,
            slot_count: 0,
        }
    }

    /// Enable or disable the scheduler.
    #[inline]
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Update the number of available event slots.
    pub fn set_slot_count(&mut self, count: usize) {
        self.slot_count = count;
        if count == 0 {
            self.enabled = false;
        }
    }

    /// Schedule the next event trigger using the given timing parameters.
    /// Picks a random interval between min and max in milliseconds, then
    /// converts to sample count.
    pub fn schedule_next(&mut self, timing: &EventTiming) {
        let min_ms = timing.min_interval_ms as u64;
        let max_ms = timing.max_interval_ms as u64;
        let interval_ms = self.rng.range_u64(min_ms, max_ms);
        self.samples_until_next = (interval_ms as f32 * self.sample_rate / 1000.0) as u64;
    }

    /// Schedule next with explicit min/max (used when aggregating across slots).
    pub fn schedule_next_with_range(&mut self, min_ms: u32, max_ms: u32) {
        let min = min_ms as u64;
        let max = max_ms as u64;
        let interval_ms = self.rng.range_u64(min, max);
        self.samples_until_next = (interval_ms as f32 * self.sample_rate / 1000.0) as u64;
    }

    /// Advance the scheduler by one sample frame.
    ///
    /// Returns `Some(slot_index)` if an event should be triggered this frame,
    /// `None` otherwise. The caller is responsible for calling `schedule_next`
    /// after handling the trigger.
    ///
    /// Real-time safe: no allocation, deterministic.
    #[inline]
    pub fn advance(&mut self) -> Option<usize> {
        if !self.enabled || self.slot_count == 0 {
            return None;
        }

        if self.samples_until_next > 0 {
            self.samples_until_next -= 1;
        }

        if self.samples_until_next == 0 {
            let slot = self.next_slot % self.slot_count;
            self.next_slot = (self.next_slot + 1) % self.slot_count;
            // Set to u64::MAX so we don't re-trigger on the next frame.
            // The caller must call schedule_next to arm the next trigger.
            self.samples_until_next = u64::MAX;
            return Some(slot);
        }

        None
    }

    /// Get a random f32 value in [min, max] from the scheduler's PRNG.
    /// Useful for randomizing volume and pan at trigger time.
    #[inline]
    pub fn random_f32(&mut self, min: f32, max: f32) -> f32 {
        self.rng.range_f32(min, max)
    }

    /// Whether the scheduler is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift64_produces_different_values() {
        let mut rng = Xorshift64::new(42);
        let a = rng.next_u64();
        let b = rng.next_u64();
        let c = rng.next_u64();
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn xorshift64_f32_in_range() {
        let mut rng = Xorshift64::new(123);
        for _ in 0..1000 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn xorshift64_range_u64_respects_bounds() {
        let mut rng = Xorshift64::new(7);
        for _ in 0..1000 {
            let v = rng.range_u64(10, 20);
            assert!((10..=20).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn xorshift64_range_f32_respects_bounds() {
        let mut rng = Xorshift64::new(99);
        for _ in 0..1000 {
            let v = rng.range_f32(-0.5, 0.5);
            assert!((-0.5..=0.5).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn scheduler_disabled_returns_none() {
        let mut sched = EventScheduler::new(48000.0, 42);
        sched.set_slot_count(2);
        // Not enabled
        assert!(sched.advance().is_none());
    }

    #[test]
    fn scheduler_triggers_at_zero_countdown() {
        let mut sched = EventScheduler::new(48000.0, 42);
        sched.set_slot_count(2);
        sched.set_enabled(true);
        // samples_until_next is 0, so first advance triggers
        let result = sched.advance();
        assert_eq!(result, Some(0));
    }

    #[test]
    fn scheduler_counts_down() {
        let mut sched = EventScheduler::new(48000.0, 42);
        sched.set_slot_count(1);
        sched.set_enabled(true);

        // Consume the initial trigger (counter starts at 0)
        let initial = sched.advance();
        assert_eq!(initial, Some(0));

        // Schedule 1ms from now = 48 samples
        sched.schedule_next_with_range(1, 1);

        // Should not trigger for 47 advances
        for _ in 0..47 {
            assert!(sched.advance().is_none());
        }
        // Should trigger at the 48th advance
        let result = sched.advance();
        assert_eq!(result, Some(0));
    }

    #[test]
    fn scheduler_round_robins_slots() {
        let mut sched = EventScheduler::new(48000.0, 42);
        sched.set_slot_count(3);
        sched.set_enabled(true);

        // First trigger
        let a = sched.advance().unwrap();
        assert_eq!(a, 0);

        // Immediately trigger again (don't schedule wait)
        sched.samples_until_next = 0;
        let b = sched.advance().unwrap();
        assert_eq!(b, 1);

        sched.samples_until_next = 0;
        let c = sched.advance().unwrap();
        assert_eq!(c, 2);

        sched.samples_until_next = 0;
        let d = sched.advance().unwrap();
        assert_eq!(d, 0); // wraps around
    }

    #[test]
    fn zero_seed_becomes_one() {
        let mut rng = Xorshift64::new(0);
        // Should not produce all zeros
        let v = rng.next_u64();
        assert_ne!(v, 0);
    }
}
