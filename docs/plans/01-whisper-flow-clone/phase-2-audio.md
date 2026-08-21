# Phase 2. Audio capture

Back to [overview](./overview.md).

## Goal

Hold a virtual "key," get a `Pcm16kMono` buffer back. Prove capture on this Linux host before any model download.

## Changes

`crates/echo/src/audio.rs` opens the default input with cpal, resamples to 16 kHz mono, and stops on a cancel token.

`crates/echo/src/audio_test.rs` or a `tests/record_once.rs` integration test writes a short fixture when `ECHO_LIVE_MIC=1`.

`crates/echo-core` gains `AudioChunk` only if the session needs a streaming hook. Prefer one shot buffer until a later streaming engine needs chunks.

## Data structures

`AudioCapture` is `{ device: DeviceName, cancel: CancellationToken }`.

`CaptureResult` is `{ pcm: Pcm16kMono, duration, peak_rms }`. Silent captures stay legal. The HUD will use `peak_rms` to show a dead mic.

## Verification

Static. `cargo test --workspace` and clippy as in phase 1.

Runtime. Default path uses a committed 16 kHz fixture, no hardware. Live path is `ECHO_LIVE_MIC=1 cargo test -p echo --test record_once -- --ignored`. Operator speaks for two seconds. The test fails if RMS is below a floor or if the sample rate is not 16 kHz. This host can run that. A Mac run is the same command later.
