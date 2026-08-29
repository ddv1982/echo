# Phase 8: Enumerate GPU devices over IPC

[Back to overview](overview.md)

## Goal

Give the frontend a list of real Vulkan devices with names and stable identities, so the picker in phase 9 has something to show.

## Changes

**`crates/echo/src/stt/backend/vulkan.rs`.** Expose enumeration as a list rather than a selection. `enumerate()` already returns every route with its stable receipt; stop treating `drm_driver` as a filter that silently drops devices, and instead carry the resolved driver name as metadata. That filter requires a receipt's PCI identifiers to map to exactly one render node, which drops every route on a machine with two identical GPUs.

**`crates/echo/src/stt/whisper_probe.rs`.** `vulkan_device` already extracts human-readable names such as `Intel(R) Iris(R) Xe Graphics (ADL GT2)` and `AMD Radeon RX 7800 XT (RADV)` from the runtime's stderr. Pair each enumerated receipt with its name and flag software rasterizers such as `llvmpipe` rather than excluding them.

**`src-tauri/src/main.rs`.** Add a `list_gpu_devices` command returning the device list, and cache the result for the process. Enumeration spawns one probe subprocess per ICD manifest, so it runs on demand and on explicit refresh, never on a timer like the microphone list.

**`frontend/src/tauri.ts`.** Add the binding and a preview fixture with two devices and one software rasterizer.

## Data structures

`VulkanDeviceId { device_uuid, driver_uuid }`, the stable pair the runtime selector already accepts through `ECHO_WHISPER_VULKAN_DEVICE_UUID` and `ECHO_WHISPER_VULKAN_DRIVER_UUID`.

`GpuDevice { id: VulkanDeviceId, name, vendor_id, device_id, drm_driver: Option<String>, software: bool }`, one entry per enumerated Vulkan device.

## Verification

Static:

- `xvfb-run -a cargo test --workspace` passes, including a test that two devices with identical PCI identifiers both survive enumeration.
- A parser test proves `llvmpipe` is listed and flagged rather than dropped.
- Frontend tests render the preview device list.

Runtime: run `list_gpu_devices` on the development machine with the Vulkan component installed and confirm it names the Iris Xe with a nonzero UUID pair. Confirm it returns an empty list, not an error, on a host with no usable device.
