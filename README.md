# hexaTuneDsp

Real-time audio synthesis engine written in Rust with a C-compatible FFI interface for Flutter.

Generates binaural beats, amplitude-modulated tones, and mixes in ambient audio (rain, white noise, etc.) with timed frequency cycle scheduling — all in a real-time safe render loop with zero allocations during playback.

---

## Table of Contents

- [Features](#features)
- [Architecture](#architecture)
- [Project Structure](#project-structure)
- [Building](#building)
- [Testing](#testing)
- [FFI API Reference](#ffi-api-reference)
  - [Types](#types)
  - [Lifecycle Functions](#lifecycle-functions)
  - [Audio Rendering](#audio-rendering)
  - [Parameter Control](#parameter-control)
  - [Configuration Update](#configuration-update)
  - [Error Codes](#error-codes)
- [Flutter Integration (Dart)](#flutter-integration-dart)
- [Usage Example (C)](#usage-example-c)
- [Audio Engine Details](#audio-engine-details)
  - [Binaural Mode](#binaural-mode)
  - [AM Mode (Non-Binaural)](#am-mode-non-binaural)
  - [Frequency Cycle Scheduling](#frequency-cycle-scheduling)
  - [Rain / Ambient Sound](#rain--ambient-sound)
  - [Mixer](#mixer)
- [Real-Time Safety](#real-time-safety)
- [Cross-Compilation](#cross-compilation)
- [License](#license)

---

## Features

- **Binaural beat generation** — stereo output with different frequencies per ear
- **Amplitude modulation fallback** — mono pulsing mode when binaural is disabled
- **Ambient sound mixing** — loops pre-loaded WAV audio (rain, white noise, etc.)
- **Timed frequency cycles** — automatically rotates through frequency deltas on a schedule
- **C-compatible FFI** — works with any language that supports C FFI (Dart, Swift, Kotlin, C++)
- **Real-time safe** — zero allocations, zero I/O, zero locks in the audio callback
- **Auto-generated C header** — `include/hexatune_dsp_ffi.h` via cbindgen

---

## Architecture

```
Flutter UI Layer          (Dart — UI, state management, presets)
       ↓
Flutter Control Layer     (Dart — calls FFI functions)
       ↓
Rust FFI Bridge           (src/ffi.rs — extern "C" functions, no DSP logic)
       ↓
Rust DSP Engine           (src/engine.rs — orchestrates all audio modules)
       ↓
 ┌─────────────┬───────────────┬────────────────┬──────────┐
 │ Oscillator  │   Binaural    │  Rain Player   │ Scheduler│
 │ (sine wave) │ (stereo/AM)   │ (WAV looping)  │ (cycles) │
 └─────────────┴───────────────┴────────────────┴──────────┘
                        ↓
                      Mixer  →  interleaved stereo f32 output
```

Flutter sends control commands. Rust generates all audio.

---

## Project Structure

```
hexaTuneDsp/
├── Cargo.toml              # Crate config (cdylib + staticlib + lib)
├── cbindgen.toml           # C header generation settings
├── build.rs                # Runs cbindgen at build time
├── include/
│   └── hexatune_dsp_ffi.h  # Auto-generated C header
├── src/
│   ├── lib.rs              # Module declarations
│   ├── oscillator.rs       # Phase-accumulator sine oscillator
│   ├── binaural.rs         # Binaural beat / AM tone generator
│   ├── rain_player.rs      # WAV file loader + looping playback
│   ├── scheduler.rs        # Frequency delta cycle scheduler
│   ├── mixer.rs            # Stereo audio mixer
│   ├── engine.rs           # Main DSP engine orchestrator
│   └── ffi.rs              # C-compatible FFI interface
└── ARCHITECTURE.md         # Detailed architecture specification
```

---

## Building

### Prerequisites

- [Rust](https://rustup.rs/) (1.85+ recommended, edition 2024)

### Debug Build

```bash
cargo build
```

### Release Build (optimized, LTO enabled)

```bash
cargo build --release
```

### Build Artifacts

| Platform | Library                                             |
|----------|-----------------------------------------------------|
| Linux    | `target/release/libhexatune_dsp_ffi.so`             |
| macOS    | `target/release/libhexatune_dsp_ffi.dylib`          |
| Windows  | `target/release/hexatune_dsp_ffi.dll`               |
| (static) | `target/release/libhexatune_dsp_ffi.a`              |

The C header is automatically generated at `include/hexatune_dsp_ffi.h` during build.

---

## Testing

```bash
cargo test
```

Run with clippy linting:

```bash
cargo clippy
```

---

## FFI API Reference

All functions use the `htd_` prefix. Include the generated header:

```c
#include "hexatune_dsp_ffi.h"
```

### Types

#### `HtdCycleItem`

A single step in the frequency cycle.

```c
typedef struct HtdCycleItem {
    float frequency_delta;    // Hz delta added to carrier
    float duration_seconds;   // How long this step lasts
} HtdCycleItem;
```

#### `HtdEngineConfig`

Configuration for engine initialization and runtime updates.

```c
typedef struct HtdEngineConfig {
    float carrier_frequency;              // Base frequency in Hz (e.g. 400.0)
    bool binaural_enabled;                // true = binaural, false = AM mode
    const char *rain_sound_path;          // Path to WAV file, or NULL
    const HtdCycleItem *cycle_items;      // Array of cycle steps
    uint32_t cycle_count;                 // Number of cycle items
    float sample_rate;                    // Sample rate (default: 48000)
    float rain_gain;                      // Rain volume 0.0–1.0 (default: 0.8)
    float tone_gain;                      // Tone volume 0.0–1.0 (default: 0.2)
} HtdEngineConfig;
```

### Lifecycle Functions

#### `htd_engine_init`

Create and initialize the DSP engine.

```c
HtdEngine *htd_engine_init(const HtdEngineConfig *config, int32_t *out_error);
```

- Returns an opaque `HtdEngine*` pointer on success, `NULL` on failure.
- Error code is written to `out_error` (pass `NULL` to ignore).
- Loads the rain WAV file if `rain_sound_path` is non-null.

#### `htd_engine_destroy`

Free the engine and all associated memory.

```c
void htd_engine_destroy(HtdEngine *engine);
```

- Must call `htd_engine_stop` first and ensure no `htd_engine_render` is in progress.

#### `htd_engine_start`

Start audio generation.

```c
int32_t htd_engine_start(HtdEngine *engine);  // Returns 0 on success
```

#### `htd_engine_stop`

Stop audio generation. Render will output silence.

```c
int32_t htd_engine_stop(HtdEngine *engine);  // Returns 0 on success
```

#### `htd_engine_is_running`

Query engine state.

```c
int32_t htd_engine_is_running(const HtdEngine *engine);
// Returns: 1 = running, 0 = stopped, <0 = error
```

#### `htd_engine_sample_rate`

Query the engine's sample rate.

```c
float htd_engine_sample_rate(const HtdEngine *engine);
```

### Audio Rendering

#### `htd_engine_render`

Fill a buffer with interleaved stereo f32 audio.

```c
int32_t htd_engine_render(HtdEngine *engine, float *output, uint32_t num_frames);
```

- `output` must hold at least `num_frames * 2` floats (L, R, L, R, ...).
- **Real-time safe** — no allocation, no I/O, no blocking.
- Call this from your platform audio callback.
- Returns `0` on success.

### Parameter Control

#### `htd_engine_set_rain_gain`

Set the rain/ambient volume level. Thread-safe (uses atomics).

```c
int32_t htd_engine_set_rain_gain(HtdEngine *engine, float gain);  // 0.0–1.0
```

#### `htd_engine_set_tone_gain`

Set the tone volume level. Thread-safe (uses atomics).

```c
int32_t htd_engine_set_tone_gain(HtdEngine *engine, float gain);  // 0.0–1.0
```

#### `htd_engine_load_rain`

Load or replace the ambient sound from a WAV file.

```c
int32_t htd_engine_load_rain(HtdEngine *engine, const char *path);
```

- **Not real-time safe** — allocates memory, performs I/O.
- Must not be called concurrently with `htd_engine_render`.

### Configuration Update

#### `htd_engine_update_config`

Update engine parameters at runtime. Changes are applied on the next render call.

```c
int32_t htd_engine_update_config(HtdEngine *engine, const HtdEngineConfig *config);
```

- `carrier_frequency`: applied if > 0
- `binaural_enabled`: always applied
- `cycle_items` + `cycle_count`: applied if non-null and count > 0
- `rain_sound_path`, `sample_rate`, gain fields are **ignored** (use dedicated setters)
- Thread-safe — can be called from any thread.

### Error Codes

| Code | Name           | Description                       |
|------|----------------|-----------------------------------|
| 0    | Ok             | Success                           |
| -1   | NullPointer    | Null pointer argument             |
| -2   | InvalidConfig  | Invalid configuration             |
| -3   | InitFailed     | Engine initialization failed      |
| -4   | InvalidUtf8    | Invalid UTF-8 string              |
| -5   | BufferTooSmall | Output buffer too small           |
| -6   | LoadFailed     | WAV file load failed              |

---

## Flutter Integration (Dart)

### Loading the Library

```dart
import 'dart:ffi';
import 'package:ffi/ffi.dart';

// Load the native library
final DynamicLibrary nativeLib = Platform.isAndroid
    ? DynamicLibrary.open('libhexatune_dsp_ffi.so')
    : DynamicLibrary.process(); // iOS uses static linking

// Bind functions
typedef InitNative = Pointer<Void> Function(Pointer<HtdEngineConfig>, Pointer<Int32>);
typedef InitDart = Pointer<Void> Function(Pointer<HtdEngineConfig>, Pointer<Int32>);

final htdEngineInit = nativeLib.lookupFunction<InitNative, InitDart>('htd_engine_init');
```

### Complete Dart Usage Example

```dart
import 'dart:ffi';
import 'package:ffi/ffi.dart';

void startBinauralSession() {
  // 1. Build cycle items
  final cycleItems = calloc<HtdCycleItem>(3);
  cycleItems[0].frequency_delta = 3.0;  // 3 Hz delta
  cycleItems[0].duration_seconds = 30.0; // 30 seconds
  cycleItems[1].frequency_delta = 4.0;
  cycleItems[1].duration_seconds = 30.0;
  cycleItems[2].frequency_delta = 5.0;
  cycleItems[2].duration_seconds = 30.0;

  // 2. Build config
  final config = calloc<HtdEngineConfig>();
  config.ref.carrier_frequency = 400.0;
  config.ref.binaural_enabled = true;
  config.ref.rain_sound_path = '/path/to/rain.wav'.toNativeUtf8().cast();
  config.ref.cycle_items = cycleItems;
  config.ref.cycle_count = 3;
  config.ref.sample_rate = 48000.0;
  config.ref.rain_gain = 0.8;
  config.ref.tone_gain = 0.2;

  // 3. Initialize engine
  final errorPtr = calloc<Int32>();
  final engine = htdEngineInit(config, errorPtr);
  if (engine == nullptr) {
    print('Init failed with error: ${errorPtr.value}');
    return;
  }

  // 4. Start playback
  htdEngineStart(engine);

  // 5. In your audio callback, call render:
  //    htdEngineRender(engine, outputBuffer, numFrames);

  // 6. Adjust gains at any time from the UI thread:
  htdEngineSetRainGain(engine, 0.6);
  htdEngineSetToneGain(engine, 0.4);

  // 7. When done:
  htdEngineStop(engine);
  htdEngineDestroy(engine);

  // 8. Free native memory
  calloc.free(config);
  calloc.free(cycleItems);
  calloc.free(errorPtr);
}
```

### Audio Callback Integration

In your Flutter audio plugin (e.g., `flutter_sound`, `miniaudio`, or a custom platform channel):

```dart
// Called by the platform audio system for each buffer
void audioCallback(Pointer<Float> buffer, int numFrames) {
  htdEngineRender(engine, buffer, numFrames);
}
```

---

## Usage Example (C)

```c
#include "hexatune_dsp_ffi.h"
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    // Define a frequency cycle: 3Hz→4Hz→5Hz, 30s each
    HtdCycleItem cycle[] = {
        { .frequency_delta = 3.0f, .duration_seconds = 30.0f },
        { .frequency_delta = 4.0f, .duration_seconds = 30.0f },
        { .frequency_delta = 5.0f, .duration_seconds = 30.0f },
    };

    // Configure the engine
    HtdEngineConfig config = {
        .carrier_frequency = 400.0f,
        .binaural_enabled = true,
        .rain_sound_path = "assets/rain.wav",  // or NULL for no rain
        .cycle_items = cycle,
        .cycle_count = 3,
        .sample_rate = 48000.0f,
        .rain_gain = 0.8f,
        .tone_gain = 0.2f,
    };

    // Initialize
    int32_t error = 0;
    HtdEngine *engine = htd_engine_init(&config, &error);
    if (!engine) {
        fprintf(stderr, "Failed to init engine: %d\n", error);
        return 1;
    }

    // Start
    htd_engine_start(engine);

    // Render audio (call this from your audio callback)
    float buffer[1024];  // 512 stereo frames
    htd_engine_render(engine, buffer, 512);

    // Adjust gains at runtime
    htd_engine_set_rain_gain(engine, 0.6f);
    htd_engine_set_tone_gain(engine, 0.4f);

    // Update cycle at runtime
    HtdCycleItem new_cycle[] = {
        { .frequency_delta = 6.0f, .duration_seconds = 60.0f },
        { .frequency_delta = 8.0f, .duration_seconds = 60.0f },
    };
    HtdEngineConfig update = {
        .carrier_frequency = 432.0f,
        .binaural_enabled = true,
        .rain_sound_path = NULL,
        .cycle_items = new_cycle,
        .cycle_count = 2,
        .sample_rate = 0,      // ignored in update
        .rain_gain = 0,        // ignored in update
        .tone_gain = 0,        // ignored in update
    };
    htd_engine_update_config(engine, &update);

    // Cleanup
    htd_engine_stop(engine);
    htd_engine_destroy(engine);
    return 0;
}
```

---

## Audio Engine Details

### Binaural Mode

When `binaural_enabled = true`, the engine generates two sine waves:

- **Left channel** = `sin(2π × carrier × t)`
- **Right channel** = `sin(2π × (carrier + delta) × t)`

The listener perceives a "beat" at the delta frequency when using headphones. For example, with `carrier = 400 Hz` and `delta = 5 Hz`, the left ear hears 400 Hz and the right ear hears 405 Hz, producing a perceived 5 Hz binaural beat.

### AM Mode (Non-Binaural)

When `binaural_enabled = false`, the engine generates a single carrier tone with amplitude modulation:

```
output = sin(2π × carrier × t) × ((sin(2π × delta × t) + 1) / 2)
```

This produces rhythmic pulsing at the delta frequency, output identically to both channels. Useful when the listener is not using headphones.

### Frequency Cycle Scheduling

The engine cycles through a list of frequency deltas, each with a specified duration:

```
cycle = [
    { delta: 3 Hz, duration: 30s },
    { delta: 4 Hz, duration: 30s },
    { delta: 5 Hz, duration: 30s },
]
```

Playback:
1. First 30 seconds → 400 / 403 Hz (3 Hz delta)
2. Next 30 seconds → 400 / 404 Hz (4 Hz delta)
3. Next 30 seconds → 400 / 405 Hz (5 Hz delta)
4. Loop back to step 1

Transitions are sample-accurate (no timer jitter). Phase is preserved across delta changes to avoid clicks.

### Rain / Ambient Sound

- WAV files (16-bit int or 32-bit float, mono or stereo) are loaded into memory before playback
- Playback loops seamlessly
- The rain buffer is pre-allocated — zero allocations during render

### Mixer

The final output combines tone and rain with configurable gains:

```
left  = rain_left  × rain_gain + tone_left  × tone_gain
right = rain_right × rain_gain + tone_right × tone_gain
```

Default gains: `rain_gain = 0.8`, `tone_gain = 0.2`. Output is clamped to `[-1.0, 1.0]`.

---

## Real-Time Safety

The `htd_engine_render` function is designed for audio callbacks. Inside the render path, the engine does **NOT**:

- Allocate or free memory
- Open or read files
- Log messages
- Acquire blocking locks (uses `try_lock` for config updates)
- Call any blocking OS API

All assets (WAV files, cycle arrays) must be loaded before calling `htd_engine_start`. Gain updates use lock-free atomics. Config updates use a non-blocking `try_lock` — if contended, the update is deferred to the next render call.

---

## Cross-Compilation

### Android (via cargo-ndk)

```bash
# Install targets
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# Build
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release
```

Copy the `.so` files into your Flutter project:

```
android/app/src/main/jniLibs/
├── arm64-v8a/libhexatune_dsp_ffi.so
├── armeabi-v7a/libhexatune_dsp_ffi.so
└── x86_64/libhexatune_dsp_ffi.so
```

### iOS

```bash
# Install targets
rustup target add aarch64-apple-ios aarch64-apple-ios-sim

# Build
cargo build --release --target aarch64-apple-ios
cargo build --release --target aarch64-apple-ios-sim
```

Create an XCFramework and add it to your Xcode project.

---

## License

MIT — [hexaTune LLC](https://hexatune.com)