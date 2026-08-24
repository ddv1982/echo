# Testing

## Failing-before regressions

- A successful sherpa JSON object must produce only its `text` value. The current adapter returns the whole object.
- `ClipboardOnly` must leave the transcript on the clipboard. The current wrapper restores the previous value.
- Explicit Parakeet must still show a Speech model row. The current UI hides Model quality entirely.

Each regression is run before its production fix and must fail for the named reason.

## Focused Rust checks

- Parakeet JSON text, empty text, legacy plain text, malformed JSON, and nonzero stderr behavior.
- Clipboard fallback retains transcript; missing focus does not modify it.
- Recommendation at unknown, just below 8 GiB, exactly 8 GiB, and high memory.
- Auto resolver behavior remains unchanged.
- Parakeet plan activation clears the dormant file model where directly testable.

## Frontend checks

- Whisper renders a Speech model selector.
- Parakeet renders its fixed model and 25-language capability.
- Auto with backend Parakeet projection does not render a Whisper selector.
- Changing engines refreshes language capability.
- Recommended setup names Large v3 Turbo on an 8 GiB preview machine.
- Typecheck, lint, unit tests, and responsive Playwright suite pass.

## Benchmark checks

- Unicode normalization and edit-distance scoring have deterministic unit coverage.
- Missing candidates and failed subprocesses stop the run.
- Fake speech produces zero word errors.
- Fake silence stays empty and does not count as a hallucination.
- JSON Lines and Markdown output order is stable.

## Full verification

```text
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run typecheck --prefix frontend
npm run lint --prefix frontend
npm run test --prefix frontend
npm run build --prefix frontend
npm run test:responsive --prefix frontend
./scripts/verify-transcribe-cli.sh
./scripts/verify-settings-ux.sh
./scripts/verify-stt-benchmark.sh
```

The real Small, Turbo Q5, and Parakeet matrix is not claimed on this macOS host. The PR records published quality evidence and ships the exact command a Linux reviewer can rerun against installed models.
