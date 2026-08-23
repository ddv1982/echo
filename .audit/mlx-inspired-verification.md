# MLX-inspired transcription verification

Verified on 2026-08-23 before opening the v0.6.0 pull request.

## Product proofs

- `scripts/verify-transcribe-cli.sh`: passed. It covers exact text and JSON output, raw mode, read-only stores, exit codes, Whisper language and prompt arguments, VAD retry, and both language catalogs. The desktop CLI integration tests also cover relative and absolute output paths.
- `scripts/verify-fixed-toggle.sh`: passed. It covers frontend build, lint, 68 frontend tests, clippy, desktop tests, removed shortcut surfaces, and the fixed toggle contract.
- `cargo test --workspace -- --skip rec::tests::toggle_starts_stops_and_can_restart --skip upgrade::tests::path_scan_finds_installs_in_path_order_and_stale_ones_differ`: passed. The skipped tests are existing Linux-assumption tests that fail on macOS because `/proc` is absent and `/tmp` canonicalizes through `/private`.
- `cargo build --release`: passed. `target/release/echo-desktop --version` printed `0.6.0`.
- `python3 scripts/changelog-notes.py --self-test` and `python3 scripts/changelog-notes.py v0.6.0`: passed.
- `git diff --check`: passed.

## Adversarial review

Four independent pstack reviewers covered Rust correctness, CLI boundaries, multilingual behavior, and recorder/release regressions. Their reproduced findings were fixed before this verification:

- relative output filenames now resolve in the current directory;
- higher-priority model and language choices cannot be discarded by Parakeet;
- language capability projection uses the same source-aware constraints as inference;
- nonzero Parakeet exits reject partial stdout;
- recorder cleanup failures retain the previous dictionary fallback;
- stored language/model incompatibility is no longer reported as a missing engine.
- raw output bypasses cleanup, and normalized output aliases cannot overwrite the source WAV.
