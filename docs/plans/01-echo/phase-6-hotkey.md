# Phase 6. Hotkeys

Back to [overview](./overview.md).

## Goal

A hold-to-talk key that moves the session from `Idle` to `Recording` and back. Linux works in this environment. macOS uses `CGEventTap`. A CLI verb `echo rec` exists so a compositor keybind can drive the same loop when global grabs fail.

## Changes

`crates/echo/src/hotkey/linux.rs` reads evdev for a configured key (default Right Ctrl). It also exposes stdin/CLI edge triggers.

`crates/echo/src/hotkey/macos.rs` installs a `CGEventTap` for the same default. Fn is allowed as a config alias because that is what the video used.

`crates/echo/src/hotkey/mod.rs` picks the module and defines `HotkeyEvent` as `Down | Up`.

Do not pull a "works everywhere" hotkey crate and hope. Wayland global shortcuts are compositor policy.

## Data structures

`HotkeyConfig` is `{ hold: KeySpec, toggle: Option<KeySpec> }`. Toggle is parsed and rejected until a later phase so we do not ship two modes half-done.

`HotkeyEvent` is the only thing the session sees. Platform codes stop at this file.

## Verification

Static. Workspace test and clippy. Table tests parse `RightCtrl`, `Super+Alt+Space`, and reject unknown names.

Runtime. Linux. `echo rec --once` in a terminal, hold the key or send the CLI down/up, assert the session log shows `Recording` then `Transcribing`. If evdev is permission-denied, the CLI path still passes and the fail reason is `InjectPermission` or a new `HotkeyPermission` if we have to split it. macOS runtime is the same log on a Mac, plus a check that the tap dies cleanly when Accessibility is off instead of eating keys.
