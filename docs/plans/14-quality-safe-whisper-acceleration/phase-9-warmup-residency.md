# Phase 9: Evaluate warmup and residency

[Back to overview](overview.md)

## Goal

Reduce first-use or model-load latency only for an already admitted one-shot identity.

## Changes

- Measure optional idle and on-AC cache warmup against the admitted one-shot path.
- Build the resident worker only for an identity that independently clears the resident thresholds.
- Enforce one worker key, one active request, readiness, leases, destructive uncertain cancellation, and a bounded idle timeout.
- Keep cache roots and resident state exact-identity scoped.

## Data structures

- `WarmupPolicy`: identity, trigger, power condition, budget, and invalidation.
- `ResidentWorkerKey`: execution identity and protocol.
- `ResidentState`: starting, ready, busy, stopping, or failed.

## Verification

Static: lifecycle tests cover concurrency, readiness, cancellation, crash recovery, lease release, and TTL cleanup.

Runtime: use three fresh worker cycles and ten warm observations per fixture. Compare with the best admitted warmed one-shot path. Resident median must improve by at least 25 percent and 300 ms, p95 must fall, and quality, failures, memory, and power must not regress.

## Stop gate

Stop residency if either latency threshold fails, memory falls below the product floor, cancellation is uncertain, or leases survive more than ten seconds after idle exit. Existing Iris Xe Base and Turbo resident results remain stopped.
