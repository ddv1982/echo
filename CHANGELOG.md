# Changelog

## v0.1.0-alpha.1

First Linux alpha of Echo. Hold a key or press a toggle shortcut, speak, and cleaned text lands at the cursor. Audio stays on the machine.

- Desktop app with Home, History, Dictionary, Settings, and a tray icon.
- `echo-desktop rec --once`, `--toggle`, and `--hold` for compositor shortcuts and evdev hold-to-talk.
- X11 HUD capsule while recording. `ECHO_HUD=off` disables it.
- Ubuntu check workflow that builds the frontend, then clippy and tests.
- GitHub Release attaches `.deb`, `.rpm`, and the loose `echo-desktop` binary. AppImage is a best-effort job and is not attached to the Release.
