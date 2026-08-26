# Large Turbo acceleration plan

## Definition of done

Large v3 Turbo acceleration is complete only when all of these predicates are true:

- A short screen on this laptop proves the exact Q5_0 model uses the admitted Vulkan device, keeps VAD active, preserves transcript quality, improves both median and p95 latency, and passes the memory and stability limits.
- Two cache cycles from distinct Linux boot IDs bind the same Large model, runtime, VAD, decoding, device, ICD, and populated cache identity.
- Ten randomized CPU/Vulkan pairs per corpus fixture pass every existing research and binding gate for both Debian and RPM executable variants built from exact merged `main`.
- One package contains independently verifiable Small and Large admission records and cache seeds without duplicating the runtime.
- Runtime selection chooses exactly one matching record. Missing, malformed, duplicate, expired, stopped, or changed records stay on managed CPU.
- Small and Large quarantine keys remain independent, and either accelerated failure gets one same-model CPU retry.
- Full product verification, adversarial review, PR review, and CI pass on the exact merged code.

An available device, a successful one-file transcription, or an upstream benchmark is not completion.

## Rigor

This is a high-rigor run. A false pass can regress transcription quality, exhaust shared memory, or replace the already qualified Small path. The work changes production selection and Linux release contents, so every phase ends in a rerunnable artifact and a stop decision.

## Phases

### 1. Ground and design

Trace runtime selection, admission, recovery, benchmarks, promotion, packaging, and release verification. Use an architecture arena to compare multi-admission package shapes and memory telemetry boundaries.

Acceptance:

- The grounding names every exact identity and fail-closed boundary.
- The chosen design has one source of truth for package inventory and no “closest record” selection.
- An independent judge scores the candidates before implementation.

### 2. Build the screen lever

Add reusable one-shot memory and stability telemetry to the product benchmark path. Record child peak RSS, minimum host available memory, swap delta, exit status, timeout state, backend receipt, and artifact identity.

Acceptance:

- Deterministic self-tests fail before the new fields and pass after them.
- Existing benchmark bundles remain replayable or migrate in the same change.
- The screen emits `VERIFIED`, `NOT VERIFIED`, or `INCONCLUSIVE` from explicit gates.

### 3. Screen Large Turbo

Download the pinned catalog artifact, verify its SHA-256, capture Large cache cycle A, and run one randomized pair per full product fixture through the shipping CLI.

Proceed only when:

- Every invocation succeeds within its timeout and parses valid JSON.
- Vulkan backend and receipt match the exact Intel device.
- VAD stays active and CPU/Vulkan language outputs pass the existing quality gates.
- Median improves by at least 20 percent and 500 ms, and Vulkan p95 is lower.
- No new silence hallucination appears.
- No OOM or new sustained swap growth appears, and the host retains the documented memory floor.

Stop on any failed or inconclusive gate. Do not start the full run to explain away a failed screen.

### 4. Implement multi-admission packaging

Implement the arena-selected registry, exact runtime selection, promotion composition, package verification, and release manifest changes. Keep legacy single-record support only if the design proves an upgrade path needs it.

Acceptance:

- Tests cover Small-only, Large-only, both, no match, duplicate match, malformed record, path escape, changed seed, changed runtime, and independent quarantine.
- Debian and RPM fixtures deep-verify every record and seed.
- Ordinary builds without admissions stay on CPU.

### 5. Verify and merge the implementation

Run deslop, comment review, full product verification, and multi-model interrogation. Open a review-ready PR, fix valid findings and CI, then merge only when GitHub reports it mergeable and green.

Acceptance:

- The decision trail resolves to committed evidence.
- Independent reviewers find no unresolved correctness issue.
- The exact merge commit passes both normal and package workflows.

### 6. Qualify exact merged artifacts

After a real reboot, capture Large cache cycle B. Build Debian and RPM executable variants from exact merged `main`. Requalify Small and qualify Large with ten pairs per fixture for each variant. Compose and deep-verify the dual-admission packages.

Acceptance:

- Every Small and Large sweep returns `PROCEED` with all gates true.
- Package extraction reproduces each measured ELF, record, runtime alias map, probe, and cache seed.
- The final manifest binds the exact commit and complete package inventory.

## Throughput checkpoint

The checkpoint is after the Large screen and before implementation plus full qualification.

Report:

- Exact model, runtime, VAD, executable, device, ICD, and boot identities.
- Fixture and observation counts.
- CPU and Vulkan median and p95 latency.
- Per-language quality deltas and silence hallucinations.
- Peak RSS, minimum available memory, and swap delta.
- Estimated duration and disk use for the four final package sweeps.
- Whether a second boot is still required.

## Stop gates

- Stop if the model, runtime, VAD, device, ICD, receipt, or decoding identity changes.
- Stop if memory evidence is absent, noisy enough to be inconclusive, or shows pressure beyond the screen limits.
- Stop if quality, latency, p95, stability, or cache gates fail.
- Stop if the package selector can choose more than one matching record or accept a partial inventory.
- Stop before full qualification until a distinct physical boot exists.
- Stop release work unless the user requests a release after merged qualification.
