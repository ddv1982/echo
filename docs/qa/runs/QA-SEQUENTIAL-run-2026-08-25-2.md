# QA run: sequential follow-up, 2026-08-25

| Field | Value |
| --- | --- |
| Agent | Sequential product verification |
| Phase | 14 |
| Branch | `codex/whisper-gpu-admission` |
| Build | Echo 0.12.0, Rust 1.88-compatible workspace, Tauri CLI 2.11.4, Linux |
| Surface | Tauri CLI/runtime, Debian and RPM resources, Chromium fallback for the web layer |

## Summary

| Metric | Count |
| --- | ---: |
| Scenarios run | 13 |
| Passed | 11 |
| Failed | 0 |
| Blocked by post-merge release evidence | 2 |

## Results

| Test ID | Result | Evidence |
| --- | --- | --- |
| P14-A1 | PASS | Frontend build and lint passed. Five Vitest files ran 94 tests. Workspace clippy passed with all targets. The full workspace rerun passed 180 Echo tests, 52 core tests, 25 desktop tests, and every active integration. |
| P14-A2 | PASS | Benchmark, corpus, acceleration, runtime archive, transcription CLI, first-run, fixed-toggle, recording-limit, and release-tool checks exited 0. |
| P14-B1 | PASS | The managed runtime remains the universal floor and now receives `--no-gpu` whenever no exact admission is available. |
| P14-B2 | PASS | The current reviewed branch screen retained Intel Vulkan parity. It remains research evidence because final package variants require new VAD-active confirmation. |
| P14-C1 | PASS | CLI integration tests retain child-only loader, ICD, cache, and device-selector replacement. |
| P14-C2 | PASS | Recovery tests injected six accelerator failures, VAD rejection, pre-existing quarantine, and corrupt quarantine. Each user-visible fallback ran managed CPU once. |
| P14-C3 | PASS | Model-free probe SHA `3948213f` emitted the admitted Intel receipt against the original runtime libraries before user audio. |
| P14-D1 | PASS | Cycle A boot `f4fd3d6f` and cycle B boot `e8f95880` bind the same runtime, model, VAD, decoding, ICD, and device receipt. Cycle B reset state is `COMPLETE`. |
| P14-D2 | BLOCKED | The final merged Debian and RPM ELF variants have not yet run 280 CPU and 280 Vulkan measurements each with VAD active. |
| P14-D3 | PASS | The 28-fixture corpus covers five languages and all eight product classes. Deterministic reconstruction and replay pass. |
| P14-E1 | PASS | Playwright passed light and dark Settings checks at every supported width. |
| P14-F1 | PASS | Package smoke extracted Debian and RPM resources under `usr/lib/io.github.ddv1982.echo/whisper-acceleration`; each ELF differed from the canonical build only at the Tauri bundle marker. |
| P14-F2 | BLOCKED | The commit-specific draft, tag workflow, and published 0.12.0 assets are post-merge work. |

## Verification notes

- The loaded workspace run hit the two known 500 ms scheduler-sensitive tests. Each passed three isolated retries. The complete workspace rerun then passed.
- Computer Use is unavailable on this Linux host. The maintained Playwright suite supplied UI evidence for the Tauri web layer.
- The first admission sweep was invalid for release promotion because its rows showed VAD inactive. The final sweep now requires `--vad-path` and rejects every row without the same active VAD.
- Multi-model review used four independent models. The configured fourth model exhausted its quota twice, so the successful replacement used `gpt-5.6-sol`.

## Bugs filed

None. Review findings were fixed before this QA pass.

## Tracker sync

No tracker is configured. No external issues were created or changed.
