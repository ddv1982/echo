# Phase 4: the language model

Back to [overview](overview.md).

## Goal

Echo has a language concept. All 100 languages whisper.cpp supports, plus automatic detection. This is the other half of what the user asked for.

## Base spec

Execute [docs/plans/03-settings-and-delivery/phase-17-language-model.md](../03-settings-and-delivery/phase-17-language-model.md) as written: the generated 100-language table in `echo-core`, `-l <code>` or `-l auto` on the whisper command line, the hard refusal when a `.en` model meets a non-English choice, never `-dl`, pinned-by-default with auto as an explicit opt-in, Parakeet reported as a fixed 25-language automatic capability with no picker, the English cleanup rules gated on the resolved language, and `Config.language: Option<LanguageChoice>`.

## Amendments

Verified against upstream master in August 2026:

- **The generator's source of truth is unchanged.** The `g_lang` map in [src/whisper.cpp](https://github.com/ggml-org/whisper.cpp/blob/master/src/whisper.cpp) still runs `en` at id 0 through `yue` at id 99. The 99-versus-100 subtlety the base spec records stands: `large-v3` added Cantonese.
- **The `auto` spellings are confirmed in the public header.** [include/whisper.h](https://github.com/ggerganov/whisper.cpp/blob/master/include/whisper.h) documents `nullptr`, `""`, and `"auto"` as the auto-detect triggers. Echo passes `-l auto` explicitly, as the base spec says.
- **One new guard: never pass `--task translate`.** OpenAI's [model documentation](https://github.com/openai/whisper) states turbo models are not trained for translation and return the original language instead. Echo has no translate feature and this phase adds a test asserting no code path constructs one, so a future "translate to English" checkbox cannot silently produce German.
- The base spec's two measured traps are restated here because they are the phase: `.en` models reset `-l` to English, print a warning, and exit 0, so Echo refuses before spawning; `-dl` bypasses that guard through an upstream bug, so Echo never invokes it.

## Verification

As the base spec, with its four runtime checks unchanged: pinned German on a multilingual model, refused German on `ggml-base.en.bin`, auto-detected German reported through `result.language`, and the auto-versus-pinned latency measured on the implementer's hardware with both numbers in the PR. Add the no-translate-flag assertion to the static suite.
