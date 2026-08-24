# Architecture

## Problem

Echo needs faster Whisper without losing the model quality that justified Large v3 Turbo Q5_0. The existing one-shot subprocess is reliable and portable, but it hides where time goes and reloads the model on every request. Runtime acceleration and residency also cross managed-file, process, cancellation, and shortcut boundaries. The architecture must preserve the one-shot path as the compatibility floor while giving every optimization one owner and one measurable claim.

## Usage from the caller's view

Recording and file callers keep the current API:

```rust
let prepared = prepare_with_config(overrides, &file_config)?;
let completed = prepared.transcribe(&pcm, &dictionary, cleanup_policy)?;
```

The resolved plan chooses a cold CLI request or the internal broker. Callers do not select threads, beam size, GPU backend, or worker lifetime.

The file CLI remains stable for ordinary use. Advanced Whisper overrides exist only on the CLI so the benchmark can test candidates through the shipping boundary.

The benchmark produces three separate reports:

1. Cold one-shot CLI latency and quality.
2. First and warm resident request latency on one worker identity.
3. Runtime backend probe and acceleration results.

## Shape

```rust
struct WhisperExecutionPlan {
    runtime: WhisperRuntimeCandidate,
    model: ModelAsset,
    vad: Option<ModelAsset>,
    tuning: WhisperTuning,
    protocol: WhisperProtocol,
}

enum WhisperProtocol {
    OneShotCli,
    ResidentBroker,
}

struct WhisperRuntimeCandidate {
    source: RuntimeSource,
    backend: RuntimeBackend,
    cli: PathBuf,
    server: Option<PathBuf>,
    probe: RuntimeProbeReport,
    managed_components: Vec<ComponentId>,
}

enum RuntimeSource {
    Managed,
    System,
}

enum RuntimeBackend {
    Cpu,
    Cuda,
    Vulkan,
    OpenVino,
    Rocm,
    Unknown,
}

struct WhisperTuning {
    threads: NonZeroUsize,
    beam_size: u8,
    best_of: u8,
    no_fallback: bool,
}

struct WhisperWorkerKey {
    runtime_identity: FileIdentity,
    model_identity: FileIdentity,
    vad_identity: Option<FileIdentity>,
    backend: RuntimeBackend,
    tuning: WhisperTuning,
    managed_generations: Vec<GenerationIdentity>,
}

struct WhisperRunTelemetry {
    mode: WhisperRunMode,
    total_ms: u64,
    audio_encode_ms: u64,
    queue_ms: u64,
    process_start_ms: Option<u64>,
    model_load_ms: Option<u64>,
    inference_ms: u64,
    parse_ms: u64,
    attempts: Vec<WhisperAttemptTelemetry>,
    runtime: RuntimeIdentity,
}

enum WhisperRunMode {
    ColdCli,
    ResidentFirst,
    ResidentWarm,
    ColdFallback,
}
```

`WhisperExecutionPlan` is the only production owner of tuning, backend, model, VAD, and protocol. `SpeechRuntimeInventory` discovers candidates and managed provenance. A pure policy selects one candidate and one tuning value. `WhisperEngine` executes the plan and does not invent flags.

The broker is a hidden, user-scoped Echo process. The first client starts it under an exclusive state lock. It launches one `whisper-server` bound to loopback on a random port and random request path. It disables conversion, does not expose a public directory, uses no previous audio context, serves one request at a time, and exits after a bounded idle period. The state file is atomic and user-only.

The worker key uses file and managed generation identities. A model, runtime, VAD, tuning, or backend change creates a new key. The broker acquires its own managed leases before reporting ready. The launching client retains its leases until that handshake completes.

Cold and resident paths set beam size, best-of, fallback, threads, language, prompt, VAD, and no-context explicitly. Upstream CLI and server defaults differ, so implicit defaults are not comparable.

Cancellation is destructive when necessary. If the server cannot prove it returned to a clean idle state, the broker kills it, removes its state record, and lets the next request start a fresh worker. One cold retry is allowed. Performance fallback may change runtime backend but never model.

## Module map

- `crates/echo/src/stt/whisper.rs` owns Whisper request execution and response parsing.
- `crates/echo/src/stt/whisper_plan.rs` owns the execution plan, tuning, worker key, and argument parity.
- `crates/echo/src/stt/runtime.rs` owns candidate discovery, backend provenance, precedence, and managed paths.
- `crates/echo/src/stt/whisper_probe.rs` owns real runtime and backend probes.
- `crates/echo/src/stt/whisper_broker.rs` owns broker state, server lifecycle, queueing, idle shutdown, cancellation, and lease handoff.
- `crates/echo/src/transcribe.rs` prepares one resolved plan and keeps caller behavior unchanged.
- `src-tauri/src/cli.rs` validates advanced benchmark overrides and hosts the hidden broker command.
- `scripts/benchmark-stt.py` owns cold quality and latency comparison.
- A resident probe script owns first and warm request comparison.
- An acceleration probe script owns backend comparison.

## Synthesis decision

Candidate B is the base because its `WhisperExecutionPlan` gives tuning, backend, protocol, and artifacts one owner. Candidate C contributes separate cold, resident, and acceleration reports and the rule that uncertain cancellation destroys the worker. The cross-judge scored Candidate B 28 out of 30 and Candidate C 23 out of 30.

Arena verification rejected an in-process worker. GNOME fallback dictation runs in separate CLI processes, so it would repeatedly lose the loaded model. The selected resident shape is an on-demand cross-process broker. Verification also confirmed that the pinned upstream runtime archive already contains `whisper-server`; the current inventory generator simply does not select it.

## Tradeoffs accepted

- We keep the cold CLI path indefinitely in exchange for a reliable fallback and a stable comparison boundary.
- We add an internal broker only after a measured gate in exchange for speeding GUI, CLI, and GNOME shortcut sessions equally.
- We keep one loaded model per user in exchange for bounded memory and simple lease ownership.
- We use a loopback server child in exchange for reusing the pinned upstream binary. The broker hides its random endpoint and accepts no remote traffic.
- We test accelerated system runtimes before packaging one in exchange for learning which backend earns long-term support.
- We accept separate performance reports in exchange for honest cold, warm, and hardware claims.

## Alternatives considered

- Direct flags in `whisper_args` lost because policy would leak into callers and benchmark code.
- C++ FFI lost because unsafe integration and in-process lifetime work arrive before measurement, and it misses separate shortcut processes.
- A desktop-only worker lost because GNOME fallback and CLI sessions are separate processes.
- A system service lost because installation and recovery burden is not justified before residency is measured.
- A worker that still spawns `whisper-cli` per request lost because it does not preserve the loaded model.
- Several managed GPU variants lost because the catalog and support cost should follow evidence, not precede it.

## Open questions and risks

- Does `whisper-server` expose enough timing detail, or must Echo classify upstream stderr into optional load and inference fields?
- Which backend appears often enough in real Echo hardware to earn the first managed accelerator artifact?
- What idle timeout keeps repeated dictation fast without unacceptable memory residency on 8 GiB machines?
- Does `whisper-server` preserve exact output and prompt behavior when CLI and server decoding parameters are aligned?
- Can every managed generation identity be reacquired safely by the broker before the launching client releases its lease?

## Next implementation step

Add split, backward-compatible Whisper telemetry to the existing one-shot path and preserve it in CLI JSON and benchmark JSONL.
