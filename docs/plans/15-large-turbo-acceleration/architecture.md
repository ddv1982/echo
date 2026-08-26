# Large Turbo acceleration architecture

## Caller view

The product call does not change:

```rust
let decision = production_whisper_decision(managed_cpu);
```

The selector computes the live identity from the managed plan and returns Vulkan only when exactly one package record matches every field. Callers never choose a record, model label, backend, or “closest” identity.

The screen also keeps the existing entrypoint:

```sh
python3 scripts/sweep-whisper-admission.py \
  --model-name large-v3-turbo-q5_0 \
  --model-path "$MODEL" \
  --repeats 1 \
  --require-resource-evidence \
  --minimum-available-memory-bytes "$FLOOR" \
  --maximum-sustained-swap-growth-bytes "$ALLOWANCE" \
  ...
```

One pair per fixture is the screen. Ten pairs per fixture is the full qualification. Both use the shipping CLI, the same raw observations, and the same replay path.

## One-shot resource evidence

`scripts/process_observation.py` owns one child process and its Linux resource samples. `benchmark-stt.py` remains the measurement entrypoint and calls this module instead of `subprocess.run`.

```python
@dataclass(frozen=True)
class ProcessTreeSample:
    monotonic_ns: int
    rss_bytes: int | None
    vm_swap_bytes: int | None
    mem_available_bytes: int | None
    host_swap_used_bytes: int | None

@dataclass(frozen=True)
class ProcessObservation:
    schema_version: int
    sampling: Literal["complete", "partial", "unavailable"]
    process_tree_peak_rss_bytes: int | None
    process_tree_peak_swap_bytes: int | None
    host_min_mem_available_bytes: int | None
    host_swap_used_before_bytes: int | None
    host_swap_used_peak_bytes: int | None
    host_swap_used_after_settle_bytes: int | None
    sample_count: int
    interval_ms: int
    exit: Literal["success", "nonzero", "signaled", "timeout", "spawn-failed"]
```

The module starts the command in a new process group, walks `/proc/<pid>/task/<pid>/children`, and sums `VmRSS` and `VmSwap` across the observed tree. It samples `MemAvailable` and `SwapTotal - SwapFree` from `/proc/meminfo`. A timeout kills the process group and remains a failed observation.

The sampler records raced or unreadable `/proc` data as partial evidence. It never replaces a missing value with zero.

Each raw observation gets a digest-addressed `process-observation.json`. The normalized row contains the artifact path and digest. Replay verifies the artifact before using its fields.

This is process and host pressure evidence. It is not GPU memory telemetry. Intel Iris Xe uses shared system memory, and driver-owned allocations may not appear in process RSS. The claim remains limited to the measured process tree, host memory floor, swap delta, exits, and timeouts.

## Three-valued resource decision

The analyzer produces one of these results:

- `VERIFIED`. Every observation has complete samples, succeeds, respects the host memory floor, and stays within the sustained swap allowance.
- `NOT VERIFIED`. An observation times out, exits unsuccessfully, fails parsing, is killed, breaches the memory floor, or retains swap growth beyond the allowance after the settle window.
- `INCONCLUSIVE`. Sampling is absent or partial, the process disappears before a useful sample, or host noise prevents a sustained-swap decision.

Screen and full-sweep decisions require `VERIFIED`. `INCONCLUSIVE` never passes.

The existing screen excludes only the ten-pair sample-size gate. Resource, quality, backend, receipt, cache, and identity gates remain mandatory.

Admission schema v2 adds these gates:

```rust
pub struct AdmissionGates {
    // Existing gates remain.
    pub stability_success: bool,
    pub memory_evidence: bool,
    pub memory_floor: bool,
    pub swap_stable: bool,
}
```

Promotion replays the raw resource artifacts and requires the exact gate set. Rust only accepts a record when every gate is true.

## Package shape

One package has one shared Vulkan runtime and any number of exact admission records:

```text
whisper-acceleration/
  admission-set.json
  runtime/whisper-cli
  runtime/echo-whisper-runtime-probe
  runtime/lib*.so*
  cache-seeds/<identity-key>/...
```

`admission-set.json` is a strict schema. Duplicate JSON keys, unknown fields, empty record sets, duplicate identity keys, duplicate exact identities, duplicate cache paths, and incompatible runtime identities reject the whole package.

```rust
struct AdmissionSet {
    schema_version: u32,
    shared: SharedRuntimeArtifacts,
    records: Vec<ModelAdmission>,
    inventory: Vec<PackageEntry>,
}

struct SharedRuntimeArtifacts {
    runtime_relative_path: SafeRelativePath,
    runtime_library_bindings: BTreeMap<LibraryName, Sha256>,
    probe_relative_path: SafeRelativePath,
    probe_sha256: Sha256,
}

struct ModelAdmission {
    identity: AdmissionIdentity,
    identity_key: AdmissionIdentityKey,
    evidence_sha256: Sha256,
    cache_seed: CacheSeedArtifact,
    gates: AdmissionGates,
    verdict: AdmissionVerdict,
    accepted_at: UnixSeconds,
    expires_at: UnixSeconds,
}

struct PackageEntry {
    path: SafeRelativePath,
    kind: PackageEntryKind,
    bytes: u64,
    sha256: Option<Sha256>,
    link_target: Option<SafeRelativePath>,
}
```

The inventory lists every owned file and symlink below the resource root except `admission-set.json` itself. Directories derive from contained paths. Package verification rejects missing entries, extra entries, type changes, digest changes, link changes, and escapes.

Small, Large, and future hardware records may share a model digest. Selection uses the full identity, not model cardinality. Two records for the same model on different devices are valid. Two records that both match the same live identity are unsafe and disable the package.

There is no legacy `admission.json` read path. A new executable cannot match an old record because the executable hash changed. Coordinated re-promotion of Small gives the new release one unambiguous schema.

## Exact selection

```rust
impl AdmissionSet {
    fn load(root: &Path) -> Result<Self, PackageError>;
    fn verify_inventory(&self, root: &Path) -> Result<VerifiedAdmissionSet, PackageError>;
}

impl VerifiedAdmissionSet {
    fn select(&self, context: &SelectionContext) -> AdmissionSelection;
}

enum AdmissionSelection {
    Exact(VerifiedAdmission),
    NoMatch,
    Unsafe(PackageError),
}
```

Loading validates the whole package before selection. Selection hashes the executable, managed model, VAD, runtime, ICD manifest, and ICD library once. It observes the DRM inventory independently of package records. It constructs candidate live identities and accepts one complete equality match.

Zero matches stays on managed CPU. More than one match, any invalid record, or any invalid inventory also stays on managed CPU. The selector never uses filename, model label, vendor-only matching, record order, acceptance date, or best effort.

After exact selection, the existing code copies that record’s cache seed into the identity-keyed user cache, runs the live receipt probe, and builds the same Vulkan primary plus same-model managed CPU fallback.

Quarantine stays keyed by `AdmissionIdentityKey`. Small and Large failures remain independent without a new quarantine schema.

## Promotion and composition

One sweep still promotes one admission fragment. A deterministic composer owns the package set:

```sh
python3 scripts/compose-whisper-admission-set.py \
  --promotion small-promotion \
  --promotion large-promotion \
  --output dual-promotion
```

The composer requires the same executable, commit, runtime identity, probe, library bindings, VAD contract, and package type across inputs. It copies the runtime once, copies each cache seed under its identity key, rejects duplicates, writes the complete inventory, and verifies its own output. It always writes a fresh directory and never mutates a prior promotion.

`stage-qualified-whisper-release.py` accepts one composed promotion per package type. Debian and RPM extraction verify every record, seed, runtime alias, probe, inventory entry, executable variant, and release-manifest identity.

## Module map

- `scripts/process_observation.py` owns process groups, `/proc` parsing, sampling, and resource evidence.
- `scripts/benchmark-stt.py` owns the shipping observation and writes the resource artifact.
- `scripts/analyze-stt-host-matrix.py` replays resource evidence and applies explicit limits.
- `scripts/sweep-whisper-admission.py` joins resource results to the existing screen and full decisions.
- `scripts/promote-whisper-admission.py` promotes one exact record with resource gates.
- `scripts/compose-whisper-admission-set.py` composes shared artifacts and records into one fresh package set.
- `crates/echo/src/stt/whisper_admission.rs` owns strict v2 domain types and exact state checks.
- `crates/echo/src/stt/whisper_acceleration.rs` loads one set and turns one exact record into the existing plan.
- `scripts/stage-qualified-whisper-release.py` deep-verifies the composed package and release manifest.

## Verification matrix

Resource tests cover successful exit, nonzero exit, signal, timeout and process-group kill, spawn failure, no `/proc`, process exit before the first sample, partial child walks, known RSS peak, existing swap baseline, persistent swap growth, settle recovery, and memory-floor breach.

Selector tests cover Small only, Large only, both models, several hardware records for one model, no match, duplicate exact match, duplicate key, malformed unrelated record, expired record, false gate, changed runtime, changed probe, changed seed, path escape, extra inventory entry, missing inventory entry, and independent quarantine.

Composition and package tests cover one shared runtime, incompatible inputs, deterministic record order, record and seed digest drift, contained aliases, Debian extraction, RPM extraction, and exact final manifest inventory.

The real hardware screen proves the exact Large model, VAD, decoding, runtime, executable, Intel device, ICD, quality, latency, p95, resource result, and boot identity. Full promotion waits for the second boot and ten-pair package sweeps.
