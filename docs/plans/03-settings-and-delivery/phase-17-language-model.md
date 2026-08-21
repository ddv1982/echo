# Phase 17: the language model

Back to [overview](overview.md).

## Goal

Echo has a language concept. All 100 that whisper.cpp supports, plus automatic detection.

Today there is no language field, no environment variable, no config key, and nothing in the STT layer models a language at all. This is a new concept, not a new value.

## Changes

**`crates/echo-core/src/language.rs`, new.** The 100-language table. The canonical source is the `g_lang` map in whisper.cpp's `src/whisper.cpp`, an ISO code to `{id, english_name}` mapping running from `en` at 0 to `yue` at 99.

**Generate the table from upstream; do not hand-type it** (**principle-build-the-lever**). A script that reads `g_lang` and emits the Rust table is rerunnable when upstream adds a language, and 100 hand-typed pairs will contain a typo that surfaces as one language silently not working.

Note the 99-versus-100 subtlety for the code comment. `large-v3` reports 100 and `large-v2` reports 99; Cantonese is what v3 added.

**`crates/echo/src/stt/whisper.rs`.** Pass `-l <code>` or `-l auto`. Both spellings are current and verified working.

**Refuse rather than pass the flag through when the model cannot honour it.** This is the most important behaviour in the phase. Verified by running it: `ggml-base.en.bin -l de -f jfk.wav` prints `WARNING: model is not multilingual, ignoring language and translation options`, resets `params.language = "en"`, transcribes English, and **exits 0**. The library only injects a language token when `whisper_is_multilingual(ctx)` is true, so `params.language` is never consulted on an `.en` model.

whisper-cli will produce confident English text from German speech and report success. Echo must catch this before spawning, using phase 14's filename-derived multilingual flag, and tell the user their model cannot do the language they picked.

**Never invoke `-dl`.** It bypasses the multilingual guard: `params.detect_language` sets `params.language = "auto"` on the line *after* the reset. Measured, `ggml-base.en.bin -dl` on plainly English audio returned `auto-detected language: nl (p = 0.010000)`. That is an upstream bug, and `-l auto` on a multilingual model is the correct path anyway since `-dl` exits without transcribing.

**Default to a pinned language, with auto as an explicit opt-in.** Auto costs one extra encoder pass, measured at 2610 ms against 2287 ms for a pinned language on 33 seconds of audio with `base`. The cost is a fixed extra encode and it will be several times larger on `large-v3`. For push-to-talk, where the user is waiting, that is perceptible. Accuracy also favours pinning: Whisper's own language-identification accuracy on Fleurs runs from roughly 45% for `tiny` to roughly 65% for `large-v2` across 102 languages, and one study measured removing the language specification as its largest single degradation. Detection is close to reliable for common European languages and much worse in the tail.

Two limits to record in the code, because a future reader will assume otherwise. Detection runs on the first 30-second window only, with `offset_ms = 0` passed once before the seek loop, and the result applies to the whole file. There is no per-window re-detection and no CLI way to ask for one.

**`crates/echo/src/stt/parakeet.rs`.** Report Parakeet's 25 languages as a fixed capability list and its selection as automatic. There is no language flag to pass. Do not fake a picker for it.

**`crates/echo-core/src/cleanup.rs`.** Stop the English rules corrupting other languages. `punctuate` (`:103-112`) appends an ASCII `.` and recognises only `.`, `!`, `?` as terminators, so it will append a Latin period after a Japanese `。` or an Arabic `؟`. `is_filler` (`:81-88`) matches only `um` and `uh` and strips to `is_ascii_alphabetic`, so no accented token can ever match it.

Gate the English-specific rules on the resolved language. Writing rules for other languages is out of scope; not damaging their output is not.

**`crates/echo-core/src/config.rs`.** Add `language: Option<LanguageChoice>`.

## Data structures

`Language(&'static str)` as a branded code rather than a bare `String`, constructible only from the table, so an invalid code cannot reach the command line (**principle-type-system-discipline**).

`LanguageChoice { Auto, Pinned(Language) }`. Two states, both explicit. `Option<Language>` would make `None` ambiguous between "auto" and "unset".

## Verification

**Static.** `cargo test --workspace`.

- The generated table has 100 entries, `en` is id 0, `yue` is id 99.
- An unknown code is rejected at construction.
- Argument construction: `Pinned(de)` yields `-l de`; `Auto` yields `-l auto`; neither yields `-dl`, asserted explicitly since invoking it is a correctness bug.
- `Pinned(de)` against a `.en` model returns a refusal before spawning, not a command line.
- Cleanup: a Japanese string ending in `。` gains no ASCII period.

**Runtime.** Via **control-cli**, with a real multilingual model.

1. German speech, `-l de`, multilingual model. Correct German.
2. The same German speech with `-l de` and `ggml-base.en.bin`. Echo refuses with a message naming the model. **This is the assertion that matters most**, because without it whisper-cli returns plausible English and exits 0.
3. The same German speech with `-l auto`. Confirm `result.language` is `de` in the phase 13 JSON.
4. Measure the auto-versus-pinned latency on your own hardware and put both numbers in the PR. The 2287 ms and 2610 ms figures are from `base` on one machine and the gap scales with model size.
