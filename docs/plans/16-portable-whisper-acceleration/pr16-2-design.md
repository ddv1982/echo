# PR 16.2 identity and release binding design

## Decision

PR 16.2 separates reusable Whisper evidence from the Echo application release.
It adds five strict v3 records:

- `ExecutionArtifactId` identifies the portable runtime package.
- `InferenceContractId` identifies the model and inference behavior.
- `LocalEnvironmentKey` identifies the GPU and driver environment.
- `PerformanceEvidenceId` identifies one measured combination of the first three records.
- `ReleaseBindingId` identifies one exact Echo ELF and package type.

PR 16.2 does not change production GPU selection.
Existing v2 resources and the exact-machine selector remain active.
PR 16.3 consumes the v3 records for local selection.
PR 16.5 deletes v2 resources and code.

This timing keeps current accelerated packages working while app-only staging gains evidence reuse.

## Caller contract

The product caller keeps the current operation:

```rust
let decision = production_whisper_decision(managed_cpu_plan);
```

PR 16.2 does not add Auto, GPU, or CPU modes.
It does not change the managed CPU plan, the qualified Vulkan plan, or recovery behavior.

Release staging accepts a reusable v3 evidence package:

```sh
python3 scripts/stage-qualified-whisper-release.py \
  --canonical-binary target/release/echo-desktop \
  --reusable-evidence target/whisper-release/reusable-v3 \
  --output target/whisper-release/assets \
  --version X.Y.Z \
  --commit "$commit"
```

For an app-only change, staging prints these facts:

```text
physicalRequalificationRequired=false
reusedInferenceEvidence=true
executionArtifactId=<unchanged>
inferenceContractIds=<unchanged>
releaseBindingIds=<new Debian and RPM bindings>
```

## Canonical content IDs

All five records use this algorithm:

```text
SHA256(domain-prefix || canonical-json-utf8)
```

Each domain prefix ends with a NUL byte:

```text
echo-whisper-execution-artifact-v3\0
echo-whisper-inference-contract-v3\0
echo-whisper-local-environment-v3\0
echo-whisper-performance-evidence-v3\0
echo-whisper-release-binding-v3\0
```

Canonical JSON follows these rules:

- Reject duplicate keys and unknown fields before hashing.
- Reject floats and integers outside each schema's bounds.
- Sort object keys by their ASCII names.
- Sort set-like arrays before hashing.
- Keep ordered test cases in their declared order.
- Encode UTF-8 with no whitespace and no ASCII escaping.
- Exclude the derived ID field from its own preimage.
- Use lowercase SHA-256 strings.

Rust builds a sorted canonical JSON value before hashing.
Python uses `json.dumps` with `sort_keys=True`, compact separators, `ensure_ascii=False`, and `allow_nan=False` after schema validation.

Both languages load the same committed fixtures.
The fixtures include the canonical bytes and the expected ID for each record.

## Independent inputs

### Execution artifact

`ExecutionArtifactId` contains only shipped runtime facts:

```json
{
  "schemaVersion": 3,
  "runtimeArtifactId": "<PR 16.1 artifact ID>",
  "runtimeIdentitySha256": "<Whisper runtime identity>",
  "runtimeRelativePath": "runtime/whisper-cli",
  "runtimeSha256": "<sha256>",
  "runtimeLibraryBindings": {"libggml.so": "<sha256>"},
  "probeRelativePath": "runtime/echo-whisper-runtime-probe",
  "probeSha256": "<sha256>",
  "buildReceiptSha256": "<sha256>",
  "reusableInventorySha256": "<sha256>"
}
```

It excludes the model, VAD, tuning, hardware, Echo commit, Echo ELF, app version, and package marker.

PR 16.1 `artifactId` and the existing `runtimeIdentitySha256` use different formulas.
The v3 execution record binds both values until PR 16.5 removes the v2 launch identity.

### Inference contract

`InferenceContractId` contains only inference inputs and behavior:

```json
{
  "schemaVersion": 3,
  "protocol": "oneShotCli",
  "modelSha256": "<sha256>",
  "vadSha256": "<sha256 or null>",
  "tuning": {"threads": 4, "beamSize": 1, "bestOf": 2, "noFallback": true},
  "requestPolicy": {"language": "pinned", "prompt": "empty", "hints": "qualifiedOnly"},
  "behavior": {
    "launchSchema": 1,
    "receiptSchema": 1,
    "telemetrySchema": 1,
    "recoverySchema": 1,
    "projectionSha256": "<sha256>"
  },
  "claimScope": "product-stt-corpus-v1"
}
```

It excludes runtime bytes, hardware, cache contents, Echo release identity, and performance results.

### Local environment

`LocalEnvironmentKey` contains only hardware and driver identity:

```json
{
  "schemaVersion": 3,
  "architecture": "x86_64",
  "backend": "vulkan",
  "vendorId": 32902,
  "deviceId": 18086,
  "apiVersion": 4211006,
  "driverVersion": 104865800,
  "deviceUUID": "<32 lower hex>",
  "driverUUID": "<32 lower hex>",
  "pipelineCacheUUID": "<32 lower hex>",
  "drmDriver": "i915",
  "icdManifestSha256": "<sha256>",
  "icdLibrarySha256": "<sha256>"
}
```

It excludes `selectedIndex`, absolute ICD paths, runtime bytes, model inputs, Echo release identity, and cache seeds.
The environment record may store launch paths outside the hashed key while v2 exact selection remains active.

### Performance evidence

`PerformanceEvidenceId` joins the three independent IDs with immutable measurement facts:

```json
{
  "schemaVersion": 3,
  "executionArtifactId": "<id>",
  "inferenceContractId": "<id>",
  "localEnvironmentKey": "<id>",
  "measurementProtocol": "paired-product-sweep-v2",
  "corpusManifestSha256": "<sha256>",
  "coverageManifestSha256": "<sha256>",
  "observationBundleSha256": "<sha256>",
  "cacheCycleSha256": "<sha256>",
  "gatePolicySha256": "<sha256>",
  "acceptedAt": 0,
  "expiresAt": 0
}
```

It excludes Echo commit, Echo ELF, version, and package marker.
Changing any referenced runtime, contract, environment, measurement, or gate fact creates a new evidence ID.

### Release binding

`ReleaseBindingId` is the only ID that contains app and package facts:

```json
{
  "schemaVersion": 3,
  "packageType": "deb",
  "version": "0.12.5",
  "echoCommit": "<40 lower hex>",
  "echoBinarySha256": "<sha256>",
  "bundleMarker": "deb",
  "accelerationSetSha256": "<sha256>",
  "executionArtifactId": "<id>",
  "allowedInferenceContractIds": ["<id>"],
  "allowedPerformanceEvidenceIds": ["<id>"],
  "reusableInventorySha256": "<sha256>"
}
```

The embedded release binding excludes the final package digest to avoid a hash cycle.
The outer `qualified-release.json` binds each Debian and RPM asset digest to its embedded release binding.

## Reusable package layout

Promotion creates one reusable tree:

```text
whisper-acceleration/
  acceleration-set.v3.json
  runtime/
  cache-seeds/<performance-evidence-id>/
```

Staging adds one package-specific file:

```text
whisper-acceleration/release-binding.v3.json
```

`acceleration-set.v3.json` contains the execution record, inference contracts, environment records, performance evidence, cache references, gates, and a reusable inventory.
It is byte-identical across app-only releases.

The Debian and RPM packages contain different release bindings because their ELF bytes and markers differ.
Both bindings point to the same reusable evidence IDs.

## Behavior guard

The guard combines a production value projection with a conservative source check.

The Rust projection emits these values from the functions and constants that production uses:

- launch environment keys and CLI argument policy;
- decode defaults and request policy;
- required receipt fields and backend truth rules;
- telemetry fields used by admission;
- recovery validation, quarantine, and retry policy.

The projection digest is part of `InferenceContractId`.
Rust and Python fixture tests require the same projection.

A CI script also compares base and head changes in the owned launch, decode, receipt, telemetry, and recovery files.
If one of those files changes, either the projection and inference contract must change or the guard fails.
Version, documentation, frontend, icon, package marker, and release metadata changes do not trigger the guard.

## PR boundaries

PR 16.2 performs these changes:

1. Add strict Rust and Python v3 schemas and cross-language fixtures.
2. Add reusable execution, contract, environment, and evidence records.
3. Make promotion write v3 evidence beside the existing v2 package.
4. Make staging create and verify exact Debian and RPM release bindings.
5. Add the behavior guard and invalidation matrix.
6. Keep the v2 production selector and current plan behavior unchanged.

PR 16.3 reads the v3 records for local receipt-driven selection.
PR 16.4 adds Auto, GPU, and CPU modes after the hardware matrix and operator review.
PR 16.5 deletes `AdmissionIdentity`, `AdmissionIdentityKey`, `admission-set.json`, shipped cache seeds, and the `qualification-$commit` release flow.

`whisper_acceleration.rs` does not receive a selector rewrite in PR 16.2.
It may receive narrow tests or compile plumbing only.

## Invalidation truth table

| Change | Execution | Inference | Environment | Evidence | Release binding | Physical sweep |
| --- | --- | --- | --- | --- | --- | --- |
| Version, Echo commit, app ELF, Debian marker, or RPM marker | same | same | same | same | changes | no |
| Runtime CLI, library, probe, or build receipt | changes | same | same | changes | changes | yes |
| Model, VAD, tuning, request policy, or behavior projection | same | changes | same | changes | changes | yes |
| Device, driver, UUID, pipeline cache, or ICD digest | same | same | changes | changes | unchanged until rebound | yes |
| Corpus, measurement protocol, observation bundle, or gate policy | same | same | same | changes | changes | yes |

Old evidence remains an immutable historical object when an ID changes.
Staging refuses to bind that evidence to the new inputs.

## Verification

Rust and Python load the same valid and invalid fixture corpus.
The fixtures cover all five records, permuted input keys, every independent mutation, duplicate keys, unknown fields, invalid paths, invalid digest casing, and out-of-range integers.

The ten live lanes from the program plan remain the acceptance criteria.
Lanes 1 through 3 must reuse evidence and create new bindings without a sweep, a cache-cycle run, a GPU probe, or a reboot.
Lanes 4 through 9 must invalidate the correct ID or fail the behavior guard.
Lane 10 must extract fresh Debian and RPM packages and verify each exact ELF against an unchanged reusable evidence set.

Staging performance records three cases:

- the current v2 baseline and every physical command it requires;
- one cold v3 staging run with unchanged evidence;
- two warm v3 staging runs with the same evidence.

Every v3 app-only run must report zero physical commands and zero reboots.
Every run still performs exact package extraction and release binding verification.
The warm v3 run must not exceed the current package-verification time.

## Synthesis record

Candidate 1 supplied the ID boundaries, package layout, release binding, and value-based guard.
Candidate 3 supplied the independent-input mutation matrix and the rule that `InferenceContractId` excludes runtime bytes.
Candidate 4 supplied the staged migration that preserves v2 until PR 16.5.

The design rejects these choices:

- It does not put execution or inference IDs inside `LocalEnvironmentKey`.
- It does not put the final package digest inside the embedded binding.
- It does not force PR 16.2 to return managed CPU for every package.
- It does not delete v2 before PR 16.5.
- It does not use a source-path-only behavior guard.

## Adversarial review synthesis

The pre-PR interrogate review found that the first implementation kept several v3 invariants in caller convention. Four independent reviewers reproduced stale behavior reuse, incomplete release bindings, arbitrary claim-scope backfill, and weak ELF commit detection. Two reviewers also confirmed that a v3-only package is not consumed by the PR 16.2 production selector.

An architect arena selected a shared contract-authority boundary as the repair:

- Promotion derives the complete expected inference contract from measured model, VAD, tuning, fixed policy, fixed claim scope, and the behavior fixture at the measured commit. The caller-supplied contract must match exactly.
- Staging verifies every reusable contract against the behavior fixture at the commit being packaged.
- Release bindings exactly cover every contract and performance record in the packaged acceleration set.
- The ELF contains one framed build-commit marker. Raw commit substrings are not identity evidence.
- `transcribe.rs` is part of the watched inference-behavior surface.
- PR 16.2 v3 packages are proof-only until PR 16.3 adds the receipt-driven selector. Their embedded binding and outer manifest record that status, and production-ready verification rejects them.

Candidate B supplied the base because it bound proof-only readiness into the package and verifier. Candidate A supplied the immutable verified-contract result, exact marker grammar, and explicit composed-set claim-scope checks. The design rejected an early v3 selector and release-specific v2 rebinding because both would cross the approved PR boundary.
