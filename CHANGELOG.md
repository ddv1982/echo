# Changelog

## v0.14.15

- Dictionary replacements now preserve shorter valid matches when a longer overlapping phrase takes priority. Adding unrelated dictionary entries no longer changes which replacements apply.

## v0.14.14

- Echo now retains its native Linux tray menu for the desktop process lifetime, preventing Open, recording, Language, and Quit entries from disappearing.
- The tray now includes a language selector synchronized with Settings. It reflects the active engine's available language choices and keeps environment-controlled or fixed language modes read-only.

## v0.14.13

- Desktop status polling, Settings, setup, History, Dictionary, shortcut recovery, stale-install removal, and voice-training startup now keep blocking filesystem and device work off the async runtime. Cached health and last-run details refresh when their inputs change without making routine polling expensive.
- Echo now rejects unsafe temporary-directory fallbacks for private data and managed executables. History and Dictionary updates are failure-atomic across processes, and recording sessions recover safely from supported legacy and poisoned-lock states.
- Text injection no longer retries globally, treats leading hyphens as text, uses consistent desktop-session detection, and restores the previous clipboard after a confirmed paste. Failed injection keeps the transcript recoverable.
- Integer WAV input is normalized by its real bit depth, spoken text such as `(yes)` and `*hello*` is no longer removed as non-speech, audio peaks stay bounded, and dropped capture samples are reported.
- Settings, setup, shortcut verification, microphone tests, Dictionary training, and copy feedback now reject stale post-unmount results. Keyboard focus, reduced motion, pressed-state semantics, contrast, midnight statistics, and mobile layouts are preserved across affected views.
- Managed downloads and speech-engine failures are bounded and cancellable, model sources are pinned to immutable revisions, and releases now include maintained third-party attribution plus managed runtime and model entries in the CycloneDX SBOM.

## v0.14.12

- The X11 recording HUD now uses a compact waveform capsule with distinct recording, transcribing, success, and failure indicators, plus compositor-aware translucency, highlighting, and shadow treatment.
- Success and failure indicators now remain visible for their intended terminal-state duration, and composited shadows no longer clip at the window edge.

## v0.14.11

- Recording, toggle, voice-training, and upgrade takeover sessions now share one cross-process lease. Upgrade replacement waits for the existing desktop process to exit, closing races that could start overlapping sessions or lose the replacement window.
- Private configuration and data reads now repair owner-only permissions, reject symlinks, and preserve every corrupt-file backup. Runtime configuration errors are reported instead of silently falling back to defaults.
- Personal Dictionary matching now handles Unicode canonical equivalence and full case folding, including length-changing folds, while preserving the original transcript ranges.
- Session status now distinguishes application phases and validates process identity across PID reuse. Failed text injection refreshes History reliably, and local-day statistics remain correct across daylight-saving transitions.
- X11 injection now pins the window focused when audio capture ends, so Home-button recordings can target the application selected while speaking without allowing later focus changes to redirect the transcript.

## v0.14.10

- Echo's private configuration and data storage is now owner-only and symlink-resistant. Atomic file replacement preserves the previous contents if a write is interrupted.
- Temporary speech WAVs now use unique owner-only files that are removed automatically on success and failure instead of predictable paths that could leak audio or leave stale files behind.
- X11 text insertion is restricted to the window captured for the session. A failed targeted type or paste no longer falls back to dispatching input globally.
- Upgrade takeover now proceeds only while recording is idle across local and cross-process session gates. Active or concurrent recordings defer restart, and a failed replacement launch reopens recording.
- Speech-engine execution is now bounded by a shared deadline and cancellable. Whisper and Parakeet run in isolated process groups so timeout or cancellation terminates descendants and reaps the direct child.
- Personal Dictionary replacements now match case-insensitively across Unicode text, including accented, Greek, and Cyrillic input, while respecting Unicode word boundaries and combining marks.
- Settings read failures now block mutations, while Dictionary and History failures produce actionable status without discarding recoverable transcript text or presenting failed writes as durable.
- History timestamps now record when audio capture starts rather than when transcription finishes, keeping long recordings on their correct time and calendar day.

## v0.14.9

- Rust cancellation and recording-stop tests now use causal synchronization instead of scheduler-sensitive internal watchdogs.
- GitHub Actions, Cargo, and frontend dependencies were refreshed, including notify-rust 4.18, Lucide 1.x, and TypeScript 7.
- Builds now require Rust 1.89. The frontend uses TypeScript 7 for strict full-project typechecking and Oxlint's native type-aware rules, replacing the legacy ESLint and TypeScript 6 compatibility bridge.
- Setup completion refreshes are exhaustive and processed in order, while failed frontend operations are handled without unowned rejections or duplicate error reports.
- Shortcut verification now expires after 30 days, reacts when the active shortcut identity becomes available, and reliably stops attributed recordings after a timeout or unmount.

## v0.14.8

- Rust cancellation and recording-stop tests are now deterministic.
- Confirmed stale suppressions and imports, plus duplicate frontend helpers, were removed without behavior changes.
- Ruff script linting is pinned for reproducible maintenance checks.
- Weekly Actions, Cargo, and npm Dependabot updates are bounded, with Cargo and npm lockfiles audited.
- Completed plan-18 execution artifacts were retired while preserving maintained managed-integrity documentation.

## v0.14.7

- Echo now has a visible **Quit Echo** action that exits the process without relying on native titlebar controls. Restoring a tray-hidden window on Linux Wayland also repairs unresponsive titlebar controls without changing X11 or close-to-tray behavior.
- History can now permanently delete one saved transcript or clear all saved transcripts after confirmation. Deletions persist across reloads and immediately update shared history and usage views.

## v0.14.6

- Dictionary now has **Teach by voice**. Enter the exact text, record five takes with the selected transcription model, review what the model heard, and save the useful variants together. Manual entries remain available.
- Echo keeps training audio local and temporary. Training bypasses existing dictionary corrections, does not add entries to history, and cannot overlap normal dictation. Batch saves reject conflicts without partial changes.

## v0.14.5

- Echo no longer applies English-only cleanup rules after Whisper or Parakeet. The Cleanup setting, `ECHO_CLEANUP`, and external cleanup commands are gone.
- The personal Dictionary still provides Whisper recognition hints and applies replacements after recognition. `--raw` prints the engine transcript before those replacements, and legacy cleanup settings disappear the next time Echo saves the configuration.

## v0.14.4

- Settings now groups controls by task instead of hiding engine and GPU choices in a top-level Advanced drawer. The Transcription section distinguishes the saved preference, the resolved next transcription, and the processing used by the previous transcription.
- A saved GPU preference stays dormant when the next transcription resolves to Parakeet. Echo does not probe devices or offer a Whisper runtime download until Whisper applies. The explicit **Use Whisper with GPU** action changes both preferences together and stays unavailable when a conflicting environment override controls either setting.
- An unavailable Whisper run keeps its model picker. A missing saved model remains visible as not installed, and an incompatible English-only model can be replaced without editing the configuration file.

## v0.14.3

- Closing Echo now remains responsive while Settings collects readiness or detects Vulkan devices. Those blocking probes run outside GTK's native window-event path without changing GPU selection, explicit Detect refresh, or close-to-tray behavior.
- On the affected GPU-selected sequence, automatic Settings close settled at 107 to 109 ms. Advanced, scroll, Detect, then close settled at 108 to 117 ms. The previous healthy-host samples reached 640 ms, while a hung Vulkan probe could block for much longer.

## v0.14.2

- Status checks no longer rebuild the speech runtime inventory on every 400 ms poll. The existing ten-second health snapshot now caches the language warning and refreshes it after settings or setup changes.
- In matched release WebView measurements with existing user data, warm status p95 fell from 20 ms to 1 ms. The no-op and fixed-payload controls remained at 1 ms p95.

## v0.14.1

- The frontend now reads `workspace.package.version` from the correct Cargo manifest field. The Rust toolchain version can no longer replace the Echo version in frontend build metadata.
- Maintainers can measure cold and warm status-call latency through a release WebView. The probe is excluded from normal builds, and committed baselines separate Tauri overhead from backend discovery and presentation costs.

## v0.14.0

- Managed components no longer trust a persistent verification marker. Echo validates strong file identity in each process, rehashes after metadata changes, and always hashes during an explicit Verify.
- Status polling, setup subscriptions, and settings writes are serialized. React Strict Mode no longer starts duplicate polls, and stale setup results cannot replace a newer setting.
- Rust now owns the generated desktop IPC contract. CI rejects command, event, payload, and TypeScript drift before merge.
- The desktop, shortcut, installer, and frontend feature boundaries are smaller and independently tested without changing command names or saved settings.
- Echo is licensed under the MIT License. Releases contain the raw binary, Debian package, RPM, AppImage, MIT license, a CycloneDX SBOM, and `SHA256SUMS`; CI verifies the exact set and records build-provenance attestations.
- Public documentation now leads with installation and first dictation. Retired plans and raw QA evidence moved out of the active source tree behind a reproducible archive manifest.

## v0.13.0

- Whisper acceleration is now a choice between CPU and GPU, and defaults to CPU. The Auto mode is gone: it only ever accelerated when a packaged qualification matched the exact host, so on nearly every machine it silently meant CPU. Saved `auto` values load as CPU.
- Selecting GPU shows every Vulkan device on the machine and runs on the one picked, pinned by its device and driver UUID pair so it survives reordering. A single-device machine needs no choice.
- The GPU runtime is a managed component downloaded on demand rather than bundled, so a user who stays on CPU never pays the 19 MB, and acceleration no longer depends on the package format the build came from. AppImage can now accelerate.
- The Advanced acceleration readout names the device that ran and, when GPU was asked for and CPU ran, says why: no runtime installed, no device found, the pinned device absent, the device disabled after a failure, the managed CPU runtime it falls back to missing, or a GPU run that failed and retried on CPU.
- Tagged releases open every published deb and rpm and check it carries the binary CI built, instead of inspecting a hand-staged draft. v0.12.6 shipped with no acceleration payload because the only check that ran on a tag looked at the wrong artefact.
- The Advanced last-run readout keeps the resolved engine, last run, acceleration, version, and config path, and drops the model file, binary, multilingual, VAD, Whisper mode, runtime, timing split, decoding, and attempt rows.
- Segmented controls in Settings no longer shrink below their labels, which pushed the Whisper acceleration CPU button outside the card above 920px.

## v0.12.6

- Unset Whisper acceleration is Auto. Auto uses a receipt-verified local Vulkan device when one enumerates, otherwise managed CPU. CPU remains an explicit opt-out.
- Automatic language and recognition hints run on the same backend as the rest of the decode.
- Local GPU selection pins the device by UUID, requires an exact Vulkan receipt, quarantines a failed identity for 24 hours, and recovers once on CPU.
- Version tags publish Debian, RPM, and echo-desktop when no `qualification-$commit` draft exists.

## v0.12.5

- Unset Whisper acceleration is Auto. Auto uses a receipt-verified local Vulkan device when one enumerates, otherwise managed CPU. CPU remains an explicit opt-out.
- Automatic language and recognition hints run on the same backend as the rest of the decode.
- Local GPU selection pins the device by UUID, requires an exact Vulkan receipt, quarantines a failed identity for 24 hours, and recovers once on CPU.

## v0.12.4

- Linux packages can accelerate both Whisper Small and Large v3 Turbo when the executable, model, runtime, Vulkan device, driver, decoding policy, and cache seed match their independently measured admission. Other identities stay on managed CPU.
- Switching to a selected PipeWire microphone no longer discards a valid recording when the audio backend reports a disconnect during intentional stream shutdown.
- Qualified releases compose exact Small and Large Turbo admissions into one schema-v2 package with a shared Vulkan runtime and identity-keyed cache seeds.
- Promotion and release verification now reject resource-gate failures, duplicate identities, incompatible binaries or runtimes, and any missing, extra, changed, or type-shifted packaged file.

## v0.12.3

- New recommended setup installs and pins Whisper Small, matching the model qualified for Vulkan acceleration. Existing explicit model and language choices remain unchanged, and automatic language detection stays on managed CPU until separately qualified.

## v0.12.2

- Linux packages now ship qualified Whisper Vulkan acceleration only when the packaged admission exactly matches the executable, runtime, model, VAD, decoding policy, DRM device, ICD files, and seeded Mesa cache.
- Automatic language detection, non-empty recognition hints, missing or changed identities, and quarantined accelerators stay on managed CPU. A qualified GPU failure performs one same-model CPU retry.
- Debian and RPM releases carry independently measured executables, root-owned acceleration resources, and package-specific admissions. Release verification re-extracts each package before publication.
- RPM verification falls back to a pinned 7z path when Ubuntu `rpm2cpio` emits a valid archive but exits nonzero.

## v0.12.1

- Linux packages now ship qualified Whisper Vulkan acceleration only when the packaged admission exactly matches the executable, runtime, model, VAD, decoding policy, DRM device, ICD files, and seeded Mesa cache.
- Automatic language detection, non-empty recognition hints, missing or changed identities, and quarantined accelerators stay on managed CPU. A qualified GPU failure performs one same-model CPU retry.
- Debian and RPM releases carry independently measured executables, root-owned acceleration resources, and package-specific admissions. Release verification re-extracts each package before publication.
- The tagged release job can read its private commit-specific qualification draft, while publication permission remains isolated to release jobs.

## v0.12.0

- Linux packages can now ship a qualified Whisper Vulkan accelerator as a root-owned `whisper-acceleration` resource instead of relying on an ambient system runtime.
- Echo selects GPU transcription only when the packaged admission record exactly matches the current executable, Whisper runtime, model, VAD, decoding policy, DRM device, ICD manifest and library, and seeded Mesa cache. Any missing, changed, expired, stopped, or quarantined identity stays on managed CPU.
- A qualified accelerated failure now quarantines only that exact identity and performs one same-model managed CPU logical retry. Automatic language detection and non-empty recognition hints stay on managed CPU until they pass paired qualification.
- Release tooling now stages Debian and RPM specific qualified executables, admissions, runtimes, and cache seeds, then verifies the extracted package identities again before a tagged release can publish them.

## v0.11.0

- Whisper runs now report separate WAV encoding, child-process, parsing, runtime, backend, decoding, and attempt detail while keeping the existing `inferMs` boundary compatible with old history and CLI consumers.
- One typed execution plan owns the selected Whisper runtime, model, VAD, protocol, and any explicit decoding overrides. Normal runs preserve the runtime's own tuning defaults, managed CPU keeps its existing precedence, and system or manually imported assets remain supported.
- The file CLI adds unsaved Whisper tuning overrides for reproducible experiments. The benchmark records outer wall time, artifact hashes, host identity, seeds, warmups, randomized candidate order, resolved tuning, and every VAD retry.
- New managed Whisper installs include the matching `whisper-server` from the already verified upstream archive. Existing one-shot installations remain valid without repair. A separate loopback-only probe measures model load, first request, warm requests, memory, and cleanup without enabling resident dictation before it passes the quality and latency gates.
- Advanced diagnostics show the actual cold path, runtime source, backend, split timing, decoding values, and VAD retries. General Settings gains no performance knobs.
- Echo retries without VAD only when Whisper reports a VAD model or context failure, failed VAD computation, or an exact unsupported VAD flag. Decoder and model failures now preserve their original error instead of paying for an unrelated second inference.

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
