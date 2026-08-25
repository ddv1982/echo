# Phase 7: Select, quarantine, and recover

[Back to overview](overview.md)

## Goal

Enable exact passed identities and preserve availability through one managed CPU retry.

## Changes

- Add an admission index that verifies bundled and local records before lookup.
- Preserve the existing `prepare_with_config` caller and build both the selected accelerator and managed CPU fallback plan.
- Add exact, bounded quarantine records written atomically after accelerated failures.
- Add one same-model managed CPU logical retry and backward-compatible attempt telemetry.
- Keep automatic language detection and non-empty recognition hints on managed CPU until those policies pass paired qualification.
- Verify the live Vulkan receipt with a package-owned, model-free probe before user audio can reach the accelerator.
- Disable the legacy no-VAD retry in both halves of a qualified plan.

## Data structures

- `AdmissionState`: unknown, passed, stopped, or quarantined.
- `AdmissionRecord`: exact identity key, evidence digest, gates, acceptance and expiry.
- `QuarantineRecord`: exact identity, reason, failure count, and bounded expiry.
- `WhisperPlanDecision`: managed CPU or qualified accelerator plus managed CPU fallback.

## Verification

Static: table tests cover missing, stopped, expired, changed, software, and quarantined identities. Integration tests inject missing libraries, crashes, timeouts, malformed JSON, silent CPU fallback, and wrong-device receipts.

Runtime: an exact passed test identity can select acceleration. Every injected failure quarantines only that identity and performs one CPU retry with the same model, language, prompt, VAD, tuning, and cleanup policy.

Package: a missing, user-writable, expired, changed, or unqualified admission record selects managed CPU without a GPU probe.

## Stop gate

Stop if any unpassed identity can run in production, if a fallback changes the model or decoding contract, or if more than one managed CPU logical retry is possible.
