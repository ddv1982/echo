# Changelog

## v0.3.0-alpha.1

Third Linux alpha of Echo. Upgrades now reach you, and Echo tells you when they cannot.

- One version source: the workspace `Cargo.toml` drives the frontend, the packages, and the changelog gate. `echo-desktop --version` prints it.
- The release workflow validates the changelog before building and asserts the `.deb` and `.rpm` metadata match the workspace version, so an uninstallable build cannot be published.
- Echo runs as a single instance. A second launch focuses the window; if the binary on disk changed since the running process started, Echo restarts into the new build, so installing an upgrade and clicking the launcher is enough.
- Home warns when another `echo-desktop` on PATH shadows the running one, with the exact paths and the removal command. The packaged desktop entry launches `/usr/bin/echo-desktop` by absolute path.

## v0.2.0-alpha.1

Corrected release of the identity, language, model, HUD, and UI work first published as `v0.1.0-alpha.2`.

- Package metadata now reports version `0.2.0`, so Debian and RPM installers recognize this build as newer than `v0.1.0-alpha.1`.
- The in-app transparency panel now reports `0.2.0`.
- Includes the redesigned app and tray icons, Whisper model and language controls, guided downloads, real-level recording HUD, and desktop UI redesign from `v0.1.0-alpha.2`.

## v0.1.0-alpha.2

Second Linux alpha of Echo. A redesigned icon, Whisper model and language choice, guided model downloads, a truthful recording HUD, and a polished desktop window.

- New app icon, a matching dual-tone tray glyph with real alpha that reads on light and dark panels, and a GNOME symbolic icon, all regenerated from SVG masters.
- Whisper model catalog and picker: Echo scans the model directory, shows family, size, quantization, and multilingual capability, and reports what actually ran, including binary path, model path, VAD state, and inference time.
- Language selection across all 100 whisper.cpp languages plus auto-detect with a detected-language readout. Impossible combinations, like a non-English language on an English-only model, are refused before recording.
- Guided model downloads from inside Settings: four curated offers with size, source URL, SHA-1 verification, progress, and cancel.
- The recording HUD shows real microphone levels with broadcast-style smoothing, covers Recording, Transcribing, Done, and Failed states, and draws with per-pixel alpha when a compositor is present.
- The desktop window gains a brand lockup, a record hero with live levels, usage stats, a setup checklist, and day-grouped history.

## v0.1.0-alpha.1

First Linux alpha of Echo. Hold a key or press a toggle shortcut, speak, and cleaned text lands at the cursor. Audio stays on the machine.

- Desktop app with Home, History, Dictionary, Settings, and a tray icon.
- `echo-desktop rec --once`, `--toggle`, and `--hold` for compositor shortcuts and evdev hold-to-talk.
- X11 HUD capsule while recording. `ECHO_HUD=off` disables it.
- Ubuntu check workflow that builds the frontend, then clippy and tests.
- GitHub Release attaches `.deb`, `.rpm`, and the loose `echo-desktop` binary. AppImage is a best-effort job and is not attached to the Release.
