// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Stereo audio mixer for the soundscape engine.
//!
//! Combines binaural tone, base layer, texture layers, and event layer
//! with configurable per-layer gain and a master gain. Real-time safe:
//! no allocations, no branching.

use crate::binaural::StereoSample;

/// Default gain values for each layer.
pub const DEFAULT_BASE_GAIN: f32 = 0.6;
pub const DEFAULT_TEXTURE_GAIN: f32 = 0.3;
pub const DEFAULT_EVENT_GAIN: f32 = 0.4;
pub const DEFAULT_BINAURAL_GAIN: f32 = 0.15;
pub const DEFAULT_MASTER_GAIN: f32 = 1.0;

/// Per-layer gain settings.
#[derive(Debug, Clone, Copy)]
pub struct LayerGains {
    pub base: f32,
    pub texture: f32,
    pub event: f32,
    pub binaural: f32,
    pub master: f32,
}

impl Default for LayerGains {
    fn default() -> Self {
        Self {
            base: DEFAULT_BASE_GAIN,
            texture: DEFAULT_TEXTURE_GAIN,
            event: DEFAULT_EVENT_GAIN,
            binaural: DEFAULT_BINAURAL_GAIN,
            master: DEFAULT_MASTER_GAIN,
        }
    }
}

/// Stereo mixer for combining soundscape layers.
pub struct Mixer {
    gains: LayerGains,
}

impl Mixer {
    /// Create a new mixer with the given gain levels.
    pub fn new(gains: LayerGains) -> Self {
        Self { gains }
    }

    /// Set the base layer gain.
    #[inline]
    pub fn set_base_gain(&mut self, gain: f32) {
        self.gains.base = gain;
    }

    /// Set the texture layer gain.
    #[inline]
    pub fn set_texture_gain(&mut self, gain: f32) {
        self.gains.texture = gain;
    }

    /// Set the event layer gain.
    #[inline]
    pub fn set_event_gain(&mut self, gain: f32) {
        self.gains.event = gain;
    }

    /// Set the binaural/tone layer gain.
    #[inline]
    pub fn set_binaural_gain(&mut self, gain: f32) {
        self.gains.binaural = gain;
    }

    /// Set the master gain.
    #[inline]
    pub fn set_master_gain(&mut self, gain: f32) {
        self.gains.master = gain;
    }

    /// Current gains.
    #[inline]
    pub fn gains(&self) -> &LayerGains {
        &self.gains
    }

    /// Mix all layers into a final stereo output.
    ///
    /// `texture_sum` is the pre-summed stereo sample from all active texture
    /// layers (the caller sums them before passing here).
    ///
    /// The output is clamped to [-1.0, 1.0].
    #[inline]
    pub fn mix(
        &self,
        binaural: StereoSample,
        base: StereoSample,
        texture_sum: StereoSample,
        event: StereoSample,
    ) -> StereoSample {
        let g = &self.gains;

        let left = (binaural.left * g.binaural
            + base.left * g.base
            + texture_sum.left * g.texture
            + event.left * g.event)
            * g.master;

        let right = (binaural.right * g.binaural
            + base.right * g.base
            + texture_sum.right * g.texture
            + event.right * g.event)
            * g.master;

        StereoSample {
            left: left.clamp(-1.0, 1.0),
            right: right.clamp(-1.0, 1.0),
        }
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new(LayerGains::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_applies_gains() {
        let mixer = Mixer::new(LayerGains {
            base: 1.0,
            texture: 0.0,
            event: 0.0,
            binaural: 0.0,
            master: 1.0,
        });
        let base = StereoSample {
            left: 0.5,
            right: 0.5,
        };
        let silence = StereoSample::default();
        let out = mixer.mix(silence, base, silence, silence);
        assert!((out.left - 0.5).abs() < 1e-6);
        assert!((out.right - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mix_zero_gain_produces_silence() {
        let mixer = Mixer::new(LayerGains {
            base: 0.0,
            texture: 0.0,
            event: 0.0,
            binaural: 0.0,
            master: 1.0,
        });
        let loud = StereoSample {
            left: 0.9,
            right: -0.8,
        };
        let out = mixer.mix(loud, loud, loud, loud);
        assert_eq!(out.left, 0.0);
        assert_eq!(out.right, 0.0);
    }

    #[test]
    fn output_is_clamped() {
        let mixer = Mixer::new(LayerGains {
            base: 1.0,
            texture: 1.0,
            event: 1.0,
            binaural: 1.0,
            master: 1.0,
        });
        let full = StereoSample {
            left: 1.0,
            right: -1.0,
        };
        let out = mixer.mix(full, full, full, full);
        assert!(out.left <= 1.0);
        assert!(out.right >= -1.0);
    }

    #[test]
    fn default_gains_are_correct() {
        let mixer = Mixer::default();
        let g = mixer.gains();
        assert!((g.base - DEFAULT_BASE_GAIN).abs() < 1e-6);
        assert!((g.texture - DEFAULT_TEXTURE_GAIN).abs() < 1e-6);
        assert!((g.event - DEFAULT_EVENT_GAIN).abs() < 1e-6);
        assert!((g.binaural - DEFAULT_BINAURAL_GAIN).abs() < 1e-6);
        assert!((g.master - DEFAULT_MASTER_GAIN).abs() < 1e-6);
    }

    #[test]
    fn master_gain_scales_everything() {
        let mixer = Mixer::new(LayerGains {
            base: 1.0,
            texture: 0.0,
            event: 0.0,
            binaural: 0.0,
            master: 0.5,
        });
        let base = StereoSample {
            left: 0.8,
            right: 0.8,
        };
        let silence = StereoSample::default();
        let out = mixer.mix(silence, base, silence, silence);
        assert!((out.left - 0.4).abs() < 1e-6);
        assert!((out.right - 0.4).abs() < 1e-6);
    }

    #[test]
    fn all_layers_contribute() {
        let mixer = Mixer::new(LayerGains {
            base: 0.25,
            texture: 0.25,
            event: 0.25,
            binaural: 0.25,
            master: 1.0,
        });
        let sample = StereoSample {
            left: 1.0,
            right: 1.0,
        };
        let out = mixer.mix(sample, sample, sample, sample);
        assert!((out.left - 1.0).abs() < 1e-6);
        assert!((out.right - 1.0).abs() < 1e-6);
    }
}
