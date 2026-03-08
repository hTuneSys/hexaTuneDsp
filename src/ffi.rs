// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

//! C-compatible FFI interface for Flutter integration.
//!
//! This module exposes `extern "C"` functions that Flutter (Dart) can call
//! via `dart:ffi`. The FFI layer contains NO DSP logic — it only bridges
//! between Flutter and the [`Engine`].
//!
//! # Memory Model
//!
//! - [`htd_engine_init`] allocates an engine and returns an opaque pointer.
//! - All subsequent calls take this pointer as the first argument.
//! - [`htd_engine_destroy`] frees the engine. The caller must ensure
//!   [`htd_engine_stop`] was called and no concurrent [`htd_engine_render`]
//!   is in progress before calling destroy.
//!
//! # Return Convention
//!
//! - `0` = success
//! - Negative values = error codes (see [`HtdError`])

use std::ffi::CStr;
use std::os::raw::c_char;

use crate::engine::{DEFAULT_SAMPLE_RATE, Engine, EngineConfig, PendingConfig};
use crate::mixer::{
    DEFAULT_BASE_GAIN, DEFAULT_BINAURAL_GAIN, DEFAULT_EVENT_GAIN, DEFAULT_MASTER_GAIN,
    DEFAULT_TEXTURE_GAIN,
};
use crate::scheduler::CycleItem;

// ---------------------------------------------------------------------------
// FFI types
// ---------------------------------------------------------------------------

/// Opaque engine handle. Do not access fields directly.
pub type HtdEngine = Engine;

/// A single cycle item passed from Flutter.
#[repr(C)]
pub struct HtdCycleItem {
    pub frequency_delta: f32,
    pub duration_seconds: f32,
}

/// Engine configuration passed from Flutter.
#[repr(C)]
pub struct HtdEngineConfig {
    pub carrier_frequency: f32,
    pub binaural_enabled: bool,
    /// Pointer to an array of cycle items.
    pub cycle_items: *const HtdCycleItem,
    /// Number of cycle items.
    pub cycle_count: u32,
    pub sample_rate: f32,
    pub base_gain: f32,
    pub texture_gain: f32,
    pub event_gain: f32,
    pub binaural_gain: f32,
    pub master_gain: f32,
}

/// Audio layer configuration (raw PCM data).
#[repr(C)]
pub struct HtdLayerConfig {
    /// Pointer to interleaved f32 PCM samples.
    pub samples: *const f32,
    /// Number of sample frames (not individual samples).
    pub num_frames: u32,
    /// Number of channels: 1 (mono) or 2 (stereo interleaved).
    pub channels: u32,
}

/// Random event configuration.
#[repr(C)]
pub struct HtdEventConfig {
    /// Pointer to interleaved f32 PCM samples.
    pub samples: *const f32,
    /// Number of sample frames.
    pub num_frames: u32,
    /// Number of channels: 1 (mono) or 2 (stereo interleaved).
    pub channels: u32,
    /// Minimum interval between triggers in milliseconds.
    pub min_interval_ms: u32,
    /// Maximum interval between triggers in milliseconds.
    pub max_interval_ms: u32,
    /// Minimum playback volume (0.0–1.0).
    pub volume_min: f32,
    /// Maximum playback volume (0.0–1.0).
    pub volume_max: f32,
    /// Minimum stereo pan (-1.0 = left, 1.0 = right).
    pub pan_min: f32,
    /// Maximum stereo pan (-1.0 = left, 1.0 = right).
    pub pan_max: f32,
}

/// A stereo audio frame (used for documentation / cbindgen export).
#[repr(C)]
pub struct HtdStereoFrame {
    pub left: f32,
    pub right: f32,
}

/// Error codes returned by FFI functions.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtdError {
    Ok = 0,
    NullPointer = -1,
    InvalidConfig = -2,
    InitFailed = -3,
    InvalidUtf8 = -4,
    BufferTooSmall = -5,
    LoadFailed = -6,
    LayerLimitExceeded = -7,
    BaseRequired = -8,
}

// ---------------------------------------------------------------------------
// Helper: convert FFI config to Rust config
// ---------------------------------------------------------------------------

unsafe fn ffi_config_to_rust(config: *const HtdEngineConfig) -> Result<EngineConfig, HtdError> {
    if config.is_null() {
        return Err(HtdError::NullPointer);
    }

    let cfg = unsafe { &*config };

    let cycle_items = if cfg.cycle_items.is_null() || cfg.cycle_count == 0 {
        Vec::new()
    } else {
        let slice =
            unsafe { std::slice::from_raw_parts(cfg.cycle_items, cfg.cycle_count as usize) };
        slice
            .iter()
            .map(|item| CycleItem {
                frequency_delta: item.frequency_delta,
                duration_seconds: item.duration_seconds,
            })
            .collect()
    };

    let sample_rate = if cfg.sample_rate > 0.0 {
        cfg.sample_rate
    } else {
        DEFAULT_SAMPLE_RATE
    };

    let base_gain = if cfg.base_gain >= 0.0 {
        cfg.base_gain
    } else {
        DEFAULT_BASE_GAIN
    };
    let texture_gain = if cfg.texture_gain >= 0.0 {
        cfg.texture_gain
    } else {
        DEFAULT_TEXTURE_GAIN
    };
    let event_gain = if cfg.event_gain >= 0.0 {
        cfg.event_gain
    } else {
        DEFAULT_EVENT_GAIN
    };
    let binaural_gain = if cfg.binaural_gain >= 0.0 {
        cfg.binaural_gain
    } else {
        DEFAULT_BINAURAL_GAIN
    };
    let master_gain = if cfg.master_gain >= 0.0 {
        cfg.master_gain
    } else {
        DEFAULT_MASTER_GAIN
    };

    Ok(EngineConfig {
        carrier_frequency: cfg.carrier_frequency,
        binaural_enabled: cfg.binaural_enabled,
        cycle_items,
        sample_rate,
        base_gain,
        texture_gain,
        event_gain,
        binaural_gain,
        master_gain,
    })
}

// ---------------------------------------------------------------------------
// FFI functions — Lifecycle
// ---------------------------------------------------------------------------

/// Initialize the DSP engine with the given configuration.
///
/// Returns an opaque engine pointer on success, or null on failure.
/// The error code is written to `out_error` if non-null.
///
/// # Safety
///
/// - `config` must point to a valid `HtdEngineConfig`.
/// - The returned pointer must be freed with [`htd_engine_destroy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_init(
    config: *const HtdEngineConfig,
    out_error: *mut i32,
) -> *mut HtdEngine {
    let write_error = |err: HtdError, out: *mut i32| {
        if !out.is_null() {
            unsafe { *out = err as i32 };
        }
    };

    let rust_config = match unsafe { ffi_config_to_rust(config) } {
        Ok(c) => c,
        Err(e) => {
            write_error(e, out_error);
            return std::ptr::null_mut();
        }
    };

    match Engine::new(rust_config) {
        Ok(engine) => {
            write_error(HtdError::Ok, out_error);
            Box::into_raw(Box::new(engine))
        }
        Err(_) => {
            write_error(HtdError::InitFailed, out_error);
            std::ptr::null_mut()
        }
    }
}

/// Destroy the engine and free all associated memory.
///
/// # Safety
///
/// - `engine` must be a pointer returned by [`htd_engine_init`].
/// - Must not be called while [`htd_engine_render`] is in progress.
/// - After this call, the pointer is invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_destroy(engine: *mut HtdEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Start audio generation.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_start(engine: *mut HtdEngine) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.start();
    HtdError::Ok as i32
}

/// Stop audio generation. [`htd_engine_render`] will output silence.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_stop(engine: *mut HtdEngine) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.stop();
    HtdError::Ok as i32
}

// ---------------------------------------------------------------------------
// FFI functions — Render
// ---------------------------------------------------------------------------

/// Render `num_frames` of interleaved stereo f32 audio into `output`.
///
/// # Real-time safety
///
/// Safe to call from an audio callback.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `output` must point to a buffer of at least `num_frames * 2` f32 values.
/// - Must not be called concurrently with itself or [`htd_engine_destroy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_render(
    engine: *mut HtdEngine,
    output: *mut f32,
    num_frames: u32,
) -> i32 {
    if engine.is_null() || output.is_null() {
        return HtdError::NullPointer as i32;
    }

    let engine = unsafe { &mut *engine };
    let frames = num_frames as usize;
    let buffer = unsafe { std::slice::from_raw_parts_mut(output, frames * 2) };
    engine.render(buffer, frames);

    HtdError::Ok as i32
}

// ---------------------------------------------------------------------------
// FFI functions — Layer management
// ---------------------------------------------------------------------------

/// Set the base ambient layer from raw PCM data.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `config` must point to a valid `HtdLayerConfig` with valid `samples`.
/// - Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_base(
    engine: *mut HtdEngine,
    config: *const HtdLayerConfig,
) -> i32 {
    if engine.is_null() || config.is_null() {
        return HtdError::NullPointer as i32;
    }

    let cfg = unsafe { &*config };
    if cfg.samples.is_null() || cfg.num_frames == 0 {
        return HtdError::InvalidConfig as i32;
    }

    let total_samples = cfg.num_frames as usize * cfg.channels as usize;
    let data = unsafe { std::slice::from_raw_parts(cfg.samples, total_samples) };
    let engine = unsafe { &mut *engine };

    match engine.set_base_layer(data, cfg.channels) {
        Ok(()) => HtdError::Ok as i32,
        Err(_) => HtdError::InvalidConfig as i32,
    }
}

/// Remove the base layer and all dependent layers (textures, events).
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
/// Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_clear_base(engine: *mut HtdEngine) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &mut *engine }.clear_base_layer();
    HtdError::Ok as i32
}

/// Set a texture layer at the given index (0–2).
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `config` must point to valid `HtdLayerConfig`.
/// - Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_texture(
    engine: *mut HtdEngine,
    index: u32,
    config: *const HtdLayerConfig,
) -> i32 {
    if engine.is_null() || config.is_null() {
        return HtdError::NullPointer as i32;
    }

    let cfg = unsafe { &*config };
    if cfg.samples.is_null() || cfg.num_frames == 0 {
        return HtdError::InvalidConfig as i32;
    }

    let total_samples = cfg.num_frames as usize * cfg.channels as usize;
    let data = unsafe { std::slice::from_raw_parts(cfg.samples, total_samples) };
    let engine = unsafe { &mut *engine };

    match engine.set_texture_layer(index as usize, data, cfg.channels) {
        Ok(()) => HtdError::Ok as i32,
        Err(e) => {
            if e.contains("base") {
                HtdError::BaseRequired as i32
            } else {
                HtdError::LayerLimitExceeded as i32
            }
        }
    }
}

/// Remove a texture layer at the given index.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
/// Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_clear_texture(engine: *mut HtdEngine, index: u32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &mut *engine }.clear_texture_layer(index as usize);
    HtdError::Ok as i32
}

/// Register a random event at the given index (0–4).
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `config` must point to valid `HtdEventConfig`.
/// - Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_event(
    engine: *mut HtdEngine,
    index: u32,
    config: *const HtdEventConfig,
) -> i32 {
    if engine.is_null() || config.is_null() {
        return HtdError::NullPointer as i32;
    }

    let cfg = unsafe { &*config };
    if cfg.samples.is_null() || cfg.num_frames == 0 {
        return HtdError::InvalidConfig as i32;
    }

    let total_samples = cfg.num_frames as usize * cfg.channels as usize;
    let data = unsafe { std::slice::from_raw_parts(cfg.samples, total_samples) };
    let engine = unsafe { &mut *engine };

    match engine.set_event(
        index as usize,
        data,
        cfg.channels,
        cfg.min_interval_ms,
        cfg.max_interval_ms,
        cfg.volume_min,
        cfg.volume_max,
        cfg.pan_min,
        cfg.pan_max,
    ) {
        Ok(()) => HtdError::Ok as i32,
        Err(e) => {
            if e.contains("base") {
                HtdError::BaseRequired as i32
            } else {
                HtdError::LayerLimitExceeded as i32
            }
        }
    }
}

/// Remove a random event at the given index.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
/// Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_clear_event(engine: *mut HtdEngine, index: u32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &mut *engine }.clear_event(index as usize);
    HtdError::Ok as i32
}

/// Remove all layers (base, textures, events). Binaural is unaffected.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
/// Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_clear_all_layers(engine: *mut HtdEngine) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &mut *engine }.clear_all_layers();
    HtdError::Ok as i32
}

// ---------------------------------------------------------------------------
// FFI functions — Gain control
// ---------------------------------------------------------------------------

/// Set the base layer gain. Thread-safe (atomic).
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_base_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_base_gain(gain);
    HtdError::Ok as i32
}

/// Set the texture layer gain. Thread-safe (atomic).
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_texture_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_texture_gain(gain);
    HtdError::Ok as i32
}

/// Set the event layer gain. Thread-safe (atomic).
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_event_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_event_gain(gain);
    HtdError::Ok as i32
}

/// Set the binaural/tone layer gain. Thread-safe (atomic).
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_binaural_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_binaural_gain(gain);
    HtdError::Ok as i32
}

/// Set the master output gain. Thread-safe (atomic).
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_master_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_master_gain(gain);
    HtdError::Ok as i32
}

// ---------------------------------------------------------------------------
// FFI functions — Config & query
// ---------------------------------------------------------------------------

/// Update the engine binaural configuration at runtime.
///
/// Fields applied:
/// - `carrier_frequency`: updated if > 0
/// - `binaural_enabled`: always applied
/// - `cycle_items` / `cycle_count`: updated if non-null and count > 0
///
/// Gain fields and `sample_rate` are ignored — use dedicated setters.
///
/// # Safety
///
/// `engine` and `config` must be valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_update_config(
    engine: *mut HtdEngine,
    config: *const HtdEngineConfig,
) -> i32 {
    if engine.is_null() || config.is_null() {
        return HtdError::NullPointer as i32;
    }

    let cfg = unsafe { &*config };

    let carrier_frequency = if cfg.carrier_frequency > 0.0 {
        Some(cfg.carrier_frequency)
    } else {
        None
    };

    let cycle_items = if !cfg.cycle_items.is_null() && cfg.cycle_count > 0 {
        let slice =
            unsafe { std::slice::from_raw_parts(cfg.cycle_items, cfg.cycle_count as usize) };
        Some(
            slice
                .iter()
                .map(|item| CycleItem {
                    frequency_delta: item.frequency_delta,
                    duration_seconds: item.duration_seconds,
                })
                .collect(),
        )
    } else {
        None
    };

    let pending = PendingConfig {
        carrier_frequency,
        binaural_enabled: Some(cfg.binaural_enabled),
        cycle_items,
    };

    unsafe { &*engine }.queue_config_update(pending);

    HtdError::Ok as i32
}

/// Load a base layer from a WAV file (convenience function).
///
/// This allocates memory and performs I/O.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `path` must be a valid null-terminated UTF-8 string.
/// - Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_load_base_wav(
    engine: *mut HtdEngine,
    path: *const c_char,
) -> i32 {
    if engine.is_null() || path.is_null() {
        return HtdError::NullPointer as i32;
    }

    let cstr = unsafe { CStr::from_ptr(path) };
    let path_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return HtdError::InvalidUtf8 as i32,
    };

    let engine = unsafe { &mut *engine };
    match engine.load_base_wav(path_str) {
        Ok(()) => HtdError::Ok as i32,
        Err(_) => HtdError::LoadFailed as i32,
    }
}

/// Query whether the engine is currently running.
///
/// Returns `1` if running, `0` if stopped, or a negative error code.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_is_running(engine: *const HtdEngine) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    if unsafe { &*engine }.is_running() {
        1
    } else {
        0
    }
}

/// Return the engine's sample rate.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_sample_rate(engine: *const HtdEngine) -> f32 {
    if engine.is_null() {
        return 0.0;
    }
    unsafe { &*engine }.sample_rate()
}
