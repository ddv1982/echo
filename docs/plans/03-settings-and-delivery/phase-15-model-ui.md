# Phase 15: model picker and transparency panel

Back to [overview](overview.md).

## Goal

Answer "what model is transcribing my speech" exactly, and let the user change it. This is the phase the user asked for directly.

## Changes

**`src-tauri/src/main.rs`.** One command, `list_models() -> Result<ModelInventory, String>`, over phase 14's scan.

**`frontend/src/App.tsx`.** A Transcription panel with two controls and one readout.

**Engine select.** Whisper, Parakeet, Fake, and an explicit Auto. Auto must be a visible option, not the absence of a choice; today "unset" and "auto-detect" are conflated and the UI cannot show the difference. Mark an engine whose binary or model is missing as unavailable with the reason, rather than hiding it. "Parakeet needs `sherpa-onnx-offline` on PATH" is actionable; a missing row is not.

**Model select**, shown only when the engine is Whisper. Each option shows the family, whether it is multilingual, the quantisation, and the on-disk size. A user choosing between a 60 MB and a 574 MB model is making a speed-versus-quality trade and the numbers are the whole basis for it.

**The transparency readout, which is the real deliverable.** Show what actually ran on the last transcription, sourced from phase 13's JSON rather than from configuration:

- The resolved binary path, from the `whisper-cli` / `whisper-cpp` / `whisper` probe.
- The resolved model file path, absolute.
- Whether that model is multilingual, from `model.multilingual`.
- Whether VAD was active.
- The measured inference time, which `Transcript.infer_ms` already carries and nothing displays.

Configuration says what was requested. This says what happened. When they disagree the user needs to see both, and today the Settings row shows a compile-time constant dressed up as a fact.

**Surface engine stderr on failure.** `EngineError::Infer(String)` already carries it and nothing shows it. A user whose transcription fails currently gets "speech engine failed" and no way forward. whisper.cpp's stderr says exactly what went wrong (**principle-experience-first**).

**`frontend/src/tauri.ts`.** Wrappers plus preview fixtures. The existing fixtures hardcode `Whisper · base.en` at `:15`, `:30`, `:39`, `:48`; replace them with a plausible inventory.

## Data structures

Reuse phase 14's `ModelInventory` on the wire. Extend `AppStatus` with a last-run block carrying the resolved binary, model, multilingual flag, VAD state, and inference time. Distinct from the settings fields, because one is a request and the other is an observation and merging them is how the current row came to lie.

## Verification

**Static.** `npm run build --prefix frontend`, `npm run lint --prefix frontend`, `npm run test --prefix frontend`, `cargo test --workspace`.

Frontend tests: the model select appears for Whisper and is absent for Parakeet; an unavailable engine renders its reason; the transparency readout renders the last-run values from the fixture.

**Runtime.** Via **control-ui**.

1. With `ggml-base.en.bin` and `ggml-small.bin` installed, confirm both appear with correct multilingual flags and sizes.
2. Select `small`, record, and confirm the readout shows the `small` path and `multilingual: true`. Then check `~/.cache/echo` and confirm the path shown is the file that exists.
3. Select Parakeet and confirm the model select disappears and the readout reports Parakeet with no language.
4. Rename the model file out from under a running app, record, and confirm the failure names the missing file rather than saying "speech engine failed".
5. Screenshot both themes at 920x680 and attach to the PR.

The assertion that matters is step 2. Reading a path off the screen and finding that exact file on disk is what makes this transparency rather than a label (**principle-prove-it-works**).
