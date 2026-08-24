# Phase 4: Prove cache and reset state

[Back to overview](overview.md)

## Goal

Replace cache and reset labels with recorded operations and snapshots.

## Changes

- Add a Linux host-evidence collector for boot ID, power profile, governors, DRM devices, loader-selected ICD, and available memory.
- Make the runner create an empty backend cache root, snapshot it, warm it, verify population, and reuse it for timed rows.
- Bind reset strata to complete bundles with distinct boot IDs or another captured reset mechanism.
- Teach the analyzer to reject cache or reset claims without the required artifact chain.

## Data structures

- `CacheSnapshot`: root ownership, member digests, and capture time.
- `CacheEvidence`: fresh before/after snapshots and the bundle that seeded populated state.
- `ResetEvidence`: boot ID and the prior complete bundle used for comparison.

## Verification

Static: run benchmark and analyzer self-tests with fake cache trees and boot IDs.

Runtime: collect a fresh and populated Mesa cache on this host. A current-boot run remains `INCOMPLETE` for reset until a later boot supplies the second complete bundle.

## Stop gate

Stop if Echo cannot isolate the backend cache root, prove that it began empty, or bind a reset claim to captured state.
