# PR 16.3: Receipt-driven local Whisper selection

## Outcome

PR 16.3 adds the portable runtime contract and local evidence model needed to select Vulkan safely on previously unknown Linux hardware. It does not change Echo's user-visible acceleration preference. Production remains on managed CPU or the existing exact v2 admission path until PR 16.4 passes its hardware matrix.

The phase is complete only when all of these claims are measured at one exact commit:

- an empty-cache Auto request returns a managed CPU transcript before calibration starts;
- a bounded helper survives a short-lived caller and records a local decision;
- explicit GPU discovers a local device without a shipped device allowlist;
- device selection follows stable Vulkan UUIDs rather than `selectedIndex`;
- every GPU attempt has ready and result receipts, and a bad attempt causes a 24-hour per-key quarantine plus exactly one managed CPU recovery;
- two processes write separate immutable records without corrupting shared state;
- installed runtime resources contain no host ICD path or shader-cache seed;
- cached planning p95 is at most 25 ms, and cold Auto adds at most 5 percent to managed-CPU result latency.

Unit tests alone do not satisfy this predicate. The ten live lanes and both performance gates in `overview.md` are required.

## Architecture decision

The four-candidate arena selected Candidate B as the base. The implementation uses:

- one `WhisperAccelerationPlanner` boundary for contract, route, receipt, and recovery decisions, with thin engine wrappers enforcing the PR 16.3 rollout hold;
- a host-neutral `portable-selection.v1.json`, a sanitized legacy exact index, and an exact package binding derived from the full v3 proof archive;
- append-only local observations keyed by execution artifact, inference contract, stable Vulkan receipt, DRM driver, and local ICD manifest/library digests;
- an Echo-owned UUID selector, with `selectedIndex` retained only as diagnostic evidence;
- a finite detached calibrator that owns durable jobs after the foreground CPU result;
- ready/result receipt validation, per-key quarantine, and one non-recursive CPU recovery.

The full PR 16.2 `acceleration-set.v3.json` remains audit evidence. Production Rust does not consume its host paths, cache seeds, or performance verdicts as runtime selection authority.

## Ownership

- `whisper_acceleration.rs`: the legacy exact-v2 compatibility path kept through PR 16.3.
- `whisper_portable.rs`: strict portable package parsing, binding, and verification caching.
- `whisper_planner.rs`: contract matching, the rollout hold, and the deep planner.
- `whisper_accel_cache.rs`: local key derivation, strict immutable records, folds, jobs, calibration leases, and quarantine; its tests live in `whisper_accel_cache_tests.rs`.
- `backend/vulkan.rs`: local ICD discovery, stable device enumeration, UUID selection, and ready probes.
- `whisper_probe.rs`: strict enumeration, ready-receipt, and result-receipt parsing.
- `whisper_plan.rs`: preference and sealed managed-CPU or GPU-then-CPU plans.
- `whisper_recovery.rs`: result validation, quarantine append, and exactly one CPU recovery.
- a hidden Echo CLI entry point: finite calibration ownership only; no resident model or request service.

Host paths and shader-cache locations stay private to the local Vulkan invocation. Callers receive an Engine and never sequence discovery, cache reads, probing, fallback, or scheduling.

## Local identity and state

`LocalSelectionKey` is the domain-separated SHA-256 of:

- `ExecutionArtifactId`;
- `InferenceContractId`;
- Vulkan backend, vendor/device/API/driver values;
- device UUID, driver UUID, and pipeline-cache UUID;
- DRM driver;
- local ICD manifest digest and ICD library digest.

It excludes device index, absolute paths, Echo binary identity, shader-cache paths, and cache bytes. A driver, ICD, runtime, model, VAD, tuning, or behavior change therefore rotates the key. Reordering identical physical devices does not.

Local JSON observations are create-new and immutable. Readers preserve and fail closed on malformed or ambiguous records. Per-scope advisory locks serialize finite calibration ownership; they do not turn the ledger into mutable shared state. Shader data is local, disposable, and never evidence.

## Runtime flow

1. The caller resolves and leases the managed CPU plan first.
2. `Cpu` returns it without Vulkan or calibration work.
3. Cold `Auto` returns it with a post-success ticket. It does no foreground enumeration or probing.
4. After a CPU transcript parses successfully, the wrapper publishes a durable job and spawns the finite helper. Spawn failure cannot invalidate the transcript.
5. The helper reopens and verifies the installed package, claims the scope, checks the recording interlock, discovers local ICDs and stable devices, and runs a fixed non-user-audio CPU/GPU canary within a wall-clock budget.
6. Explicit `Gpu` may perform bounded foreground discovery and a ready probe. It never uses a shipped device allowlist.
7. The runtime selects through Echo-owned device and driver UUIDs. A cached route reuses the ready receipt from its successful calibration; every current inference must still return a fresh result receipt matching that stable identity. A mismatch, timeout, malformed output, missing receipt, or internal CPU fallback quarantines that key and runs managed CPU once.
8. PR 16.3 records and reports local eligibility only in hidden diagnostics. Its rollout gate does not make local Auto the production default.

## Verifiable implementation phases

### Slice 1: portable boundary and immutable state

Build strict Rust readers and fixtures for the host-neutral manifests. Update staging so installed packages reject host ICD paths, `cache-seeds/`, and shader files. Add typed stable receipts, `LocalSelectionKey`, immutable records, deterministic folds, concurrent-writer tests, corrupt-record preservation, and 24-hour quarantine tests.

Stop if strict duplicate-key rejection cannot be made equivalent across Rust and the staging tools, or if two-process publication can expose partial JSON. Do not start native selector work until this slice passes.

### Slice 2: stable native selection

Extend the pinned runtime probe to enumerate physical devices and select by Echo-owned device/driver UUID. Discover and hash ICD inputs locally. Prove index reorder follows the UUID and that missing or ambiguous DRM identity fails to CPU.

Stop with local GPU held if the pinned runtime cannot select by UUID before backend initialization. Receipt-based recovery is not permission to ship an index-race selector.

### Slice 3: planner, owner, and recovery

Add the deep planner and hidden rollout path, durable calibration jobs, finite detached owner, recording interlock, ready/result validation, one recovery, and proof-only status. Preserve the existing production adapter.

Stop if the helper cannot survive caller exit, duplicate owners can repeat work, recording cannot cancel calibration, or recovery can recurse.

### Slice 4: measured acceptance

Build one rerunnable verification tool that drives the ten live lanes and records exact artifact identities. Measure 100 warm plans and cold managed-CPU result latency against trunk. Run the full workspace/product checks, deslop, Comment Sicko, multi-model interrogate, and an exact-head independent verdict.

Stop if any live lane fails, cached p95 exceeds 25 ms, cold Auto delay exceeds 5 percent, or evidence cannot be bound to the exact commit and runtime artifacts.

## Deferred by design

- Settings and visible Auto/GPU/CPU policy, compatibility matrix, and 20-percent/250-ms promotion threshold: PR 16.4.
- Removal of the v2 exact bridge and general cross-hardware release cutover: PR 16.5.
- Resident model ownership, IPC, warm lifetime, idle TTL, and serialized user inference: PR 16.6, only if measurements justify it.

## Arena record

Candidate B is the base because it makes the installed runtime contract a projection of proof rather than the proof archive itself. Candidate A contributes UUID selection and index discipline. Candidate C contributes caller-exit, duplicate-owner, recording-cancel, deadline, and recovery tests. Candidate D's string index selector and packaged device candidates were rejected because they preserve index authority and recreate a device allowlist.
