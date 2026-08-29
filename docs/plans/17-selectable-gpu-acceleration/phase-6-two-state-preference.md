# Phase 6: Make acceleration a two-state force

[Back to overview](overview.md)

## Goal

Replace `Auto` and `Cpu` with `Gpu` and `Cpu`, defaulting to CPU, so the setting states a user intent rather than a guess.

## Changes

**`crates/echo-core/src/engine.rs`.** `WhisperAccelerationPreference` becomes `{ Cpu, Gpu }`. Migration runs in opposite directions for the two legacy strings: `auto` resolves to `Cpu`, matching what Auto does today given nothing accelerates, and `gpu` resolves to `Gpu`, honouring what a user who wrote it originally asked for. Both are accepted through a serde alias and through `parse`, so no config file breaks.

**`crates/echo/src/stt/mod.rs`.** `whisper_acceleration_factory_default` returns `Cpu`.

**`src-tauri/src/cli.rs`.** `CliWhisperAcceleration` becomes `{ Cpu, Gpu }` with `auto` as a hidden value alias on `Cpu`, so existing scripts keep parsing.

**`src-tauri/src/main.rs`.** No shape change. `settings_from` and `config_from_values_with_base` carry the new strings.

**`frontend/src/types.ts` and `App.tsx`.** The segmented control offers CPU and GPU. `LastRunPerformance.selection.preference` narrows to `'cpu' | 'gpu'`. Copy states plainly that GPU is measured on Intel and unproven on other vendors.

Selecting GPU has no effect yet. Phase 10 gives it one.

## Data structures

`WhisperAccelerationPreference` is `Cpu | Gpu`, default `Cpu`. `Config` gains nothing this phase; the device pin arrives in phase 9.

## Verification

Static:

- `npm run typecheck --prefix frontend`, `npm run test --prefix frontend`, and `xvfb-run -a cargo test --workspace` pass.
- A round-trip test proves a config holding `"auto"` loads as `Cpu` and one holding `"gpu"` loads as `Gpu`.
- A component test asserts the control renders CPU and GPU and no Auto button.

Runtime: open Settings, expand Advanced, switch between CPU and GPU, and confirm both persist across a restart and that transcription still succeeds in each.
