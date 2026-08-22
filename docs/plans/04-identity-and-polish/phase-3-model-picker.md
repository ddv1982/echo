# Phase 3: model picker and transparency panel

Back to [overview](overview.md).

## Goal

Answer "what model is transcribing my speech" exactly, and let the user change it. This is half of what the user asked for.

## Base spec

Execute [docs/plans/03-settings-and-delivery/phase-15-model-ui.md](../03-settings-and-delivery/phase-15-model-ui.md) as written: the `list_models()` IPC command over phase 2's scan, the engine select with an explicit Auto, the Whisper-only model select showing family, multilingual flag, quantization, and on-disk size, the last-run transparency readout sourced from the engine's JSON rather than from configuration, and engine stderr surfaced on failure.

## Amendments

- **Sizes shown in the picker come from the scan, not a hardcoded table.** The point of showing 57 MiB versus 547 MiB is that the numbers are true for the files actually on disk. The published upstream sizes (verified August 2026 against the [catalog](https://huggingface.co/ggerganov/whisper.cpp)) belong in the phase 6 offer table, not here.
- **Build the controls in the current token system.** Phase 8 restyles Settings holistically. Do not pre-style; do not leave the new rows unstyled either. The existing `setting-row`, `segmented-control`, and `SettingSelect` patterns are the target, per plan 03's constraint that pickers use the existing design language.
- The frontend fixtures that hardcode `Whisper · base.en` are replaced with a plausible inventory, as the base spec says. That inventory should include a `-q8_0` file so the quantization column renders a third shape.

## Verification

As the base spec. The assertion that matters is still its step 2: select `small`, record, and confirm the readout's absolute model path is the file that exists on disk, with `multilingual: true` read from the engine's JSON. Screenshots at 920x680 in both themes go on the PR; phase 8 will replace them, and that is fine.
