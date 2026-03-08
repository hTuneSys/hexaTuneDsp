// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! One-shot event player and event management system.
//!
//! Manages up to [`MAX_EVENT_SLOTS`] event definitions and plays at most one
//! event at a time. Events are triggered by the [`EventScheduler`] with
//! randomized volume and stereo pan for each playback instance.
//!
//! All playback operations are real-time safe.

use crate::binaural::StereoSample;
use crate::event_scheduler::{EventScheduler, EventTiming, MAX_EVENT_SLOTS};

/// A registered event definition holding audio data and scheduling parameters.
pub struct EventSlot {
    pub buffer_left: Vec<f32>,
    pub buffer_right: Vec<f32>,
    pub timing: EventTiming,
    pub volume_range: (f32, f32),
    pub pan_range: (f32, f32),
}

/// Active event playback state.
struct EventPlayback {
    slot_index: usize,
    position: usize,
    length: usize,
    volume: f32,
    pan: f32,
}

/// Event management system.
///
/// Owns event slot definitions, the scheduler, and the active playback state.
pub struct EventSystem {
    slots: [Option<EventSlot>; MAX_EVENT_SLOTS],
    active: Option<EventPlayback>,
    scheduler: EventScheduler,
}

impl EventSystem {
    /// Create a new event system.
    pub fn new(sample_rate: f32, seed: u64) -> Self {
        Self {
            slots: [const { None }; MAX_EVENT_SLOTS],
            active: None,
            scheduler: EventScheduler::new(sample_rate, seed),
        }
    }

    /// Register an event in the given slot index.
    ///
    /// Accepts raw interleaved PCM data. `channels` must be 1 or 2.
    /// Returns an error if the index is out of bounds or channels invalid.
    ///
    /// NOT real-time safe (allocates).
    #[allow(clippy::too_many_arguments)]
    pub fn set_event(
        &mut self,
        index: usize,
        samples: &[f32],
        channels: u32,
        min_interval_ms: u32,
        max_interval_ms: u32,
        volume_min: f32,
        volume_max: f32,
        pan_min: f32,
        pan_max: f32,
    ) -> Result<(), String> {
        if index >= MAX_EVENT_SLOTS {
            return Err(format!(
                "event index {index} exceeds max {MAX_EVENT_SLOTS}"
            ));
        }
        if samples.is_empty() {
            return Err("event samples are empty".to_string());
        }

        let (left, right) = match channels {
            1 => (samples.to_vec(), samples.to_vec()),
            2 => {
                let frame_count = samples.len() / 2;
                let mut l = Vec::with_capacity(frame_count);
                let mut r = Vec::with_capacity(frame_count);
                for frame in samples.chunks_exact(2) {
                    l.push(frame[0]);
                    r.push(frame[1]);
                }
                (l, r)
            }
            _ => {
                return Err(format!("unsupported channel count: {channels}"));
            }
        };

        self.slots[index] = Some(EventSlot {
            buffer_left: left,
            buffer_right: right,
            timing: EventTiming {
                min_interval_ms,
                max_interval_ms,
            },
            volume_range: (volume_min, volume_max),
            pan_range: (pan_min, pan_max),
        });

        self.update_scheduler_state();
        Ok(())
    }

    /// Remove an event from the given slot.
    pub fn clear_event(&mut self, index: usize) {
        if index < MAX_EVENT_SLOTS {
            self.slots[index] = None;
            // If the currently playing event was from this slot, stop it.
            if let Some(ref active) = self.active
                && active.slot_index == index
            {
                self.active = None;
            }
            self.update_scheduler_state();
        }
    }

    /// Remove all events and stop playback.
    pub fn clear_all(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.active = None;
        self.update_scheduler_state();
    }

    /// Advance the event system by one sample frame. This checks the
    /// scheduler and may trigger a new event if none is currently playing.
    ///
    /// Real-time safe.
    #[inline]
    pub fn advance(&mut self) {
        // If something is playing, check if it finished.
        if let Some(ref active) = self.active
            && active.position >= active.length
        {
            self.active = None;
            self.schedule_from_any_slot();
        }

        // If nothing is playing, check the scheduler.
        if self.active.is_none()
            && let Some(slot_index) = self.scheduler.advance()
        {
            self.trigger_event(slot_index);
        }
    }

    /// Get the next stereo sample from the active event playback.
    ///
    /// Returns silence if no event is playing. Applies per-instance
    /// volume and stereo panning.
    ///
    /// Real-time safe.
    #[inline]
    pub fn next_sample(&mut self) -> StereoSample {
        let active = match self.active.as_mut() {
            Some(a) => a,
            None => return StereoSample::default(),
        };

        if active.position >= active.length {
            return StereoSample::default();
        }

        let slot = match &self.slots[active.slot_index] {
            Some(s) => s,
            None => return StereoSample::default(),
        };

        let raw_left = slot.buffer_left[active.position];
        let raw_right = slot.buffer_right[active.position];
        active.position += 1;

        // Apply volume
        let vol = active.volume;
        let left = raw_left * vol;
        let right = raw_right * vol;

        // Apply constant-power-ish pan: pan in [-1, 1]
        // pan = -1 → full left, pan = 0 → center, pan = 1 → full right
        let pan = active.pan;
        let left_gain = ((1.0 - pan) * 0.5).min(1.0);
        let right_gain = ((1.0 + pan) * 0.5).min(1.0);

        StereoSample {
            left: left * left_gain,
            right: right * right_gain,
        }
    }

    /// Whether an event is currently playing.
    #[inline]
    pub fn is_playing(&self) -> bool {
        self.active.is_some()
    }

    /// Number of registered event slots.
    pub fn slot_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    // -- private --

    fn trigger_event(&mut self, slot_index: usize) {
        let slot = match &self.slots[slot_index] {
            Some(s) => s,
            None => return,
        };

        let volume = self
            .scheduler
            .random_f32(slot.volume_range.0, slot.volume_range.1);
        let pan = self
            .scheduler
            .random_f32(slot.pan_range.0, slot.pan_range.1);

        self.active = Some(EventPlayback {
            slot_index,
            position: 0,
            length: slot.buffer_left.len(),
            volume,
            pan,
        });
    }

    fn schedule_from_any_slot(&mut self) {
        // Aggregate min/max intervals from all active slots.
        let mut min_ms = u32::MAX;
        let mut max_ms = 0u32;
        let mut has_any = false;

        for slot in self.slots.iter().flatten() {
            min_ms = min_ms.min(slot.timing.min_interval_ms);
            max_ms = max_ms.max(slot.timing.max_interval_ms);
            has_any = true;
        }

        if has_any && max_ms >= min_ms {
            self.scheduler.schedule_next_with_range(min_ms, max_ms);
        }
    }

    fn update_scheduler_state(&mut self) {
        let count = self.slot_count();
        self.scheduler.set_slot_count(count);
        if count > 0 {
            self.scheduler.set_enabled(true);
            self.schedule_from_any_slot();
        } else {
            self.scheduler.set_enabled(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_event(len: usize) -> Vec<f32> {
        (0..len).map(|i| (i as f32) / (len as f32)).collect()
    }

    #[test]
    fn empty_system_returns_silence() {
        let mut sys = EventSystem::new(48000.0, 42);
        sys.advance();
        let s = sys.next_sample();
        assert_eq!(s.left, 0.0);
        assert_eq!(s.right, 0.0);
    }

    #[test]
    fn set_and_clear_event() {
        let mut sys = EventSystem::new(48000.0, 42);
        let data = make_test_event(100);
        sys.set_event(0, &data, 1, 100, 200, 0.5, 1.0, -0.5, 0.5)
            .unwrap();
        assert_eq!(sys.slot_count(), 1);

        sys.clear_event(0);
        assert_eq!(sys.slot_count(), 0);
    }

    #[test]
    fn rejects_invalid_index() {
        let mut sys = EventSystem::new(48000.0, 42);
        let data = make_test_event(100);
        let result = sys.set_event(5, &data, 1, 100, 200, 0.5, 1.0, 0.0, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn event_plays_and_finishes() {
        let mut sys = EventSystem::new(48000.0, 42);
        // Short event, very short interval so it triggers fast
        let data = vec![0.5; 10]; // 10 samples of 0.5
        sys.set_event(0, &data, 1, 0, 0, 1.0, 1.0, 0.0, 0.0)
            .unwrap();

        // Advance until event triggers
        let mut triggered = false;
        for _ in 0..100 {
            sys.advance();
            if sys.is_playing() {
                triggered = true;
                break;
            }
        }
        assert!(triggered, "event should have triggered");

        // Play through the event
        let mut non_silent = 0;
        for _ in 0..10 {
            let s = sys.next_sample();
            if s.left.abs() > 1e-10 || s.right.abs() > 1e-10 {
                non_silent += 1;
            }
        }
        assert!(non_silent > 0, "event should produce audio");
    }

    #[test]
    fn max_one_concurrent_event() {
        let mut sys = EventSystem::new(48000.0, 42);
        let data = vec![0.5; 1000]; // long event
        sys.set_event(0, &data, 1, 0, 0, 1.0, 1.0, 0.0, 0.0)
            .unwrap();
        sys.set_event(1, &data, 1, 0, 0, 1.0, 1.0, 0.0, 0.0)
            .unwrap();

        // Trigger first event
        sys.advance();
        assert!(sys.is_playing());

        // Even with more advances, only one event plays at a time
        for _ in 0..50 {
            sys.advance();
            // active should still be the same one
        }
        assert!(sys.is_playing());
    }

    #[test]
    fn clear_all_stops_playback() {
        let mut sys = EventSystem::new(48000.0, 42);
        let data = vec![0.5; 1000];
        sys.set_event(0, &data, 1, 0, 0, 1.0, 1.0, 0.0, 0.0)
            .unwrap();
        sys.advance(); // trigger
        assert!(sys.is_playing());

        sys.clear_all();
        assert!(!sys.is_playing());
        assert_eq!(sys.slot_count(), 0);
    }

    #[test]
    fn pan_affects_stereo_output() {
        let mut sys = EventSystem::new(48000.0, 42);
        // Pan hard left: pan_range = (-1.0, -1.0)
        let data = vec![1.0; 100];
        sys.set_event(0, &data, 1, 0, 0, 1.0, 1.0, -1.0, -1.0)
            .unwrap();

        // Trigger
        sys.advance();
        let s = sys.next_sample();
        // Hard left: right should be ~0 (or very small)
        assert!(s.left > s.right, "left should be louder with left pan");
    }

    #[test]
    fn stereo_event_loads_correctly() {
        let mut sys = EventSystem::new(48000.0, 42);
        // Interleaved stereo: L=0.3, R=0.7, L=0.3, R=0.7
        let data = vec![0.3, 0.7, 0.3, 0.7];
        sys.set_event(0, &data, 2, 0, 0, 1.0, 1.0, 0.0, 0.0)
            .unwrap();

        sys.advance(); // trigger
        let s = sys.next_sample();
        // Center pan (0.0): left_gain=0.5, right_gain=0.5
        assert!((s.left - 0.15).abs() < 1e-5, "got {}", s.left);
        assert!((s.right - 0.35).abs() < 1e-5, "got {}", s.right);
    }
}
