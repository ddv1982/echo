# Phase 7. HUD

Back to [overview](./overview.md).

## Goal

A click-through overlay that shows we are listening, then that we are transcribing, then the fail reason. No settings chrome. The video's waveform is the whole point of this phase.

## Changes

`crates/echo/src/ui/hud.rs` draws a small always-on-top window with egui plus winit, or iced if egui cannot be click-through on the target compositor. Pick one toolkit in the first commit of the phase and stay there.

`crates/echo/src/ui/waveform.rs` turns `peak_rms` samples into bars. Cosmetic only. Do not block inject on animation.

Linux may need a layer-shell path for wlroots. If that is a second window backend, it still lives under `ui/`, not a new crate.

## Data structures

`HudState` is a projection of `SessionState` plus a ring buffer of RMS samples. The UI does not own the session.

`HudConfig` is `{ enabled, anchor }`. Default anchor is bottom-center, matching the video.

## Verification

Static. Workspace test and clippy. Waveform math is unit-tested with a sine fixture.

Runtime. No control skill for this surface. Linux. Launch `echo --hud-demo`, confirm the window ignores clicks into the app beneath it, and confirm it hides on `Idle`. macOS. Same check plus "does not steal focus from TextEdit during inject." Screenshot optional. Focus theft is a phase failure.
