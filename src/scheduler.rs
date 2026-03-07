// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Frequency cycle scheduler.
//!
//! Manages a list of cycle items, each specifying a frequency delta and
//! duration. The scheduler counts samples and advances through the cycle,
//! looping indefinitely. All operations are real-time safe.

/// A single step in the frequency cycle.
#[derive(Debug, Clone, Copy)]
pub struct CycleItem {
    /// Frequency delta in Hz (added to carrier for binaural, or used as AM
    /// frequency in non-binaural mode).
    pub frequency_delta: f32,
    /// Duration of this step in seconds.
    pub duration_seconds: f32,
}

/// Sample-counted frequency cycle scheduler.
pub struct Scheduler {
    /// Pre-allocated cycle items.
    items: Vec<CycleItem>,
    /// Index of the currently active cycle item.
    current_index: usize,
    /// Samples remaining before switching to the next item.
    samples_remaining: u64,
    /// Sample rate used to convert seconds to sample counts.
    sample_rate: f32,
    /// Current frequency delta (cached from the active item).
    current_delta: f32,
}

impl Scheduler {
    /// Create a new scheduler with the given cycle items and sample rate.
    pub fn new(items: Vec<CycleItem>, sample_rate: f32) -> Self {
        let current_delta = items.first().map_or(0.0, |item| item.frequency_delta);
        let samples_remaining = items
            .first()
            .map_or(0, |item| (item.duration_seconds * sample_rate) as u64);

        Self {
            items,
            current_index: 0,
            samples_remaining,
            sample_rate,
            current_delta,
        }
    }

    /// Advance the scheduler by one sample frame. Returns `true` if the
    /// active cycle item changed this frame.
    ///
    /// Real-time safe: no allocation, deterministic execution time.
    #[inline]
    pub fn advance(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }

        self.samples_remaining = self.samples_remaining.saturating_sub(1);

        if self.samples_remaining == 0 {
            self.current_index = (self.current_index + 1) % self.items.len();
            let item = &self.items[self.current_index];
            self.current_delta = item.frequency_delta;
            self.samples_remaining = (item.duration_seconds * self.sample_rate) as u64;
            return true;
        }

        false
    }

    /// The current frequency delta value.
    #[inline]
    pub fn current_delta(&self) -> f32 {
        self.current_delta
    }

    /// The index of the currently active cycle item.
    #[inline]
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Replace the cycle items. Resets playback to the first item.
    ///
    /// This function allocates and must NOT be called from the audio callback.
    /// Use the engine's pending config mechanism instead.
    pub fn set_items(&mut self, items: Vec<CycleItem>) {
        self.current_index = 0;
        self.current_delta = items.first().map_or(0.0, |item| item.frequency_delta);
        self.samples_remaining = items
            .first()
            .map_or(0, |item| (item.duration_seconds * self.sample_rate) as u64);
        self.items = items;
    }

    /// Reset to the beginning of the cycle.
    pub fn reset(&mut self) {
        self.current_index = 0;
        self.current_delta = self
            .items
            .first()
            .map_or(0.0, |item| item.frequency_delta);
        self.samples_remaining = self
            .items
            .first()
            .map_or(0, |item| (item.duration_seconds * self.sample_rate) as u64);
    }

    /// Number of cycle items.
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the schedule is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_through_items() {
        let items = vec![
            CycleItem {
                frequency_delta: 3.0,
                duration_seconds: 0.001,
            },
            CycleItem {
                frequency_delta: 5.0,
                duration_seconds: 0.001,
            },
        ];
        let mut sched = Scheduler::new(items, 48000.0);

        assert_eq!(sched.current_delta(), 3.0);
        assert_eq!(sched.current_index(), 0);

        // Advance 48 samples (0.001s at 48kHz)
        for _ in 0..48 {
            sched.advance();
        }

        assert_eq!(sched.current_delta(), 5.0);
        assert_eq!(sched.current_index(), 1);

        // Advance another 48 samples to wrap around
        for _ in 0..48 {
            sched.advance();
        }

        assert_eq!(sched.current_delta(), 3.0);
        assert_eq!(sched.current_index(), 0);
    }

    #[test]
    fn empty_schedule_returns_zero_delta() {
        let mut sched = Scheduler::new(vec![], 48000.0);
        assert_eq!(sched.current_delta(), 0.0);
        assert!(!sched.advance());
    }

    #[test]
    fn single_item_stays_forever() {
        let items = vec![CycleItem {
            frequency_delta: 7.0,
            duration_seconds: 1.0,
        }];
        let mut sched = Scheduler::new(items, 48000.0);

        for _ in 0..48000 {
            sched.advance();
        }

        // After one full duration it wraps to the same (only) item
        assert_eq!(sched.current_delta(), 7.0);
        assert_eq!(sched.current_index(), 0);
    }

    #[test]
    fn set_items_resets_state() {
        let items = vec![
            CycleItem {
                frequency_delta: 3.0,
                duration_seconds: 1.0,
            },
        ];
        let mut sched = Scheduler::new(items, 48000.0);

        // Advance partway
        for _ in 0..10000 {
            sched.advance();
        }

        sched.set_items(vec![CycleItem {
            frequency_delta: 10.0,
            duration_seconds: 2.0,
        }]);

        assert_eq!(sched.current_delta(), 10.0);
        assert_eq!(sched.current_index(), 0);
    }
}
