# QA run: sequential, 2026-08-25

| Field | Value |
| --- | --- |
| Agent | Sequential product verification |
| Phase | 14 |
| Branch | `codex/whisper-phase3-quality-safe-acceleration` |
| Build | Rust 1.88.0, Node 22-compatible toolchain, Linux |
| Surface | Tauri CLI/runtime plus Chromium fallback for the web layer |

## Summary

| Metric | Count |
| --- | ---: |
| Scenarios run | 9 |
| Passed | 6 |
| Failed | 0 |
| Blocked by missing qualification evidence | 3 |

## Results

| Test ID | Result | Evidence |
| --- | --- | --- |
| P14-A1 | PASS | Frontend: 5 files / 94 tests; workspace: 160 Echo library tests plus full integration suites; clippy green; release build green. |
| P14-A2 | PASS | `verify-stt-benchmark`, corpus replay, acceleration self-tests, runtime archive install, and cache-cycle validation all exited 0. |
| P14-B1 | PASS | [Managed CPU JSON](../../../.audit/whisper-phase6-current/managed-cpu.json): 307 ms, managed CPU, adjacent library identity. |
| P14-B2 | PASS | [System Vulkan JSON](../../../.audit/whisper-phase6-current/system-vulkan.json): 1377 ms cold smoke on Intel Iris Xe, adjacent library identity. |
| P14-C1 | PASS | CLI integration poisoned LD, Vulkan, Mesa, DRI, and CUDA selectors; all three integration tests passed. |
| P14-D1 | BLOCKED | Committed cycle has one boot ID and reports `resetState: INCOMPLETE`. |
| P14-D2 | BLOCKED | The 57.777% Phase 5 result predates the current launch contract and cannot qualify this code state. |
| P14-D3 | BLOCKED | The corpus lacks complete product-speech-class coverage. |
| P14-E1 | PASS | Playwright ran light and dark Settings checks across widths 760, 761, 800, 920, 959, 960, 961, and 1024; both tests passed. |

## Verification notes

- The initial cold workspace run tripped two unrelated 500 ms timing assertions. Each passed three consecutive isolated retries, and the final full workspace rerun passed.
- Repository-wide `cargo fmt --all -- --check` is not a CI command and reports pre-existing formatting drift in unchanged files. Every changed Rust file passes Rust 1.88 `rustfmt --check` with child-module recursion disabled.
- T3 preview reached the Vite page but snapshot capture failed twice. The maintained Playwright suite supplied browser evidence instead.
- Native tray, OS dialog, and window-shell interaction were not manually driven on Linux. The changed product surface is the CLI child launch; that real path was exercised for both CPU and Vulkan.

## Bugs filed

None.

## Tracker sync

No tracker was configured or mutated because no bugs were filed.
