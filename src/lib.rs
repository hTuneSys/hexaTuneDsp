// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! HexaTune DSP — real-time audio synthesis engine with C FFI for Flutter.
//!
//! This crate provides a binaural beat / amplitude modulation audio engine
//! with rain/ambient sound mixing and timed frequency cycle scheduling.
//!
//! The engine is designed to be controlled from Flutter via a C-compatible
//! FFI interface. All audio generation happens in Rust with real-time safety
//! guarantees: no heap allocation, no I/O, and no blocking inside the audio
//! render path.
//!
//! # Modules
//!
//! - [`oscillator`] — Phase-accumulator sine oscillator
//! - [`binaural`] — Binaural beat / AM tone generator
//! - [`rain_player`] — Looping ambient sound player
//! - [`scheduler`] — Frequency cycle scheduler
//! - [`mixer`] — Stereo audio mixer
//! - [`engine`] — Main DSP engine orchestrator
//! - [`ffi`] — C-compatible FFI interface

pub mod binaural;
pub mod engine;
pub mod ffi;
pub mod mixer;
pub mod oscillator;
pub mod rain_player;
pub mod scheduler;
