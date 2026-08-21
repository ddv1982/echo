# Phase 11: device enumeration

Back to [overview](overview.md).

## Goal

Echo can list the input devices and record from a named one. Also fixes a live bug.

## Changes

**Restore the `device` field to `AudioCapture`.** This is not a new concept, it is a dropped one. `docs/plans/01-echo/phase-2-audio.md:19` specifies `AudioCapture` as `{ device: DeviceName, cancel: CancellationToken }`. The implemented struct at `crates/echo/src/audio.rs:39-41` has only `cancel`.

**The bug.** `open_default` calls `default_input_device()` and discards the result (`:81-82`), a liveness probe with no binding. Then `record` re-creates the host and queries the default again (`:89-90`). **The device probed at construction is not the device recorded from.** Unplug a USB mic between the two, or let the default change, and `record` silently follows the new default. That is wrong today and it becomes much worse once a user has pinned a device and expects it to be honoured.

Putting the resolved device in the struct fixes it as a side effect, because there is then exactly one resolution point.

**`crates/echo/src/audio.rs`.** Add enumeration over `host.input_devices()`, and a constructor that takes an optional requested device name and resolves it. `build_stream` (`:135-140`) already takes `&cpal::Device`, so it needs no change.

**Keep the diff inside `AudioCapture` and its constructor** (**principle-laziness-protocol**). The literal reading of "thread a device through" touches seven signatures: `open_default`, `record`, `capture_pcm` (`crates/echo/src/rec.rs:206`), `run_record` (`:81`), and the three `run_rec_*` entry points. Do not do that. `AudioCapture` is already the handle that survives from construction to `record`, so `record` reads `self.device` and nothing past the constructor changes. A maintainer should not have to trace a device name through four functions to answer "which microphone did it use".

**Split resolution from enumeration so it is testable without hardware.** The tested unit takes a candidate list and a requested name and returns a choice. That mirrors the established pattern at `crates/echo/src/hotkey.rs`, where the env read (`hold_key`, `:114`) is separate from the tested parser (`parse_hold_key`, exercised at `:198-218`).

**Resolution rules.** A missing named device falls back to the default and reports that it did. It must not fail. Bluetooth headsets and USB microphones disappear, and a dictation app that refuses to record because yesterday's headset is gone is broken (**principle-experience-first**). Surface the fallback in the status so the user knows why they are on the laptop mic.

**`crates/echo-core/src/config.rs`.** Add the `microphone: Option<String>` field. Store the cpal device name, not an index. Indices reorder across reboots.

## Data structures

`InputDevice { name: String, is_default: bool }` for the list. `fn resolve_device(candidates: &[InputDevice], requested: Option<&str>) -> DeviceChoice` where `DeviceChoice` distinguishes requested-and-found, requested-and-missing-so-defaulted, and no-devices-at-all. Three outcomes, three variants, so the caller cannot forget the middle one (**principle-type-system-discipline**).

## Verification

**Static.** `cargo test --workspace`.

Unit tests in `crates/echo/src/audio.rs`'s existing `mod tests`, all hardware-free because resolution is split out: name matches; name missing falls back to default and says so; empty list yields `AudioError::NoDevice`.

There is no fake audio device and cpal offers no seam for one, so the `hotkey.rs` injectable-device pattern cannot be followed directly. The split is the substitute.

**Runtime.** Via **control-cli**.

New ignored integration test `crates/echo/tests/record_device.rs`, copying `record_once.rs` line for line: the same `#[ignore]` reason-string shape and the same runtime `ECHO_LIVE_MIC` assert, so `cargo test -- --ignored` fails loudly rather than quietly grabbing a microphone. Enumerate, select by name, record, assert `peak_rms > 0.001`.

New CLI test in `src-tauri/tests/`, following `rec_once_without_mic_names_permission` (`src-tauri/tests/cli_rec.rs:54-77`), which already tolerates a machine with no microphone. A nonexistent device name must fall back and still record, not exit nonzero.

Manually, with two real input devices: pin the non-default one, record, and confirm from the audio which one was live. Then unplug it mid-session and confirm the fallback message rather than a crash.
