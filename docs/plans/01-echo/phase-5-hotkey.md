# Phase 5. Hotkeys

Back to [overview](./overview.md).

## Goal

A hold-to-talk key that moves the session from `Idle` to `Recording` and back. A CLI verb `echo rec` exists so a compositor keybind can drive the same loop when global grabs fail.

## Changes

`crates/echo/src/hotkey.rs` reads evdev for a configured key (default Right Ctrl). It also exposes stdin/CLI edge triggers.

Do not pull a "works everywhere" hotkey crate and hope. Wayland global shortcuts are compositor policy.

## Data structures

`HotkeyConfig` is `{ hold: KeySpec, toggle: Option<KeySpec> }`. Toggle is parsed and rejected until a later phase so we do not ship two modes half-done.

`HotkeyEvent` is `Down | Up`. Platform codes stop in this file. The session only sees these two events.

## Verification

Static. Workspace test and clippy. Table tests parse `RightCtrl`, `Super+Alt+Space`, and reject unknown names.

Runtime. `echo rec --once` in a terminal, hold the key or send the CLI down/up, assert the session log shows `Recording` then `Transcribing`. If evdev is permission-denied, the CLI path still passes and the fail reason names the missing permission.
