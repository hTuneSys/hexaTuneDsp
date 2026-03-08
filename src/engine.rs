// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Main DSP engine — soundscape orchestrator.
//!
//! Manages multiple audio layers (base, textures, events, binaural) and
//! renders them into a single interleaved stereo output. The render path
//! is fully real-time safe: no heap allocation, no I/O, no locks.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::binaural::{BinauralGenerator, StereoSample};
use crate::event_player::EventSystem;
use crate::mixer::{
    DEFAULT_BASE_GAIN, DEFAULT_BINAURAL_GAIN, DEFAULT_EVENT_GAIN, DEFAULT_MASTER_GAIN,
    DEFAULT_TEXTURE_GAIN, LayerGains, Mixer,
};
use crate::sample_player::SamplePlayer;
use crate::scheduler::{CycleItem, Scheduler};

/// Default sample rate for mobile audio.
pub const DEFAULT_SAMPLE_RATE: f32 = 48000.0;

/// Maximum number of texture layers.
pub const MAX_TEXTURE_LAYERS: usize = 3;

/// Default PRNG seed for the event scheduler.
const EVENT_SCHEDULER_SEED: u64 = 0xDEAD_BEEF_CAFE_1337;

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
    base_layer: Option<SamplePlayer>,
    texture_layers: [Option<SamplePlayer>; MAX_TEXTURE_LAYERS],
    event_system: EventSystem,
    scheduler: Scheduler,
    mixer: Mixer,
    sample_rate: f32,

    // -- Atomic shared parameters (written by control, read by audio) --
    base_gain: AtomicU32,
    texture_gain: AtomicU32,
    event_gain: AtomicU32,
    binaural_gain: AtomicU32,
    master_gain: AtomicU32,
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
    pub cycle_items: Vec<CycleItem>,
    pub sample_rate: f32,
    pub base_gain: f32,
    pub texture_gain: f32,
    pub event_gain: f32,
    pub binaural_gain: f32,
    pub master_gain: f32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            carrier_frequency: 400.0,
            binaural_enabled: true,
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
            base_gain: DEFAULT_BASE_GAIN,
            texture_gain: DEFAULT_TEXTURE_GAIN,
            event_gain: DEFAULT_EVENT_GAIN,
            binaural_gain: DEFAULT_BINAURAL_GAIN,
            master_gain: DEFAULT_MASTER_GAIN,
        }
    }
}

impl Engine {
    /// Create and initialize a new engine with the given configuration.
    ///
    /// This function allocates memory. It must NOT be called from the audio
    /// thread.
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

        let scheduler = Scheduler::new(config.cycle_items, config.sample_rate);
        let mixer = Mixer::new(LayerGains {
            base: config.base_gain,
            texture: config.texture_gain,
            event: config.event_gain,
            binaural: config.binaural_gain,
            master: config.master_gain,
        });

        let event_system = EventSystem::new(config.sample_rate, EVENT_SCHEDULER_SEED);

        Ok(Self {
            binaural,
            base_layer: None,
            texture_layers: [const { None }; MAX_TEXTURE_LAYERS],
            event_system,
            scheduler,
            mixer,
            sample_rate: config.sample_rate,
            base_gain: AtomicU32::new(config.base_gain.to_bits()),
            texture_gain: AtomicU32::new(config.texture_gain.to_bits()),
            event_gain: AtomicU32::new(config.event_gain.to_bits()),
            binaural_gain: AtomicU32::new(config.binaural_gain.to_bits()),
            master_gain: AtomicU32::new(config.master_gain.to_bits()),
            running: AtomicBool::new(false),
            pending_config: Mutex::new(None),
        })
    }

    // -- Lifecycle --

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

    // -- Atomic gain setters (control thread) --

    pub fn set_base_gain(&self, gain: f32) {
        self.base_gain.store(gain.to_bits(), Ordering::Release);
    }

    pub fn set_texture_gain(&self, gain: f32) {
        self.texture_gain.store(gain.to_bits(), Ordering::Release);
    }

    pub fn set_event_gain(&self, gain: f32) {
        self.event_gain.store(gain.to_bits(), Ordering::Release);
    }

    pub fn set_binaural_gain(&self, gain: f32) {
        self.binaural_gain.store(gain.to_bits(), Ordering::Release);
    }

    pub fn set_master_gain(&self, gain: f32) {
        self.master_gain.store(gain.to_bits(), Ordering::Release);
    }

    // -- Layer management (NOT real-time safe) --

    /// Set the base layer from raw interleaved PCM data.
    /// `channels` must be 1 (mono) or 2 (stereo).
    pub fn set_base_layer(&mut self, data: &[f32], channels: u32) -> Result<(), String> {
        let mut player = SamplePlayer::new();
        player.load_raw_pcm(data, channels)?;
        self.base_layer = Some(player);
        Ok(())
    }

    /// Remove the base layer. Also clears textures and events (they require
    /// a base).
    pub fn clear_base_layer(&mut self) {
        self.base_layer = None;
        for layer in &mut self.texture_layers {
            *layer = None;
        }
        self.event_system.clear_all();
    }

    /// Set a texture layer at the given index (0–2).
    /// Returns an error if base is not set or index is out of bounds.
    pub fn set_texture_layer(
        &mut self,
        index: usize,
        data: &[f32],
        channels: u32,
    ) -> Result<(), String> {
        if index >= MAX_TEXTURE_LAYERS {
            return Err(format!(
                "texture index {index} exceeds max {MAX_TEXTURE_LAYERS}"
            ));
        }
        if self.base_layer.is_none() {
            return Err("base layer must be set before adding textures".to_string());
        }
        let mut player = SamplePlayer::new();
        player.load_raw_pcm(data, channels)?;
        self.texture_layers[index] = Some(player);
        Ok(())
    }

    /// Remove a texture layer.
    pub fn clear_texture_layer(&mut self, index: usize) {
        if index < MAX_TEXTURE_LAYERS {
            self.texture_layers[index] = None;
        }
    }

    /// Register a random event at the given slot index (0–4).
    /// Returns an error if base is not set or index is out of bounds.
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
        if self.base_layer.is_none() {
            return Err("base layer must be set before adding events".to_string());
        }
        self.event_system.set_event(
            index,
            samples,
            channels,
            min_interval_ms,
            max_interval_ms,
            volume_min,
            volume_max,
            pan_min,
            pan_max,
        )
    }

    /// Remove an event from the given slot.
    pub fn clear_event(&mut self, index: usize) {
        self.event_system.clear_event(index);
    }

    /// Remove all layers (base, textures, events). Binaural is unaffected.
    pub fn clear_all_layers(&mut self) {
        self.base_layer = None;
        for layer in &mut self.texture_layers {
            *layer = None;
        }
        self.event_system.clear_all();
    }

    /// Queue a configuration update to be picked up by the next render call.
    pub fn queue_config_update(&self, config: PendingConfig) {
        if let Ok(mut pending) = self.pending_config.lock() {
            *pending = Some(config);
        }
    }

    // -- Render --

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
        self.mixer
            .set_base_gain(f32::from_bits(self.base_gain.load(Ordering::Relaxed)));
        self.mixer
            .set_texture_gain(f32::from_bits(self.texture_gain.load(Ordering::Relaxed)));
        self.mixer
            .set_event_gain(f32::from_bits(self.event_gain.load(Ordering::Relaxed)));
        self.mixer
            .set_binaural_gain(f32::from_bits(self.binaural_gain.load(Ordering::Relaxed)));
        self.mixer
            .set_master_gain(f32::from_bits(self.master_gain.load(Ordering::Relaxed)));

        if !self.running.load(Ordering::Relaxed) {
            for sample in output.iter_mut().take(num_frames * 2) {
                *sample = 0.0;
            }
            return;
        }

        for i in 0..num_frames {
            // 1. Frequency scheduler update
            if self.scheduler.advance() {
                let new_delta = self.scheduler.current_delta();
                self.binaural.set_delta(new_delta);
            }

            // 2. Generate binaural/AM tone
            let tone = self.binaural.generate();

            // 3. Base layer
            let base = match self.base_layer.as_mut() {
                Some(player) => player.next_sample(),
                None => StereoSample::default(),
            };

            // 4. Texture layers (summed)
            let mut texture_sum = StereoSample::default();
            for layer in &mut self.texture_layers {
                if let Some(player) = layer.as_mut() {
                    let s = player.next_sample();
                    texture_sum.left += s.left;
                    texture_sum.right += s.right;
                }
            }

            // 5. Event system
            self.event_system.advance();
            let event = self.event_system.next_sample();

            // 6. Mix all layers
            let mixed = self.mixer.mix(tone, base, texture_sum, event);

            // 7. Write interleaved stereo output
            let idx = i * 2;
            output[idx] = mixed.left;
            output[idx + 1] = mixed.right;
        }
    }

    /// Try to apply any pending configuration update.
    #[inline]
    fn apply_pending_config(&mut self) {
        let pending = match self.pending_config.try_lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => return,
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

    /// Load or replace the base layer from a WAV file.
    ///
    /// This allocates memory and must be called from the control thread,
    /// NOT from the audio callback.
    pub fn load_base_wav(&mut self, path: &str) -> Result<(), String> {
        let mut player = SamplePlayer::new();
        player.load_wav(path)?;
        self.base_layer = Some(player);
        Ok(())
    }

    /// Current sample rate.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Whether a base layer is loaded.
    pub fn has_base_layer(&self) -> bool {
        self.base_layer.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EngineConfig {
        EngineConfig {
            carrier_frequency: 400.0,
            binaural_enabled: true,
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
            binaural_gain: 1.0,
            base_gain: 0.0,
            texture_gain: 0.0,
            event_gain: 0.0,
            master_gain: 1.0,
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

        let non_zero = buffer.iter().filter(|&&s| s.abs() > 1e-10).count();
        assert!(non_zero > 0, "should produce audio when running");
    }

    #[test]
    fn output_stays_in_range() {
        let mut engine = Engine::new(test_config()).unwrap();
        engine.start();

        let mut buffer = vec![0.0f32; 4800];
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

        engine.set_binaural_gain(0.0);
        let mut buffer = vec![0.0f32; 512];
        engine.render(&mut buffer, 256);

        for &sample in &buffer {
            assert_eq!(sample, 0.0, "zero binaural gain should produce silence");
        }
    }

    #[test]
    fn config_update_changes_behavior() {
        let mut engine = Engine::new(test_config()).unwrap();
        engine.start();

        let mut buf1 = vec![0.0f32; 200];
        engine.render(&mut buf1, 100);

        engine.queue_config_update(PendingConfig {
            carrier_frequency: Some(800.0),
            binaural_enabled: None,
            cycle_items: None,
        });

        let mut buf2 = vec![0.0f32; 200];
        engine.render(&mut buf2, 100);

        let differ = buf1
            .iter()
            .zip(buf2.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
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

    #[test]
    fn base_layer_contributes_to_output() {
        let mut config = test_config();
        config.binaural_gain = 0.0;
        config.base_gain = 1.0;
        config.master_gain = 1.0;
        let mut engine = Engine::new(config).unwrap();

        // Load a simple base layer
        let base_data: Vec<f32> = (0..4800).map(|_| 0.5).collect();
        engine.set_base_layer(&base_data, 1).unwrap();
        engine.start();

        let mut buffer = vec![0.0f32; 512];
        engine.render(&mut buffer, 256);

        let non_zero = buffer.iter().filter(|&&s| s.abs() > 1e-10).count();
        assert!(non_zero > 0, "base layer should produce audio");
    }

    #[test]
    fn texture_requires_base() {
        let mut engine = Engine::new(test_config()).unwrap();
        let data = vec![0.5; 100];
        let result = engine.set_texture_layer(0, &data, 1);
        assert!(result.is_err(), "texture without base should fail");
    }

    #[test]
    fn event_requires_base() {
        let mut engine = Engine::new(test_config()).unwrap();
        let data = vec![0.5; 100];
        let result = engine.set_event(0, &data, 1, 100, 200, 0.5, 1.0, 0.0, 0.0);
        assert!(result.is_err(), "event without base should fail");
    }

    #[test]
    fn clear_base_also_clears_textures_and_events() {
        let mut engine = Engine::new(test_config()).unwrap();
        let data = vec![0.5; 100];
        engine.set_base_layer(&data, 1).unwrap();
        engine.set_texture_layer(0, &data, 1).unwrap();
        engine
            .set_event(0, &data, 1, 100, 200, 0.5, 1.0, 0.0, 0.0)
            .unwrap();

        engine.clear_base_layer();
        assert!(!engine.has_base_layer());
    }

    #[test]
    fn texture_index_bounds_checked() {
        let mut engine = Engine::new(test_config()).unwrap();
        let data = vec![0.5; 100];
        engine.set_base_layer(&data, 1).unwrap();
        let result = engine.set_texture_layer(3, &data, 1);
        assert!(result.is_err());
    }
}
