// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Looping sample player for soundscape layers.
//!
//! Loads audio into memory during initialization (non-real-time) and loops
//! playback from the pre-allocated buffer. Supports loop-point crossfade
//! to eliminate clicks at loop boundaries. The playback path performs zero
//! allocations.

use crate::binaural::StereoSample;

/// Default crossfade length in sample frames (~42 ms at 48 kHz).
pub const DEFAULT_CROSSFADE_FRAMES: usize = 2048;

/// Looping sample player for base and texture layers.
pub struct SamplePlayer {
    buffer_left: Vec<f32>,
    buffer_right: Vec<f32>,
    position: usize,
    loaded: bool,
    /// Number of frames used for loop-point crossfade.
    crossfade_frames: usize,
}

impl SamplePlayer {
    /// Create an empty player with no audio loaded.
    pub fn new() -> Self {
        Self {
            buffer_left: Vec::new(),
            buffer_right: Vec::new(),
            position: 0,
            loaded: false,
            crossfade_frames: DEFAULT_CROSSFADE_FRAMES,
        }
    }

    /// Create a player with a custom crossfade length.
    pub fn with_crossfade(crossfade_frames: usize) -> Self {
        Self {
            crossfade_frames,
            ..Self::new()
        }
    }

    /// Load a WAV file from disk. This allocates memory and must be called
    /// before playback starts — never from the audio callback.
    pub fn load_wav(&mut self, path: &str) -> Result<(), String> {
        let reader = hound::WavReader::open(path)
            .map_err(|e| format!("failed to open WAV file '{path}': {e}"))?;

        let spec = reader.spec();
        let channels = spec.channels as usize;
        let sample_format = spec.sample_format;
        let bits = spec.bits_per_sample;

        let samples: Vec<f32> = match sample_format {
            hound::SampleFormat::Int => {
                let max_val = (1i64 << (bits - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / max_val)
                    .collect()
            }
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .filter_map(|s| s.ok())
                .collect(),
        };

        if samples.is_empty() {
            return Err("WAV file contains no samples".to_string());
        }

        self.store_samples(&samples, channels)
    }

    /// Load raw interleaved PCM f32 data received from FFI.
    ///
    /// `channels` must be 1 (mono) or 2 (stereo interleaved).
    /// This allocates memory and must NOT be called from the audio callback.
    pub fn load_raw_pcm(&mut self, data: &[f32], channels: u32) -> Result<(), String> {
        if data.is_empty() {
            return Err("PCM data is empty".to_string());
        }
        self.store_samples(data, channels as usize)
    }

    /// Load raw f32 PCM samples directly (mono).
    pub fn load_raw_mono(&mut self, samples: Vec<f32>) {
        self.buffer_right = samples.clone();
        self.buffer_left = samples;
        self.position = 0;
        self.loaded = !self.buffer_left.is_empty();
    }

    /// Load raw f32 PCM samples directly (stereo, separate channels).
    pub fn load_raw_stereo(&mut self, left: Vec<f32>, right: Vec<f32>) {
        assert_eq!(left.len(), right.len(), "channel length mismatch");
        self.buffer_left = left;
        self.buffer_right = right;
        self.position = 0;
        self.loaded = !self.buffer_left.is_empty();
    }

    /// Fetch the next stereo sample with loop-point crossfade. Returns
    /// silence if no audio is loaded.
    ///
    /// Real-time safe: no allocation, deterministic execution.
    #[inline]
    pub fn next_sample(&mut self) -> StereoSample {
        if !self.loaded {
            return StereoSample::default();
        }

        let len = self.buffer_left.len();
        let cf = self.effective_crossfade(len);

        let left;
        let right;

        if cf > 0 && self.position >= len - cf {
            // Inside crossfade region: blend tail with head.
            let fade_pos = self.position - (len - cf);
            let alpha = (fade_pos + 1) as f32 / cf as f32; // 0→1
            let head_pos = fade_pos;

            left = self.buffer_left[self.position] * (1.0 - alpha)
                + self.buffer_left[head_pos] * alpha;
            right = self.buffer_right[self.position] * (1.0 - alpha)
                + self.buffer_right[head_pos] * alpha;
        } else {
            left = self.buffer_left[self.position];
            right = self.buffer_right[self.position];
        }

        self.position += 1;
        if self.position >= len {
            // Jump past the crossfade region at the head so it isn't
            // played twice (the crossfade already blended it in).
            self.position = cf;
        }

        StereoSample { left, right }
    }

    /// Reset the playback position to the start.
    #[inline]
    pub fn reset(&mut self) {
        self.position = 0;
    }

    /// Whether valid audio data is loaded.
    #[inline]
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Number of sample frames in the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer_left.len()
    }

    /// Whether the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer_left.is_empty()
    }

    // -- private helpers --

    /// Store raw samples into the player buffers.
    fn store_samples(&mut self, samples: &[f32], channels: usize) -> Result<(), String> {
        match channels {
            1 => {
                self.buffer_left = samples.to_vec();
                self.buffer_right = samples.to_vec();
            }
            2 => {
                let frame_count = samples.len() / 2;
                let mut left = Vec::with_capacity(frame_count);
                let mut right = Vec::with_capacity(frame_count);
                for frame in samples.chunks_exact(2) {
                    left.push(frame[0]);
                    right.push(frame[1]);
                }
                self.buffer_left = left;
                self.buffer_right = right;
            }
            _ => {
                return Err(format!(
                    "unsupported channel count: {channels} (expected 1 or 2)"
                ));
            }
        }

        self.position = 0;
        self.loaded = !self.buffer_left.is_empty();
        Ok(())
    }

    /// Effective crossfade length clamped so it never exceeds half the buffer.
    #[inline]
    fn effective_crossfade(&self, len: usize) -> usize {
        if len < 4 {
            return 0;
        }
        self.crossfade_frames.min(len / 2)
    }
}

impl Default for SamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_player_returns_silence() {
        let mut player = SamplePlayer::new();
        let sample = player.next_sample();
        assert_eq!(sample.left, 0.0);
        assert_eq!(sample.right, 0.0);
    }

    #[test]
    fn mono_load_duplicates_channels() {
        let mut player = SamplePlayer::new();
        player.load_raw_mono(vec![0.1, 0.2, 0.3]);

        let s1 = player.next_sample();
        assert_eq!(s1.left, 0.1);
        assert_eq!(s1.right, 0.1);

        let s2 = player.next_sample();
        assert_eq!(s2.left, 0.2);
        assert_eq!(s2.right, 0.2);
    }

    #[test]
    fn loops_at_end_of_buffer() {
        // Use a player with zero crossfade to test basic wrapping.
        let mut player = SamplePlayer::with_crossfade(0);
        player.load_raw_mono(vec![1.0, 2.0]);

        let _ = player.next_sample(); // 1.0
        let _ = player.next_sample(); // 2.0
        let s3 = player.next_sample(); // loops to 1.0
        assert_eq!(s3.left, 1.0);
    }

    #[test]
    fn stereo_load_deinterleaves() {
        let mut player = SamplePlayer::new();
        player.load_raw_stereo(vec![0.1, 0.3], vec![0.2, 0.4]);

        let s1 = player.next_sample();
        assert_eq!(s1.left, 0.1);
        assert_eq!(s1.right, 0.2);

        let s2 = player.next_sample();
        assert_eq!(s2.left, 0.3);
        assert_eq!(s2.right, 0.4);
    }

    #[test]
    fn load_raw_pcm_mono() {
        let mut player = SamplePlayer::with_crossfade(0);
        let data = vec![0.5, -0.5, 0.25];
        player.load_raw_pcm(&data, 1).unwrap();
        assert!(player.is_loaded());
        assert_eq!(player.len(), 3);

        let s = player.next_sample();
        assert_eq!(s.left, 0.5);
        assert_eq!(s.right, 0.5);
    }

    #[test]
    fn load_raw_pcm_stereo() {
        let mut player = SamplePlayer::with_crossfade(0);
        let data = vec![0.1, 0.2, 0.3, 0.4]; // L R L R
        player.load_raw_pcm(&data, 2).unwrap();
        assert_eq!(player.len(), 2);

        let s1 = player.next_sample();
        assert_eq!(s1.left, 0.1);
        assert_eq!(s1.right, 0.2);
    }

    #[test]
    fn crossfade_blends_at_loop_boundary() {
        // Buffer of 10 frames, crossfade of 2 frames.
        // Values: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        // Crossfade region = last 2 frames (indices 8, 9).
        // At index 8: blend buf[8] * (1-alpha) + buf[0] * alpha  (alpha = 1/2)
        // At index 9: blend buf[9] * (1-alpha) + buf[1] * alpha  (alpha = 2/2)
        // After wrap, position jumps to 2 (past crossfade head region).
        let mut player = SamplePlayer::with_crossfade(2);
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        player.load_raw_mono(data);

        // Consume first 8 samples (indices 0..8, non-crossfade)
        for _ in 0..8 {
            player.next_sample();
        }

        // Frame at index 8: alpha = 1/2, blend 8.0*(0.5) + 0.0*(0.5) = 4.0
        let cf1 = player.next_sample();
        assert!((cf1.left - 4.0).abs() < 1e-5, "got {}", cf1.left);

        // Frame at index 9: alpha = 2/2 = 1.0, blend 9.0*(0.0) + 1.0*(1.0) = 1.0
        let cf2 = player.next_sample();
        assert!((cf2.left - 1.0).abs() < 1e-5, "got {}", cf2.left);

        // After wrap, position is 2 (past crossfade head)
        let after = player.next_sample();
        assert!((after.left - 2.0).abs() < 1e-5, "got {}", after.left);
    }

    #[test]
    fn crossfade_clamped_for_short_buffers() {
        // Buffer of 4 frames with crossfade of 100 → effective = 2 (half)
        let mut player = SamplePlayer::with_crossfade(100);
        player.load_raw_mono(vec![1.0, 2.0, 3.0, 4.0]);
        assert!(player.is_loaded());

        // Should not panic — crossfade is clamped to len/2
        for _ in 0..20 {
            let s = player.next_sample();
            assert!(s.left.is_finite());
        }
    }
}
