# Language-honest transcription and a real CLI

## Goal

Echo should transcribe a WAV file as clean text or structured JSON, honor one-shot engine/model/language choices, and use the same resolved request for microphone inference and cleanup. Dictionary spellings should bias Whisper before recognition as well as repair text afterward.

This plan borrows the useful product ideas from [MLX Audio's STT CLI](https://github.com/Blaizzy/mlx-audio/blob/ebf44dfcdcf28cac2993445e46e7c72a9eca7421/mlx_audio/stt/generate.py) and [uniform hotword helper](https://github.com/Blaizzy/mlx-audio/blob/ebf44dfcdcf28cac2993445e46e7c72a9eca7421/mlx_audio/stt/utils.py). It does not adopt MLX, Python, Apple-specific inference, arbitrary model downloads, or backend-specific kwargs.

## Product contract

```text
echo-desktop transcribe FILE.wav
  [--engine auto|whisper|parakeet]
  [--model NAME]
  [--language auto|CODE]
  [--format text|json]
  [--output -|PATH]
  [--raw]

echo-desktop languages
  [--engine whisper|parakeet]
  [--format text|json]
```

- Input is WAV. Echo already handles sample-rate conversion and stereo-to-mono conversion.
- Output defaults to cleaned text on stdout. Diagnostics use stderr.
- `--output` is exact. Echo never appends an extension.
- JSON schema version 1 includes raw and cleaned text, timing, engine/model details, requested and observed language, language probability, VAD state, and recognition-hint count.
- Exit 0 is success, 1 is runtime failure, and 2 is invalid syntax or an invalid CLI-only combination.
- File transcription never shows a HUD, injects text, notifies, writes status/history/config, or takes a recording lock.
- CLI overrides beat environment, then config, then model-aware defaults. Overrides never persist.

## Language correctness

- Auto engine plus a pinned language selects Whisper.
- Auto engine plus an explicit Whisper model selects Whisper.
- Higher-priority language or model choices select Whisper over a lower-priority Parakeet choice. Explicit CLI Parakeet ignores lower-priority stored constraints, while Parakeet plus a CLI pin fails before audio decode.
- Configured Parakeet remains automatic-only and does not apply a stored pin.
- Multilingual Whisper accepts Auto or a supported pin.
- English-only Whisper accepts pinned English only.
- Auto plus no observed language never enables English filler removal or ASCII punctuation. This makes Parakeet conservative when sherpa-onnx cannot report the detected language.

## Recognition hints

Echo derives at most 32 unique, newest-first `written` dictionary phrases. The comma-separated UTF-8 prompt stays at or below 512 bytes, never truncates a phrase, and excludes empty, control-containing, or overlong values.

Whisper receives those spellings through its supported `--prompt` argument. Parakeet and Fake report zero applied hints. Post-transcription dictionary rewriting remains the final authority.

## Architecture

- `echo-core` owns `RecognitionHints`, conservative language cleanup, `DecodeOptions`, and the engine trait.
- `echo::transcribe` resolves CLI/environment/config/model capability into one prepared run, calls one engine, and applies cleanup from the same resolved language.
- `rec.rs` keeps capture, HUD, injection, status, and history, but uses the shared prepared run.
- `src-tauri/src/cli.rs` owns Clap syntax, exit codes, exact output destinations, and the versioned JSON projection.
- `main.rs` keeps the zero-argument desktop path and delegates non-desktop arguments.

No compatibility resolver remains after the recorder migrates.

## Explicit non-goals

- SRT, VTT, segment or word timestamps.
- Streaming or partial transcription.
- Diarization or meeting transcription.
- Translation.
- CLI model download or history browsing.
- A generic model registry or Hugging Face resolution.
- Frontend redesign.

See [testing.md](testing.md) for the release gates.
