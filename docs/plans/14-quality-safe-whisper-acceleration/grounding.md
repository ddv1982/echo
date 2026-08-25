# Grounding

## Current product path

Echo discovers a managed CPU runtime and an optional system runtime. `preferred_runtime` ranks candidates by source, so managed CPU remains first. `WhisperEngine` starts a new `whisper-cli` process for each request. Successful stderr supplies the observed backend and device, but the observation does not affect later selection.

The only automatic retry removes VAD after a VAD-specific failure. Echo has no accelerated-to-CPU retry, qualification cache, quarantine state, or resident worker.

The managed runtime is the upstream whisper.cpp v1.9.2 CPU archive. The measured Vulkan runtime is a locally built system candidate. The acceleration benchmark added the candidate directory to `LD_LIBRARY_PATH`; product execution does not add an adjacent runtime directory. Any packaged accelerator must make its shared-library resolution part of the production contract.

## Measured evidence

The first Intel Iris Xe host slice used the optimized Echo binary, Base Q5_1, twenty FLEURS clean-read recordings, and ten randomized CPU and Vulkan observations per recording.

- CPU median was 1,399.561 ms. CPU p95 was 1,626.493 ms.
- Vulkan median was 645.537 ms. Vulkan p95 was 819.285 ms.
- The paired median reduction was 761.251 ms, or 54.209 percent.
- German, English, Spanish, and French WER did not change.
- Dutch WER increased from 33.78 percent to 43.24 percent.

The result is `STOP`. It also lacks verified driver and ICD identity, fresh and populated cache evidence, reset evidence, complete product-speech coverage, other hardware, memory, power, and failure-path evidence. The authoritative record is [the Phase 2 decision](../../../.audit/whisper-phase2-intel-base/decision.md).

The run used `threads=4`, `beam=1`, `best-of=1`, and `no-fallback`. Upstream whisper.cpp defaults use larger candidate searches and keep temperature fallback enabled. A CPU control used the same aggressive settings, so the Dutch divergence remains a backend or driver concern until a version and decoding sweep disproves it.

## Gate weaknesses

The current host-matrix analyzer treats `cacheState`, `resetCycle`, `driverIdentity`, and `icdIdentity` as trusted caller labels. It does not bind those labels to collected files, cache operations, boot identity, or the selected Vulkan device. Relabelled duplicate observations can satisfy those gates.

The analyzer also trusts stored WER, hallucination, hashes, timing, backend, and device fields. It does not recompute them from the corpus and run artifacts. Corpus coverage metadata is not bound to the run rows. Output directories have no running or failed marker, so stale reports can survive an interrupted attempt.

These weaknesses make the current analyzer useful for investigation but unsafe as a production admission authority.

## Upstream and comparator evidence

whisper.cpp exposes CPU, cross-vendor Vulkan, NVIDIA CUDA, AMD ROCm, and Intel OpenVINO as distinct builds or assets. The [official Vulkan instructions](https://github.com/ggml-org/whisper.cpp#vulkan-gpu-support) require a host Vulkan driver. The [CUDA instructions](https://github.com/ggml-org/whisper.cpp#nvidia-gpu-support) require a separate CUDA build. [OpenVINO support](https://github.com/ggml-org/whisper.cpp#openvino-support) requires generated encoder IR files and creates a device-specific compiled cache after a slow first run.

Upstream reports show that Vulkan can produce incorrect transcripts on some GPU and driver combinations. The long-running [Vulkan quality issue](https://github.com/ggml-org/whisper.cpp/issues/2400) includes hardware-specific good and bad results. This supports exact-identity qualification instead of a global Vulkan switch.

whisper.cpp v1.9.3 is a pre-release from 2026-08-20. Its changelog includes Intel Xe Vulkan work and driver gating. Echo stays pinned to stable v1.9.2 until a same-host sweep proves that a newer revision fixes the quality failure and passes the full contract.

[Handy](https://github.com/cjpais/Handy) begins model loading when recording starts and supports idle unload. Its issue history shows that Vulkan lifecycle, shader rebuild, and transcript quality can vary by device and driver. This makes persistence a conditional optimization, not a safe default.

[Sona](https://github.com/thewh1teagle/sona) runs a local sidecar, loads one model, serializes requests, skips an identical reload, and unloads after an activity-protected idle timeout. [Vibe's Sona integration](https://github.com/thewh1teagle/vibe/pull/1259) removed duplicate residency state and added process cleanup. Vibe also retries with CPU after any GPU model-load failure, not only a child crash.

The useful common pattern is one exact loaded identity, one active request, bounded residency, and a CPU fallback. Echo cannot adopt the resident part until warmed one-shot clears quality and its own latency threshold.

## Hardware identity contract

The [Vulkan specification](https://registry.khronos.org/vulkan/specs/latest/html/vkspec.html) separates `deviceUUID`, `driverUUID`, and `pipelineCacheUUID`. Pipeline cache compatibility also depends on vendor ID, device ID, driver version, and implementation details. A device name or process-local index is insufficient.

The [official Vulkan loader documentation](https://github.com/KhronosGroup/Vulkan-Loader/blob/main/docs/LoaderDriverInterface.md) says `VK_DRIVER_FILES` selects driver manifests and replaces the older `VK_ICD_FILENAMES`. Default Linux manifest enumeration order is not stable. `VK_LOADER_DEBUG=error,warn,driver` reports the selected manifest and shared library. [vulkaninfo](https://github.com/KhronosGroup/Vulkan-Tools/blob/main/vulkaninfo/vulkaninfo.md) can provide device properties when installed.

An admission identity must include the Echo and whisper runtime hashes, adjacent library hashes, model and VAD hashes, protocol and tuning, backend, physical-device properties, selected ICD manifest and library hashes, driver identity, cache state, and reset or boot evidence.

## Design constraints

- Managed CPU one-shot remains the universal floor.
- No accelerated candidate outranks CPU before a fail-closed qualification passes.
- Qualification evidence comes from collected artifacts, not operator labels.
- CPU and accelerated controls use the same whisper.cpp binary, model, VAD policy, language, prompt, and decoding settings.
- A product failure permits at most one same-model managed CPU retry.
- A failed identity enters bounded quarantine.
- Echo does not bundle host drivers or ICDs.
- Vulkan is the first cross-vendor candidate. CUDA, ROCm, and OpenVINO remain separate runtime variants.
- Residency remains stopped on this Iris Xe identity until warmed one-shot passes and a resident path saves at least 25 percent and 300 ms.

## Architecture question

Design the smallest sequence that can turn the current investigative evidence into a trustworthy, cross-hardware acceleration release. The first implementation phase must improve the decision or remove a blocker on this Linux host. It must not enable production acceleration before the evidence passes.
