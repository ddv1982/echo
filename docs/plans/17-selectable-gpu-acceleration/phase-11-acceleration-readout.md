# Phase 11: Report what actually ran

[Back to overview](overview.md)

## Goal

Make the Advanced readout state which device executed the last transcription and why acceleration was skipped when it was, so a silent fallback is visible.

## Changes

**`src-tauri/src/main.rs`.** Extend `project_last_run_performance` so the projection carries the resolved device name alongside the backend, and carries a skip reason when the preference read GPU but the run went to CPU. Reasons are a closed set: no Vulkan runtime installed, no device enumerated, the pinned device is absent, the device is quarantined, and recovery after a failed accelerated run.

**`frontend/src/App.tsx`.** The `Acceleration` row already renders backend and device. Extend it to render the skip reason when present. This row is the only place a user can see that GPU was requested and CPU was used, so its wording carries the whole signal.

**`frontend/src/types.ts` and `App.test.tsx`.** Carry the reason and cover each variant.

This is the compensating control for the plan's accepted risk. No AMD or NVIDIA device has been measured, so a user on unproven hardware needs to see what happened without reading a log.

## Data structures

`AccelerationSkipReason`, a closed enum over the five cases above. Modelled as a variant rather than a free string so the frontend renders known copy and cannot display a raw internal message.

## Verification

Static:

- `npm run typecheck --prefix frontend`, `npm run test --prefix frontend`, and `xvfb-run -a cargo test --workspace` pass.
- A component test covers each skip reason and the accelerated case.
- The responsive suite proves the row stays inside the panel with the longest reason string rendered.

Runtime: with GPU selected and the runtime component absent, dictate and confirm the readout names that reason. Repeat with a pin for an absent device, and with `ECHO_WHISPER_TEST_FAULT=no-devices`.
