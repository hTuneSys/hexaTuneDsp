// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Rain / ambient sound sample player.
//!
//! Loads a WAV file into memory during initialization (non-real-time) and
//! loops playback from the pre-allocated buffer. The playback path performs
//! zero allocations.

use crate::binaural::StereoSample;

/// Looping sample player for ambient audio (rain, white noise, etc.).
pub struct RainPlayer {
    /// Interleaved or mono sample buffer (pre-loaded).
    buffer_left: Vec<f32>,
    buffer_right: Vec<f32>,
    /// Current playback position in samples.
    position: usize,
    /// Whether the player has valid audio data loaded.
    loaded: bool,
}

impl RainPlayer {
    /// Create an empty player with no audio loaded.
    pub fn new() -> Self {
        Self {
            buffer_left: Vec::new(),
            buffer_right: Vec::new(),
            position: 0,
            loaded: false,
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

        match channels {
            1 => {
                // Mono: duplicate to both channels
                self.buffer_left = samples.clone();
                self.buffer_right = samples;
            }
            2 => {
                // Stereo: de-interleave
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
        self.loaded = true;
        Ok(())
    }

    /// Load raw f32 PCM samples directly (mono). Useful for testing or
    /// when the audio is already decoded.
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

    /// Fetch the next stereo sample. Loops when reaching the end of the
    /// buffer. Returns silence if no audio is loaded.
    ///
    /// This function is real-time safe: no allocation, no branching beyond
    /// the loop wrap check.
    #[inline]
    pub fn next_sample(&mut self) -> StereoSample {
        if !self.loaded {
            return StereoSample::default();
        }

        let left = self.buffer_left[self.position];
        let right = self.buffer_right[self.position];

        self.position += 1;
        if self.position >= self.buffer_left.len() {
            self.position = 0;
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
}

impl Default for RainPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_player_returns_silence() {
        let mut player = RainPlayer::new();
        let sample = player.next_sample();
        assert_eq!(sample.left, 0.0);
        assert_eq!(sample.right, 0.0);
    }

    #[test]
    fn mono_load_duplicates_channels() {
        let mut player = RainPlayer::new();
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
        let mut player = RainPlayer::new();
        player.load_raw_mono(vec![1.0, 2.0]);

        let _ = player.next_sample(); // 1.0
        let _ = player.next_sample(); // 2.0
        let s3 = player.next_sample(); // loops to 1.0
        assert_eq!(s3.left, 1.0);
    }

    #[test]
    fn stereo_load_deinterleaves() {
        let mut player = RainPlayer::new();
        player.load_raw_stereo(vec![0.1, 0.3], vec![0.2, 0.4]);

        let s1 = player.next_sample();
        assert_eq!(s1.left, 0.1);
        assert_eq!(s1.right, 0.2);

        let s2 = player.next_sample();
        assert_eq!(s2.left, 0.3);
        assert_eq!(s2.right, 0.4);
    }
}
