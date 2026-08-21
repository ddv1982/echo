# Phase 13: Whisper JSON output

Back to [overview](overview.md).

## Goal

Read what whisper.cpp actually did instead of assuming it. This is the phase that makes real model transparency possible, and phases 14, 15, 17 and 18 all depend on it.

## Changes

Today the command is `-m <model> -f <wav> -nt -otxt -of <prefix>` (`crates/echo/src/stt/whisper.rs:86-90`), and Echo reads a `.txt` sidecar with a stdout fallback (`:109-114`). The text is all it gets. The model name it reports is a compile-time constant, and there is nowhere for a detected language to come from.

**Switch to `-oj -of -`.** `-of -` writes to stdout instead of a file, verified working: `fout_factory` sets `is_stdout` when the output stem is `-` and opens `/dev/stdout`. So Echo gets structured output on a pipe with **no temp file at all**, which also removes the sidecar cleanup at `:93-94` and the collision hazard in the `echo-stt-{pid}-{len}` naming.

The JSON carries everything the later phases need:

```json
{
  "model":  { "type": "base", "multilingual": true, "vocab": 51865, "ftype": 1 },
  "params": { "model": "models/ggml-base.bin", "language": "auto" },
  "result": { "language": "en" },
  "transcription": [ { "text": " ..." } ]
}
```

**Read `result.language`, not `params.language`.** The first is what was used, the second is what was asked for.

**Guard two traps found by measurement** (**principle-boundary-discipline**).

`result.language` is unreliable when `transcription` is empty. On silence with `-l de --vad`, the JSON reported `result.language: "en"` while `params.language` was `de`, because the early return on zero speech segments happens before `state->lang_id` is assigned. Only read the field when `transcription` is non-empty.

`model.multilingual` is the **only** correct multilingual test. Do not parse `n_langs` from stderr: `base.en` prints `n_langs = 99`, because the count subtracts the multilingual adjustment only for multilingual vocabularies. The filename `.en` suffix is a usable second signal.

**Fix the error handling while here.** The current branch only reports failure when the exit status is bad **and** the transcript is empty (`:95-99`), so a non-zero exit that produced text is treated as success. A crashed decoder that emitted a partial line should not silently look fine.

**Delete the `.txt` path in the same diff** (**principle-migrate-callers-then-delete-legacy-apis**). Two output paths mean two behaviours to test and one of them will rot.

Keep stderr. It is the diagnostic channel and phase 15 surfaces it when something fails. `-np` would suppress it; do not pass it.

## Data structures

Typed deserialisation structs for exactly the fields Echo consumes, not the whole document. Parse at the boundary and hand the rest of the pipeline a domain type (**principle-type-system-discipline**).

`WhisperOutput { model: ModelInfo, result: ResultInfo, transcription: Vec<Segment> }`, and extend `Transcript` (`crates/echo-core/src/engine.rs:3-9`) with the resolved model identity and an optional detected language. `EngineId::Whisper { model: String }` (`crates/echo-core/src/types.rs:92-96`) becomes the model that actually loaded rather than the constant that was requested.

## Verification

**Static.** `cargo test --workspace`.

Unit tests over captured JSON, which is the right shape here because the parser is pure. Cover: a normal multilingual result; an `.en` result where `multilingual` is false; an **empty `transcription` array**, asserting the language is reported as unknown rather than the stale `en`; malformed JSON producing a named error rather than a panic; a non-zero exit with partial text producing an error.

Commit the captured JSON fixtures. They are the record of what this version of whisper.cpp emits, and the format is a log-adjacent surface with no compatibility guarantee.

**Runtime.** Via **control-cli**, with a real `whisper-cli`. An `#[ignore]`d test following `crates/echo/tests/transcribe_fixture.rs`.

1. `claude_code.wav` through the new path. Same text as before, and confirm no `.txt` file appears in the temp directory.
2. Confirm the reported model matches the file on disk rather than `base.en`. Point `ECHO_MODEL_DIR` at a directory holding only `ggml-small.bin` and confirm Echo says `small`.
3. Confirm `-oj -of -` produces nothing on stdout except the JSON, so the parse is not competing with log output.
