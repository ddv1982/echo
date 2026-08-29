# Phase 5: Delete the portable package gate

[Back to overview](overview.md)

## Goal

Remove the v3 package format and the content-addressed identity algebra, including the binary binding that forced acceleration to be bundled rather than downloaded.

## Changes

**`crates/echo/src/stt/whisper_portable.rs`.** Delete. This removes `InstalledPortableSelection`, `PortableSelection`, `PortableSelectionBinding`, `LegacyExactIndex`, `LegacyExactRecord`, `installed_package_root`, `verify_files`, the verification stamp cache, the `CalibrationFixture` contract, and the literal `production_readiness` marker string.

**`crates/echo/src/stt/whisper_identity.rs`.** Reduce to its digest primitives. Delete `ExecutionArtifactInput`, `InferenceContractInput`, `LocalEnvironmentInput`, `PerformanceEvidenceInput`, `ReleaseBindingInput`, their five content identifiers, and the canonical JSON algebra. Keep `Sha256Digest`, `UuidDigest`, and `IdentityError`, which `crates/echo/src/stt/backend/vulkan.rs` uses to hold receipt fields and which this plan keeps.

**`crates/echo/tests/fixtures/whisper-v3-identities.json`.** Delete. This phase removes its last Rust reader. The offline scripts that also read it keep their own copy under `scripts/`, so the research tooling works without a fixture in the test tree.

**`crates/echo/src/stt/mod.rs`.** Drop the module declaration and the identity re-exports.

`legacy-exact-index.v1.json` always shipped `"records": []`, so the per-host half of this format was inert by construction. The whole contribution of the offline pipeline to a shipped package was the inference contract's model digest, VAD digest, and tuning tuple, all computable without a GPU. Phase 10 pins the tuning as a constant instead.

## Data structures

None added. The `echoBinarySha256` binding at `whisper_portable.rs:407` is removed, which is what makes phase 7's managed-component delivery possible.

## Verification

Static:

- `cargo clippy --workspace --all-targets -- -D warnings` and `xvfb-run -a cargo test --workspace` pass.
- `grep -r portable-selection crates/ src-tauri/` returns nothing.
- `python3 scripts/whisper_identity_v3.py --self-test` is no longer run by CI and its failure does not gate anything.

Runtime: dictate once and confirm an unchanged CPU transcript.
