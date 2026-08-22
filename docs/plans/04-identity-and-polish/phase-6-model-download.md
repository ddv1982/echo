# Phase 6: guided model download

Back to [overview](overview.md).

## Goal

Make the multilingual and VAD models obtainable from inside Echo. Without this phase, "Echo supports 100 languages" is false for every user whose only model is `base.en`, and the VAD from plan 03 phase 4 never activates.

## Base spec

Execute [docs/plans/03-settings-and-delivery/phase-19-model-fetch.md](../03-settings-and-delivery/phase-19-model-fetch.md) as written: a small direct-HTTPS downloader in `crates/echo/src/stt/fetch.rs`, SHA-1 verification against the published hashes, temp-file-then-rename so a partial download can never be picked up by `model_file()`, idempotent re-runs, `download_model(id)` with progress events and `cancel_download` over IPC, the URL and size shown before starting, and offers surfaced at the point of failure, not just in Settings.

The design rule it states stays intact: Echo never downloads anything on its own. The user presses a button, sees where the file comes from, and watches it verify.

## Amendments

The offer table, refreshed against the live [catalog](https://huggingface.co/ggerganov/whisper.cpp) in August 2026. Sizes and hashes below are the published ones; the table in code carries the SHA-1 per offer, as the base spec requires.

| Offer | File | Size | SHA-1 | Why |
| --- | --- | --- | --- | --- |
| Fast, English | `ggml-base.en-q5_1.bin` | 57 MiB | `d26d7ce5a1b6e57bea5d0431b9c20ae49423c94a` | Today's default, quantized |
| Balanced, multilingual | `ggml-small.bin` | 466 MiB | `55356645c2b361a969dfd0ef2c5a50d530afd8d5` | The cheapest genuinely multilingual option |
| Best, multilingual | `ggml-large-v3-turbo-q5_0.bin` | 547 MiB | `e050f7970618a659205450ad97eb95a18d69c9ee` | 809 M parameters at roughly 8x `large-v3` speed |
| Silence detection | `ggml-silero-v6.2.0.bin` | 885 KB | from the [whisper-vad repo](https://huggingface.co/ggml-org/whisper-vad) | Required for VAD gating |

Two deltas from the base spec's table. The turbo q5_0 size is 547 MiB, not the 574 MB the January draft said. And `ggml-large-v3-turbo-q8_0.bin` (834 MiB, `01bf15bedffe9f39d65c1b6ff9b687ea91f59e0e`) exists now; it stays out of the curated offers because the q5_0's measured WER delta is small and 287 MiB is real disk, but the phase 2 scanner recognizes it the moment a user drops one in. Four offers, no catalog browser, as the base spec decided.

## Verification

As the base spec's six runtime steps, unchanged, including the two that prove the machinery rather than the happy path: corrupt the expected hash and confirm the download is rejected with nothing left in the model directory, and cancel the turbo download midway and confirm no partial file remains where `model_file()` would find it. The closing loop is the whole plan in one action: download `ggml-small.bin`, pick German, dictate German, watch German text land at the cursor with no blank-audio marker.
