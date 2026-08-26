# Large Turbo screen result

## Verdict

The original 28-fixture greedy screen justified building the multi-admission path, but it is no longer current qualification evidence. The review-hardened analyzer requires a pre-launch host baseline plus a row and command binding that the original observations did not record. A later `cargo clean` removed that local bundle, its first cache cycle, and four consented natural-speech captures, so the historical result cannot be migrated or replayed honestly.

An exact-implementation-head replacement screen now verifies the corrected measurement path on all twenty pinned multilingual FLEURS fixtures. It is `INCOMPLETE`, not admitted: it has one pair per fixture, no product-class coverage, and no retained earlier cache cycle for distinct-boot reset evidence.

No result in this document authorizes production selection, promotion, packaging, or a release.

## Exact implementation-head replacement screen

The replacement used implementation commit `3d9c9c8f430862457da202caef9866c4917d6f78` and binary SHA-256 `02a409725318be7392c580746d0f46729e1cf2deabacafa19b525d03c8a28e15`. The follow-up commit changes only this result document and its audit row. The model, runtime, VAD, decoding, Intel device, and ICD identities match the historical screen. The Linux boot ID is `f4fd3d6f-e34d-4da7-a540-97426cc4ce67`.

It ran one randomized CPU/Vulkan pair for each of twenty clean-read fixtures across English, Dutch, German, French, and Spanish:

- CPU median: 19,268.614 ms.
- Vulkan median: 6,045.114 ms.
- Paired median reduction: 13,128.700 ms, or 68.485 percent.
- CPU p95: 19,639.832 ms.
- Vulkan p95: 6,277.844 ms.
- Language quality: all five gates pass. English Vulkan WER was 1.136 percentage points lower; the other deltas were zero.
- New silence hallucinations: 0.
- Complete and successful process observations: 40 of 40.
- Maximum simultaneous process-tree RSS: 816,693,248 bytes.
- Maximum process-tree swap: 0 bytes.
- Minimum host available memory: 18,317,242,368 bytes.
- Maximum peak and sustained host swap growth: 0 bytes.

Fresh analyzer replay accepts every strict observation field, raw-sample aggregate, command digest, and row binding. The screen still stops because `sampleSize`, `coverageComplete`, and `resetEvidence` are false. Recreating those facts requires new natural speech captures and another retained physical boot, not a compatibility exception.

## Historical implementation screen

### Exact identity

- Echo commit: `69e4e65b718e94b0e618d5f069fdc33343510d79`.
- Echo binary: `886911281522a9b0d4ca629926b27c05fc5ef113be7ef751cd18f91d80c4e3c7`.
- Runtime CLI: `37382797bcad4b4bab155d4a59ac2d41664da19a9f7553c3838052d9efe59199`.
- Runtime identity: `d1f1e46f05be7e768c5338aa45bef07cb1132a4f75a6a84df24195ee4c27fb69`.
- Model: `large-v3-turbo-q5_0`, `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2`.
- VAD: `2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987`.
- Decoding: four threads, beam size 1, best-of 2, no temperature fallback.
- Policy: pinned language, empty prompt, VAD active.
- Device: Intel `8086:46a6`, `i915`, Vulkan index 0.
- ICD manifest: `09e7ca55461c3f2d65e5df6a6b8f06a7ce2c86fc58a93a18d2dbe3575623de83`.
- Boot: `e8f95880-665d-4ab7-9d8e-afab394beac2`.

### Hypothesis loop

### Beam 3, best-of 5, fallback enabled

The first 56-observation screen measured a 67.6 percent median reduction and lower p95. It stopped because German, English, and French exceeded the allowed CPU/Vulkan WER delta. Resource evidence was also inconclusive because the first sampler version treated normal process lifecycle samples as fatal.

Verdict: `NOT VERIFIED`.

### Beam 3, best-of 5, no fallback

Five targeted fixtures that differed in the first screen still produced different CPU and Vulkan transcripts.

Verdict: `NOT VERIFIED`.

### Beam 1, best-of 2, no fallback

The same five targeted fixtures produced exact CPU/Vulkan transcript parity with 67.5 to 69.9 percent latency reductions. A model-bound cache cycle then confirmed stable fresh and populated receipts and transcripts.

Verdict: `VERIFIED` for a full screen.

### Product screen

The screen ran 28 corpus fixtures with one randomized CPU/Vulkan pair each. It covered English, Dutch, German, French, and Spanish plus all eight product-speech classes.

- CPU median: 18,919.669 ms.
- Vulkan median: 5,854.504 ms.
- Paired median reduction: 13,028.497 ms.
- Paired median speedup: 68.908 percent.
- CPU p95: 19,441.525 ms.
- Vulkan p95: 6,263.010 ms.
- New silence hallucinations: 0.
- Per-language quality: all pass. English Vulkan WER was 0.407 percentage points lower; other deltas were zero.
- Backend, device, identity, receipt, pair integrity, coverage, cache, exact runtime, and clean environment: pass.
- Sample size: incomplete by design. The screen has one pair instead of ten.
- Reset evidence: incomplete. A distinct physical boot is still required.

### Resource screen

Every one of the 56 process observations completed successfully.

- Complete observations: 56 of 56.
- Minimum host available memory: 20,049,817,600 bytes.
- Maximum simultaneous process-tree RSS: 817,246,208 bytes.
- Maximum process-tree swap: 0 bytes.
- Maximum peak host swap growth: 0 bytes.
- Maximum sustained host swap growth: 0 bytes.
- Required memory floor: 4,294,967,296 bytes.
- Allowed sustained swap growth: 67,108,864 bytes.

This is Linux process-tree and host-pressure evidence. It is not direct GPU-memory telemetry.

### Throughput checkpoint

The screen earns implementation. The final qualification does not start until a second boot exists and the implementation has merged.

Estimated measured time from the observed medians:

- Large Debian variant: about 116 minutes.
- Large RPM variant: about 116 minutes.
- Small Debian variant: about 30 minutes.
- Small RPM variant: about 30 minutes.
- Total paired measurement: about 4 hours 52 minutes, plus builds, probes, replay, and packaging.

The screen evidence occupied 8.1 MiB. Ten-repeat evidence for both models and both variants is expected to stay below 1 GiB, excluding reusable model and runtime inputs.

## Current stop gate

Do not promote Large until all of these are true:

- A new consented natural-speech capture set restores every required product class.
- Two retained cache cycles have the same exact Large identity and distinct physical boot IDs.
- The exact merged Debian and RPM executables each pass ten pairs per fixture.
- Small is requalified for the same exact executables and new package schema.
- The composed packages deep-verify both records and cache seeds.
