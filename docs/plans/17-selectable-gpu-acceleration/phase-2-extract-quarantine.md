# Phase 2: Extract the quarantine primitives

[Back to overview](overview.md)

## Goal

Separate the quarantine machinery that later phases keep from the admission types they delete, so phase 3 can remove a file rather than pick it apart.

## Changes

**`crates/echo/src/stt/whisper_admission.rs`.** Move `QuarantineRecord`, `QuarantineReason`, and `MAX_QUARANTINE_LIFETIME_SECS` into `crates/echo/src/stt/whisper_quarantine.rs`, which already owns `QuarantineStore` and is their only structural home. These are imported live by `whisper_accel_cache.rs`, `whisper_recovery.rs`, `whisper_behavior.rs`, and `whisper_quarantine.rs`.

**`AdmissionIdentityKey`.** Rename to `AcceleratorKey` and move it alongside the quarantine types. `whisper_planner.rs:107` and `whisper_plan.rs` already use it as an opaque 64-hex container for a `LocalSelectionKey`, so the admission name is a lie today and would become a worse one after phase 3.

**Call sites.** Update the imports in `whisper_accel_cache.rs`, `whisper_recovery.rs`, `whisper_plan.rs`, `whisper_planner.rs`, and `whisper_behavior.rs`.

This is behaviour-preserving. No serialized form changes, because the quarantine document schema and the key's 64-hex representation are both untouched.

## Data structures

`AcceleratorKey(String)`, a validated lowercase 64-hex identifier for one accelerated execution route. Same invariants and same wire representation as `AdmissionIdentityKey`, without the implication that an admission produced it.

## Verification

Static:

- `cargo clippy --workspace --all-targets -- -D warnings` and `xvfb-run -a cargo test --workspace` pass.
- The quarantine tests in `whisper_recovery.rs` covering the six accelerator failure modes still pass unmodified except for the type name.
- `grep -r AdmissionIdentityKey crates/` returns nothing.

Runtime: none. Pure refactor with no behavioural surface.
