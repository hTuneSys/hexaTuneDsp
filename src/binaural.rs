// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Binaural beat generator.
//!
//! In binaural mode: generates stereo output where the left channel plays the
//! carrier frequency and the right channel plays carrier + delta.
//!
//! In non-binaural (AM) mode: generates a mono carrier modulated by a low
//! frequency oscillator at the delta frequency, output to both channels.

use crate::oscillator::Oscillator;

/// Stereo sample pair (left, right).
#[derive(Debug, Clone, Copy, Default)]
pub struct StereoSample {
    pub left: f32,
    pub right: f32,
}

/// Binaural beat and amplitude-modulation tone generator.
pub struct BinauralGenerator {
    left_osc: Oscillator,
    right_osc: Oscillator,
    mod_osc: Oscillator,
    carrier_frequency: f32,
    delta: f32,
    binaural_enabled: bool,
}

impl BinauralGenerator {
    /// Create a new generator.
    pub fn new(
        carrier_frequency: f32,
        delta: f32,
        binaural_enabled: bool,
        sample_rate: f32,
    ) -> Self {
        let left_osc = Oscillator::new(carrier_frequency, sample_rate);
        let right_osc = Oscillator::new(carrier_frequency + delta, sample_rate);
        let mod_osc = Oscillator::new(delta, sample_rate);

        Self {
            left_osc,
            right_osc,
            mod_osc,
            carrier_frequency,
            delta,
            binaural_enabled,
        }
    }

    /// Update the frequency delta. Adjusts oscillators without resetting phase.
    #[inline]
    pub fn set_delta(&mut self, delta: f32) {
        self.delta = delta;
        self.right_osc
            .set_frequency(self.carrier_frequency + delta);
        self.mod_osc.set_frequency(delta);
    }

    /// Update the carrier frequency.
    #[inline]
    pub fn set_carrier_frequency(&mut self, frequency: f32) {
        self.carrier_frequency = frequency;
        self.left_osc.set_frequency(frequency);
        self.right_osc.set_frequency(frequency + self.delta);
    }

    /// Switch between binaural and AM mode.
    #[inline]
    pub fn set_binaural_enabled(&mut self, enabled: bool) {
        self.binaural_enabled = enabled;
    }

    /// Generate the next stereo sample pair.
    #[inline]
    pub fn generate(&mut self) -> StereoSample {
        if self.binaural_enabled {
            StereoSample {
                left: self.left_osc.next_sample(),
                right: self.right_osc.next_sample(),
            }
        } else {
            let carrier = self.left_osc.next_sample();
            // AM: modulate carrier amplitude with delta-frequency LFO.
            // mod_osc outputs [-1, 1]; map to [0, 1] for unipolar modulation.
            let modulator = (self.mod_osc.next_sample() + 1.0) * 0.5;
            let output = carrier * modulator;
            StereoSample {
                left: output,
                right: output,
            }
        }
    }

    /// Current delta value.
    #[inline]
    pub fn delta(&self) -> f32 {
        self.delta
    }

    /// Current carrier frequency.
    #[inline]
    pub fn carrier_frequency(&self) -> f32 {
        self.carrier_frequency
    }

    /// Reset all oscillator phases to zero.
    pub fn reset(&mut self) {
        self.left_osc.reset();
        self.right_osc.reset();
        self.mod_osc.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binaural_produces_different_channels() {
        let mut bgen = BinauralGenerator::new(400.0, 5.0, true, 48000.0);

        // Advance past the first sample (both start at 0)
        for _ in 0..100 {
            bgen.generate();
        }

        let sample = bgen.generate();
        // Left and right should differ since frequencies differ
        assert!(
            (sample.left - sample.right).abs() > 1e-6,
            "binaural channels should differ"
        );
    }

    #[test]
    fn am_mode_produces_identical_channels() {
        let mut bgen = BinauralGenerator::new(400.0, 5.0, false, 48000.0);

        for _ in 0..100 {
            let sample = bgen.generate();
            assert!(
                (sample.left - sample.right).abs() < 1e-10,
                "AM mode should produce identical L/R"
            );
        }
    }

    #[test]
    fn set_delta_changes_output() {
        let mut bgen = BinauralGenerator::new(400.0, 3.0, true, 48000.0);
        // Collect some output
        let mut sum_before = 0.0f32;
        for _ in 0..1000 {
            sum_before += bgen.generate().right;
        }

        bgen.reset();
        bgen.set_delta(10.0);
        let mut sum_after = 0.0f32;
        for _ in 0..1000 {
            sum_after += bgen.generate().right;
        }

        // Different deltas produce different sums
        assert!(
            (sum_before - sum_after).abs() > 0.01,
            "changing delta should change output"
        );
    }
}
