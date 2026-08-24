# Phase 9. Expose truthful performance evidence

Back to [overview](overview.md).

## Goal

Let users and maintainers see what ran without turning performance internals into controls.

## Changes

- `src-tauri/src/main.rs` projects last-run mode, backend, runtime source, attempts, load or warm state, and split timing.
- `frontend/src/types.ts` models the optional diagnostics for old history compatibility.
- `frontend/src/App.tsx` adds concise values under the existing Advanced last-run readout. General Settings does not change.

Benchmark documentation publishes separate cold, resident, and acceleration tables with host, binary, model, tuning, corpus, and quality deltas.

## Data structures

- `LastRunPerformance`. Mode, backend, runtime source, total, load, inference, fallback, and attempt count.

## Verification

Static:

- Rust and TypeScript projection tests cover old rows, cold CPU, warm resident, accelerated system, and cold fallback.
- Responsive and accessibility tests keep Advanced diagnostics usable at every supported width.

Runtime:

- Drive the native Settings view after each real run mode and compare visible values with CLI JSON and raw benchmark rows.
- Confirm no thread, beam, GPU, or residency control appears in General or Advanced Settings.
