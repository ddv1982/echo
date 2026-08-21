# Phase 18: language picker and detected-language display

Back to [overview](overview.md).

## Goal

Pick a language, or pick automatic and see what was detected.

## Changes

**`src-tauri/src/main.rs`.** `list_languages() -> Vec<LanguageOption>`, derived from the resolved engine and model rather than a static list. Whisper multilingual offers 100 and Auto. Whisper `.en` offers English only. Parakeet offers its fixed 25 as informational, with selection reported as automatic.

**`frontend/src/App.tsx`.** A language control in the Transcription panel.

A hundred options in a bare `<select>` is a poor control. Order it: Auto first, then a small recent-and-common group, then the full alphabetical list. Do not build a searchable combobox. That is a new interaction pattern for one control in a five-panel settings pane, and a grouped native `<select>` with `<optgroup>` is keyboard-searchable for free (**principle-laziness-protocol**).

**Show the detected language when Auto is active.** This is what makes Auto trustworthy. Display the code, the English name, and the probability, all of which the stderr line carries: `auto-detected language: en (p = 0.958162)`. Show low confidence differently from high confidence; a misdetection the user can see is a correction they can make, and a silent one is a mystery.

Read it from phase 13's `result.language`, guarded on `transcription` being non-empty. On an empty transcript that field holds a stale default.

**Render the incompatibility rather than hiding it.** When the model is `.en` and the user picks a non-English language, phase 17 refuses. The UI must say so before they record, naming the model and offering the fix, which is a multilingual model. Phase 19 makes that fix a click.

**Be honest about Parakeet.** Automatic identification across 25 languages with **no readback**, because the `lang` field comes back empty and sherpa-onnx exposes no language option for transducers. The row should say "automatic, not reported", not show a blank where Whisper shows a detected language. Papering over the asymmetry would make the UI feel broken on the engine where it is actually a capability limit.

**`frontend/src/tauri.ts`.** Wrappers plus preview fixtures with a detected-language value, so the display renders under Vitest and `npm run dev`.

## Data structures

`LanguageOption { code, englishName, group }` where `group` drives the `<optgroup>` split. Extend the last-run block from phase 15 with the detected language and its probability.

## Verification

**Static.** `npm run build --prefix frontend`, `npm run lint --prefix frontend`, `npm run test --prefix frontend`, `cargo test --workspace`.

Frontend tests: Auto is first and selected by default; the list has 100 entries plus Auto for a multilingual model; only English for an `.en` model; the detected-language readout renders when Auto is active and is absent when a language is pinned; the incompatibility warning renders for `.en` plus a non-English choice.

**Runtime.** Via **control-ui**.

1. Pin German, record German speech, confirm correct output and that no detected-language line appears since none was detected.
2. Switch to Auto, record the same speech, confirm the readout shows German with a probability.
3. Record English under Auto and confirm the readout follows.
4. With `ggml-base.en.bin` selected, pick German. Confirm the warning appears **before** recording.
5. Keyboard-only: tab to the select and type `ger`. The native control must jump to German. If it does not, the grouping is wrong.
6. Screenshot both themes at 920x680, with Auto active and a detected language shown, and attach to the PR.
