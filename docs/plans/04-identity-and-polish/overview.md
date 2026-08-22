# Identity and polish: a better mark, every Whisper model, 100 languages, a HUD worth watching

## Context

Four requests from the user, each traced to code:

**The app icon should look better, and the tray should show the same mark with alpha.** The plumbing for this is already good. Plan 03 phases 5 and 6 built SVG masters, an `xtask` generator that renders every raster with resvg, RGBA output, transparent corners, a drift check in CI (`.github/workflows/check.yml`), and one icon name across bundle, tray, desktop entry, and favicon. What never got redesigned is the art itself. Phase 5 explicitly kept the 2019-era geometry: "It is good at 128 px and above and there is no reason to change it" (`docs/plans/03-settings-and-delivery/phase-5-icon-master.md:24`). The user now says otherwise. The tray glyph is a second problem. `assets/icons/echo-tray.svg` is three bars in `#f3efe6`, near-white, on a transparent ground. On a light panel it is close to invisible, and it is not the same mark as the app icon (three bars versus five plus a dot). There is also a dead file: `assets/icons/echo.svg` is referenced by nothing since phase 6.

**Other Whisper models and more than English.** Fully specified in plan 03 and never built. Phases 14, 15, 17, 18, and 19 are written, reviewed, and unmerged. The code still matches that plan's description: `const DEFAULT_MODEL: &str = "base.en"` (`crates/echo/src/stt/whisper.rs:16`), `whisper_args` passes no `-l` flag at all (`crates/echo/src/stt/whisper.rs:126-143`), `Config` has no `language` field (`crates/echo-core/src/config.rs:32-47`), and the Settings view has no model or language row. This plan adopts those five phase documents rather than rewriting them, with amendments where upstream facts have moved.

**A prettier UX/UI.** Plan 02 landed in full: grayscale tokens, bundled Inter Variable, redesigned shell and views, motion pass. What the user is asking for now is the next layer, the difference between minimal and designed. Today the Home view is a text panel with a button, the brand lockup is the word "Echo" in plain text (`frontend/src/App.tsx:161-164`), there are no usage stats, no onboarding beyond an attention strip, and empty states are bare text.

**A better looking recording HUD.** The capsule is palette-aligned since plan 02 phase 11, but it is still a 220x56 X11 window with hard 1-bit SHAPE edges, a pulsing dot, and thirteen bars driven by a sine wave that knows nothing about the microphone (`crates/echo/src/ui/hud.rs:259-278`). It covers only the Recording state and vanishes before transcription starts, so the longest wait in the session has no indicator at all.

## Research

Everything below was verified against live sources in August 2026.

**The Whisper model catalog.** The published table at [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) lists every GGML file with disk size and SHA-1: `tiny` 75 MiB through `large-v3` 2.9 GiB, `large-v3-turbo` 1.5 GiB, quantized `-q5_0`/`-q5_1`/`-q8_0` variants for every family, and one `small.en-tdrz` tinydiarize build. `large-v3-turbo-q5_0` is 547 MiB and `-q8_0` is 834 MiB. The [download-ggml-model.sh](https://github.com/ggml-org/whisper.cpp/blob/master/models/download-ggml-model.sh) script confirms the `resolve/main/ggml-<model>.bin` URL pattern plan 03 phase 19 relies on. OpenAI's [model table](https://github.com/openai/whisper) puts turbo at 809 M parameters and roughly 8x large's speed, and warns turbo is not trained for translation, which does not affect Echo because Echo never passes `--task translate`. Measured WER figures from the [transcribe.cpp model docs](https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/whisper.md) place turbo (2.01%) between large-v2 (2.65%) and large-v3 (1.82%), which matters for the "best installed model" ranking in phase 2.

**Languages.** The `g_lang` map in [src/whisper.cpp](https://github.com/ggml-org/whisper.cpp/blob/master/src/whisper.cpp) still runs from `en` at id 0 to `yue` (Cantonese) at id 99, 100 entries, and [include/whisper.h](https://github.com/ggerganov/whisper.cpp/blob/master/include/whisper.h) documents that `language` set to `nullptr`, `""`, or `"auto"` triggers auto-detection. Plan 03's two load-bearing measurements stand: `.en` models silently reset `-l` to English and exit 0, and `-dl` bypasses that guard and must never be invoked.

**Tray icons on Linux.** There is no reliable way to learn the panel's brightness. `iconAsTemplate` is macOS-only, and the maintainers' own advice in [tauri#3857](https://github.com/tauri-apps/tauri/issues/3857) is to ship one icon that works on both dark and light backgrounds. Tauri on Linux passes raw pixels to libappindicator and cannot reference a themed icon name at all ([tauri#9690](https://github.com/tauri-apps/tauri/issues/9690)), even though the [StatusNotifierItem spec](https://specifications.freedesktop.org/status-notifier-item/latest/index.html) prefers icon names. The robust answer is a dual-tone glyph: light fill, dark keyline, full alpha, verified by compositing tests over white and black. GNOME's [integration guide](https://developer.gnome.org/documentation/guidelines/maintainer/integrating.html) also wants a scalable SVG, a 256 px PNG, and optionally a symbolic icon in hicolor, and the [Flathub quality guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines) demand contrast on both background tones plus declared brand colors.

**HUD rendering.** A WebKitGTK overlay window was evaluated and rejected on evidence: transparent Tauri windows crash or artifact on NVIDIA proprietary drivers ([tauri#14924](https://github.com/tauri-apps/tauri/issues/14924), open in February 2026), transparent webviews leave ghost pixels through dirty-rect skipping ([tauri#12800](https://github.com/tauri-apps/tauri/issues/12800)), `alwaysOnTop` is unreliable on Wayland ([tauri#13121](https://github.com/tauri-apps/tauri/issues/13121)), and click-through on transparent regions is unimplemented ([tauri#13070](https://github.com/tauri-apps/tauri/issues/13070)). The recorder is also a separate CLI process with no Tauri runtime, so a webview HUD would require a second process architecture. The upgrade path that stays in `x11rb` is an ARGB 32-bit visual with per-pixel alpha when a compositor owns `_NET_WM_CM_S0`, falling back to the current SHAPE mask when none does; without a compositor the transparent pixels render black, per the [xorg mailing list](https://lists.x.org/archives/xorg/2017-December/059097.html). Frames get rasterized client-side with tiny-skia, the same renderer the icon generator already uses.

**What good dictation surfaces look like.** Superwhisper's [recording window docs](https://superwhisper.com/docs/get-started/interface-rec-window) treat the live waveform as a microphone health diagnostic: a flat waveform means audio is not arriving. That alone condemns Echo's sine wave. [open-wispr PR #66](https://github.com/human37/open-wispr/pull/66) replaced a pre-rendered sine loop with real RMS bars using asymmetric smoothing, fast attack and slow release, matching broadcast meter behavior; [sflow](https://github.com/daniel-carreon/sflow) publishes its constants (8 bars, decay 0.80) and a clean state table: idle pill, recording bars, processing dots, done check, error cross, with auto-dismiss. [Wispr Flow's Hub](https://docs.wisprflow.ai/articles/5096240724-navigating-the-wispr-flow-app-desktop-ios-and-android) shows what the main window of a dictation app carries: history grouped by day, a stats card with streak and total words, and guided setup. [VoiceInk](https://www.voice-ink.com/) sells nine recording animations as a feature. The pattern across all of them is a small floating pill with truthful levels and explicit states, and a main window that leads with the record action and proof of use.

## Scope

**Included**

- A redesigned app icon, a tray glyph drawn from the same mark with full alpha and dual-tone contrast, a GNOME symbolic icon, and the generator and test updates to keep them honest.
- Whisper model choice, language choice across all 100 languages plus auto-detect, and guided model download, executed from the existing plan 03 phase documents with 2026 amendments.
- A HUD redesign: real microphone levels, explicit Recording, Transcribing, Done, and Failed states, per-pixel alpha with anti-aliased edges, and a compositor-less fallback.
- A beauty pass over the desktop window: brand lockup, a real record hero, usage stats, day-grouped history, a setup checklist, and final styling for the new settings controls.

**Excluded**

- A Wayland HUD. The X11 capsule stays X11-only; a layer-shell port is a separate piece of work.
- A webview-based HUD. Rejected on the evidence cited above, not deferred.
- Streaming or partial transcription, still. The engine boundary stays "hand it a WAV, get text back".
- Plan 03 phase 16 (the sherpa-onnx stdout parse fix) and phase 20 (the parakeet-cli bakeoff). Both remain valid and open; neither blocks anything here.
- Translation (`--task translate`), diarization (`-tdrz` models), and cleanup rules for non-English languages, all for the reasons plan 03 gave.
- Windows and macOS.

## Constraints

- Every cargo command needs `npm run build --prefix frontend` first; `tauri-build` resolves `frontendDist` at compile time.
- Environment variables keep winning over the config file. The whole test suite depends on it.
- The GUI and the recorder are different processes. Settings round-trip through `$XDG_CONFIG_HOME/echo/config.json`, and the bound-shortcut path must be tested explicitly.
- `.en` Whisper models silently ignore `-l` and exit 0. Echo refuses before spawning, per plan 03 phase 17.
- Do not read `n_langs` to decide whether a model is multilingual. The filename suffix and `model.multilingual` in the JSON are the reliable signals.
- Tauri's PNG icons must be RGBA 32-bit; `tauri-codegen` panics otherwise.
- No new dependencies without naming the rejected alternative. This plan adds one: tiny-skia as a rendering dependency of `crates/echo` for HUD frames. It is already in the workspace lockfile through `xtask`, so the icon pipeline and the HUD share one rasterizer. The rejected alternatives are raw XRender trapezoid drawing (no anti-aliased curves without mask pictures, far more code) and a webview HUD (rejected above).
- The tray cannot be driven programmatically. Verification is a screenshot of a real panel at 22 px and 24 px, as plan 03 phase 6 established.
- `assets/icons/echo.svg` is unreferenced legacy. Phase 1 deletes it so the repo keeps exactly one app master.

## Alternatives

**Tray theme handling.** Detect the panel theme and switch icons: impossible in practice, since nothing on Linux reports panel brightness and GNOME's top bar is dark even in light mode. Ship two icons and follow the portal color-scheme: answers the wrong question for the same reason. **One dual-tone glyph with a compositing test, chosen.** It is the approach the Tauri maintainers themselves recommend.

**HUD rendering.** Webview window: rejected, four open upstream bugs and a second-process architecture. Keep the 1-bit SHAPE mask: rejected, aliased edges and no translucency is exactly what looks dated today. **ARGB visual plus client-side rasterization with a SHAPE fallback, chosen.**

**Waveform data.** Keep the sine loop: rejected. It animates while the microphone is dead, and every serious dictation app treats a flat waveform as the mic-health signal. **Real RMS from the cpal callback with asymmetric smoothing, chosen.** The HUD and the capture stream already live in the same process, so this is a shared atomic, not a protocol.

**Model and language scope.** Rewrite the specs: rejected. Plan 03 phases 14, 15, 17, 18, and 19 were measured against real hardware and upstream behavior, and nothing found in August 2026 contradicts them. **Adopt them with a short amendment file each**, so the decisions stay auditable in one place.

## Applicable skills

The implementer must invoke these by name:

- **how** over each subsystem before the first change to it: the icon pipeline, the STT layer, the audio path, the X11 HUD, and the frontend are five separate mental models.
- **control-ui** (from `cursor-team-kit`) for runtime verification of every phase touching the webview.
- **control-cli** (from `cursor-team-kit`) for every phase touching `echo-desktop rec --once|--toggle|--hold`.
- **interrogate** before shipping phase 1 and phase 4. The mark is the brand and the language-plus-model matrix is the design where a wrong call is expensive to undo.
- **technical-writing** (`/technical-writing`) for every PR description and the README touch-ups in phases 1 and 6.
- **unslop** over every prose surface. **deslop** (`/deslop`) over each diff before commit. **no-comments** (`/no-comments`) before review.
- **show-me-your-work** across the program.
- Cursor's built-in **babysit** skill after opening each PR.

## Phases

The identity work lands first because it is independent and the most visible. The five capability phases follow in their already-specified dependency order. The HUD comes after them, and the beauty pass last, so it styles the complete control set rather than restyling twice.

1. [Phase 1: the mark](phase-1-icon-redesign.md)
2. [Phase 2: model catalog](phase-2-model-catalog.md)
3. [Phase 3: model picker and transparency panel](phase-3-model-picker.md)
4. [Phase 4: the language model](phase-4-language-model.md)
5. [Phase 5: language picker and detected-language display](phase-5-language-picker.md)
6. [Phase 6: guided model download](phase-6-model-download.md)
7. [Phase 7: the HUD](phase-7-hud-redesign.md)
8. [Phase 8: the beauty pass](phase-8-ui-beauty-pass.md)

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

CI already runs exactly this list, including the icon drift check, so a green PR is the project-level check.

Two surfaces have no control skill. The X11 HUD is verified with `echo-desktop --hud-demo` under Xvfb plus a screenshot per state, the fallback [02-design-overhaul/testing.md](../02-design-overhaul/testing.md) established. The tray is verified by screenshotting a real panel at 22 px and 24 px on both a light and a dark shell theme.

## Implementation guidance

- Branches follow `cursor/<descriptive-name>-8122`. One phase per PR, in order.
- Each phase is independently shippable. The app must be coherent after every merge; no phase may leave a control visible that does nothing.
- Phases 2 through 6 execute plan 03 documents. When this plan and the older document disagree, this plan wins, and the disagreement is written down in the phase's amendment section so the trail stays auditable.
- Do not carry compatibility shims between phases. When a phase replaces a pattern, delete the old one in the same diff.
- Attach evidence to every PR. Screenshots at 920x680 in both themes for UI phases, HUD state screenshots from Xvfb for phase 7, panel screenshots for phase 1, and terminal transcripts for CLI phases.
