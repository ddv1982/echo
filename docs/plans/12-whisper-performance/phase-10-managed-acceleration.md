# Phase 10. Add one managed accelerator variant

Back to [overview](overview.md).

## Goal

Package only the accelerated backend that real system-runtime evidence proves worth supporting.

## Changes

- `crates/echo/src/install/catalog.rs` adds one distinct pinned runtime component with URL, size, SHA-256, version, backend, prerequisites, and probe contract. Managed CPU remains a separate fallback component.
- Managed archive inventory and generation tooling include the exact accelerated payload and libraries without weakening extraction boundaries.
- `src-tauri/src/setup.rs` detects hardware and driver prerequisites, accounts for disk space, installs the variant only when admissible, runs a real inference probe before activation, and keeps CPU repair and removal independent.

Do not begin this phase until Phase 5 data chooses one backend. Do not stretch the existing CPU component ID to contain several runtime variants.

## Data structures

- `ManagedRuntimeVariant`. Component ID, backend, platform, prerequisites, artifact, probe, and CPU fallback.

## Verification

Static:

- Catalog closure, hashes, payload inventory, disk accounting, activation, repair, removal, and fallback tests pass for CPU and the one accelerated variant.
- Package manifests remain free of driver libraries owned by the host.

Runtime:

- Install from empty state on supported hardware, transcribe with the resolved backend, corrupt and repair the variant, remove it during an idle broker, and verify managed CPU remains ready.
- Run on unsupported or broken-driver hardware and confirm Echo never downloads or activates the variant and continues on CPU.
- Build Debian, RPM, raw binary, and AppImage release artifacts and run the same package metadata and launch checks used by the release workflow.
