# Phase 2: model catalog

Back to [overview](overview.md).

## Goal

Echo knows which models are on disk, what each one is capable of, and which one it will use. Backend only.

## Base spec

Execute [docs/plans/03-settings-and-delivery/phase-14-model-catalog.md](../03-settings-and-delivery/phase-14-model-catalog.md) as written. Its design (scan the directory, derive capability from the GGML naming convention, delete `DEFAULT_MODEL`, fall back to the best installed model, fix the Parakeet `available()` lie in passing) was verified against the code in January and still matches `crates/echo/src/stt/whisper.rs` and `crates/echo/src/stt/parakeet.rs` line for line.

## Amendments

Verified against the live catalog in August 2026:

- **The filename table must cover `-q8_0` and `-tdrz`.** The [published model table](https://huggingface.co/ggerganov/whisper.cpp) now spans `-q5_0`, `-q5_1`, and `-q8_0` quantizations across the families, plus one `small.en-tdrz` tinydiarize build hosted in a different repository. The scanner must parse the q8_0 suffix and must ignore `tdrz` files rather than misparsing them; Echo has no diarization path.
- **The "best installed" ranking needs an explicit turbo rung.** Measured WER puts `large-v3-turbo` between `large-v2` and `large-v3` ([transcribe.cpp model docs](https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/whisper.md)), so the family rank is `tiny < base < small < medium < large-v1 = large-v2 < large-v3-turbo < large-v3`. Write the table down in code with that citation in a comment. The rule otherwise stands: prefer multilingual over `.en`, then the higher rung.
- **The quantization convention is a table, not a rule, exactly as the base spec says.** `tiny`, `base`, `small` use `q5_1`; `medium` and the larges use `q5_0`; `q8_0` exists for all; there is no `.en` turbo. Table-driven tests cover every one of these shapes.

## Verification

As the base spec, plus table rows for `ggml-base-q8_0.bin`, `ggml-large-v3-turbo-q8_0.bin`, and `ggml-small.en-tdrz.bin` (the last asserts the file is ignored, not offered). The runtime check is unchanged: with three real models in `ECHO_MODEL_DIR`, the selected model is the one the ranking predicts, and an explicit `Config.whisper_model` beats the ranking.
