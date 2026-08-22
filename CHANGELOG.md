# Changelog

## v0.4.1

Release delivery hotfix for `v0.4.0`. The Linux shortcut and recording HUD improvements are unchanged; this release makes their packages reproducible, verified, and safely published.

- Tauri frontend hooks now declare their working directory explicitly, so local builds and GitHub package builds resolve the same frontend instead of escaping the checkout.
- Pull requests and the exact merged `main` commit build real Debian and RPM packages before a tag can be created; tags must match the workspace version, include changelog notes, and point to `main`.
- Release publishing now runs with isolated write permission only after exactly one `.deb` and one `.rpm` pass embedded-version checks and all required artifacts are available.
- The AppImage now launches its bundled `echo-desktop` binary instead of looking for `/usr/bin/echo-desktop`, and every AppImage must execute and report the expected version before upload.
- Official GitHub actions and the Tauri CLI are pinned to known versions, concurrent stale builds are cancelled, and the maintainer release runbook documents preparation, publication, verification, and failure recovery.

## v0.4.0

Linux shortcuts are now configurable, source-aware, and resilient across modern Wayland, X11, and older GNOME sessions.

- Toggle and push-to-talk shortcuts support canonical multi-key chords, environment overrides, persisted settings, capture/reset controls, and effective-trigger reporting.
- Echo uses the GlobalShortcuts portal when available and native X11 grabs otherwise, with explicit conflict and registration errors instead of silent fallback claims.
- GNOME releases without the portal get an explicit, ownership-checked setup and repair action for the Echo custom toggle shortcut; startup and status polling never change desktop settings.
- Push-to-talk prefers native desktop shortcuts and falls back to a chord-aware evdev supervisor that handles multiple keyboards, hotplug, reconnect, cancellation, permission denial, and listener failure without granting privileges.
- Advanced Settings identifies toggle and push-to-talk sources independently, and Test shortcut accepts only a successful action from the configured shortcut command path.
- The recording HUD is smaller and its premultiplied ARGB edges render cleanly without a pale fringe.
- CI now includes a release build alongside frontend, test, and lint gates.

## v0.4.0-alpha.1

Fourth Linux alpha of Echo. Your language back by default, hotkeys you can trust, and a Settings view for humans.

- With a multilingual model, Echo detects the language automatically instead of defaulting to English. Pin a language in Settings or with `ECHO_LANGUAGE`; after a confident detection, Settings offers to pin it in one click. English-only models pin English, and impossible combinations are still refused before recording.
- A failed hotkey session now tells you: desktop notifications name the failure and the fix for shortcut-spawned sessions, instead of silence in the journal.
- Hold-to-talk works without a terminal: the desktop app listens for the hold key itself while it runs, and the Settings row says when input-group access is missing.
- The shortcut checklist item is verified, not asserted: a Test shortcut flow confirms your binding by watching a real session start.
- Tidy-up: plan documents carry status headers, the Fake test engine left the shipping selector, unreferenced tray rasters and dead dictionary hit-tracking are gone, and the README matches the app.
- Settings regroup into a short General surface (Microphone, Language, Model quality, Push-to-talk key, Shortcut, Theme) with an Advanced disclosure for the engine override, transparency readout, and the rest.

## v0.3.0-alpha.1

Third Linux alpha of Echo. Upgrades now reach you, and Echo tells you when they cannot.

- One version source: the workspace `Cargo.toml` drives the frontend, the packages, and the changelog gate. `echo-desktop --version` prints it.
- The release workflow validates the changelog before building and asserts the `.deb` and `.rpm` metadata match the workspace version, so an uninstallable build cannot be published.
- Echo runs as a single instance. A second launch focuses the window; if the binary on disk changed since the running process started, Echo restarts into the new build, so installing an upgrade and clicking the launcher is enough. Echo builds from before the single-instance gate are terminated at startup, with recorders mid-dictation left alone.
- Home warns when another `echo-desktop` on PATH shadows the running one, and the warning's Remove old copies button deletes them in place, including the user-local desktop entry and icons. The packaged desktop entry launches `/usr/bin/echo-desktop` by absolute path.

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
