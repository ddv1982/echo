# Phase 3: Delete the exact-host admission gate

[Back to overview](overview.md)

## Goal

Remove the v2 acceleration path, which cannot fire in any shipped build and can only ever match a hand-qualified exact host.

## Changes

**`crates/echo/src/stt/whisper_acceleration.rs` and `whisper_acceleration_tests.rs`.** Delete both. This removes `production_whisper_decision`, `observed_host`, `select_qualified_package`, `populate_cache_seed`, `verify_live_receipt`, and the preflight receipt memo.

**`crates/echo/src/stt/whisper_admission.rs`.** Delete the remainder after phase 2: `AdmissionSet`, `AdmissionIdentity`, `AdmissionDeviceIdentity`, `AdmissionTuning`, `AdmissionGates`, `AdmissionVerdict`, `ModelAdmission`, `PackageEntry`, `CacheSeedArtifact`, `SharedRuntimeArtifacts`, and `MAX_ADMISSION_LIFETIME_SECS`.

**`crates/echo/src/transcribe.rs`.** Remove the `production_whisper_decision` fallback arm so engine construction goes straight from the portable planner to a plain CPU engine.

**`crates/echo/src/stt/mod.rs`.** Drop the module declarations and re-exports.

The gate reads `admission-set.json` from an install prefix. No workflow produces that file, `release.yml` only consumed a hand-built draft, and the reusable staging path writes `productionReady: False` unconditionally while the tag workflow demands `True`. Deleting it changes no shipped behaviour.

## Data structures

None added. `AdmissionSet` and its 25-leaf `AdmissionIdentity` are removed. The 30-day `MAX_ADMISSION_LIFETIME_SECS` fuse goes with them.

## Verification

Static:

- `cargo clippy --workspace --all-targets -- -D warnings` and `xvfb-run -a cargo test --workspace` pass.
- `grep -r admission-set.json crates/ src-tauri/` returns nothing.
- Frontend tests still show a working Settings surface and a CPU-only readout.

Runtime: dictate once through the built binary and confirm a transcript, a CPU backend in the Advanced readout, and no change to latency against the same recording before the phase.
