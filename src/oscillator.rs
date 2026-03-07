// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! Phase-accumulator sine oscillator.
//!
//! Generates a continuous sine wave at a given frequency using an f32 phase
//! accumulator. Real-time safe: no allocations, no branching beyond wrap.

use std::f32::consts::TAU;

/// Sine oscillator using a phase accumulator.
pub struct Oscillator {
    phase: f32,
    phase_increment: f32,
    sample_rate: f32,
}

impl Oscillator {
    /// Create a new oscillator at the given frequency and sample rate.
    pub fn new(frequency: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            phase_increment: TAU * frequency / sample_rate,
            sample_rate,
        }
    }

    /// Update the oscillator frequency. Phase is preserved to avoid clicks.
    #[inline]
    pub fn set_frequency(&mut self, frequency: f32) {
        self.phase_increment = TAU * frequency / self.sample_rate;
    }

    /// Return the current frequency.
    #[inline]
    pub fn frequency(&self) -> f32 {
        self.phase_increment * self.sample_rate / TAU
    }

    /// Generate the next sample and advance the phase.
    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let sample = self.phase.sin();
        self.phase += self.phase_increment;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        sample
    }

    /// Reset phase to zero.
    #[inline]
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oscillator_produces_sine_at_correct_frequency() {
        let sample_rate = 48000.0;
        let frequency = 440.0;
        let mut osc = Oscillator::new(frequency, sample_rate);

        // At t=0, sin(0) = 0
        let first = osc.next_sample();
        assert!(first.abs() < 1e-5, "first sample should be ~0, got {first}");

        // Generate one full cycle worth of samples
        let samples_per_cycle = (sample_rate / frequency) as usize;
        let mut max_val: f32 = 0.0;
        for _ in 0..samples_per_cycle {
            let s = osc.next_sample();
            max_val = max_val.max(s.abs());
        }
        assert!(max_val > 0.99, "peak amplitude should be ~1.0, got {max_val}");
    }

    #[test]
    fn set_frequency_preserves_phase() {
        let mut osc = Oscillator::new(440.0, 48000.0);
        // Advance a bit
        for _ in 0..100 {
            osc.next_sample();
        }
        let phase_before = osc.phase;
        osc.set_frequency(880.0);
        assert!(
            (osc.phase - phase_before).abs() < 1e-10,
            "phase should be preserved after frequency change"
        );
    }

    #[test]
    fn output_stays_in_range() {
        let mut osc = Oscillator::new(1000.0, 48000.0);
        for _ in 0..96000 {
            let s = osc.next_sample();
            assert!(
                (-1.0..=1.0).contains(&s),
                "sample out of range: {s}"
            );
        }
    }
}
