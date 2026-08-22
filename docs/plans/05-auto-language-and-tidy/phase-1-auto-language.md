# Phase 1: auto by default

Back to [overview](overview.md).

## Goal

A user with a multilingual model who never opens Settings gets their own language back. The pinned-English default becomes model-aware: Auto when the resolved model is multilingual, pinned English when it is `.en`, explicit choice always wins.

## Changes

**`crates/echo/src/stt/mod.rs`.** `resolved_language` gains the model context. The rule, written down in code: `ECHO_LANGUAGE` wins, then `Config.language`, then Auto when the resolved Whisper model is multilingual, then pinned English. The model context comes from the same inventory the engine resolves from, so the picker, the warning, and the recorder never disagree.

**`crates/echo-core/src/language.rs`.** `LanguageChoice::default()` stops meaning pinned English. The type keeps its two explicit states; the *resolution* gains the model-aware rule. A bare `LanguageChoice::default()` with no model context is pinned English, which is what the `.en` and no-model paths resolve to explicitly rather than implicitly.

**`crates/echo/src/stt/whisper.rs`.** `WhisperEngine::new` resolves language with its own inventory's multilingual flag. The `.en` refusal is unchanged and stays coherent: `.en` plus unset resolves to pinned English and runs; `.en` plus a configured non-English or auto choice is still refused before spawning.

**`crates/echo/src/rec.rs`.** Cleanup gating already follows the detected language on auto runs (`rec.rs:177` passes `transcript.language` to `permits_english_rules`). No change needed beyond keeping that wiring; a test pins it: an auto run that detects Japanese must not gain an ASCII period.

**`src-tauri/src/main.rs`.** The settings IPC projects the language field's default as `"en"` today (`main.rs:694`). The projection gains the same model-aware rule so the Settings control shows Auto when that is what the recorder would do; the picker and the recorder never disagree.

**The pin suggestion.** After an auto run whose detection is confident (p ≥ 0.8), the language row in Settings shows "Detected German · pin it for speed?" as a one-click action that writes `language: "de"` to the config. Below the threshold the suggestion stays silent; a misdetection the user can see is a correction they can make, and a confident one is an offer. The detected-language chip is unchanged.

**`README.md`.** The language paragraph stops saying "Echo transcribes in English by default" and describes the model-aware default, the pin suggestion, and the latency trade in one sentence each.

## Data structures

No new types. `resolved_language(env, file, multilingual: Option<bool>) -> LanguageChoice` carries the rule. The pin suggestion reads the existing last-run block (`language`, `language_probability`); nothing new crosses IPC.

## Verification

**Static.** `cargo test --workspace`. Table-driven tests over the resolution rule: unset plus multilingual model yields Auto; unset plus `.en` yields pinned English; configured anything wins over both; `.en` plus configured German still refuses before spawning. The Japanese-cleanup test pins the gating.

**Runtime.** Via **control-cli**, with a real multilingual model and a non-English fixture. Default config on the Dutch fixture produces Dutch (or a visibly low-confidence chip when tiny misdetects), and the same run with `ECHO_LANGUAGE=nl` produces Dutch with no detection line. Attach the before/after transcripts to the PR; the overview's matrix is the before.
