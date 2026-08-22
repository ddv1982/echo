# Phase 3: real hold-to-talk

Back to [overview](overview.md).

## Goal

The Settings hold key becomes real. Today it is a UX lie by omission: it only affects `echo-desktop rec --hold`, which the user must run in a terminal, keep running, and which needs input-group evdev access (`crates/echo/src/rec.rs:65-93`). A user who sets it reasonably expects global hold-to-talk. This phase runs the listener inside the long-lived desktop process, so the setting works with zero terminal usage.

## Changes

**`crates/echo/src/hotkey.rs`.** The existing `HoldKey`/evdev machinery stays. What changes is who owns it: the desktop process spawns a listener thread at startup when `/dev/input` is readable and a hold key is configured. Key down starts an in-process toggle session (the existing `start_recording_thread` path); key up stops it. The listener tracks whether it started the session, so a key-up never stops a session it did not start, and a key-down while another process holds the toggle lock does nothing rather than truncating someone else's recording. The toggle lock already serializes sessions through `recording.lock` with pid liveness (`crates/echo/src/rec.rs:316-352`); the listener consults it before starting instead of relying on toggle semantics to stop.

**Lifecycle.** Changing the hold key in Settings restarts the listener with the new code; clearing it stops the listener. When `/dev/input` is not readable, the listener does not start and the Settings row says so with the exact fix (`sudo usermod -aG input $USER`, then log out and back in). The row shows the listener state: active, or needs permission.

**Interplay with single-instance and takeover.** The listener lives in the one GUI process the single-instance gate admits; the startup takeover terminates old pre-gate processes, whose listeners die with them. evdev allows multiple readers unless a device is grabbed, and Echo never grabs, so a dying old listener cannot wedge the new one.

**`crates/echo/src/rec.rs`.** A code comment at `capture_from` records the fixture caveat: with `ECHO_AUDIO_FIXTURE` set, capture returns before consulting `StopWhen`, so toggle and hold semantics do not hold under fixtures. Fine for tests; now written down.

**`README.md`.** The hold-to-talk paragraph stops telling users to run `rec --hold` in a terminal as the primary path and describes the desktop listener, keeping the CLI loop as the terminal-native alternative.

## Data structures

`HoldListener { thread, cancel }` owned by the desktop process, rebuilt on settings change. No new config; the existing `hold_key` setting drives it.

## Verification

**Static.** `cargo test --workspace`. Unit tests over a synthetic evdev node (the existing hotkey tests already fabricate one): listener starts a session on key down and stops it on key up; a key-up without a listener-started session is a no-op; a key-down while another process holds the lock does not start. The permission-absent path reports its reason.

**Runtime.** Via **control-ui** on hardware with evdev access: set the hold key in Settings, hold it in another app, watch the session record and insert on release, with no terminal involved. In the sandbox, drive the synthetic evdev node and assert the session machine transitions. Attach the transcript to the PR.
