# Phase 7: Deliver the Vulkan runtime as a managed component

[Back to overview](overview.md)

## Goal

Make the Vulkan-capable whisper.cpp build a downloadable component fetched on demand, so no user pays 58 MB for hardware they may not have and AppImage is no longer excluded.

## Changes

**`crates/echo/src/install/catalog.rs`.** Add `ComponentId::WhisperVulkanRuntime` with its own archive URL, archive digest, installed size, and setup-plan membership. Use a new id rather than extending `WhisperRuntime`, because `receipt_files_compatible` would otherwise flip every existing install to `NeedsRepair`. Membership is opt-in: it belongs to no default plan and is fetched when the user selects GPU.

**`crates/echo/src/install/archive_inventory.json`** and **`scripts/generate-managed-inventory.py`.** Add the per-file size and digest inventory for the new archive.

**`crates/echo/src/install/installer.rs` and `src-tauri/src/setup.rs`.** Extend the exhaustive component match and `external_components` so the new id projects into readiness.

**`crates/echo/src/stt/runtime.rs`.** `SpeechRuntimeInventory::from_cache` currently hard-codes `backend: Cpu` for the managed runtime. Emit a second candidate with `source: Managed, backend: Vulkan` when the Vulkan component is installed.

**`frontend/src/types.ts`.** Extend the `ComponentId` union. `SpeechSetupSection.tsx` needs no structural change; its component list is generic.

The archive is the tree built by `scripts/build-whisper-vulkan-receipt.sh` against whisper.cpp `v1.9.2` at the pinned commit, carrying all three Echo patches. Building it requires a host with Vulkan headers, `glslc`, and a real device, so it is an operator artifact published once per runtime version, not a per-release obligation.

## Data structures

`ComponentId::WhisperVulkanRuntime`, a managed component like the six that exist. No new install machinery: archive digest verification, per-file checks, symlink target pinning, range resume, and generation-named directories all apply unchanged.

## Verification

Static:

- `xvfb-run -a cargo test --workspace` and the frontend suite pass.
- An install test proves the component verifies, extracts with executable bits preserved, and repairs on corruption.
- A test proves an install without the component still reports ready for CPU.

Runtime: with the component absent, confirm Settings shows GPU as unavailable and offers to fetch it. Fetch it, confirm progress reporting through the existing setup event channel, and confirm `whisper-cli` and `echo-whisper-runtime-probe` land executable in the model cache.
