# Phase 6: Share one launch and identity contract

[Back to overview](overview.md)

## Goal

Make product execution reproduce the qualified child environment and identity while preserving CPU-first selection.

## Changes

- Add a child-only launch contract to `WhisperRuntimeCandidate` or `WhisperExecutionPlan`.
- Route `WhisperEngine` process creation through one launcher used by product and tools.
- Hash adjacent non-driver libraries and expose the identity preview in diagnostics.
- Remove inherited loader, layer, cache, and device-selection namespaces before applying the explicit contract.
- Keep `preferred_runtime` and normal production behavior unchanged.

## Data structures

- `RuntimeLaunchContract`: executable, ordered library roots, required library digests, driver manifest, cache root, and schema.
- `WhisperExecutionIdentity`: runtime receipt, artifacts, model, VAD, protocol, tuning, language, prompt policy, and launch contract.

## Verification

Static: Rust unit and integration tests cover environment sanitization, exact hashing, missing libraries, and identity changes.

Runtime: the measured Vulkan runtime must run through `echo-desktop transcribe` without an operator-set `LD_LIBRARY_PATH`. Managed CPU behavior must remain unchanged.

The Phase 5 Small v1.9.2 beam-3 result predates this launcher. It remains useful research evidence, but it is not a promotable Phase 6 identity. Any later selection phase must rerun the full corpus through the current launch contract and exact Echo commit.

## Stop gate

Stop if benchmark and product need different launch environments, if the launcher mutates the parent environment, or if an unproven runtime outranks CPU.
