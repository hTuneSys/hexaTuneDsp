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

use crate::engine::{Engine, EngineConfig, PendingConfig, DEFAULT_RAIN_GAIN, DEFAULT_SAMPLE_RATE, DEFAULT_TONE_GAIN};
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
    /// Null-terminated UTF-8 path, or null if no rain sound.
    pub rain_sound_path: *const c_char,
    /// Pointer to an array of cycle items.
    pub cycle_items: *const HtdCycleItem,
    /// Number of cycle items.
    pub cycle_count: u32,
    pub sample_rate: f32,
    pub rain_gain: f32,
    pub tone_gain: f32,
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
    /// Operation succeeded.
    Ok = 0,
    /// Null pointer argument.
    NullPointer = -1,
    /// Invalid configuration.
    InvalidConfig = -2,
    /// Engine initialization failed.
    InitFailed = -3,
    /// Invalid UTF-8 string.
    InvalidUtf8 = -4,
    /// Buffer too small.
    BufferTooSmall = -5,
    /// WAV file load failed.
    LoadFailed = -6,
}

// ---------------------------------------------------------------------------
// Helper: convert FFI config to Rust config
// ---------------------------------------------------------------------------

unsafe fn ffi_config_to_rust(config: *const HtdEngineConfig) -> Result<EngineConfig, HtdError> {
    if config.is_null() {
        return Err(HtdError::NullPointer);
    }

    let cfg = unsafe { &*config };

    let rain_sound_path = if cfg.rain_sound_path.is_null() {
        None
    } else {
        let cstr = unsafe { CStr::from_ptr(cfg.rain_sound_path) };
        Some(
            cstr.to_str()
                .map_err(|_| HtdError::InvalidUtf8)?
                .to_owned(),
        )
    };

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

    let rain_gain = if cfg.rain_gain >= 0.0 {
        cfg.rain_gain
    } else {
        DEFAULT_RAIN_GAIN
    };

    let tone_gain = if cfg.tone_gain >= 0.0 {
        cfg.tone_gain
    } else {
        DEFAULT_TONE_GAIN
    };

    Ok(EngineConfig {
        carrier_frequency: cfg.carrier_frequency,
        binaural_enabled: cfg.binaural_enabled,
        rain_sound_path,
        cycle_items,
        sample_rate,
        rain_gain,
        tone_gain,
    })
}

// ---------------------------------------------------------------------------
// FFI functions
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

/// Start audio generation. After this call, [`htd_engine_render`] will
/// produce audio samples instead of silence.
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

/// Render `num_frames` of interleaved stereo f32 audio into `output`.
///
/// The output buffer must hold at least `num_frames * 2` floats
/// (left, right, left, right, ...).
///
/// # Real-time safety
///
/// This function is safe to call from an audio callback. It does not
/// allocate, does not perform I/O, and does not block.
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

/// Set the rain/ambient gain level. Can be called from any thread.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_rain_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_rain_gain(gain);
    HtdError::Ok as i32
}

/// Set the tone gain level. Can be called from any thread.
///
/// # Safety
///
/// `engine` must be a valid engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_set_tone_gain(engine: *mut HtdEngine, gain: f32) -> i32 {
    if engine.is_null() {
        return HtdError::NullPointer as i32;
    }
    unsafe { &*engine }.set_tone_gain(gain);
    HtdError::Ok as i32
}

/// Update the engine configuration at runtime. Changes are applied on the
/// next render call. This function may be called from any thread.
///
/// Fields in the config are applied as follows:
/// - `carrier_frequency`: updated if > 0
/// - `binaural_enabled`: always applied
/// - `cycle_items` / `cycle_count`: updated if `cycle_items` is non-null
///   and `cycle_count` > 0
///
/// `rain_sound_path`, `sample_rate`, and gain fields are ignored.
/// Use dedicated setters for gains.
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

/// Load a rain/ambient sound from a WAV file.
///
/// This function allocates memory and performs I/O. It must NOT be called
/// from an audio callback.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `path` must be a valid null-terminated UTF-8 string.
/// - Must not be called concurrently with [`htd_engine_render`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn htd_engine_load_rain(
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
    match engine.load_rain_sound(path_str) {
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
