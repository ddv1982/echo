# Phase 2: Replay and recompute admission evidence

[Back to overview](overview.md)

## Goal

Remove authority from stored derived fields and caller labels. Recompute the decision from raw run artifacts and the pinned corpus.

## Changes

- Update `scripts/analyze-stt-host-matrix.py` to require a complete Phase 1 bundle.
- Verify the run-manifest, corpus, audio, runtime, model, VAD, and observation digests.
- Recompute normalized transcript units, WER or CER, hallucinated silence, pairing, timings, and coverage.
- Bind every run row to one corpus fixture and reject missing, extra, or changed fixtures.
- Extend `scripts/verify-stt-corpus.sh` with tamper, duplicate, relabel, stale, and unrelated-row cases.

## Data structures

- `VerifiedObservation`: derived transcript, quality, timing, pair key, and artifact identities.
- `CorpusBinding`: manifest digest and the exact fixture ID, language, class, audio digest, and reference mapping.
- `AdmissionDecision`: `PROCEED`, `STOP`, or `INCOMPLETE`, with hard gate results.

## Verification

Static:

```bash
python3 scripts/analyze-stt-host-matrix.py --self-test
./scripts/verify-stt-corpus.sh
```

Runtime: replay the current Iris Xe bundle. It must remain `STOP`, even if stored WER, cache, reset, driver, or ICD fields are edited.

## Stop gate

Stop if one-byte artifact mutation, relabelled duplicate evidence, or a fabricated coverage manifest can improve a verdict.
