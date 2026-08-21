# Phase 19: guided model download

Back to [overview](overview.md).

## Goal

Make the multilingual and VAD models obtainable from inside Echo. Without this, "Echo supports 100 languages" is false for every user whose only model is `base.en`, and phase 4's VAD never activates.

This phase changes a design rule, so it needs stating plainly. Echo deliberately does not download models: `README.md:79` says so, and commit `6e892fe` removed the fake-engine fallback specifically because typing "claude code" into a user's window when no model was installed was unacceptable. That rule stays intact in spirit. Echo never downloads anything on its own. It downloads when the user presses a button, shows the URL and the size first, and verifies what it got.

## Changes

**`crates/echo/src/stt/fetch.rs`, new.** A small downloader over the published URL patterns, all verified live:

```
https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-<MODEL>.bin
https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-<silero-v5.1.2|silero-v6.2.0>.bin
```

**Direct HTTPS, not `huggingface-cli`.** Echo is a local-only Linux app and must not require a Python toolchain to fetch a file. These are the same URLs the upstream `download-ggml-model.sh` uses.

**Verify integrity.** `models/README.md` upstream publishes SHA-1 hashes for the main Whisper models. Reimplementing roughly 40 lines of shell with the hash check added is the right call. Do not shell out to the upstream script; it hardcodes its own path conventions.

**Resumable and idempotent.** Download to a temp file in the destination directory, verify, then rename into place. A half-written `ggml-large-v3-turbo.bin` that `model_file()` later finds and hands to whisper-cli is a confusing failure. Re-running a completed download is a no-op (**principle-make-operations-idempotent**).

**A curated offer, not the full catalog.** The GGML catalog has 29 files. Offer four and name the trade:

| Offer | File | Size | Why |
| --- | --- | --- | --- |
| Fast, English | `ggml-base.en-q5_1.bin` | 60 MB | Today's default, quantised |
| Balanced, multilingual | `ggml-small.bin` | 488 MB | The cheapest genuinely multilingual option |
| Best, multilingual | `ggml-large-v3-turbo-q5_0.bin` | 574 MB | 809 M parameters at roughly 8x `large-v3` speed. The standout entry: it makes the good multilingual tier affordable on disk in a way `large-v3` at 3.1 GB never was |
| Silence detection | `ggml-silero-v6.2.0.bin` | 885 KB | Required for phase 4's VAD |

Report runtime memory alongside disk, since it matters more for a desktop app. Upstream's table runs from roughly 273 MB for `tiny` to 3.9 GB for `large`.

Anyone wanting a different model drops it in the directory and phase 14's scan finds it. Do not build a catalog browser.

**`src-tauri/src/main.rs`.** `download_model(id)` with progress events, and `cancel_download`. A 574 MB download with no progress and no cancel is worse than no download.

**`frontend/src/App.tsx`.** Offer these from the Transcription panel, and from the two places the user hits the wall: the phase 18 incompatibility warning, and the phase 4 "VAD unavailable" note. Offering the fix at the point of failure is the difference between a feature and a settings row nobody finds (**principle-experience-first**).

Show the URL and the size before starting. A local-first app asking to use the network should say exactly where it is going.

## Data structures

`ModelOffer { id, filename, url, sha1, size_bytes, runtime_mb, multilingual, label }` as a static table. `DownloadProgress { id, received, total, state }` on the event channel, where `state` distinguishes verifying from downloading. A 574 MB file's hash check is not instant and a progress bar that sits at 100% looking hung is a support ticket.

## Verification

**Static.** `cargo test --workspace`.

Test the parts that are testable without the network: URL construction per offer; a hash mismatch deletes the temp file and returns a named error, not a corrupt model in place; a completed download re-run is a no-op; a cancelled download leaves no partial file where `model_file()` would find it.

Point the downloader at a local HTTP server serving a small fixture for the transport tests. Do not put a 574 MB download in CI.

**Runtime.** Via **control-ui**, for real, once per offer.

1. Download the VAD model. 885 KB, so this is the cheap end-to-end check. Confirm the file lands in `~/.cache/echo`, that phase 4's VAD activates without a restart, and that the Settings note flips from unavailable to on.
2. Download `ggml-small.bin`. Watch progress advance. Confirm the verifying state is visible and distinct.
3. Cancel a `large-v3-turbo` download midway. Confirm no partial file remains and that a subsequent `rec --once` still uses the previous model.
4. Corrupt the expected hash in the table, retry, and confirm the download is rejected and nothing lands in the model directory. Then revert. A hash check that has never rejected anything has not been tested (**principle-prove-it-works**).
5. Disconnect the network midway and confirm the error names the problem rather than reporting a generic failure.
6. Then close the loop the whole plan exists for: with `ggml-small.bin` freshly downloaded, pick German in Settings, dictate German, and confirm German text arrives at the cursor with no blank-audio marker.
