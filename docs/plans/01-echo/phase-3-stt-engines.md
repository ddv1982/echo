# Phase 3. STT engines

Back to [overview](./overview.md).

## Goal

One `Engine` trait, two local adapters on Linux. Parakeet through sherpa-onnx is the default. whisper.cpp is the fallback.

## Changes

`crates/echo-core/src/engine.rs` defines the trait and `Transcript`.

`crates/echo/src/stt/parakeet.rs` wraps sherpa-onnx. First-run downloads the INT8 tarball from the [sherpa-onnx model releases](https://github.com/k2-fsa/sherpa-onnx/releases), then caches it under `$XDG_CACHE_HOME/echo`.

`crates/echo/src/stt/whisper.rs` wraps whisper.cpp or a `whisper-rs` binding. Default model is `base.en`. Larger models are a config key, not a rewrite.

Keep the download code behind a small `ModelCache` in `echo/src/stt/cache.rs` if the two adapters would otherwise copy it. Do not add a third adapter here.

## Data structures

`Engine` is `fn id(&self) -> EngineId` plus `fn transcribe(&self, pcm: &Pcm16kMono) -> Result<Transcript, EngineError>`.

`Transcript` is `{ raw: String, engine: EngineId, audio_ms, infer_ms }`.

`EngineId` is `ParakeetTdt06bV3 | Whisper { model }`.

## Verification

Static. `cargo test --workspace`. Clippy. A unit test feeds silence and expects an empty or near-empty `raw` without panic.

Runtime. `cargo test -p echo --test transcribe_fixture -- --ignored` runs both engines on `tests/fixtures/claude_code.wav` once the models are cached. Assert the transcript contains a known word from the fixture. Do not assert exact strings. Time `infer_ms` and write it to the test log. That number becomes the baseline for [testing.md](./testing.md).
