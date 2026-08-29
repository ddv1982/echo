# Phase 9: Add the GPU device picker

[Back to overview](overview.md)

## Goal

Let the user choose which GPU runs Whisper, so a multi-GPU machine stops being decided by a string sort.

## Changes

**`crates/echo-core/src/config.rs`.** Add `whisper_gpu_device: Option<VulkanDeviceId>`. It sits alongside `whisper_acceleration` rather than inside it, so a pinned device survives a trip through CPU and back. Toggling acceleration off and on again remembers the card you chose.

**`src-tauri/src/main.rs`.** Project the pin as a settings field and accept it in `config_from_values_with_base`. Validate that a written UUID pair is nonzero lowercase 32-hex, matching the receipt parser's existing rule.

**`frontend/src/App.tsx`.** Add a `GPU device` row inside Advanced, rendered only when the acceleration control reads GPU. It offers `Automatic` plus one option per enumerated device, labelled with the device name and marked when the device is a software rasterizer. A refresh action re-runs enumeration, mirroring the microphone row's refresh rather than its three-second poll.

**`frontend/src/types.ts` and `App.test.tsx`.** Carry the new field and cover the empty, single-device, multi-device, and pinned-but-absent cases.

When the pinned device is not present in the current enumeration, the row shows it as not detected and keeps the pin. It does not silently rewrite the user's choice to another device.

## Data structures

`Config.whisper_gpu_device: Option<VulkanDeviceId>`, where `None` means automatic and resolves to the first enumerated non-software device.

## Verification

Static:

- `npm run typecheck --prefix frontend`, `npm run test --prefix frontend`, `npm run test:responsive --prefix frontend`, and `xvfb-run -a cargo test --workspace` pass.
- Component tests prove the picker is absent when CPU is selected, present when GPU is selected, and shows a not-detected state for a pin with no matching device.
- The responsive suite proves the new row stays inside the Advanced panel at every supported width.

Runtime: on the development machine, select GPU, confirm the Iris Xe is listed by name, pin it, restart, and confirm the pin persists. Confirm a fabricated pin for an absent device reports not detected.
