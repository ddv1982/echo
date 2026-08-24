# Changelog

## v0.10.0

- Recommended setup now installs Large v3 Turbo Q5_0 on machines with at least 8 GiB RAM. The 547 MiB quantized model replaces Small as the normal high-quality multilingual choice, while Base Q5_1 remains the low-memory fallback and existing Small or manual models stay supported.
- Settings uses one Speech model row: Whisper exposes installed choices with honest quality guidance, while Parakeet shows its fixed TDT 0.6B v3 model and automatic 25-language capability instead of making the control disappear.
- Parakeet now parses the pinned sherpa-onnx JSON protocol, passes only transcript text to cleanup and insertion, reports its model path, and uses the required NeMo transducer model type.
- Linux clipboard fallback leaves the dictated text available after paste instead of racing the target by immediately restoring old clipboard contents. Wayland clipboard tools are preferred in Wayland sessions and direct typing remains clipboard-free.
- A manifest-driven benchmark runs installed speech candidates through the shipping CLI, fails on missing or broken candidates, and reports per-language WER, real-time factor, and silence hallucinations in JSON Lines and Markdown.

## v0.9.1

- Managed Whisper setup now installs the pinned 1.9.2 runtime even when its shared-library symlinks appear before their targets in the archive.
- Extraction validates the complete selected symlink graph, including cycles, missing targets, escapes, and flattened destination mismatches, before any staged link can reach activation.
- CI downloads the exact pinned Whisper archive and drives it through digest verification, extraction, payload verification, the real Linux runtime probe, immutable activation, receipt checks, and post-install Verify.

## v0.9.0

- Echo now discovers Linux microphones through native PipeWire or PulseAudio before falling back to ALSA, so Bluetooth, USB, and built-in sources use the same recognizable names exposed by the desktop sound server.
- The normal microphone picker shows the Linux system default and primary input sources. Playback sinks, ALSA plugins, aliases, resamplers, and raw endpoint IDs remain available under Advanced audio endpoints.
- Speech setup is now one compact readiness card. Installed component paths and maintenance actions, alternative models, and inactive engine plans start collapsed without removing repair, verification, removal, system-runtime, or manual-model support.
- Settings now adapts cleanly at the 760-pixel minimum and across the navigation breakpoint. Pinned Chromium tests check eight widths in both themes for horizontal overflow and closed disclosures.
- Debian and RPM packages declare the PipeWire and PulseAudio runtime libraries, and release CI inspects the generated dependency metadata before publication.

## v0.8.0

- Recording length is now a visible General setting shared by timed, button, tray, CLI, and shortcut capture, with 30-second, 1-minute, 2-minute, 5-minute, and 10-minute choices. Ten minutes is the default and ceiling.
- Active sessions snapshot their limit, Home shows that value while recording, preview behavior matches the backend, and existing `record_seconds` config plus `ECHO_RECORD_SECONDS` overrides remain supported.
- Ten-minute capture avoids the previous native-sample clone and full mono intermediate buffer while preserving exact conversion output on tested mono, stereo, and multichannel inputs.
- Shortcut verification always cleans up its test recording, and token-scoped stop requests cannot cancel a replacement session. Fixture capture now obeys the same limit and cancellation contract as live capture.

## v0.7.0

- Microphones now use CPAL stable device IDs, keep equal labels distinct, show available metadata, preserve disconnected choices with an explicit fallback, and test the exact selected input.
- Linux x86_64 users can install complete Whisper or Parakeet setups inside Echo. Managed components use resumable downloads, SHA-256, bounded archive extraction, immutable activation records, Verify, Repair, and managed-only removal.
- System runtimes and existing cache files remain external, visible, and untouched. Healthy managed components take precedence while corrupt managed components fall back to those external inputs.
- Recommended setup chooses a multilingual Whisper model from detected memory and installs its runtime and VAD. Downloads expose cumulative disk needs, progress, cancellation, resume, retry, repair, verification, and removal without activating partial or corrupt files.

## v0.6.0

- `echo-desktop transcribe FILE.wav` now writes clean text, raw text, or schema-versioned JSON to stdout or an exact output path without starting recorder or desktop side effects.
- One prepared transcription request now resolves the engine, Whisper model, language, cleanup mode, and bounded dictionary recognition hints for both microphone and file runs.
- `echo-desktop languages` reports model-aware Whisper languages and Parakeet's 25 automatic-only languages in text or JSON.
- Engine, model, and language precedence is source-aware, failed inference processes cannot leak partial output, and microphone cleanup retains its dictionary-only fallback.

## v0.5.0

- Echo now uses one fixed Super+Alt+Space toggle across the desktop portal, X11, GNOME setup, and manual compositor setup. Push-to-talk, raw-input fallback, shortcut customization, and the `rec --hold` command have been removed.
- Shortcut setup is reported through one typed status, remains available when unrelated Settings probes fail, supports explicit retry, and verifies activations against the effective binding.

## v0.4.2

Publication-path hotfix for the fully verified `v0.4.1` artifacts. The application changes and package contents remain the same.

- Release-candidate checks now download the staged Debian, RPM, and binary artifacts on every pull request and `main` build, then verify the exact directory layout consumed by the publisher.
- The GitHub Release publisher follows the artifact service's preserved `deb/` and `rpm/` subdirectories, preventing a valid tagged build from failing at its final attachment step.
- The failed `v0.4.1` tag run remains visible as an audit record; `v0.4.2` is the first release published entirely by the hardened workflow.

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
