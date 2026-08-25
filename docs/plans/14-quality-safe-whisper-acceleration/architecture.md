# Architecture

## Caller seam

Recording, GNOME fallback, and file transcription continue to converge on `prepare_with_config`. Backend policy does not move into the recorder, HUD, or CLI.

Qualification tooling builds explicit plans. Production selection remains CPU-only until Phase 7. The existing two-argument `prepare_with_config` API stays in place until the selection phase and then becomes a production-mode wrapper.

## Evidence boundary

Benchmark rows are references to raw artifacts, not authority. A run bundle owns:

- A status record with `running`, `failed`, or `complete` state.
- The Echo binary, runtime, adjacent libraries, model, VAD, and corpus digests.
- The exact command and child-only environment.
- Raw stdout, stderr, product JSON, and timing for each observation.
- Cache snapshots and reset evidence.
- An inference-process runtime receipt.

The verifier recalculates transcript normalization, WER or CER, silence hallucination, pairing, coverage, and identity. Stored derived values can aid inspection but cannot change the verdict.

## Runtime receipt

The inference process emits one machine-readable receipt after backend creation. The receipt contains:

- The observed backend and selected index.
- Vendor ID, device ID, API version, and driver version.
- Device UUID, driver UUID, and pipeline cache UUID when the backend exposes them.

Loader logs bind the receipt to the selected backend index; `vulkaninfo` can
corroborate it. Neither replaces the receipt. The receipt does not prove a
selected ICD manifest or loaded runtime-library digests: those are launch
evidence. A normal upstream runtime without enough proof remains ineligible.

The instrumentation patch stays narrow, reproducible, and pinned to the whisper.cpp source revision. If upstream gains an equivalent receipt, Echo deletes the patch.

## Launch contract

One child-only launch contract owns runtime library directories, selected driver manifests, and backend cache roots. The launcher removes inherited loader-affecting variables before applying the recorded contract. It never mutates the parent environment.

Benchmarks, probes, and product execution call the same launcher. A runtime that works only from an operator shell is ineligible.

## Admission identity

The canonical identity key covers:

- Echo binary and commit.
- whisper.cpp binary, revision, and adjacent libraries.
- Model and optional VAD.
- Protocol, tuning, language policy, and prompt policy.
- Observed backend and physical-device receipt.
- Driver and selected ICD artifacts.
- The launch-contract schema.

Cache class belongs to qualification evidence. Boot identity belongs to reset evidence. Neither field is a stable product identity wildcard.

## Selection and recovery

Discovery never implies eligibility. Selection follows this order:

1. Prepare and lease the managed CPU floor.
2. Build the exact accelerated identity under the production launch contract.
3. Reject unknown, incomplete, stopped, expired, changed, software-rendered, or quarantined identities.
4. Select an accelerator only for an exact passed identity.

If an admitted accelerator fails, reports the wrong identity, returns malformed output, or violates the launch contract, Echo quarantines that exact identity. Echo then runs one same-model managed CPU logical retry. It does not retry another GPU or switch models.

The admission record stays outside the Echo executable. Linux packages own the record and the accelerator payload. Echo rejects records or payload files that are not owned by root or that a non-root user can write. This boundary avoids a self-referential build where adding the record changes the executable hash that the record admits.

The current qualification used pinned languages and an empty prompt. Automatic language detection and recognition hints stay on managed CPU until paired tests admit those policies.

## Packaging boundary

Managed CPU and each accelerator are separate components. An accelerator component contains the whisper runtime and adjacent non-driver libraries. The host owns GPU drivers and ICDs.

Release packaging uses the executable that passed qualification. `cargo tauri bundle` creates packages around that existing executable and does not compile a replacement. The release gate extracts each package and compares its executable hash with the admission record before publication.

Vulkan, CUDA, ROCm, and OpenVINO remain separate variants. OpenVINO also binds encoder IR and compiled-cache evidence. A pass never crosses variants.

## Residency boundary

Residency is not required for one-shot acceleration. A future broker owns one exact identity, one active request, readiness, cancellation, leases, and a bounded idle timeout. It enters only after the same identity passes warmed one-shot quality and both resident latency thresholds.
