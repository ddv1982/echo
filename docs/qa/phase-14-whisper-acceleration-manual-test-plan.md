# Manual test plan: Phase 14 Whisper acceleration

**Audience:** QA, maintainers, release reviewers

**Last updated:** 2026-08-25

**Maps to:** [Gate 14](QA_GATES.md#gate-14-quality-safe-whisper-acceleration)

## Setup

Use Linux with the managed Small model, the managed CPU runtime, and a locally built patched v1.9.2 Vulkan candidate. Run Rust 1.88.0 for CI parity. The Tauri web layer supports 760 px and wider; the native window minimum is 760 × 560.

## Out of scope

- Production GPU selection, quarantine, or retry policy.
- Packaged accelerator components for hardware families.
- Resident Whisper or background warmup.
- A visible GPU override.

## Scenarios

| ID | Scenario | Runnable steps | Expected | Gate |
| --- | --- | --- | --- | --- |
| P14-A1 | Static and product build | Run frontend build/lint/tests, workspace clippy/tests, and release build using Rust 1.88.0. | All required commands exit 0. | 14.8 |
| P14-A2 | Evidence tools | Run the four Whisper verification scripts and validate the committed cache cycle. | Self-tests and replay validation exit 0; reset remains explicitly incomplete. | 14.1 |
| P14-B1 | Managed CPU floor | Run `echo-desktop transcribe` with the managed Small model. | Source is `managed`, backend is `cpu`, adjacent library path and 64-hex identity are reported. | 14.3, 14.7 |
| P14-B2 | Vulkan product smoke | Isolate the model cache, put the patched runtime first on `PATH`, unset parent loader/device selectors, and run the same CLI command. | Source is `system`, backend is `vulkan`, physical device, adjacent library path, and identity are reported. | 14.3 |
| P14-C1 | Poisoned child environment | Set conflicting LD, Vulkan, Mesa device, DRI, and CUDA values in the CLI integration test. | The child sees only explicit launch values; inherited device selectors are absent. | 14.2 |
| P14-D1 | Reset qualification | Collect and validate two new complete cache cycles from distinct boot IDs using the hardened probe. | Two complete boot identities bind fresh and populated results to the effective product launch contract. | 14.6 |
| P14-D2 | Current full-corpus qualification | Run at least ten randomized CPU/GPU pairs for every fixture through the current Echo commit and launch contract. | All latency, p95, language quality, hallucination, receipt, cache, and exact-identity gates pass. | 14.4 |
| P14-D3 | Product-speech coverage | Bind every required dictation class to licensed fixtures. | Coverage manifest is complete and replay-verifiable. | 14.5 |
| P14-E1 | Settings regression | Run the Playwright responsive suite in light and dark mode at every supported width. | No horizontal overflow; core Settings controls remain visible. | 14.8 |

## Architecture reference

| Area | Path |
| --- | --- |
| Launch contract | `crates/echo/src/stt/whisper.rs` |
| Runtime identity | `crates/echo/src/stt/runtime.rs` |
| Execution plan | `crates/echo/src/stt/whisper_plan.rs` |
| Benchmark and replay | `scripts/benchmark-stt.py`, `scripts/analyze-stt-host-matrix.py` |
| Admission sweep | `scripts/sweep-whisper-admission.py` |
| Phase plan | `docs/plans/14-quality-safe-whisper-acceleration/` |

## Pass criteria

All P14 scenarios pass, Gate 14 is fully checked, no open P0/P1 bugs exist, and the admitted identity is measured on the exact production launch contract. Missing evidence is not a pass.
