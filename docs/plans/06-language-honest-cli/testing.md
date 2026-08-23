# Testing language-honest transcription

## Pure gates

- Resolution tables cover CLI, environment, config, engine availability, multilingual Whisper, English-only Whisper, Parakeet, Fake, and missing setups.
- Auto plus a pin selects Whisper. Explicit CLI Parakeet plus a pin fails. Configured Parakeet is automatic-only.
- Auto cleanup requires observed English. Unknown and non-English observations disable English rules.
- Dictionary hints are newest-first, written-only, deduplicated, control-safe, whole-term, and bounded at 32 terms and 512 bytes.
- Whisper arguments preserve model, language, prompt, and VAD retry behavior.

## Binary gates

- Fake file transcription produces exact text and schema-versioned JSON.
- Raw output, exact output paths, and one trailing newline are pinned.
- Raw output bypasses cleanup, and aliased output paths cannot overwrite the input recording.
- Syntax failures exit 2. Audio, setup, inference, cleanup, and output failures exit 1.
- A nonzero engine exit fails even when the process wrote partial stdout.
- Stdout never contains diagnostics.
- File runs leave config, dictionary, history, status, and recording-lock state unchanged.
- A fake `whisper-cli` proves `-m`, `-f`, `-l`, `--prompt`, and VAD arguments without a real model.
- `languages` reports model-aware Whisper capability and automatic-only Parakeet capability in text and JSON.
- Existing desktop recording, shortcut, settings, and release tests remain green.
- Recorder cleanup failures fall back to dictionary rewriting so a successful microphone transcription is not discarded.

## Rerunnable proof

`scripts/verify-transcribe-cli.sh` builds once, uses isolated config/data/model directories, runs Fake and fake-Whisper cases, checks exact stdout/JSON/exit codes, and proves no state mutation. It needs no microphone, display, network, or real model.

## Live evidence

An ignored Dutch or German real-model fixture test may run on a maintainer host with Whisper installed. It is evidence, not a default CI requirement.
