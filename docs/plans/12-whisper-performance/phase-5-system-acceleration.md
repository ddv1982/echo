# Phase 5. Probe accelerated system runtimes

Back to [overview](overview.md).

## Goal

Use hardware acceleration already installed by advanced users before Echo owns GPU artifacts.

## Changes

- `crates/echo/src/stt/whisper_probe.rs` runs boundary-safe feature and short transcription probes, parses actual backend identity, and caches results by binary, model, driver, and device identity.
- `crates/echo/src/stt/runtime.rs` may prefer a proven accelerated system candidate over managed CPU. A failed or unknown candidate falls back to the existing managed CPU path.
- Runtime tests cover CUDA, Vulkan, OpenVINO, ROCm, CPU, malformed probe output, missing device, driver change, and quarantine after failure.

The probe must transcribe real audio and return valid Whisper JSON. `--help` alone is not readiness evidence.

## Data structures

- `RuntimeProbeReport`. Resolved backend, device, binary identity, model identity, driver identity, result, and checked time.
- `RuntimeHealth`. Ready, failed with reason, or unknown.

## Verification

Static:

- Candidate ordering and fallback tables pass without changing model selection.
- Probe cache invalidates when binary, model, device, or driver identity changes.

Runtime:

- Test each available system backend on its real host and retain raw probe plus benchmark reports.
- Break the accelerated runtime after a successful probe and confirm one request falls back to managed CPU with the same model and an actionable diagnostic.
