# Phase 8: Package and qualify hardware families

[Back to overview](overview.md)

## Goal

Turn proven system-runtime identities into independently managed accelerator components and gather evidence across hardware families.

## Changes

- Add one managed Vulkan component with a pinned receipt-capable runtime and adjacent non-driver libraries.
- Verify install, repair, corruption, removal, upgrade, rollback, and managed lease behavior.
- Exercise Debian, RPM, AppImage, and raw-binary launch paths.
- Run exact-identity matrices on Intel, AMD, and NVIDIA Vulkan hosts. Treat NVIDIA CUDA, AMD ROCm, and Intel OpenVINO as later separate components.

## Data structures

- `AcceleratorComponent`: backend variant, artifact inventory, launch contract, and receipt-build identity.
- `HardwareMatrixRecord`: hardware and driver family, exact decision records, negative cases, and package path.

## Verification

Static: archive inventory and installer tests cover the full accelerator payload without host drivers or ICDs.

Runtime: installed packages must reproduce the qualified identity on every admitted host. CPU-only and broken-driver hosts must start on managed CPU without probe latency.

## Stop gate

Stop a component if packaging changes its identity, leaves unresolved libraries, needs a host driver in the payload, or generalizes a pass across untested devices or backends.
