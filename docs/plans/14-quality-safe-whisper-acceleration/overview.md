# Quality-safe Whisper acceleration

## Context

Echo has measured a large Vulkan latency win on Intel Iris Xe, but the tested identity fails transcription quality. The current host-matrix gate also trusts caller-supplied cache, reset, driver, and ICD labels. Those labels cannot authorize production selection.

This plan turns the existing investigation tools into a fail-closed admission system. It then adds exact-identity selection, one managed CPU retry, and managed accelerator packaging. Managed CPU one-shot remains the universal floor.

## Scope

Included:

- Artifact-bound benchmark runs with replayable output.
- Quality metrics recomputed from raw transcripts and the pinned corpus.
- An inference-process runtime receipt for the selected physical device and loaded runtime libraries.
- Tool-owned cache evidence and captured reset evidence.
- A same-host whisper.cpp version and decoding sweep.
- One child-only launch contract shared by benchmarks and product execution.
- Exact-identity admission, bounded quarantine, and one same-model managed CPU retry.
- Separate managed accelerator components and cross-hardware qualification.
- Conditional warmup and residency after one-shot admission.

Excluded:

- A global Linux or Vulkan default.
- Driver or ICD bundling.
- Background quality qualification from user dictation.
- A visible GPU toggle that bypasses admission.
- A resident broker before warmed one-shot passes.
- Treating CUDA, ROCm, OpenVINO, or CTranslate2 as aliases for Vulkan.

## Constraints

- CPU and accelerated controls use the same Echo binary, whisper.cpp binary, model, VAD policy, language, prompt, and decoding settings. Only GPU enablement differs.
- A result applies only to its exact runtime, adjacent libraries, model, VAD, protocol, tuning, physical device, driver, ICD, and cache class.
- Boot identity proves reset separation. Boot identity does not become a stable production identity field.
- Device names and process-local indices are diagnostic fields only.
- Qualification must reproduce the production child environment. Ambient `LD_LIBRARY_PATH` and loader variables are not evidence.
- An accelerated failure permits one same-model managed CPU retry. The existing VAD retry remains inside each logical attempt.
- The current Iris Xe identity remains stopped until every gate passes.
- Phase 5 measurements made before the shared launcher are historical research evidence only. A promotable identity must be measured again through the current launch contract and Echo commit.

## Alternatives

### Global Vulkan selection

Reject. Local and upstream evidence show device-specific and driver-specific quality failures. Vulkan availability proves capability, not correctness.

### Persistent worker first

Reject. Existing resident probes miss the combined 25 percent and 300 ms threshold. Persistence would add memory, cancellation, and process-lifecycle risk before one-shot quality passes.

### Exact evidence-gated one-shot acceleration

Choose. This preserves the CPU floor, isolates failures, and lets each backend and hardware identity qualify independently. Persistence remains a later optimization.

## Applicable skills

- Use `pstack:how` before changing an unfamiliar runtime, packaging, or telemetry subsystem.
- Use `pstack:architect` and `pstack:arena` for changes to identity, selection, or recovery boundaries.
- Use `pstack:interrogate` on contested designs before shipping.
- Run `cursor-team-kit:deslop` before each commit and `pstack:no-comments` before review.
- Use `pstack:show-me-your-work` for every measured phase.
- Use the pstack Babysit and Shipping playbooks after opening the PR.

## Phases

1. [Write honest run bundles](phase-1-run-bundles.md).
2. [Replay and recompute admission evidence](phase-2-replay-verifier.md).
3. [Capture the inference runtime receipt](phase-3-runtime-receipt.md).
4. [Prove cache and reset state](phase-4-cache-reset.md).
5. [Run the version and decoding sweep](phase-5-quality-sweep.md).
6. [Share one launch and identity contract](phase-6-launch-identity.md).
7. [Select, quarantine, and recover](phase-7-selection-recovery.md).
8. [Package and qualify hardware families](phase-8-packaging-matrix.md).
9. [Evaluate warmup and residency](phase-9-warmup-residency.md).

Each phase is stop-gated. Phases 1 through 6 do not enable production acceleration. Phase 7 enables only an exact identity that passed Phases 1 through 6. Phases 8 and 9 cannot broaden that pass.

## Verification

See [testing.md](testing.md). The project-level minimum is:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
./scripts/verify-stt-benchmark.sh
./scripts/verify-stt-corpus.sh
./scripts/verify-whisper-acceleration.sh
./scripts/verify-whisper-runtime-archive.sh
```

Every measured phase also runs the optimized `echo-desktop transcribe` path on Linux. A self-test or build is not a performance claim.

## Implementation guidance

Apply Foundational Thinking by landing evidence formats before policy. Apply Build the Lever by keeping the runner, verifier, and sweep scripts reusable. Apply Sequence Work into Verifiable Units by stopping after each phase until its checks pass. Apply Prove It Works by checking raw artifacts and the real product path. Apply Boundary Discipline by parsing external process and host data once, then passing typed state inward. Apply the Laziness Protocol by preserving the current caller seam and managed CPU path.

The architecture and arena decision are in [architecture.md](architecture.md) and [arena.md](arena.md). Research and current evidence are in [grounding.md](grounding.md).
The independent review, multi-model interrogation, fixes, and final verification are in [review.md](review.md).
The continuation that clears admission, selection, packaging, and release gates is in [admission-run.md](admission-run.md).
