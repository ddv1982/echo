# Manual test plan: Phase 14 Whisper acceleration

> **Superseded, kept for the sign-off record.** This plan tests the admission
> design that [Plan 17](../plans/17-selectable-gpu-acceleration/overview.md)
> replaced in v0.13.0. Acceleration is now a CPU or GPU choice with a device
> picker and no qualification step, so nothing below is runnable: the scenarios
> reference `whisper_acceleration.rs`, `whisper_plan.rs`,
> `stage-qualified-whisper-release.py`, and `qualified-release.json`, none of
> which still exist. Read it as the record of why acceleration did not ship
> under the old design.

**Audience:** QA, maintainers, release reviewers

**Last updated:** 2026-08-25

**Maps to:** [Gate 14](QA_GATES.md#gate-14-quality-safe-whisper-acceleration)

## Setup

Use Linux with the managed Small model, the managed CPU runtime, and a locally built patched v1.9.2 Vulkan candidate. Run Rust 1.88.0 for CI parity. The Tauri web layer supports 760 px and wider; the native window minimum is 760 × 560.

## Out of scope

- AMD, NVIDIA, CUDA, ROCm, and OpenVINO admission.
- Automatic language detection and non-empty recognition hints on Vulkan.
- Resident Whisper.
- A visible GPU override.

## Scenarios

| ID | Scenario | Runnable steps | Expected | Gate |
| --- | --- | --- | --- | --- |
| P14-A1 | Static and product build | Run frontend build/lint/tests, workspace clippy/tests, and release build using Rust 1.88.0. | All required commands exit 0. | 14.8 |
| P14-A2 | Evidence tools | Run the four Whisper verification scripts and validate the committed cache cycle. | Self-tests and replay validation exit 0; reset remains explicitly incomplete. | 14.1 |
| P14-B1 | Managed CPU floor | Run `echo-desktop transcribe` with the managed Small model. | Source is `managed`, backend is `cpu`, adjacent library path and 64-hex identity are reported. | 14.3, 14.7 |
| P14-B2 | Vulkan product smoke | Isolate the model cache, put the patched runtime first on `PATH`, unset parent loader/device selectors, and run the same CLI command. | Source is `system`, backend is `vulkan`, physical device, adjacent library path, and identity are reported. | 14.3 |
| P14-C1 | Poisoned child environment | Set conflicting LD, Vulkan, Mesa device, DRI, and CUDA values in the CLI integration test. | The child sees only explicit launch values; inherited device selectors are absent. | 14.2 |
| P14-C2 | Managed CPU recovery | Inject crash, timeout, malformed JSON, missing receipt, wrong receipt, silent CPU fallback, and VAD failure into an admitted plan. | Echo quarantines only that identity and runs one same-model managed CPU logical retry. | 14.7 |
| P14-C3 | Live receipt preflight | Run the package-owned receipt probe with the qualified runtime libraries and populated cache. | The exact admitted receipt is verified before user audio can reach Vulkan. | 14.7 |
| P14-D1 | Reset qualification | Collect and validate two new complete cache cycles from distinct boot IDs using the hardened probe. | Two complete boot identities bind fresh and populated results to the effective product launch contract. | 14.6 |
| P14-D2 | Current full-corpus qualification | Run at least ten randomized CPU/GPU pairs for every fixture through each exact Debian and RPM ELF variant with VAD active. | All latency, p95, language quality, hallucination, VAD, receipt, cache, and exact-identity gates pass. | 14.4 |
| P14-D3 | Product-speech coverage | Bind every required dictation class to licensed fixtures. | Coverage manifest is complete and replay-verifiable. | 14.5 |
| P14-E1 | Settings regression | Run the Playwright responsive suite in light and dark mode at every supported width. | No horizontal overflow; core Settings controls remain visible. | 14.8 |
| P14-F1 | Package containment | Bundle and extract Debian and RPM assets with the accelerator resource. | The packaged ELF, admission, probe, runtime, cache, and contained symlinks match the staged identities. | 14.8 |
| P14-F2 | Tag promotion | Upload the exact staged files to the commit-specific draft and run the tag workflow. | CI verifies and publishes only the qualified files and `qualified-release.json`. | 14.8 |

## Architecture reference

| Area | Path |
| --- | --- |
| Launch contract | `crates/echo/src/stt/whisper.rs` |
| Runtime identity | `crates/echo/src/stt/runtime.rs` |
| Execution plan | `crates/echo/src/stt/whisper_plan.rs` |
| Benchmark and replay | `scripts/benchmark-stt.py`, `scripts/analyze-stt-host-matrix.py` |
| Admission sweep | `scripts/sweep-whisper-admission.py` |
| Selection and recovery | `crates/echo/src/stt/whisper_acceleration.rs`, `crates/echo/src/stt/whisper_recovery.rs` |
| Release promotion | `scripts/promote-whisper-admission.py`, `scripts/stage-qualified-whisper-release.py` |
| Phase plan | `docs/plans/14-quality-safe-whisper-acceleration/` |

## Pass criteria

All P14 scenarios pass, Gate 14 is fully checked, no open P0/P1 bugs exist, and the admitted identity is measured on the exact production launch contract. Missing evidence is not a pass.

## QA status

Sign-off: **NO** on 2026-08-25. The implementation passed every pre-PR scenario. The exact merged Debian and RPM qualifications and the published tag remain. See the [Phase 14 follow-up merge](runs/COORDINATOR-MERGE-2026-08-25-2.md).
