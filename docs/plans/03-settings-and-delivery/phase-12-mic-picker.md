# Phase 12: microphone picker

Back to [overview](overview.md).

## Goal

Choose a microphone in Settings and know it works before recording something important.

## Changes

**`src-tauri/src/main.rs`.** Two commands:

- `list_input_devices() -> Result<Vec<InputDevice>, String>`
- `test_input_device(name: Option<String>) -> Result<f32, String>`

`test_input_device` records a short burst and returns the level. This is the difference between a dropdown and a working microphone. Selecting a device tells you nothing; the failure mode is a muted or dead input, and the user finds out when a transcript comes back empty.

Note the constraint on the level. `Pcm16kMono::peak_rms` (`crates/echo-core/src/types.rs:44-57`) is the RMS of the **entire buffer** despite its name, with no windowing, so a one-second utterance inside a ten-second recording reads as quiet. Keep the test burst short, around one second, so a whole-buffer RMS is a meaningful number. Do not build a continuous meter on this value.

Enumeration is cheap; the 10-second health cache is for the expensive probes. Do not cache the device list, or a newly plugged microphone takes 10 seconds to appear.

**`frontend/src/App.tsx`.** A `<select>` in the Audio panel, using phase 10's `SettingSelect` and `select` styling. A segmented control cannot hold a device list. Include an explicit "System default" option distinct from any named device, so a user can choose to follow the default rather than be pinned to whatever it was on the day they opened Settings.

Beside it, a "Test" button showing the measured level. Reuse the `.status-note` dot idiom the pipeline rows already use rather than inventing a meter widget.

Show the fallback state from phase 11. When the configured microphone is absent, say so on the row: which device was requested, and which one is actually in use.

**`frontend/src/tauri.ts`.** Wrappers plus preview fixtures. Two or three plausible device names in the fixture so the dropdown has something to render under Vitest and in `npm run dev`.

**Refresh the list when the view opens**, not once at mount. Devices are hot-pluggable.

## Data structures

Reuse phase 11's `InputDevice { name, isDefault }` across the wire. No new type.

## Verification

**Static.** `npm run build --prefix frontend`, `npm run lint --prefix frontend`, `npm run test --prefix frontend`, `cargo test --workspace`.

Frontend tests: the dropdown renders the preview fixture's devices; selecting one calls `setSettings`; "System default" is present and distinguishable from a named device. Give the select an accessible name via a wrapping `<label>`, per phase 10.

**Runtime.** Via **control-ui**, on a machine with at least two real input devices. A one-device machine cannot detect the bug this feature exists to avoid.

1. Open Settings. Confirm the list matches `arecord -l` or `pactl list sources short`.
2. Select the non-default device. Confirm `config.json` changed.
3. Press Test while speaking. Confirm the level moves. Press it while silent. Confirm it does not.
4. Record through the GUI button and confirm the chosen device was used.
5. Record through a compositor-bound `echo-desktop rec --toggle` and confirm the same. Separate process, and this is where an env-only implementation would silently fall back.
6. Unplug the selected device with the app open. Reopen Settings, confirm the list updated and the row shows the fallback.
7. Screenshot both themes at 920x680 and attach to the PR.
