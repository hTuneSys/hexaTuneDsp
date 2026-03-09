// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Frequency cycle scheduler.
//!
//! Manages a list of cycle items, each specifying a frequency delta and
//! duration. The scheduler counts samples and advances through the cycle,
//! looping indefinitely. Supports one-shot items (play once, skipped on
//! subsequent iterations) and graceful stop (finish current cycle, then
//! signal completion). All operations are real-time safe.

/// A single step in the frequency cycle.
#[derive(Debug, Clone, Copy)]
pub struct CycleItem {
    /// Frequency delta in Hz (added to carrier for binaural, or used as AM
    /// frequency in non-binaural mode).
    pub frequency_delta: f32,
    /// Duration of this step in seconds.
    pub duration_seconds: f32,
    /// If true, this item plays only during the first cycle iteration and
    /// is skipped on all subsequent iterations.
    pub oneshot: bool,
}

/// Result of a scheduler advance step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceResult {
    /// No item change this frame.
    NoChange,
    /// The active item changed (caller should read `current_delta()`).
    ItemChanged,
    /// All remaining items are oneshot and have been exhausted.
    /// The binaural layer should go silent.
    AllExhausted,
    /// The current cycle iteration is complete and a graceful stop was
    /// requested. The engine should stop.
    CycleCompleteStop,
}

/// Sample-counted frequency cycle scheduler.
pub struct Scheduler {
    items: Vec<CycleItem>,
    current_index: usize,
    samples_remaining: u64,
    sample_rate: f32,
    current_delta: f32,
    /// Number of completed full cycle iterations (0 = still in first).
    cycle_count: u32,
    /// When true, the scheduler will signal stop at the next cycle wrap.
    stop_at_cycle_end: bool,
    /// Set when all non-oneshot items have been exhausted.
    exhausted: bool,
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
            cycle_count: 0,
            stop_at_cycle_end: false,
            exhausted: false,
        }
    }

    /// Advance the scheduler by one sample frame.
    ///
    /// Real-time safe: no allocation, deterministic execution time.
    #[inline]
    pub fn advance(&mut self) -> AdvanceResult {
        if self.items.is_empty() || self.exhausted {
            return AdvanceResult::NoChange;
        }

        self.samples_remaining = self.samples_remaining.saturating_sub(1);

        if self.samples_remaining == 0 {
            return self.move_to_next_item();
        }

        AdvanceResult::NoChange
    }

    /// Find and activate the next valid item after the current one finishes.
    fn move_to_next_item(&mut self) -> AdvanceResult {
        let len = self.items.len();
        let mut next = (self.current_index + 1) % len;

        // Detect cycle wrap
        if next <= self.current_index {
            self.cycle_count += 1;

            if self.stop_at_cycle_end {
                self.stop_at_cycle_end = false;
                self.exhausted = true;
                return AdvanceResult::CycleCompleteStop;
            }
        }

        // Find next non-skipped item. If cycle_count > 0, skip oneshot items.
        // We scan at most `len` items to avoid infinite loops.
        let mut scanned = 0;
        loop {
            if scanned >= len {
                // All items are skippable — exhausted.
                self.exhausted = true;
                return AdvanceResult::AllExhausted;
            }

            let item = &self.items[next];
            let skip = self.cycle_count > 0 && item.oneshot;

            if !skip {
                break;
            }

            // This item is skipped; move to next.
            let prev = next;
            next = (next + 1) % len;
            scanned += 1;

            // Check for wrap during skip scanning
            if next <= prev {
                self.cycle_count += 1;
                if self.stop_at_cycle_end {
                    self.stop_at_cycle_end = false;
                    self.exhausted = true;
                    return AdvanceResult::CycleCompleteStop;
                }
            }
        }

        self.current_index = next;
        let item = &self.items[next];
        self.current_delta = item.frequency_delta;
        self.samples_remaining = (item.duration_seconds * self.sample_rate) as u64;
        AdvanceResult::ItemChanged
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

    /// How many full cycle iterations have completed.
    #[inline]
    pub fn cycle_count(&self) -> u32 {
        self.cycle_count
    }

    /// Whether the scheduler has been exhausted (all items oneshot, past
    /// first iteration).
    #[inline]
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Request that the scheduler signals stop at the end of the current
    /// cycle iteration. Thread-safe to call from the control thread when
    /// used through the engine's atomic flag.
    pub fn set_stop_at_cycle_end(&mut self, stop: bool) {
        self.stop_at_cycle_end = stop;
    }

    /// Whether a graceful stop has been requested.
    #[inline]
    pub fn stop_at_cycle_end(&self) -> bool {
        self.stop_at_cycle_end
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
        self.cycle_count = 0;
        self.stop_at_cycle_end = false;
        self.exhausted = false;
    }

    /// Reset to the beginning of the cycle.
    pub fn reset(&mut self) {
        self.current_index = 0;
        self.current_delta = self.items.first().map_or(0.0, |item| item.frequency_delta);
        self.samples_remaining = self
            .items
            .first()
            .map_or(0, |item| (item.duration_seconds * self.sample_rate) as u64);
        self.cycle_count = 0;
        self.stop_at_cycle_end = false;
        self.exhausted = false;
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

    fn item(delta: f32, dur: f32) -> CycleItem {
        CycleItem {
            frequency_delta: delta,
            duration_seconds: dur,
            oneshot: false,
        }
    }

    fn oneshot_item(delta: f32, dur: f32) -> CycleItem {
        CycleItem {
            frequency_delta: delta,
            duration_seconds: dur,
            oneshot: true,
        }
    }

    /// Advance until a non-NoChange result, returning that result and
    /// the number of frames advanced.
    fn advance_until_change(sched: &mut Scheduler, max: usize) -> (AdvanceResult, usize) {
        for i in 1..=max {
            let r = sched.advance();
            if r != AdvanceResult::NoChange {
                return (r, i);
            }
        }
        (AdvanceResult::NoChange, max)
    }

    #[test]
    fn cycles_through_items() {
        let items = vec![item(3.0, 0.001), item(5.0, 0.001)];
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
    fn empty_schedule_returns_no_change() {
        let mut sched = Scheduler::new(vec![], 48000.0);
        assert_eq!(sched.current_delta(), 0.0);
        assert_eq!(sched.advance(), AdvanceResult::NoChange);
    }

    #[test]
    fn single_item_stays_forever() {
        let items = vec![item(7.0, 1.0)];
        let mut sched = Scheduler::new(items, 48000.0);

        for _ in 0..48000 {
            sched.advance();
        }

        assert_eq!(sched.current_delta(), 7.0);
        assert_eq!(sched.current_index(), 0);
    }

    #[test]
    fn set_items_resets_state() {
        let items = vec![item(3.0, 1.0)];
        let mut sched = Scheduler::new(items, 48000.0);

        for _ in 0..10000 {
            sched.advance();
        }

        sched.set_items(vec![item(10.0, 2.0)]);

        assert_eq!(sched.current_delta(), 10.0);
        assert_eq!(sched.current_index(), 0);
        assert_eq!(sched.cycle_count(), 0);
    }

    // -- Oneshot tests --

    #[test]
    fn oneshot_plays_in_first_iteration() {
        // 3Hz(normal), 4Hz(oneshot), 5Hz(normal) — each 1ms
        let items = vec![item(3.0, 0.001), oneshot_item(4.0, 0.001), item(5.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        // First iteration: should play all three
        let (r, _) = advance_until_change(&mut sched, 100);
        assert_eq!(r, AdvanceResult::ItemChanged);
        assert_eq!(sched.current_delta(), 4.0); // oneshot plays

        let (r, _) = advance_until_change(&mut sched, 100);
        assert_eq!(r, AdvanceResult::ItemChanged);
        assert_eq!(sched.current_delta(), 5.0);
        assert_eq!(sched.cycle_count(), 0);
    }

    #[test]
    fn oneshot_skipped_in_second_iteration() {
        let items = vec![item(3.0, 0.001), oneshot_item(4.0, 0.001), item(5.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        // Play through full first iteration (3 items × 48 samples)
        for _ in 0..(48 * 3) {
            sched.advance();
        }
        assert!(sched.cycle_count() >= 1);

        // Second iteration: should skip 4Hz, go 3Hz → 5Hz
        assert_eq!(sched.current_delta(), 3.0);
        let (r, _) = advance_until_change(&mut sched, 100);
        assert_eq!(r, AdvanceResult::ItemChanged);
        assert_eq!(sched.current_delta(), 5.0); // 4Hz skipped!
    }

    #[test]
    fn all_oneshot_exhausts_after_first_cycle() {
        let items = vec![oneshot_item(3.0, 0.001), oneshot_item(5.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        // Play through first iteration
        for _ in 0..48 {
            sched.advance();
        }
        assert_eq!(sched.current_delta(), 5.0);

        // Finish second item → try to wrap → all oneshot → exhausted
        let (r, _) = advance_until_change(&mut sched, 100);
        assert_eq!(r, AdvanceResult::AllExhausted);
        assert!(sched.is_exhausted());
    }

    // -- Graceful stop tests --

    #[test]
    fn graceful_stop_at_cycle_end() {
        let items = vec![item(3.0, 0.001), item(5.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        // Set graceful stop
        sched.set_stop_at_cycle_end(true);

        // Play through first item
        let (r, _) = advance_until_change(&mut sched, 100);
        assert_eq!(r, AdvanceResult::ItemChanged);
        assert_eq!(sched.current_delta(), 5.0);

        // Play through second item → cycle wraps → should get CycleCompleteStop
        let (r, _) = advance_until_change(&mut sched, 100);
        assert_eq!(r, AdvanceResult::CycleCompleteStop);
    }

    #[test]
    fn graceful_stop_with_oneshot() {
        // 3Hz(normal), 4Hz(oneshot), 5Hz(normal)
        let items = vec![item(3.0, 0.001), oneshot_item(4.0, 0.001), item(5.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        // Set graceful stop during first iteration
        sched.set_stop_at_cycle_end(true);

        // Should play all 3 items then stop
        advance_until_change(&mut sched, 100); // → 4Hz
        advance_until_change(&mut sched, 100); // → 5Hz
        let (r, _) = advance_until_change(&mut sched, 100); // → wrap → stop
        assert_eq!(r, AdvanceResult::CycleCompleteStop);
    }

    #[test]
    fn cycle_count_increments() {
        let items = vec![item(3.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        assert_eq!(sched.cycle_count(), 0);
        for _ in 0..48 {
            sched.advance();
        }
        assert_eq!(sched.cycle_count(), 1);
        for _ in 0..48 {
            sched.advance();
        }
        assert_eq!(sched.cycle_count(), 2);
    }

    #[test]
    fn reset_clears_all_state() {
        let items = vec![item(3.0, 0.001), item(5.0, 0.001)];
        let mut sched = Scheduler::new(items, 48000.0);

        for _ in 0..48 {
            sched.advance();
        }
        sched.set_stop_at_cycle_end(true);
        sched.reset();

        assert_eq!(sched.cycle_count(), 0);
        assert!(!sched.stop_at_cycle_end());
        assert!(!sched.is_exhausted());
        assert_eq!(sched.current_delta(), 3.0);
    }
}
