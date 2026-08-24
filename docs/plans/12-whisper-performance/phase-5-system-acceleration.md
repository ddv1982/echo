# Phase 5. Probe accelerated system runtimes

Back to [overview](overview.md).

## Goal

Use hardware acceleration already installed by advanced users before Echo owns GPU artifacts.

Research and the independent architecture decision are recorded in [gpu-research.md](gpu-research.md).

## Phased execution

### 5A. Build the measurement lever

- Compare one accelerated runtime with its own `--no-gpu` CPU control.
- Require a real JSON transcription and parse the selected backend and physical device from runtime evidence.
- Record binary, model, audio, driver/device, tuning, upstream timing, randomized order, outer timing, and raw evidence in JSONL.
- Invalidate stale reports before each run and publish atomic running, failed, or complete status.
- Report a machine-readable proceed or stop decision without turning a smoke fixture into a production claim.

Acceptance: self-tests run in normal CI, and the probe rejects unknown or silently CPU backends. This phase may merge without a performance win because the tool is the deliverable.

### 5B. Run the host bakeoff

- Build the exact pinned v1.9.2 CPU and Vulkan candidates.
- Compare Base Q5_1 and Large v3 Turbo Q5_0 separately with at least ten paired, randomized timed observations per fixture.
- Repeat after a reboot or power-policy reset on the full multilingual quality corpus.

Acceptance: at least 20 percent and 500 ms lower paired median than tuned CPU, p95 lower, exact backend identity on every row, no failure, no new silence hallucination, and no per-language WER or CER regression above 0.5 absolute percentage points.

Stop per model if Vulkan is slower, backend identity is unknown, p95 regresses, quality changes, or the win disappears after reset. Base and Turbo may reach different decisions.

### 5C. Select a proven system runtime

- Add a production `RuntimeProbeReport` keyed by binary and adjacent libraries, model, VAD, backend/device, driver and ICD identity, and decoding policy.
- Prefer a system candidate only for an exact identity that passed 5B. Keep managed CPU first for every unknown identity.
- On accelerated failure, perform at most one managed CPU retry with the same model, language, prompt, VAD, beam, best-of, and fallback policy. Record both attempts and total cost.

Acceptance: forced device, driver, runtime, and malformed-output failures all converge to the same-model managed CPU path and quarantine only the failed identity.

### 5D. Revisit other backends

Test OpenVINO only if Vulkan fails the target workload or encoder evidence predicts a material additional win. Defer SYCL until its oneAPI and driver lifecycle can fit managed installation. Test CUDA and ROCm only on hosts with matching physical devices.

### 5E. Consider managed packaging

Phase 10 remains closed until one system backend clears 5B and 5C on more than one relevant hardware generation. Echo never bundles host GPU drivers or ICDs.

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

## Current result

Phase 5A is implemented by `scripts/probe-whisper-acceleration.py`. Ten randomized v1.9.2 smoke pairs per model proved real Iris Xe Vulkan execution, exact transcript parity, and lower steady median and p95 latency for managed multilingual Base Q5_1 and Turbo. The isolated first-use warmups also proved a large Mesa shader-cache penalty. Phase 5B remains blocked on the full licensed multilingual corpus and a reset repeat, so production selection in 5C has not started.
