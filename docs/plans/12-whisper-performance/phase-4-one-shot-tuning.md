# Phase 4. Hillclimb safe one-shot tuning

Back to [overview](overview.md).

## Goal

Ship the fastest quality-preserving cold Whisper policy before adding lifecycle complexity.

## Changes

- Measure thread counts 1, 2, 4, 6, 8, and a physical-core cap.
- Measure beam sizes 1, 2, and 5 with matching best-of values where the upstream strategy uses them.
- Compare correct pinned language with automatic detection.
- Compare fallback enabled and disabled on clean, noisy, quiet, technical, multilingual, and silence fixtures.
- Record accepted and rejected candidates with raw evidence. Apply only accepted defaults in `whisper_plan.rs` and argument generation in `whisper.rs`.

Do not bundle several changes into one candidate. Hillclimb one dimension at a time, then verify the combined winner.

## Data structures

- `TuningDecision`. Host class, model identity, accepted tuning, latency delta, quality delta, and evidence path.

## Verification

Static:

- Plan and argument tests pin the accepted policy and upstream parity.
- Existing language, hint, VAD, and JSON tests remain green.

Runtime:

- Run at least ten timed repeats per fixture with randomized candidate order and documented warmups.
- Accept a change only when it meets the overview thresholds on every required language group and adds no silence hallucination.
- Repeat the final winner after reboot or a clean power-policy reset to reject cache or background-load accidents.
