# Phase 6. Measure resident value

Back to [overview](overview.md).

## Goal

Prove that a loaded model earns a cross-process broker before building it.

## Changes

- `scripts/generate-managed-inventory.py` selects `whisper-server` from the existing pinned Whisper archive.
- `crates/echo/src/install/archive_inventory.json` records and verifies the server payload alongside the CLI and shared libraries.
- A resident probe script starts managed `whisper-server` on loopback, aligns all decoding and VAD parameters with the tuned cold plan, disables previous context, measures server load, first request, warm requests, RSS, cancellation, crash, and output quality, then stops it.

Keep cold, resident-first, and resident-warm results in separate tables. Do not implement normal recording integration in this phase.

## Data structures

- `ResidentProbeObservation`. Worker key, load time, first or warm mode, request latency, RSS, transcript, quality, and exit state.

## Verification

Static:

- Archive inventory generation and exact pinned archive extraction pass.
- Probe-script self-tests use a fake local server and validate cleanup after failure.

Runtime:

- Run the real pinned server and Turbo Q5_0 on target Linux hardware.
- Confirm CLI and server output parity with explicit threads, beam, best-of, fallback, language, prompt, VAD, and no-context values.
- Proceed to Phases 7 and 8 only when the resident gate in the overview passes and idle RSS causes no swap or desktop pressure.

## Implementation result

The pinned payload inventory and the separate resident probe are implemented. The probe's automated test drives success, exit before readiness, and failure during a warm request. It proves child cleanup and that failed runs publish no partial report. The pinned v1.9.2 server already disables previous context internally and does not expose a `--no-context` flag, so the probe relies on that pinned default.

The real Linux Turbo benchmark, output parity, cancellation during inference, server crash during inference, and memory-pressure gate remain INCONCLUSIVE on the available host. Phases 7 and 8 did not proceed.
