# Phase 8: Package and qualify hardware families

[Back to overview](overview.md)

## Goal

Turn proven system-runtime identities into independently managed accelerator components and gather evidence across hardware families.

## Changes

- Add one managed Vulkan component with a pinned receipt-capable runtime and adjacent non-driver libraries.
- Keep the admission record outside the measured executable in a root-owned package resource.
- Build the executable once, qualify that exact file, then use `cargo tauri bundle` without recompiling it.
- Replay the raw run bundle and the cache cycle before generating the admission record.
- Seed the exact populated Mesa cache under an identity-specific user cache directory.
- Verify install, repair, corruption, removal, upgrade, rollback, and managed lease behavior.
- Exercise Debian, RPM, AppImage, and raw-binary launch paths.
- Run exact-identity matrices on Intel, AMD, and NVIDIA Vulkan hosts. Treat NVIDIA CUDA, AMD ROCm, and Intel OpenVINO as later separate components.

## Data structures

- `AcceleratorComponent`: backend variant, artifact inventory, launch contract, and receipt-build identity.
- `HardwareMatrixRecord`: hardware and driver family, exact decision records, negative cases, and package path.

## Verification

Static: archive inventory and installer tests cover the full accelerator payload without host drivers or ICDs.

Runtime: installed packages must reproduce the qualified identity on every admitted host. CPU-only and broken-driver hosts must start on managed CPU without probe latency.

Release: extract the Debian package, the RPM package, and the AppImage. Each packaged executable must match the qualified SHA-256. Each accelerator payload must match the generated admission record. The tag workflow publishes only those staged files.

## Stop gate

Stop a component if packaging changes its identity, leaves unresolved libraries, needs a host driver in the payload, or generalizes a pass across untested devices or backends.
