# Phase 1: Retire the behavior projection contract

[Back to overview](overview.md)

## Goal

Remove the digest chain that makes a Settings edit retire hardware evidence, so every later phase can touch `crates/echo-core/src/engine.rs` without ceremony.

## Changes

**`crates/echo/src/stt/whisper_behavior.rs`.** Delete `projection()` and `projection_sha256()` and the test that pins them. Keep the constant block. `ONE_SHOT_TIMEOUT_SECS`, `CHILD_REAP_TIMEOUT_SECS`, `RECEIPT_PROBE_TIMEOUT_SECS`, `VULKAN_RECEIPT_SCHEMA`, `VULKAN_BACKEND`, `CLEARED_ENVIRONMENT_KEYS`, and `CLEARED_ENVIRONMENT_PREFIXES` are production values consumed by the launch path and they stay.

**`crates/echo/tests/fixtures/`.** Leave both JSON fixtures in place. They keep live readers: `whisper-v3-identities.json` is read by `whisper_planner.rs`, `whisper_portable.rs`, and `whisper_identity.rs` until phases 4 and 5, and by six offline scripts that stay. `whisper-behavior-v3.json` is read by `scripts/whisper_v3_contract.py`. After this phase they are data owned by the research tooling rather than a contract binding the build.

**`.github/workflows/check.yml` and `scripts/verify-whisper-acceleration.sh`.** Remove the `Guard Whisper inference behavior` step and the acceleration self-test suite. The Python stays in the tree and remains runnable by hand.

**`crates/echo/src/stt/backend/vulkan.rs`.** Change `live_uuid_selector_when_probe_is_supplied` from a silent early return on a missing `ECHO_TEST_VULKAN_PROBE` to `#[ignore]` with a reason. It currently reports a pass without executing an assertion.

The guard was a two-sided consistency lock over 13 whole files, and `projection()` was a hand-written `#[cfg(test)]` literal rather than a value derived from production code, so satisfying it was always a restatement rather than a proof. Commits `384ce8d` and `f0711e8` on the current branch exist solely to perform that restatement for a Settings-only change.

## Data structures

None added. `BehaviorAuthority`, `InferenceBehavior`, and the five v3 content identifiers stop having a runtime consumer; they are removed with their owning modules in phases 3 and 4.

## Verification

Static:

- `cargo clippy --workspace --all-targets -- -D warnings` and `xvfb-run -a cargo test --workspace` pass with no reference to either deleted fixture.
- `grep -r projectionSha256 crates/ .github/` returns nothing. The string survives only in `scripts/` and in the fixtures those scripts read.
- The Vulkan live test reports as ignored rather than passed.

Runtime: none. This phase removes CI plumbing and changes no executable behavior.
