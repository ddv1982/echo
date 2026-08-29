# Phase 4: Delete the planner and route store

[Back to overview](overview.md)

## Goal

Remove the receipt-driven planner and its append-only route store, completing the subtraction and closing a latent permanent-outage defect.

## Changes

**`crates/echo/src/stt/whisper_planner.rs`.** Delete. This removes `WhisperAccelerationPlanner`, `ReceiptDrivenWhisperEngine`, and `local_whisper_engine_from_process`. Move the preference resolution chain, which reads an override then `ECHO_WHISPER_ACCELERATION` then the config file then the factory default, into `crates/echo/src/stt/mod.rs` where the factory default already lives.

**`crates/echo/src/stt/whisper_accel_cache.rs` and `whisper_accel_cache_tests.rs`.** Delete. This removes `LocalSelectionStore`, `LocalSelectionKey`, `LocalRouteObservation`, `ModelRouteView`, `CalibrationObservation`, the job queue, and the flock leases.

**`crates/echo/src/transcribe.rs`.** Reduce engine construction to the managed CPU plan. Acceleration does not exist again until phase 10.


The store wrote one immutable JSON file per accelerated transcription through `append_route` and never pruned, while `read_directory` hard-errors above 256 records. That error propagated through `model_view` and `contract()` to `EngineError::Infer` with no CPU fallback, so a heavy user would have lost dictation permanently after roughly 257 accelerated runs. Phase 10 stores a single pinned device rather than a growing history, so the failure mode cannot return.

## Data structures

None added. Phase 10 replaces the entire store with one persisted value, the chosen `VulkanDeviceId`, held in the existing config rather than in a bespoke record tree.

## Verification

Static:

- `cargo clippy --workspace --all-targets -- -D warnings` and `xvfb-run -a cargo test --workspace` pass.
- `crates/echo/src/stt/` no longer references `whisper-local-selection`.
- `backend/vulkan.rs`, `whisper_probe.rs`, `whisper_recovery.rs`, `whisper_quarantine.rs`, and `whisper_plan.rs` remain and still compile. These are the capability this plan keeps.

Runtime: dictate once and confirm an unchanged CPU transcript. Confirm no directory is created under the user data dir for local selection state.
