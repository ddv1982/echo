# Phase 5: language picker and detected-language display

Back to [overview](overview.md).

## Goal

Pick a language, or pick automatic and see what was detected.

## Base spec

Execute [docs/plans/03-settings-and-delivery/phase-18-language-ui.md](../03-settings-and-delivery/phase-18-language-ui.md) as written: `list_languages()` derived from the resolved engine and model, the grouped `<select>` with Auto first and keyboard search for free, the detected-language readout with its probability shown when Auto is active, low confidence rendered differently from high, the `.en`-plus-non-English incompatibility warning shown before recording, and the honest "automatic, not reported" row for Parakeet.

## Amendments

- **The detected-language readout is a chip, not a row.** A small inline pill next to the control carrying `de · German · p=0.96`, styled from the existing status-note pattern. Phase 8 gives it final styling; this phase ships it readable.
- **The common group in the picker is fixed and short.** Auto, then English, German, Spanish, French, and the detected language from the last run when there is one, then the full alphabetical list. Do not build recency tracking for a settings control; the last-detected entry covers the realistic second language.
- Nothing else changes. The base spec's keyboard-only check (tab to the select, type `ger`, land on German) remains the interaction acceptance test.

## Verification

As the base spec, including its six runtime steps. The one that matters most is still step 4: with `ggml-base.en.bin` selected and German picked, the warning appears before recording, naming the model and pointing at the fix. Phase 6 turns that fix into a button.
