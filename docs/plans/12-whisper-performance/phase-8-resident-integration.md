# Phase 8. Integrate resident transcription

Back to [overview](overview.md).

## Goal

Use the broker on the real dictation path while preserving one-shot CPU fallback.

## Changes

- `crates/echo/src/stt/whisper.rs` executes resident or cold protocol from the resolved plan and parses both through the same transcript boundary.
- `crates/echo/src/transcribe.rs` reconciles broker identity with model, runtime, VAD, tuning, backend, and managed generations before each request.
- `crates/echo/src/rec.rs` passes cancellation through recording transcription and reports resident failure or cold fallback truthfully.

One resident failure may retry through cold CLI with the same model and tuning. It never silently switches model. A new managed generation creates a new worker key. The old broker-held leases live until the old server exits.

## Data structures

- `ExecutionOutcome`. Requested mode, actual mode, attempts, fallback reason, transcript, and telemetry.

## Verification

Static:

- Cold and resident paths share language, hints, VAD, no-context, cleanup, and parser tests.
- Cancellation and fallback tests pin same-model behavior and attempt limits.

Runtime:

- Drive two GUI dictations, two file CLI calls, and two GNOME-style `rec --toggle` processes. The second request in each sequence must reuse the same worker key and report warm mode.
- Repair or replace the managed runtime and model between requests. Existing work finishes on leased generations; new work starts a new worker.
- Kill the server during inference and confirm one cold fallback succeeds with an actionable diagnostic.
