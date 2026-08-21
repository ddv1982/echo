# Phase 20: the parakeet-cli bakeoff

Back to [overview](overview.md).

## Goal

Decide, with measurements, whether Echo drops sherpa-onnx.

**The deliverable is a verdict, not a migration.** This phase is allowed to end in "keep sherpa-onnx" and that is a successful outcome. It exists because the alternative to measuring is asking, and this is a question a throwaway run answers better than an opinion does (the **Prototype** playbook, and **principle-never-block-on-the-human**).

## Why this is worth a phase

whisper.cpp now ships a native Parakeet backend, `examples/parakeet-cli`, with converted GGML weights published at `ggml-org/parakeet-GGUF` and a conversion script at `models/convert-parakeet-to-ggml.py`. Its flags are `-t`, `-m`, `-f`, `-ng`, `-dev`, `-ps`.

If it holds up, Echo's whole engine layer collapses to one binary family and one model format. That deletes a large amount of surface:

- The sherpa-onnx binary probe, `sherpa-onnx-offline` then `sherpa-onnx` (`crates/echo/src/stt/parakeet.rs:53-57`).
- The four-file ONNX directory layout and its INT8-preferring probes (`:42-51`, `:74-79`), plus the `available()`-versus-`transcribe()` disagreement phase 14 patches.
- The second output format phase 16 writes a parser for.
- The ONNX runtime as a user-installed dependency.
- One of the two "install this yourself" paths in the README.

One model directory, one set of `ggml-*.bin` files, one download mechanism from phase 19, one catalog scan from phase 14. That is a real simplification and exactly the shape **principle-subtract-before-you-add** points at.

The GGML Parakeet weights, for sizing: `f32` 2508.5 MB, `f16` 1255.9 MB, `q8_0` 668.8 MB, `q4_k` 415.6 MB, `q4_0` 355.6 MB. Against roughly 640 MB for the INT8 ONNX set. So `q8_0` is a straight swap on disk and `q4_k` is smaller.

## Why it is not simply done

It is new, and no accuracy comparison against sherpa-onnx exists. Committing to it on the strength of "one binary is nicer than two" would be a bet, and phase 16 is about to invest real work in the sherpa-onnx path, so the honest sequencing is to fix the incumbent, then measure the challenger against a working baseline.

## Changes

**Depends on phase 16.** A bakeoff against a broken baseline measures nothing. Phase 16 establishes that sherpa-onnx produces correct text; only then is a comparison meaningful.

**Build the harness, do not eyeball it** (**principle-build-the-lever**). `crates/echo/tests/compare_engines.rs` is the natural home and it needs the work anyway. Today it asserts nothing at all and swallows errors at `:16-24`, printing `engine=`, `infer_ms=`, `raw=` per engine. It is a benchmark harness wearing a test's name, and it is the reason the phase 16 bug survived. Turn it into a real comparison that emits a table.

**Record a real dictation corpus first.** The one committed fixture is `claude_code.wav`, roughly 400 ms. Two words cannot separate two engines. Record perhaps 20 utterances of actual dictation: normal speech, fast speech, a technical sentence with identifiers, a quiet utterance, one with background noise, and several in non-English languages inside Parakeet's 25. Commit them, or commit a manifest plus a fetch script if they are large.

**Measure four axes and publish the table:**

1. **Word error rate** against hand-written references. This is the axis that decides it.
2. **Latency**, as `infer_ms` per utterance, cold and warm.
3. **Resident memory** at peak. It matters more than disk for a desktop app.
4. **Non-speech behaviour**, using phase 3's silent fixture. Whether either invents text on silence is directly the user-visible complaint this whole plan started from.

Include `ggml-small.bin` and `ggml-large-v3-turbo-q5_0.bin` as columns. If Whisper turbo matches Parakeet on accuracy and latency, the interesting conclusion may be that Echo needs fewer engines rather than a different one.

**Then decide, and write the decision down.** Three admissible outcomes:

- **Migrate.** `parakeet-cli` wins or ties on accuracy and latency. Then a follow-up phase replaces the sherpa-onnx adapter and deletes it in one wave (**principle-migrate-callers-then-delete-legacy-apis**), rather than keeping both behind a flag.
- **Keep sherpa-onnx.** It wins on accuracy. Record the numbers so nobody re-litigates this in six months.
- **Drop Parakeet entirely.** Whisper turbo matches it and the second engine family stops earning its place.

Do not ship a third engine option to avoid choosing. Two engine paths already produced one silent bug, two hand-synced resolver functions, and two output parsers. Three would be worse (**principle-laziness-protocol**).

## Data structures

No production types. The harness emits one row per `(engine, model, utterance)` with WER, `infer_ms`, and peak RSS, aggregated into the comparison table. Keep it in the test, not in `echo-core`; this is measurement scaffolding and it should not leak into the product.

## Verification

**Static.** `cargo test --workspace`. The harness itself, with an assertion this time. At minimum, every configured engine must return non-empty text for a speech fixture and empty text for the silent one, so the harness fails when an engine is broken instead of quietly reporting a blank row.

**Runtime.** The measurement *is* the verification, and this is the one phase where the artifact is a document.

1. Run the full matrix. Attach the table to the PR.
2. State the verdict and the reason in one paragraph, including the numbers that decided it.
3. Publish the corpus and the harness so a reviewer can rerun it. A benchmark nobody can reproduce is an anecdote.
4. If the verdict is "migrate", open the follow-up as its own phase with its own PR. Do not migrate inside this one; the value here is the decision, and mixing it with a rewrite makes both harder to review.

## Note

This phase can be dropped without affecting anything before it. Phases 1 through 19 leave Echo with two working engines, and this only asks whether it needs both. Treat it as the first item of the next plan if this one is already long enough.
