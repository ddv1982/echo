# Auto language by default, a tidy repo, and settings for humans

## Context

Three requests from the user, each traced to code.

**"When I speak a different language it still makes it back into English."** A real defect with a named cause. With a multilingual model installed and no language configured, Echo passes `-l en` to whisper-cli. The resolution chain: `Config.language` is `None` (`crates/echo-core/src/config.rs:49`), `resolved_language` falls back to `LanguageChoice::default()` (`crates/echo/src/stt/mod.rs:42-48`), and that default is `Pinned(Language::ENGLISH)` (`crates/echo-core/src/language.rs:63-66`). `whisper_args` then emits `-l en` unconditionally (`crates/echo/src/stt/whisper.rs:212-236`). The settings IPC projects the same default into the UI as `"en"` (`src-tauri/src/main.rs:694`). This was a deliberate choice: plan 03 phase 17 pinned by default for latency and tail accuracy, with auto as an opt-in. The user has now explicitly asked for auto. The measured result of the current default, reproduced below, is that a Dutch speaker's words come back as confident English-sounding gibberish with exit 0 and no warning.

**"Tidy up a bit, get rid of what we no longer need."** Four plans have shipped into the repo. The sweep findings, each with evidence:

- The plan folders carry no status. Plans 01, 02, and 04 shipped in full. Plan 03 shipped except phase 16 (the sherpa-onnx stdout parse fix; `crates/echo/src/stt/parakeet.rs:79` still trims whole stdout) and phase 20 (the parakeet-cli bakeoff), both explicitly left open by plan 04 and both still valid.
- The Fake engine is a user-facing option in the Settings engine selector (`frontend/src/App.tsx:693-697`) and is always listed as available (`crates/echo/src/stt/mod.rs:221`). It exists for smoke tests (`README.md:89`); a test-only engine in a shipping UI is exactly what the user means.
- `xtask` generates four tray rasters but only `tray-24.png` is consumed (`src-tauri/tauri.conf.json:30`, `src-tauri/src/main.rs:932`). `tray-22.png`, `tray-32.png`, and `tray-48.png` are generated and drift-checked but referenced by nothing.
- `Rewrite.hits` and `DictHit` are dead outside their own test: the only reader of `.hits` is `crates/echo-core/src/cleanup.rs:180`; nothing in `crates/echo` or `src-tauri` reads it.
- The README still says "Echo does not download models or engine binaries" (`README.md:84`), false since guided downloads shipped, and still tells users to hand-place `ggml-base.en.bin` (`README.md:86`) instead of pointing at the in-app offers.
- `frontend/package.json` is frozen at 0.0.0 and nothing user-visible renders it; the version shown in Settings comes from `CARGO_PKG_VERSION` through IPC. Nothing to do.
- The C2PA `echo.png` raster is already gone from `assets/`; only the three current masters remain. Nothing to do.
- Dependency review (manual, per crate): every dependency in all five manifests is used. `serde_json` is not a dependency of `src-tauri` and is not needed there. Nothing to remove.
- Three of plan 03's four hand-synced pairs survive: `resolve_engine`/`engine_summary` (`crates/echo/src/stt/mod.rs:142,150`), `from_env`/`mode_name` (`crates/echo/src/cleanup/mod.rs:40,63`), and `hud_disabled`/`enabled` (`crates/echo/src/ui/hud.rs:123,133`). All three carry agreement tests. The fourth (the two `default_input_device` lookups) is already consolidated to one site (`crates/echo/src/audio.rs:211`). The survivors are small and tested; the tidy answer is to keep them and say so, not to merge them for its own sake.

**"Apple's principles of ease of use and minimalism" for Settings.** The current view (evidence/settings-light-top.png through the sections, both themes) groups by implementation, not by intent. Concretely: env-var names appear as user-facing hint text (`ECHO_ENGINE`, `ECHO_LANGUAGE`); "Fake" is an engine choice; "Hold key" and "Timed recording" document `rec --hold` and `rec --once`, CLI concepts most GUI users never touch; the transparency readout shows absolute file paths, "VAD", and "Multilingual" as jargon rows; the settings path footer shows the config file location; and the suggested shortcut appears twice (sidebar card and a Settings row). The two decisions a normal user actually has — which language, which model quality — sit at the same visual weight as all of this.

## Research

Verified against live sources and local runs in August 2026.

**The language defect, measured.** whisper-cli built from ggml-org/whisper.cpp master (commit 233fe1f), model `ggml-tiny-q5_1.bin` (31 MiB, SHA-1 `2827a03e495b1ed3048ef28a6a4620537db4ee51`), input synthesized with espeak-ng and resampled to 16 kHz mono. The Dutch sentence is "Dit is een test van de spraakherkenning. Ik spreek Nederlands en geen Engels."; the German one is "Das ist ein Test der Spracherkennung auf Deutsch."

| Input | Flag | Output | Exit |
| --- | --- | --- | --- |
| Dutch | `-l en` | "It is an testman that's cracked at getting X-ray, K-N-A-Landz, and J-N-A-N-L." | 0 |
| Dutch | `-l auto` | same English garble; stderr: `auto-detected language: en (p = 0.628587)` | 0 |
| Dutch | `-l nl` | "Het is een test van de straat erkenning, ik spreek na de land en geïnne enkels." | 0 |
| German | `-l en` | "That is an test match that can end up noise." | 0 |
| German | `-l auto` | "Das ist ein Test des Dracherkend und auf Deutsch." | 0 |
| German | `-l de` | same German | 0 |

Through Echo end to end (`rec --once`, the Dutch fixture, `ggml-tiny-q5_1` in `ECHO_MODEL_DIR`, real whisper-cli on PATH): default config produces the English garble with `language: en` in the run detail; `ECHO_LANGUAGE=auto` produces the same garble with `language: en, p = 0.629` persisted; `ECHO_LANGUAGE=nl` produces Dutch. The current default is the defect, reproduced.

Two nuances the design must respect. Auto is not free: on tiny, the 4.8 s Dutch clip took 720 ms pinned versus 933 ms auto (the extra encoder pass plan 03 measured at 2287 → 2610 ms on base; the gap scales with model size). And auto is not magic on tiny: it misdetected the robotic Dutch as English at p = 0.63, matching plan 03's Fleurs measurements (~45% for tiny, ~65% for large-v2). German detected correctly. The detected-language chip already renders low confidence differently, which is the mitigation surface; the pin suggestion below is the other.

**Apple's principles, applied to settings.** Sensible defaults so most people never open Settings; progressive disclosure (a simple surface, advanced behind a click); group by user intent, not implementation; plain language without jargon; no choices that do not change outcomes. The engine override is the clearest case: Auto already picks the right engine, so Whisper-versus-Parakeet is an implementation detail for nearly everyone.

## Scope

**Included**

- Auto language detection by default when the resolved model is multilingual, pinned English as the implicit choice for `.en` models, explicit pinning preserved, and a one-click "pin the detected language" suggestion after confident auto runs.
- The tidy-up inventory above: plan status headers, the Fake engine out of the shipping selector, the unreferenced tray rasters, the dead `Rewrite.hits`, and the README corrections.
- A Settings redesign in two tiers: a short General surface (Microphone, Language, Model quality, Theme) and an Advanced disclosure (engine override, hold key, timed recording, cleanup, HUD toggle, the transparency readout, env overrides, config path). Same grayscale design language; this is IA and copy, not a re-theme.

**Excluded**

- Plan 03 phases 16 and 20 themselves. They stay open as named work; this plan only marks status.
- A searchable language combobox, again. The grouped native select stays.
- Translation (`--task translate`), diarization, non-English cleanup rules, Wayland HUD, Windows, macOS. Unchanged from plan 04.
- Removing the Fake engine from the codebase. It stays for CLI smoke tests and the test suite; it leaves the shipping selector.

## Constraints

- The release gate requires a fresh native version per alpha; this plan targets **0.4.0-alpha.1**.
- Auto detection runs on the first 30-second window only and the result applies to the whole file; there is no per-window re-detection (plan 03 phase 17, still true upstream).
- `.en` models reset `-l` to English and exit 0. The refusal before spawning stays; under the new default it must stay coherent (`.en` + unset resolves to pinned English, which runs).
- Env vars keep winning over the config file; the test suite depends on it.
- The cleanup rules gate on the *detected* language for auto runs, which plan 04 phase 4 already built (`permits_english_rules` takes the detected code); the new default must route through it, not around it.
- Each phase is independently shippable; no phase leaves a control visible that does nothing.

## Alternatives

**Language default.** Keep pinned English: rejected; it is the defect. Auto unconditionally, even for `.en` models: rejected; `.en` models cannot detect, and refusing the default state would break every English-only install. **Model-aware default: Auto when the resolved model is multilingual, pinned English for `.en`, explicit choice always wins, chosen.** The latency cost is real and bounded (one extra encoder pass); the pin suggestion after a confident detection gives frequent one-language users the fast path back.

**The Fake engine.** Delete it: rejected; it is the smoke-test engine and the test suite's mutation target. Keep it in the selector: rejected; it is a test-only engine in a shipping UI. **Hide it unless `ECHO_ENGINE=fake` is already set or `ECHO_SHOW_FAKE` is on, chosen.** The frontend tests that click it migrate to another mutation target.

**Settings structure.** A full re-theme or a tabbed layout: rejected; the grayscale language is right, and tabs hide as much as they organize. **One page, General plus an Advanced disclosure, chosen.** Progressive disclosure without a second navigation concept.

## Applicable skills

The implementer must invoke these by name:

- **control-ui** (from `cursor-team-kit`) for every phase touching the webview, with screenshots at 920x680 in both themes attached to each PR.
- **control-cli** (from `cursor-team-kit`) for the language phases' `rec --once` checks.
- **interrogate** before shipping phase 1. The language default is user-felt and the auto tail is genuinely worse for some languages; the trade deserves adversarial review.
- **technical-writing** (`/technical-writing`) for the README corrections and every PR description.
- **unslop** over every prose surface. **deslop** (`/deslop`) over each diff before commit. **no-comments** (`/no-comments`) before review.
- **show-me-your-work** across the program.
- Cursor's built-in **babysit** skill after opening each PR.

## Phases

1. [Phase 1: auto by default](phase-1-auto-language.md)
2. [Phase 2: tidy-up](phase-2-tidy-up.md)
3. [Phase 3: settings for humans](phase-3-settings-ia.md)

Verification detail per phase lives in [testing.md](testing.md).

## Verification

Project-level commands, run per phase and at completion. The frontend build is first because every cargo command depends on it:

```sh
npm ci --prefix frontend
npm run build --prefix frontend
npm run test --prefix frontend
npm run lint --prefix frontend
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

CI runs this list plus the icon drift check, so a green PR is the project-level check.

## Implementation guidance

- Branches follow `cursor/<descriptive-name>-8122`. One phase per PR, in order.
- Do not carry compatibility shims between phases. When a phase replaces a pattern, delete the old one in the same diff.
- Attach evidence to every PR. Phase 1 attaches the before/after transcript of a non-English fixture through `rec --once`. Phase 2's diffs are their own evidence. Phase 3 attaches screenshots at 920x680 in both themes.
- The empirical matrix above is reproducible: whisper-cli from upstream master, `ggml-tiny-q5_1.bin`, espeak-ng fixtures resampled to 16 kHz mono.
