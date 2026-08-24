# Phase 1. Split Whisper telemetry

Back to [overview](overview.md).

## Goal

Measure the current user path honestly before changing performance behavior.

## Changes

- `crates/echo-core/src/engine.rs` gains backward-compatible Whisper timing, attempt, runtime, and mode detail under `RunDetail`.
- `crates/echo/src/stt/whisper.rs` measures WAV encoding, child spawn, child wall time, parsing, total time, VAD attempts, and retry classification. Optional upstream timing parsing may report model load and encoder or decoder time without making those fields mandatory.
- `src-tauri/src/cli.rs` preserves existing JSON fields and adds the structured timing and runtime detail.

Keep `inferMs` compatible with existing history and CLI users. Document its exact boundary instead of silently redefining it.

## Data structures

- `WhisperRunTelemetry`. Mode, total time, split timings, runtime identity, and ordered attempts.
- `WhisperAttemptTelemetry`. VAD state, child wall time, exit class, backend, and retry reason.

## Verification

Static:

- Rust type, serde compatibility, Whisper parser, CLI JSON, history round-trip, and old-row tests pass.
- Clippy passes with no new warning.

Runtime:

- Drive the file CLI through a fake Whisper binary and verify one-attempt success, VAD-specific retry, non-VAD failure, and malformed output.
- Run one real Turbo Q5_0 fixture on Linux and reconcile Echo totals against upstream stderr within documented boundaries.
