# Linux GPU research

## Question

Can Echo make Whisper faster on Linux by using the GPU without weakening the selected model, hiding fallback cost, or labeling CPU work as accelerated?

## Existing system

Echo currently resolves one `WhisperExecutionPlan`, prefers the managed CPU runtime, and launches `whisper-cli` for every request. Runtime telemetry can name CPU, CUDA, Vulkan, OpenVINO, ROCm, or unknown, but system runtimes are still discovered as unknown and no real backend probe can promote one. The managed `whisper.cpp` version is 1.9.2.

The test host has an Intel Core i7-12700H and Intel Iris Xe `8086:46a6`, using `i915` with `/dev/dri/renderD128`. Mesa's Intel Vulkan driver can execute compute. There is no NVIDIA PCI device or loaded NVIDIA kernel module; installed NVIDIA user-space packages are therefore not device evidence.

## Upstream findings

- Vulkan is whisper.cpp's cross-vendor full GPU path and is built with `GGML_VULKAN`. [Upstream Vulkan instructions](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/README.md#vulkan-gpu-support)
- OpenVINO moves the encoder to supported Intel devices. It also needs a generated encoder IR beside each GGML model and a separate OpenVINO-enabled runtime. [Upstream OpenVINO instructions](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/README.md#openvino-support)
- SYCL can target Intel GPUs, including Intel iGPUs, but needs Intel GPU drivers, oneAPI/Level Zero, a SYCL build, runtime environment setup, and device selection. [Upstream SYCL instructions](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/README_sycl.md)
- `--help`, `use gpu = 1`, a loader library, or an installed ICD does not prove which device performed inference. A valid probe needs a real JSON transcription, a selected backend/device line, and a `--no-gpu` negative control.

## Diagnostic measurements

The reusable probe ran ten randomized pairs per model with the exact pinned v1.9.2 source, one 0.4-second committed fixture, four threads, beam one, best-of one, no fallback, and a new isolated Mesa cache. It retained the first-use warmups separately and ranked only steady one-shot rows.

| Model | First Vulkan | CPU median / p95 | Vulkan median / p95 | Paired median gain | Probe gate |
| --- | ---: | ---: | ---: | ---: | --- |
| Base multilingual Q5_1 | 8,129 ms | 1,090 / 1,184 ms | 464 / 474 ms | 57.6%, 631 ms | Proceed |
| Large v3 Turbo Q5_0 | 11,516 ms | 21,515 / 21,803 ms | 5,708 / 5,989 ms | 73.6%, 15,811 ms | Proceed |

Every timed accelerated row resolved the physical Iris Xe Vulkan device, every control row resolved CPU, and paired transcripts matched exactly. Base transcribed as `(phone ringing)` and Turbo as `...`, so these reports prove backend truth and latency mechanics but no useful recognition quality. The first Vulkan request paid a large shader-cache penalty, proving that policy and evidence must be keyed by model, device, driver, runtime, and cache state. The full corpus gate remains closed. Raw reports are under `.audit/whisper-vulkan-iris-xe-v1.9.2-base` and `.audit/whisper-vulkan-iris-xe-v1.9.2-turbo`; user-specific absolute paths use `$REPO` and `$HOME` placeholders while existing relative paths remain relative. The environment records the Echo commit, version, dirty state, and adjacent-library search path.

The same pinned Vulkan runtime also completed Echo's shipping `echo-desktop transcribe` boundary in 5,856 ms with the exact Turbo model and tuning. Echo truthfully reported the system runtime backend as `unknown`, because production probing is not implemented yet. That compatibility result is not permission to relabel or prefer the candidate; it is direct evidence for Phase 5C's backend-truth work.

## Architecture arena

An independent judge scored the options against plausible benefit, host readiness, observability, fallback safety, and packaging cost.

| Candidate | Score | Decision |
| --- | ---: | --- |
| Managed CPU plus one-shot tuning | 91/100 | Keep as the production floor and tune first on the full corpus. |
| Vulkan | 70/100 | First accelerator to probe; do not ship from smoke evidence. |
| Residency | 49/100 | Complete its existing gate; skip broker integration if it misses. |
| OpenVINO | 47/100 | Second accelerator experiment because it is model-derived and encoder-only. |
| SYCL | 35/100 | Defer because of oneAPI/runtime and packaging cost. |
| CUDA or ROCm on this host | 0/100 | Stop: no matching physical device. |

The selected architecture is a model-specific Vulkan probe followed by conditional system-runtime selection. Managed CPU remains the compatibility and failure fallback. OpenVINO, SYCL, residency, and a managed GPU artifact remain separate decisions.

## Decision boundary

This investigation may add reproducible probes and raw evidence immediately. The host smoke gate passed for both models after shader-cache warmup. It may not prefer an accelerated runtime until the full quality corpus and same-model failure fallback also pass. It may not add a managed GPU artifact until the result reproduces on representative Linux hosts.

See [phase 5](phase-5-system-acceleration.md) for the implementation sequence and [testing](testing.md) for the gates.
