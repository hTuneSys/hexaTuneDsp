// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Stereo audio mixer.
//!
//! Combines tone and rain audio sources with configurable gain levels.
//! Real-time safe: no allocations, no branching.

use crate::binaural::StereoSample;

/// Stereo mixer for combining tone and rain audio sources.
pub struct Mixer {
    rain_gain: f32,
    tone_gain: f32,
}

impl Mixer {
    /// Create a new mixer with the given gain levels.
    pub fn new(rain_gain: f32, tone_gain: f32) -> Self {
        Self {
            rain_gain,
            tone_gain,
        }
    }

    /// Set the rain/ambient gain level.
    #[inline]
    pub fn set_rain_gain(&mut self, gain: f32) {
        self.rain_gain = gain;
    }

    /// Set the tone gain level.
    #[inline]
    pub fn set_tone_gain(&mut self, gain: f32) {
        self.tone_gain = gain;
    }

    /// Current rain gain.
    #[inline]
    pub fn rain_gain(&self) -> f32 {
        self.rain_gain
    }

    /// Current tone gain.
    #[inline]
    pub fn tone_gain(&self) -> f32 {
        self.tone_gain
    }

    /// Mix a tone sample and a rain sample into a final stereo output.
    ///
    /// `output = rain * rain_gain + tone * tone_gain`
    ///
    /// The output is soft-clamped to [-1.0, 1.0].
    #[inline]
    pub fn mix(&self, tone: StereoSample, rain: StereoSample) -> StereoSample {
        StereoSample {
            left: (rain.left * self.rain_gain + tone.left * self.tone_gain).clamp(-1.0, 1.0),
            right: (rain.right * self.rain_gain + tone.right * self.tone_gain).clamp(-1.0, 1.0),
        }
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new(0.8, 0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_applies_gains() {
        let mixer = Mixer::new(0.5, 0.5);
        let tone = StereoSample {
            left: 1.0,
            right: 1.0,
        };
        let rain = StereoSample {
            left: 1.0,
            right: 1.0,
        };
        let out = mixer.mix(tone, rain);
        assert!((out.left - 1.0).abs() < 1e-6);
        assert!((out.right - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mix_zero_gain_produces_silence() {
        let mixer = Mixer::new(0.0, 0.0);
        let tone = StereoSample {
            left: 0.8,
            right: -0.5,
        };
        let rain = StereoSample {
            left: 0.3,
            right: 0.7,
        };
        let out = mixer.mix(tone, rain);
        assert_eq!(out.left, 0.0);
        assert_eq!(out.right, 0.0);
    }

    #[test]
    fn output_is_clamped() {
        let mixer = Mixer::new(1.0, 1.0);
        let tone = StereoSample {
            left: 1.0,
            right: -1.0,
        };
        let rain = StereoSample {
            left: 1.0,
            right: -1.0,
        };
        let out = mixer.mix(tone, rain);
        assert!(out.left <= 1.0);
        assert!(out.right >= -1.0);
    }

    #[test]
    fn default_gains_are_correct() {
        let mixer = Mixer::default();
        assert!((mixer.rain_gain() - 0.8).abs() < 1e-6);
        assert!((mixer.tone_gain() - 0.2).abs() < 1e-6);
    }
}
