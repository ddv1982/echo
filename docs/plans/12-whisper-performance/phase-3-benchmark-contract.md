# Phase 3. Expand the benchmark contract

Back to [overview](overview.md).

## Goal

Make candidate comparisons reproducible and impossible to mislabel.

## Changes

- `src-tauri/src/cli.rs` accepts advanced per-run Whisper overrides for threads, beam size, best-of, and no-fallback. They are CLI-only and never become saved Settings.
- `scripts/benchmark-stt.py` records outer subprocess wall time, Echo split timings, attempts, tuning, binary and model identities, resolved backend, runtime source, host metadata, candidate order, warmups, and raw quality results.
- `scripts/verify-stt-benchmark.sh` proves option construction, deterministic output, candidate randomization seed, backward JSON support, and fail-loud behavior.

Keep cold CLI observations in `runs.jsonl`. Resident and acceleration scripts introduced later write different report kinds.

## Data structures

- `BenchmarkCandidate`. Model, tuning override, language mode, VAD policy, and requested runtime source.
- `BenchmarkObservation`. Workload identity, host identity, resolved plan, timing, attempts, transcript, and quality inputs.

## Verification

Static:

- Python self-tests cover parsing, edit distance, WER or CER routing, silence, randomization, aggregation, and schema compatibility.
- CLI tests prove every override reaches both VAD and no-VAD attempts.

Runtime:

- Run Fake in CI for deterministic contract proof.
- Run Turbo Q5_0 on target Linux hardware with raw JSONL retained and verify that report metadata matches the actual binary, model, CPU, GPU, and requested flags.
