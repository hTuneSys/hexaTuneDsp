# hexaTuneDsp

Real-time multi-layer soundscape engine written in Rust with a C-compatible FFI interface for Flutter.

Generates binaural beats, amplitude-modulated tones, and mixes in multiple ambient layers — base, texture, and random events — with timed frequency cycle scheduling. All processing is real-time safe with zero allocations during playback.

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
  - [Layer Management](#layer-management)
  - [Gain Control](#gain-control)
  - [Configuration Update](#configuration-update)
  - [Error Codes](#error-codes)
- [Flutter Integration (Dart)](#flutter-integration-dart)
- [Usage Example (C)](#usage-example-c)
- [Audio Engine Details](#audio-engine-details)
  - [Binaural Mode](#binaural-mode)
  - [AM Mode (Non-Binaural)](#am-mode-non-binaural)
  - [Frequency Cycle Scheduling](#frequency-cycle-scheduling)
  - [Layer System](#layer-system)
  - [Mixer](#mixer)
- [Real-Time Safety](#real-time-safety)
- [Cross-Compilation](#cross-compilation)
- [License](#license)

---

## Features

- **Multi-layer soundscape** — base, texture (x3), random events (x5), and binaural/AM layers
- **Binaural beat generation** — stereo output with different frequencies per ear
- **Amplitude modulation fallback** — mono pulsing mode when binaural is disabled
- **Random event scheduling** — sample-accurate PRNG-based one-shot sounds with randomized volume/pan
- **Loop crossfade** — seamless looping with configurable crossfade region
- **Timed frequency cycles** — automatically rotates through frequency deltas on a schedule
- **Per-layer gain control** — base, texture, event, binaural, and master gains (all atomic/thread-safe)
- **Layer validation** — texture/event layers require a base layer to be set
- **C-compatible FFI** — works with any language that supports C FFI (Dart, Swift, Kotlin, C++)
- **Real-time safe** — zero allocations, zero I/O, zero locks in the audio callback
- **Auto-generated C header** — `include/hexatune_dsp_ffi.h` via cbindgen

---

## Architecture

```
Flutter UI Layer          (Dart — UI, state management, scene presets)
       |
Flutter Control Layer     (Dart — decodes .m4a to PCM f32, calls FFI)
       |
Rust FFI Bridge           (src/ffi.rs — extern "C" functions, no DSP logic)
       |
Rust DSP Engine           (src/engine.rs — orchestrates all audio layers)
       |
 +-------------+---------------+----------------+-----------------+----------+
 |  Oscillator  |   Binaural    | SamplePlayer   | EventSystem     | Scheduler|
 |  (sine wave) | (stereo/AM)   | (loop+xfade)   | (PRNG one-shot) | (cycles) |
 +-------------+---------------+----------------+-----------------+----------+
                          |
                        Mixer  ->  interleaved stereo f32 output
```

**Layer stack (per frame):**

```
binaural x binaural_gain    (always available)
base     x base_gain        (0 or 1 continuous loop)
textures x texture_gain     (0-3 continuous loops, summed)
events   x event_gain       (0-5 definitions, max 1 playing at a time)
---
  sum x master_gain -> clamp to [-1.0, 1.0]
```

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
│   ├── sample_player.rs    # PCM loop player with crossfade
│   ├── event_player.rs     # Random one-shot event system
│   ├── event_scheduler.rs  # PRNG-based event timing (Xorshift64)
│   ├── scheduler.rs        # Frequency delta cycle scheduler
│   ├── mixer.rs            # Multi-layer stereo mixer
│   ├── engine.rs           # Main DSP engine orchestrator
│   └── ffi.rs              # C-compatible FFI interface
└── AGENTS.md               # AI agent coding rules
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

Configuration for engine initialization.

```c
typedef struct HtdEngineConfig {
    float carrier_frequency;              // Base frequency in Hz (e.g. 400.0)
    bool binaural_enabled;                // true = binaural, false = AM mode
    const HtdCycleItem *cycle_items;      // Array of cycle steps
    uint32_t cycle_count;                 // Number of cycle items
    float sample_rate;                    // Sample rate (default: 48000)
    float base_gain;                      // Base layer gain (default: 0.6)
    float texture_gain;                   // Texture layer gain (default: 0.3)
    float event_gain;                     // Event layer gain (default: 0.4)
    float binaural_gain;                  // Binaural gain (default: 0.15)
    float master_gain;                    // Master output gain (default: 1.0)
} HtdEngineConfig;
```

#### `HtdLayerConfig`

Raw PCM audio data for base or texture layers.

```c
typedef struct HtdLayerConfig {
    const float *samples;   // Interleaved f32 PCM data
    uint32_t num_frames;    // Number of sample frames
    uint32_t channels;      // 1 (mono) or 2 (stereo interleaved)
} HtdLayerConfig;
```

#### `HtdEventConfig`

Configuration for a random one-shot event.

```c
typedef struct HtdEventConfig {
    const float *samples;         // Interleaved f32 PCM data
    uint32_t num_frames;          // Number of sample frames
    uint32_t channels;            // 1 (mono) or 2 (stereo interleaved)
    uint32_t min_interval_ms;     // Min time between triggers (ms)
    uint32_t max_interval_ms;     // Max time between triggers (ms)
    float volume_min;             // Min random volume (0.0-1.0)
    float volume_max;             // Max random volume (0.0-1.0)
    float pan_min;                // Min stereo pan (-1.0 left to 1.0 right)
    float pan_max;                // Max stereo pan (-1.0 left to 1.0 right)
} HtdEventConfig;
```

### Lifecycle Functions

#### `htd_engine_init`

Create and initialize the DSP engine.

```c
HtdEngine *htd_engine_init(const HtdEngineConfig *config, int32_t *out_error);
```

- Returns an opaque `HtdEngine*` pointer on success, `NULL` on failure.
- Error code is written to `out_error` (pass `NULL` to ignore).

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

### Layer Management

#### `htd_engine_set_base`

Set the base ambient layer from raw PCM data.

```c
int32_t htd_engine_set_base(HtdEngine *engine, const HtdLayerConfig *config);
```

- The base layer is required before adding textures or events.
- Not real-time safe — must not be called during `htd_engine_render`.

#### `htd_engine_clear_base`

Remove the base layer. Also clears all textures and events (they depend on base).

```c
int32_t htd_engine_clear_base(HtdEngine *engine);
```

#### `htd_engine_set_texture`

Set a texture layer at index 0-2. Requires base to be set.

```c
int32_t htd_engine_set_texture(HtdEngine *engine, uint32_t index, const HtdLayerConfig *config);
```

#### `htd_engine_clear_texture`

Remove a texture layer at the given index.

```c
int32_t htd_engine_clear_texture(HtdEngine *engine, uint32_t index);
```

#### `htd_engine_set_event`

Register a random event at index 0-4. Requires base to be set.

```c
int32_t htd_engine_set_event(HtdEngine *engine, uint32_t index, const HtdEventConfig *config);
```

#### `htd_engine_clear_event`

Remove a random event at the given index.

```c
int32_t htd_engine_clear_event(HtdEngine *engine, uint32_t index);
```

#### `htd_engine_clear_all_layers`

Remove all layers (base, textures, events). Binaural is not affected.

```c
int32_t htd_engine_clear_all_layers(HtdEngine *engine);
```

#### `htd_engine_load_base_wav`

Load the base layer from a WAV file (convenience function).

```c
int32_t htd_engine_load_base_wav(HtdEngine *engine, const char *path);
```

- Not real-time safe — allocates memory, performs I/O.

### Gain Control

All gain setters are thread-safe (use atomics). Can be called from any thread.

```c
int32_t htd_engine_set_base_gain(HtdEngine *engine, float gain);
int32_t htd_engine_set_texture_gain(HtdEngine *engine, float gain);
int32_t htd_engine_set_event_gain(HtdEngine *engine, float gain);
int32_t htd_engine_set_binaural_gain(HtdEngine *engine, float gain);
int32_t htd_engine_set_master_gain(HtdEngine *engine, float gain);
```

### Configuration Update

#### `htd_engine_update_config`

Update binaural parameters at runtime. Changes are applied on the next render call.

```c
int32_t htd_engine_update_config(HtdEngine *engine, const HtdEngineConfig *config);
```

- `carrier_frequency`: applied if > 0
- `binaural_enabled`: always applied
- `cycle_items` + `cycle_count`: applied if non-null and count > 0
- Gain fields and `sample_rate` are **ignored** — use dedicated setters.
- Thread-safe — can be called from any thread.

### Error Codes

| Code | Name               | Description                              |
|------|--------------------|------------------------------------------|
| 0    | Ok                 | Success                                  |
| -1   | NullPointer        | Null pointer argument                    |
| -2   | InvalidConfig      | Invalid configuration                    |
| -3   | InitFailed         | Engine initialization failed             |
| -4   | InvalidUtf8        | Invalid UTF-8 string                     |
| -5   | BufferTooSmall     | Output buffer too small                  |
| -6   | LoadFailed         | WAV file load failed                     |
| -7   | LayerLimitExceeded | Layer index out of bounds                |
| -8   | BaseRequired       | Texture/event requires base to be set    |

---

## Flutter Integration (Dart)

### Audio Data Flow

```
.m4a file -> Flutter decoder -> raw PCM f32 -> FFI -> Rust engine
```

Flutter decodes audio files to raw PCM f32 buffers, then passes them via FFI.

### Complete Dart Usage Example

```dart
import 'dart:ffi';
import 'package:ffi/ffi.dart';

void startSoundscape() {
  // 1. Build frequency cycle
  final cycleItems = calloc<HtdCycleItem>(3);
  cycleItems[0]
    ..frequency_delta = 3.0
    ..duration_seconds = 30.0;
  cycleItems[1]
    ..frequency_delta = 4.0
    ..duration_seconds = 30.0;
  cycleItems[2]
    ..frequency_delta = 5.0
    ..duration_seconds = 30.0;

  // 2. Build config
  final config = calloc<HtdEngineConfig>();
  config.ref
    ..carrier_frequency = 400.0
    ..binaural_enabled = true
    ..cycle_items = cycleItems
    ..cycle_count = 3
    ..sample_rate = 48000.0
    ..base_gain = 0.6
    ..texture_gain = 0.3
    ..event_gain = 0.4
    ..binaural_gain = 0.15
    ..master_gain = 1.0;

  // 3. Initialize engine
  final errorPtr = calloc<Int32>();
  final engine = htdEngineInit(config, errorPtr);
  if (engine == nullptr) {
    print('Init failed: ${errorPtr.value}');
    return;
  }

  // 4. Load layers (Flutter decodes .m4a to PCM f32 first)
  final basePcm = decodeAudioToPcm('assets/rain_on_roof.m4a');
  final baseConfig = calloc<HtdLayerConfig>();
  baseConfig.ref
    ..samples = basePcm.pointer
    ..num_frames = basePcm.frameCount
    ..channels = basePcm.channels;
  htdEngineSetBase(engine, baseConfig);

  // 5. Add a texture layer
  final windPcm = decodeAudioToPcm('assets/distant_wind.m4a');
  final textureConfig = calloc<HtdLayerConfig>();
  textureConfig.ref
    ..samples = windPcm.pointer
    ..num_frames = windPcm.frameCount
    ..channels = windPcm.channels;
  htdEngineSetTexture(engine, 0, textureConfig);

  // 6. Add a random event
  final thunderPcm = decodeAudioToPcm('assets/distant_thunder.m4a');
  final eventConfig = calloc<HtdEventConfig>();
  eventConfig.ref
    ..samples = thunderPcm.pointer
    ..num_frames = thunderPcm.frameCount
    ..channels = thunderPcm.channels
    ..min_interval_ms = 40000
    ..max_interval_ms = 120000
    ..volume_min = 0.3
    ..volume_max = 0.7
    ..pan_min = -0.5
    ..pan_max = 0.5;
  htdEngineSetEvent(engine, 0, eventConfig);

  // 7. Start playback
  htdEngineStart(engine);

  // 8. Adjust gains from UI:
  htdEngineSetBinauralGain(engine, 0.2);
  htdEngineSetBaseGain(engine, 0.7);

  // 9. Cleanup when done:
  htdEngineStop(engine);
  htdEngineClearAllLayers(engine);
  htdEngineDestroy(engine);
}
```

### Audio Callback Integration

```dart
void audioCallback(Pointer<Float> buffer, int numFrames) {
  htdEngineRender(engine, buffer, numFrames);
}
```

---

## Usage Example (C)

```c
#include "hexatune_dsp_ffi.h"
#include <stdio.h>

int main(void) {
    // Frequency cycle: 3Hz->4Hz->5Hz, 30s each
    HtdCycleItem cycle[] = {
        { .frequency_delta = 3.0f, .duration_seconds = 30.0f },
        { .frequency_delta = 4.0f, .duration_seconds = 30.0f },
        { .frequency_delta = 5.0f, .duration_seconds = 30.0f },
    };

    // Engine configuration
    HtdEngineConfig config = {
        .carrier_frequency = 400.0f,
        .binaural_enabled = true,
        .cycle_items = cycle,
        .cycle_count = 3,
        .sample_rate = 48000.0f,
        .base_gain = 0.6f,
        .texture_gain = 0.3f,
        .event_gain = 0.4f,
        .binaural_gain = 0.15f,
        .master_gain = 1.0f,
    };

    // Initialize
    int32_t error = 0;
    HtdEngine *engine = htd_engine_init(&config, &error);
    if (!engine) {
        fprintf(stderr, "Failed: %d\n", error);
        return 1;
    }

    // Load base layer from WAV (convenience)
    htd_engine_load_base_wav(engine, "assets/rain.wav");

    // Or load from raw PCM:
    // float pcm_data[] = { ... };
    // HtdLayerConfig layer = { .samples = pcm_data, .num_frames = 48000, .channels = 2 };
    // htd_engine_set_base(engine, &layer);

    // Add a random event
    float thunder[] = { /* decoded PCM */ };
    HtdEventConfig evt = {
        .samples = thunder,
        .num_frames = 24000,
        .channels = 1,
        .min_interval_ms = 40000,
        .max_interval_ms = 120000,
        .volume_min = 0.3f,
        .volume_max = 0.7f,
        .pan_min = -0.5f,
        .pan_max = 0.5f,
    };
    htd_engine_set_event(engine, 0, &evt);

    // Start
    htd_engine_start(engine);

    // Render (call from audio callback)
    float buffer[1024];
    htd_engine_render(engine, buffer, 512);

    // Adjust gains at runtime
    htd_engine_set_base_gain(engine, 0.7f);
    htd_engine_set_binaural_gain(engine, 0.2f);

    // Cleanup
    htd_engine_stop(engine);
    htd_engine_clear_all_layers(engine);
    htd_engine_destroy(engine);
    return 0;
}
```

---

## Audio Engine Details

### Binaural Mode

When `binaural_enabled = true`, the engine generates two sine waves:

- **Left channel** = `sin(2pi * carrier * t)`
- **Right channel** = `sin(2pi * (carrier + delta) * t)`

The listener perceives a "beat" at the delta frequency when using headphones.

### AM Mode (Non-Binaural)

When `binaural_enabled = false`, the engine generates a single carrier tone with amplitude modulation:

```
output = sin(2pi * carrier * t) * ((sin(2pi * delta * t) + 1) / 2)
```

This produces rhythmic pulsing at the delta frequency, output identically to both channels.

### Frequency Cycle Scheduling

The engine cycles through frequency deltas with sample-accurate timing:

```
cycle = [
    { delta: 3 Hz, duration: 30s },
    { delta: 4 Hz, duration: 30s },
    { delta: 5 Hz, duration: 30s },
]
```

Phase is preserved across delta changes to avoid clicks.

### Layer System

| Layer    | Count  | Behavior                        | Gain Default |
|----------|--------|---------------------------------|--------------|
| Base     | 0-1    | Continuous loop with crossfade  | 0.6          |
| Texture  | 0-3    | Continuous loops, summed        | 0.3          |
| Event    | 0-5    | Random one-shots (max 1 active) | 0.4          |
| Binaural | always | Sine tone (binaural or AM)      | 0.15         |
| Master   | -      | Scales final output             | 1.0          |

**Validation rules:**
- Texture and event layers require a base layer to be set.
- Clearing the base also clears all textures and events.
- The binaural layer is independent and always available.

**Loop crossfade:** SamplePlayer uses a configurable crossfade region (default 2048 frames, approx 42 ms at 48 kHz). At the loop boundary, the tail blends linearly with the head to eliminate clicks.

**Event scheduling:** Uses a Xorshift64 PRNG for sample-accurate random intervals. Each event has configurable min/max interval, volume range, and pan range. At trigger time, volume and pan are randomized within the configured ranges.

### Mixer

The mixer combines all layers with per-layer gains:

```
output = (base * base_gain + textures * texture_gain + events * event_gain + binaural * binaural_gain) * master_gain
```

Output is clamped to `[-1.0, 1.0]`.

---

## Real-Time Safety

The `htd_engine_render` function is designed for audio callbacks. Inside the render path, the engine does **NOT**:

- Allocate or free memory
- Open or read files
- Log messages
- Acquire blocking locks (uses `try_lock` for config updates)
- Call any blocking OS API

All audio buffers must be loaded before or between render calls. Gain updates use lock-free atomics. Config updates use a non-blocking `try_lock` — if contended, the update is deferred to the next render call.

---

## Cross-Compilation

### Android (via cargo-ndk)

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release
```

Copy `.so` files to `android/app/src/main/jniLibs/`.

### iOS

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo build --release --target aarch64-apple-ios
cargo build --release --target aarch64-apple-ios-sim
```

Create an XCFramework and add it to your Xcode project.

---

## License

MIT — [hexaTune LLC](https://hexatune.com)
