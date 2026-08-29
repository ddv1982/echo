# Testing

[Back to overview](overview.md)

## Static, every phase

```bash
npm run build --prefix frontend
npm run lint --prefix frontend
npm run test --prefix frontend
npm run test:responsive --prefix frontend
cargo clippy --workspace --all-targets -- -D warnings
xvfb-run -a cargo test --workspace
```

No phase is complete from these alone. Its matching runtime path must pass.

## What the automated suite cannot prove

No test in this repository drives a real GPU. Every Vulkan device in the suite comes from a fabricated `#!/bin/sh` stub printing a receipt line, or from a hand-built Rust struct. The crate carries no Vulkan binding. `crates/echo/src/stt/backend/vulkan.rs:466` is the only test capable of touching hardware, and before phase 1 it returns silently when `ECHO_TEST_VULKAN_PROBE` is unset, reporting a pass it did not earn.

Treat a green suite as proof of the CPU path, the settings plumbing, the parsers, and the failure handling. Nothing more.

## Runtime harness

**CPU path, every phase.** Dictate once through the built binary and confirm a transcript, the expected backend in the Advanced readout, and no latency change against the same recording before the phase.

**Accelerator failure modes, phases 10 and 11.** Reuse the six-case shell-stub harness in `crates/echo/src/stt/whisper_recovery.rs`: crash, non-JSON output, missing receipt line, wrong device identifier, CPU evidence in stderr, and hang. Each must quarantine the device once and produce exactly one CPU retry.

**Live device lanes, phases 10 and 12.** `scripts/verify-whisper-acceleration-modes.py --verify-live` drives the built binary through its lanes and asserts the backend is `vulkan` where expected and that CPU recovery fires under `ECHO_WHISPER_TEST_FAULT=no-devices`. It is operator-only and appears in no workflow. Run it by hand on a Vulkan host.

**Archive install, phase 7.** The Vulkan runtime archive is not published until phase 12, so its install path is proven against a local copy:

```bash
ECHO_PINNED_VULKAN_ARCHIVE=<path> cargo test -p echo --lib \
  install::tests::pinned_vulkan_runtime_archive_installs -- --ignored --exact
```

This verifies the archive size and digest against the catalog, every payload entry, symlink resolution, and executable bits. Until phase 12 publishes the archive, the catalog digest describes an artifact that exists only where an operator built it.

**Enumeration, phase 8.** Confirm the device list on a host with a usable GPU, and confirm an empty list rather than an error on a host without one.

## Thresholds

| Check | Threshold |
| --- | ---: |
| Accelerated run beats CPU on the same audio | paired median lower |
| CPU recovery after any accelerated failure | exactly 1 retry |
| Quarantine lifetime after a failure | 24 h |
| Cold enumeration before the first sample decodes | under 30 s |
| New route or cache records per accelerated run | 0 |

The last row is a regression guard. The deleted route store grew one file per accelerated transcription and hard-failed dictation above 256 records, so a design that persists anything per run has reintroduced that defect.

## Required negative cases

- A config holding `"auto"` loads as CPU. A config holding `"gpu"` loads as GPU.
- A pinned device that no longer enumerates yields CPU with a stated reason, never a different GPU.
- Two enumerated devices sharing PCI identifiers both survive enumeration.
- A software rasterizer is listed and flagged, not silently dropped.
- Selecting GPU with no Vulkan runtime component installed yields CPU with a stated reason.
- A user who never selects GPU never downloads the Vulkan runtime and never executes a Vulkan code path.

## Product verification

Before phase 12 closes, install each published artifact on a clean machine, dictate on CPU with no acceleration component present, then install the component through Settings and dictate on GPU. The changelog claim and the observed behaviour must match. That match is the whole point of this plan.
