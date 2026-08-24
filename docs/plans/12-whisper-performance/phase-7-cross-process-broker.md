# Phase 7. Build the cross-process broker

Back to [overview](overview.md).

## Goal

Own one loaded Whisper server safely across GUI, CLI, and GNOME shortcut processes.

## Changes

- `crates/echo/src/stt/whisper_broker.rs` owns the broker lock, atomic state, request queue, server child, random loopback endpoint, worker key, idle shutdown, crash recovery, cancellation, and managed lease handoff.
- `src-tauri/src/cli.rs` adds a hidden broker command used only by Echo. Normal CLI help and Settings do not expose it.
- Broker integration tests use a fake server to prove concurrent startup, stale state recovery, one-request serialization, cancellation, crash, key replacement, idle exit, and cold fallback eligibility.

Use one worker per user to bound memory. A different worker key replaces the worker rather than hot-loading a model. The broker binds only to loopback, uses a random request path, disables conversion, exposes no public directory, and stores user-only state.

## Data structures

- `BrokerState`. Schema, PID, endpoint, secret request path, worker key, generation, readiness, and last use.
- `BrokerCommand`. Start, transcribe, cancel, status, and stop.

## Verification

Static:

- State transitions are exhaustive and idempotent.
- File permissions, stale PID checks, endpoint validation, and request parsing stay at the broker boundary.

Runtime:

- Start two separate Echo processes simultaneously and prove exactly one broker and one server survive.
- Kill the client, broker, and server at each lifecycle point and verify the next request converges to one healthy state.
- Confirm uncertain cancellation kills the server and removes stale state before the next request.
