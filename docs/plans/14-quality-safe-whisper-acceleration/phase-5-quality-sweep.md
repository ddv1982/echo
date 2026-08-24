# Phase 5: Run the version and decoding sweep

[Back to overview](overview.md)

## Goal

Find whether any stable whisper.cpp and decoding identity clears latency and quality on this host. Keep every result separate and fail closed.

## Changes

- Add `scripts/sweep-whisper-admission.py` to generate paired cells and invoke the trusted runner.
- Test v1.9.2 aggressive decoding, v1.9.2 runtime defaults, and bounded intermediate beam and best-of settings.
- Test v1.9.3 pre-release only as a separate investigative identity.
- Produce one decision per runtime, decoding, cache, and reset identity.

## Data structures

- `SweepCell`: runtime build, decode contract, CPU control, accelerated candidate, and evidence bundle IDs.
- `SweepDecision`: exact identity, gate results, and `PROCEED`, `STOP`, or `INCOMPLETE`.

## Verification

Static: the sweep self-test must prove that CPU and GPU settings cannot differ inside one cell and that hypothesis rows never satisfy sample-size gates.

Runtime: use the optimized Echo binary, all licensed product classes, and at least ten randomized pairs per fixture and stratum. The existing one-repeat default and greedy probes remain hypotheses only.

## Stop gate

Stop this host identity if every stable cell exceeds a 0.5 percentage-point language regression, adds hallucinations or failures, loses p95, or misses either median threshold. A pre-release-only pass does not authorize packaging.
