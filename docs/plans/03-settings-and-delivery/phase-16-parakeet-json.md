# Phase 16: fix the sherpa-onnx parse

Back to [overview](overview.md).

## Goal

Make Parakeet work. The investigation found strong evidence it is broken today, and it is the **first** engine auto-detection tries.

## Changes

`ParakeetEngine::transcribe` takes the whole of stdout, trims it, and calls that the transcript (`crates/echo/src/stt/parakeet.rs:90`). There is no parsing at all.

Current `sherpa-onnx-offline` prints the recognition result to stdout as a **JSON object** via `AsJsonString()`, with the WAV filename and separators on stderr. Before an upstream change, results went to stderr and stdout would have been empty. Either way, `raw` is not the plain transcript the code assumes. The documented output shape is:

```json
{"lang": "", "emotion": "", "event": "", "text": " Ask not what your country can do for you, ...", "timestamps": [], "tokens": [], "words": []}
```

So a user on the auto-detect path with sherpa-onnx installed gets a JSON blob typed at their cursor, or nothing. Nobody has noticed because both engine integration tests are `#[ignore]`d and `compare_engines.rs` asserts nothing at all, swallowing errors at `:16-24`.

**Confirm before fixing** (**principle-fix-root-causes**). Install `sherpa-onnx-offline`, run it against `claude_code.wav`, and capture stdout and stderr verbatim. This claim was reached by reading upstream source, not by running the binary. Do not write a parser against a format you have not seen.

**`crates/echo/src/stt/parakeet.rs`.** Parse the JSON and read `text`. Handle a plain-text stdout too, since the format changed upstream and a user's installed binary may predate it. Detect which shape arrived rather than guessing from a version number.

Note the `lang` field is empty for Parakeet and there is no way to make it otherwise. Parakeet is a transducer; sherpa-onnx registers language options only on the families that have them, and `OfflineTransducerModelConfig` has three fields, all filenames. Report it as "automatic, not reported" rather than blank. Phase 18 depends on that being explicit.

**Add `--model-type=nemo_transducer`** to the argument list. The documented invocation for v3 includes it and Echo omits it (`:81-88`).

## Data structures

`SherpaOutput { text: String, lang: String }`, deserialising only the fields Echo uses. Same discipline as phase 13.

## Verification

**Static.** `cargo test --workspace`. Unit tests over captured stdout: the JSON shape; a plain-text shape; empty stdout producing a named error rather than an empty transcript that looks like successful silence.

Commit the captured output as a fixture, and note in the PR which sherpa-onnx version produced it.

**Runtime.** Via **control-cli**, with a real `sherpa-onnx-offline` and the Parakeet v3 model. This phase cannot be verified any other way, and that is the point.

1. `claude_code.wav` through Parakeet. The transcript must contain a word from `["claude", "code", "clawed"]`, matching the loose-assertion convention at `crates/echo/tests/transcribe_fixture.rs:10-15`. No JSON, no braces.
2. Silent fixture through Parakeet. Empty, not a JSON blob with an empty `text`.
3. Auto-detect with both engines installed. Confirm Parakeet is chosen, per the documented Parakeet-then-Whisper order, and that it produces usable text.
4. Give `compare_engines.rs` an actual assertion while here. A test that swallows errors is a benchmark harness wearing a test's name, and it is why this bug survived.
