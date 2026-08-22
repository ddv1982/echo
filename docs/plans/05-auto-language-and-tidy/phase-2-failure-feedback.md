# Phase 2: failure feedback for shortcut sessions

Back to [overview](overview.md).

## Goal

A failed hotkey session tells the user what happened and what to do. Today a compositor-shortcut failure is silent where the user is looking: the detail goes to stderr, which a desktop-spawned process writes to the journal nobody reads (`crates/echo/src/rec.rs:59,71,142`), the status file is only read by the desktop window when it happens to be open, and the HUD's failure flash is X11-only and brief. "I pressed the hotkey and nothing happened" is the support symptom; this phase makes the failure visible.

## Changes

**`crates/echo/src/notify.rs`, new.** One function, `session_failed(reason, detail)`, mapping each `FailReason` to a plain sentence naming the failure and the fix: no engine or model points at Settings → Get a model; no microphone points at Settings → Microphone; injection permission names the desktop's input settings. Sent with notify-rust, a pure-Rust D-Bus client for `org.freedesktop.Notifications` whose default zbus backend shares the zbus 5 stack the single-instance plugin already brought in. The rejected alternative is shelling out to `notify-send`: it is a separate binary that is not guaranteed installed, which is the same class of silent failure this phase exists to kill. A failed notification logs and is swallowed; it must never fail or change the session.

**`crates/echo/src/rec.rs`.** The failure paths call `session_failed` when the session runs in a bare CLI process. The distinction is the process, not the hotkey source: `rec` subcommands run in `try_cli` before the Tauri builder (`src-tauri/src/main.rs:823-829`), while the GUI's record button records in-process, where the status poll already shows the failure. A process-global flag set by the CLI entry points marks the bare process; the GUI's in-process sessions leave it unset.

**No notification on Done.** Inserted text at the cursor is the feedback; a "transcription inserted" toast on every dictation is noise on the happy path. The Failed state is the only one that says something the user cannot otherwise see.

## Data structures

`session_failed(reason: FailReason, detail: Option<&str>)` over the existing `FailReason` enum. A static `AtomicBool` marks the bare-CLI process. No IPC, no config.

## Verification

**Static.** `cargo test --workspace`. Unit tests pin the reason-to-message mapping (every `FailReason` variant has a sentence with a fix) and the flag gating (GUI-origin sessions do not notify). The notification send is behind a function the tests stub; the mapping is pure.

**Runtime.** Via **control-cli** under `dbus-run-session`: with no engine installed, `rec --once` fails and a notification lands on the session bus (assert via `dbus-monitor` or a bus stub), naming the missing model and the fix. With the GUI's own record button failing, no notification fires. Attach the transcript to the PR.
