# Phase 1: Write honest run bundles

[Back to overview](overview.md)

## Goal

Make every benchmark attempt distinguishable as running, failed, or complete. Preserve the raw child artifacts needed for later replay. Do not change product runtime selection.

## Changes

- Update `scripts/benchmark-stt.py` to refuse a nonempty output directory and create a unique run identity.
- Write `status.json` before the first observation. Replace it atomically on failure or completion.
- Store the exact command, selected non-secret environment, stdout, stderr, product JSON, and wall time for every observation.
- Snapshot the source corpus manifest and its digest.
- Extend `scripts/verify-stt-benchmark.sh` with success, failure, interruption, mutation, and stale-directory cases.

## Data structures

- `RunStatus`: schema, run ID, state, timestamps, and optional failure.
- `RunManifest`: binary, corpus, candidates, seed, repeats, warmups, and artifact index.
- `ObservationArtifact`: row ID plus command, environment, stdout, stderr, result, and timing paths.

## Verification

Static:

```bash
python3 scripts/benchmark-stt.py --self-test
./scripts/verify-stt-benchmark.sh
```

Runtime: run the fake candidate through `echo-desktop transcribe`, force one child failure, and verify that only the successful run is `complete`.

## Stop gate

Stop if stale or partial output can be mistaken for a complete run, or if raw artifacts cannot reproduce the stored row.
