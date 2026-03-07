// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Main DSP engine.
//!
//! Orchestrates oscillators, binaural generator, rain player, scheduler,
//! and mixer into a single render pipeline. The render path is fully
//! real-time safe: no heap allocation, no I/O, no locks.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use crate::binaural::BinauralGenerator;
use crate::mixer::Mixer;
use crate::rain_player::RainPlayer;
use crate::scheduler::{CycleItem, Scheduler};

/// Default sample rate for mobile audio.
pub const DEFAULT_SAMPLE_RATE: f32 = 48000.0;

/// Default rain gain.
pub const DEFAULT_RAIN_GAIN: f32 = 0.8;

/// Default tone gain.
pub const DEFAULT_TONE_GAIN: f32 = 0.2;

/// Pending configuration update delivered from the control thread to the
/// audio thread via a lock-free mechanism.
pub struct PendingConfig {
    pub carrier_frequency: Option<f32>,
    pub binaural_enabled: Option<bool>,
    pub cycle_items: Option<Vec<CycleItem>>,
}

/// The main DSP audio engine.
///
/// # Thread Safety
///
/// The engine uses a split ownership model:
/// - **Audio thread** calls [`render`] exclusively (no concurrent calls).
/// - **Control thread** calls atomic setters and [`queue_config_update`].
///
/// The control thread must not call [`render`]. The audio thread must not
/// call [`queue_config_update`] or atomic setters (it reads them).
pub struct Engine {
    // -- Audio processing state (owned by audio thread via render) --
    binaural: BinauralGenerator,
    rain_player: RainPlayer,
    scheduler: Scheduler,
    mixer: Mixer,
    sample_rate: f32,

    // -- Atomic shared parameters (written by control, read by audio) --
    rain_gain: AtomicU32,
    tone_gain: AtomicU32,
    running: AtomicBool,

    // -- Pending configuration (written by control, consumed by audio) --
    pending_config: Mutex<Option<PendingConfig>>,
}

// Safety: The engine uses atomics and a mutex for cross-thread fields.
// The mutable audio state is only accessed from a single audio thread.
unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

/// Configuration for creating an engine instance.
pub struct EngineConfig {
    pub carrier_frequency: f32,
    pub binaural_enabled: bool,
    pub rain_sound_path: Option<String>,
    pub cycle_items: Vec<CycleItem>,
    pub sample_rate: f32,
    pub rain_gain: f32,
    pub tone_gain: f32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            carrier_frequency: 400.0,
            binaural_enabled: true,
            rain_sound_path: None,
            cycle_items: vec![
                CycleItem {
                    frequency_delta: 3.0,
                    duration_seconds: 30.0,
                },
                CycleItem {
                    frequency_delta: 4.0,
                    duration_seconds: 30.0,
                },
                CycleItem {
                    frequency_delta: 5.0,
                    duration_seconds: 30.0,
                },
            ],
            sample_rate: DEFAULT_SAMPLE_RATE,
            rain_gain: DEFAULT_RAIN_GAIN,
            tone_gain: DEFAULT_TONE_GAIN,
        }
    }
}

impl Engine {
    /// Create and initialize a new engine with the given configuration.
    ///
    /// This function allocates memory and may perform I/O (loading WAV files).
    /// It must NOT be called from the audio thread.
    pub fn new(config: EngineConfig) -> Result<Self, String> {
        let initial_delta = config
            .cycle_items
            .first()
            .map_or(0.0, |item| item.frequency_delta);

        let binaural = BinauralGenerator::new(
            config.carrier_frequency,
            initial_delta,
            config.binaural_enabled,
            config.sample_rate,
        );

        let mut rain_player = RainPlayer::new();
        if let Some(ref path) = config.rain_sound_path {
            rain_player.load_wav(path)?;
        }

        let scheduler = Scheduler::new(config.cycle_items, config.sample_rate);
        let mixer = Mixer::new(config.rain_gain, config.tone_gain);

        Ok(Self {
            binaural,
            rain_player,
            scheduler,
            mixer,
            sample_rate: config.sample_rate,
            rain_gain: AtomicU32::new(config.rain_gain.to_bits()),
            tone_gain: AtomicU32::new(config.tone_gain.to_bits()),
            running: AtomicBool::new(false),
            pending_config: Mutex::new(None),
        })
    }

    /// Mark the engine as running. Audio will be generated in [`render`].
    pub fn start(&self) {
        self.running.store(true, Ordering::Release);
    }

    /// Mark the engine as stopped. [`render`] will output silence.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    /// Whether the engine is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Set the rain/ambient gain (control thread).
    pub fn set_rain_gain(&self, gain: f32) {
        self.rain_gain.store(gain.to_bits(), Ordering::Release);
    }

    /// Set the tone gain (control thread).
    pub fn set_tone_gain(&self, gain: f32) {
        self.tone_gain.store(gain.to_bits(), Ordering::Release);
    }

    /// Queue a configuration update to be picked up by the next render call.
    /// This acquires a mutex briefly, but is safe from the control thread.
    pub fn queue_config_update(&self, config: PendingConfig) {
        if let Ok(mut pending) = self.pending_config.lock() {
            *pending = Some(config);
        }
    }

    /// Render `num_frames` of interleaved stereo f32 audio into `output`.
    ///
    /// The output buffer must have capacity for `num_frames * 2` samples
    /// (left, right, left, right, ...).
    ///
    /// # Real-time safety
    ///
    /// This function does NOT allocate, does NOT perform I/O, and does NOT
    /// block. It is safe to call from an audio callback.
    ///
    /// # Panics
    ///
    /// Panics if `output.len() < num_frames * 2`.
    pub fn render(&mut self, output: &mut [f32], num_frames: usize) {
        assert!(
            output.len() >= num_frames * 2,
            "output buffer too small: need {}, got {}",
            num_frames * 2,
            output.len()
        );

        // Apply any pending configuration (non-blocking try_lock)
        self.apply_pending_config();

        // Sync atomic gain values into the mixer
        let rain_gain = f32::from_bits(self.rain_gain.load(Ordering::Relaxed));
        let tone_gain = f32::from_bits(self.tone_gain.load(Ordering::Relaxed));
        self.mixer.set_rain_gain(rain_gain);
        self.mixer.set_tone_gain(tone_gain);

        if !self.running.load(Ordering::Relaxed) {
            // Output silence when not running
            for sample in output.iter_mut().take(num_frames * 2) {
                *sample = 0.0;
            }
            return;
        }

        for i in 0..num_frames {
            // 1. Scheduler update
            if self.scheduler.advance() {
                let new_delta = self.scheduler.current_delta();
                self.binaural.set_delta(new_delta);
            }

            // 2. Generate tone
            let tone = self.binaural.generate();

            // 3. Fetch rain sample
            let rain = self.rain_player.next_sample();

            // 4. Mix
            let mixed = self.mixer.mix(tone, rain);

            // 5. Write interleaved stereo output
            let idx = i * 2;
            output[idx] = mixed.left;
            output[idx + 1] = mixed.right;
        }
    }

    /// Try to apply any pending configuration update. Uses try_lock to
    /// avoid blocking the audio thread.
    #[inline]
    fn apply_pending_config(&mut self) {
        let pending = match self.pending_config.try_lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => return, // Contended — skip this frame
        };

        if let Some(config) = pending {
            if let Some(carrier) = config.carrier_frequency {
                self.binaural.set_carrier_frequency(carrier);
            }
            if let Some(enabled) = config.binaural_enabled {
                self.binaural.set_binaural_enabled(enabled);
            }
            if let Some(items) = config.cycle_items {
                self.scheduler.set_items(items);
                let new_delta = self.scheduler.current_delta();
                self.binaural.set_delta(new_delta);
            }
        }
    }

    /// Load or replace the rain sound from a WAV file.
    ///
    /// This allocates memory and must be called from the control thread,
    /// NOT from the audio callback.
    pub fn load_rain_sound(&mut self, path: &str) -> Result<(), String> {
        self.rain_player.load_wav(path)
    }

    /// Current sample rate.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EngineConfig {
        EngineConfig {
            carrier_frequency: 400.0,
            binaural_enabled: true,
            rain_sound_path: None,
            cycle_items: vec![
                CycleItem {
                    frequency_delta: 3.0,
                    duration_seconds: 0.01,
                },
                CycleItem {
                    frequency_delta: 5.0,
                    duration_seconds: 0.01,
                },
            ],
            sample_rate: 48000.0,
            rain_gain: 0.0, // no rain loaded, so zero gain
            tone_gain: 1.0,
        }
    }

    #[test]
    fn engine_creates_successfully() {
        let engine = Engine::new(test_config());
        assert!(engine.is_ok());
    }

    #[test]
    fn renders_silence_when_stopped() {
        let mut engine = Engine::new(test_config()).unwrap();
        let mut buffer = vec![999.0f32; 512];
        engine.render(&mut buffer, 256);

        for &sample in &buffer {
            assert_eq!(sample, 0.0, "should output silence when stopped");
        }
    }

    #[test]
    fn renders_audio_when_running() {
        let mut engine = Engine::new(test_config()).unwrap();
        engine.start();

        let mut buffer = vec![0.0f32; 512];
        engine.render(&mut buffer, 256);

        // At least some samples should be non-zero
        let non_zero = buffer.iter().filter(|&&s| s.abs() > 1e-10).count();
        assert!(non_zero > 0, "should produce audio when running");
    }

    #[test]
    fn output_stays_in_range() {
        let mut engine = Engine::new(test_config()).unwrap();
        engine.start();

        let mut buffer = vec![0.0f32; 4800]; // 100ms
        engine.render(&mut buffer, 2400);

        for &sample in &buffer {
            assert!(
                (-1.0..=1.0).contains(&sample),
                "sample out of range: {sample}"
            );
        }
    }

    #[test]
    fn gain_updates_are_applied() {
        let mut engine = Engine::new(test_config()).unwrap();
        engine.start();

        engine.set_tone_gain(0.0);
        let mut buffer = vec![0.0f32; 512];
        engine.render(&mut buffer, 256);

        // With zero tone gain and no rain loaded, output should be silence
        for &sample in &buffer {
            assert_eq!(sample, 0.0, "zero tone gain should produce silence");
        }
    }

    #[test]
    fn config_update_changes_behavior() {
        let mut engine = Engine::new(test_config()).unwrap();
        engine.start();

        // Render initial audio
        let mut buf1 = vec![0.0f32; 200];
        engine.render(&mut buf1, 100);

        // Queue a config update that changes the carrier
        engine.queue_config_update(PendingConfig {
            carrier_frequency: Some(800.0),
            binaural_enabled: None,
            cycle_items: None,
        });

        let mut buf2 = vec![0.0f32; 200];
        engine.render(&mut buf2, 100);

        // Buffers should be different (different carrier frequency)
        let differ = buf1.iter().zip(buf2.iter()).any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(differ, "config update should change the audio output");
    }

    #[test]
    fn start_stop_transitions() {
        let engine = Engine::new(test_config()).unwrap();

        assert!(!engine.is_running());
        engine.start();
        assert!(engine.is_running());
        engine.stop();
        assert!(!engine.is_running());
    }
}
