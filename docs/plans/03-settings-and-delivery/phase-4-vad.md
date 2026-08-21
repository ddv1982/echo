# Phase 4: VAD gating

Back to [overview](overview.md).

## Goal

Stop the decoder from ever seeing non-speech audio, which is the only measure that reliably produces an empty result instead of an invention (**principle-fix-root-causes**).

Measured at whisper.cpp commit `45f1593`, `--vad` with a Silero model gave an empty transcript on every non-speech fixture tried, in every language tried, while `-sns` produced fluent fabricated sentences and `--suppress-regex` produced repetition loops. The mechanism is visible in the stderr: `whisper_vad_segments_from_probs: Final speech segments after filtering: 0`. The window never reaches the model, so the model has nothing to hallucinate from.

## Changes

**`crates/echo/src/stt/whisper.rs`.** Append `--vad` and `-vm <path>` to the argument list when a VAD model is present in the model cache directory, and omit both when it is not. Echo does not download models, so VAD is opportunistic until phase 19 makes acquiring one a click.

**`crates/echo/src/stt/cache.rs`.** Add VAD model discovery beside the existing model probes. Look for `ggml-silero-v6.2.0.bin`, then `ggml-silero-v5.1.2.bin`. These are the two files `models/download-vad-model.sh` publishes. The model is 885 KB and detection measured 22.55 ms for three seconds of audio, so the cost is not a consideration.

**`crates/echo/src/stt/mod.rs`.** Extend `engine_summary` so Settings can report whether VAD is active. "Whisper, VAD on" and "Whisper, VAD unavailable" are different enough to a user debugging bad output that hiding the distinction would be a mistake.

Verified as safe for dictation before choosing this: `samples/jfk.wav` transcribes identically with and without VAD, and a 1.2-second clip survived intact, well under the 250 ms `--vad-min-speech-duration-ms` default. The only difference observed was cosmetic punctuation drift on a padded clip, caused by VAD trimming the padding and changing the decoder's context.

**Do not add the tuning flags yet.** `-vt`, `-vspd`, `-vsd`, `-vp` and `-vo` all have defaults, and the defaults were only exercised against synthetic fixtures. Ship the defaults, then tune against real microphone input with evidence.

## Data structures

Extend the value `engine_summary` returns so it carries VAD state rather than a bare label. Do not add a second parallel function; `resolve_engine` and `engine_summary` are already two hand-synced copies of one decision and phase 8 collapses them.

## Verification

**Static.** `cargo test --workspace`. A unit test asserting the argument vector contains `--vad` and `-vm` when a VAD model file exists in a temp cache directory, and contains neither when it does not. This tests argument construction, which is the part that can regress silently.

**Runtime.** Via **control-cli**, with a real `whisper-cli` and a real VAD model, so this is an `#[ignore]`d integration test plus a manual run following the convention at `crates/echo/tests/transcribe_fixture.rs:18`.

1. Silent fixture, VAD model present. Transcript empty, and confirm `Final speech segments after filtering: 0` in stderr. Empty output for the wrong reason is not a pass.
2. Silent fixture, VAD model absent. Transcript empty because phase 3's filter caught the marker. Both layers must work alone.
3. `claude_code.wav` with VAD on. Still transcribes. This is the regression that matters; a VAD that eats real speech is worse than the problem.
4. Quiet speech, recorded at the edge of audibility on a real microphone. The `--vad-threshold 0.5` default was only validated against synthetic audio, and clipping a whisper is the failure mode nobody would notice until a user complains. Record what you observe in the PR even if it passes.

## Open question for the implementer

Very long recordings are unverified. `--vad-max-speech-duration-s` defaults to `FLT_MAX`, meaning no automatic split, and testing reached only 33 seconds against Echo's 60-second cap. Run one full 60-second recording through VAD before merging and report the result.
