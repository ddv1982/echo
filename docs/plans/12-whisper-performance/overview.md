# Whisper performance

## Context

Echo now recommends Whisper Large v3 Turbo Q5_0 on capable machines, but its Whisper path still starts a new `whisper-cli` process and reloads the model for every transcription. The current `inferMs` combines temporary WAV work, child startup, model load, inference, VAD retry, and parsing. That number is useful to users but too coarse to decide whether threads, decoding policy, residency, or hardware acceleration caused a speed change.

This plan improves the existing model without changing its quality target. It starts with measurement and safe one-shot tuning. Resident execution and managed acceleration must pass explicit gates before they reach normal dictation.

## Scope

Included:

- Split timing and attempt telemetry for the current Whisper path.
- One internal execution plan that owns runtime, backend, tuning, model, VAD, and protocol.
- Repeatable tests for threads, beam size, best-of, language pinning, fallback, VAD, and runtime backend.
- Safe CPU tuning selected by measurements, not a user setting.
- Proven accelerated system runtimes with managed CPU fallback.
- A measured resident prototype using the `whisper-server` already present in the pinned runtime archive.
- A cross-process, user-scoped broker only if residency earns its lifecycle cost.
- One conditional managed accelerator variant after system-runtime evidence identifies the backend worth supporting.

Excluded:

- Cloud transcription.
- A weaker default model.
- Unverified Q4 or lower-bit models.
- Thread, beam, backend, GPU, or residency controls in normal Settings.
- Partial transcript streaming.
- A permanent system service.
- Several managed GPU backends in one release.

## Constraints

- Existing CPU, system-runtime, manual-model, VAD, language, dictionary, CLI, and recording paths remain supported.
- Managed files retain immutable activation, verification, repair, removal, and lease semantics.
- General Settings stays small. Advanced diagnostics may report what ran but do not expose tuning knobs.
- GNOME fallback shortcuts launch `echo-desktop rec --toggle` in separate processes. An in-process cache cannot accelerate that path.
- The managed runtime is Linux x86_64 and CPU-oriented today. The same pinned archive already contains `whisper-server`, but Echo does not extract or probe it yet.
- Cold CLI, warm resident, and accelerated backend results remain separate claims.

## Definition of done

- Every result records exact model, binary, runtime source, resolved backend, tuning, attempts, and timing mode.
- A one-shot tuning change needs at least 15 percent lower median user-path latency. Per-language WER or CER may not regress by more than 0.5 absolute percentage points, and silence may not gain a hallucination.
- Resident execution needs at least 25 percent and 300 ms lower warm median latency on the dictation corpus. It must not regress quality, p95 latency, cancellation, or memory pressure.
- An accelerated runtime must pass a real transcription probe, identify its actual backend, and fall back to the same model on managed CPU after failure.
- Model selection never changes as a performance fallback.
- GUI recording, GNOME `rec --toggle`, file CLI, managed runtime, compatible system runtime, and manual model paths all pass direct runtime checks.

## Alternatives

| Shape | Decision | Reason |
| --- | --- | --- |
| Tune the existing CLI only | Keep as the first performance track | It is small and low risk, but cannot remove repeated model load. |
| Embed whisper.cpp through FFI | Reject for this program | Workspace code forbids unsafe code, packaging grows, and an in-process model would not help separate GNOME shortcut processes. |
| Run a worker only inside the desktop app | Reject | It misses CLI and GNOME fallback sessions. |
| Install a permanent systemd user service | Reject | It adds installation and support burden before residency is proven. |
| Start an internal broker on demand and stop it after idle | Select conditionally | It can reuse one loaded model across GUI and CLI processes without becoming a permanent service. |

The architecture synthesis uses Candidate B's `WhisperExecutionPlan` as the base. It adopts Candidate C's separate cold, resident, and acceleration reports and its rule that uncertain cancellation destroys the worker. Arena verification changed the resident owner from an in-process pool to a cross-process broker. The broker uses the pinned archive's existing `whisper-server` binary.

## Throughput checkpoint

- Blocking first steps. Phases 1 through 3 establish telemetry, the execution plan, and a valid benchmark contract before optimization work fans out.
- Independent workstreams. CPU experiments and system-runtime probes may run independently after Phase 3. Resident work waits for the Phase 6 gate. Managed acceleration waits for Phase 5 evidence.
- Shared mutable state. One broker process owns its state record, server child, request queue, and managed leases. Benchmark runs use separate output directories and never share mutable reports.
- Smallest safe decomposition. Each production phase has one owner because `whisper_plan`, runtime selection, and broker lifecycle are shared invariants. Corpus recording, source research, and hardware measurements can fan out as independent read-only work.

## Architecture

See [architecture.md](architecture.md).

## Phases

1. [Split Whisper telemetry](phase-1-split-telemetry.md)
2. [Introduce the execution plan](phase-2-execution-plan.md)
3. [Expand the benchmark contract](phase-3-benchmark-contract.md)
4. [Hillclimb safe one-shot tuning](phase-4-one-shot-tuning.md)
5. [Probe accelerated system runtimes](phase-5-system-acceleration.md)
6. [Measure resident value](phase-6-resident-prototype.md)
7. [Build the cross-process broker](phase-7-cross-process-broker.md)
8. [Integrate resident transcription](phase-8-resident-integration.md)
9. [Expose truthful performance evidence](phase-9-product-evidence.md)
10. [Add one managed accelerator variant](phase-10-managed-acceleration.md)

Phase 6 is a stop gate. If residency misses its threshold, skip Phases 7 and 8. Phase 10 is also conditional. It proceeds only when Phase 5 identifies one backend with enough coverage and benefit to justify a pinned artifact.

The Linux GPU continuation is recorded in [gpu-research.md](gpu-research.md). Phase 5 now advances through measurement, host bakeoff, same-model fallback integration, and only then packaging. Its decisions are per model and cache state: first-use Iris Xe Vulkan carried a large penalty, while steady one-shot smoke runs made multilingual Base and Turbo faster.

## Verification

Project checks:

```text
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run typecheck --prefix frontend
npm run lint --prefix frontend
npm run test --prefix frontend
npm run build --prefix frontend
npm run test:responsive --prefix frontend
./scripts/verify-transcribe-cli.sh
./scripts/verify-stt-benchmark.sh
./scripts/verify-whisper-acceleration.sh
./scripts/verify-whisper-runtime-archive.sh
cargo build --release
```

Performance checks run on the target Linux hardware with a release build, fixed power policy, stable background load, model and binary hashes, randomized candidate order, documented warmups, at least ten timed repeats, and raw JSONL retained as a review artifact.

See [testing.md](testing.md) for the full matrix and failure cases.

## Applicable skills

- Use `pstack:how` before changing the engine, runtime inventory, managed installer, or recording process lifecycle.
- Use `pstack:architect` before changing the broker protocol, worker key, runtime candidate, or telemetry schema.
- Use `pstack:principle-boundary-discipline` for CLI flags, server responses, state files, and runtime probes.
- Use `pstack:principle-model-the-domain` for execution plans, runtime candidates, attempts, and broker state.
- Use `pstack:principle-build-the-lever` for every benchmark and probe.
- Use `pstack:principle-prove-it-works` on the real CLI, GUI, GNOME toggle process, and target hardware.
- Use `pstack:interrogate` on the resident lifecycle and runtime precedence before shipping.
- Run `pstack:deslop` before every commit and `pstack:unslop` over docs, errors, and release notes.
- Keep a `pstack:show-me-your-work` decision trail because performance acceptance depends on measurements and rejected candidates.
- Use `pstack:babysit` after each PR opens.

## Implementation guidance

Deliver one small, green unit at a time. Re-run the phase's target check before starting the next phase. Keep benchmark and probe code as rerunnable artifacts. Do not convert an experimental candidate into a durable setting. Do not claim a speedup without naming cold or warm mode, runtime backend, model identity, corpus, quality delta, and host.

## Continuation

Cross-hardware caching, residency, backend truth, and specialized accelerator ordering continue in [Plan 13](../13-cross-hardware-whisper/overview.md).
