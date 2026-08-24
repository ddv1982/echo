# Cross-hardware Whisper execution

## Context

[Plan 12](../12-whisper-performance/overview.md) proved that pinned whisper.cpp v1.9.2 can use Intel Iris Xe through Vulkan, but acceleration is still not selected in production. The next question is broader than GPU availability: should Echo keep a model resident, warm driver caches, or choose a specialized backend, and how can that remain truthful across Intel, AMD, NVIDIA, and CPU-only Linux machines?

The answer is conditional. Managed CPU one-shot remains the universal floor. Same-engine acceleration and residency are admitted only for exact runtime, model, device, driver, and tuning identities that pass their own gates. A result from one machine never becomes a global Linux default.

## Current decision

- Keep warmed one-shot execution as the current product shape.
- Record the backend and selected device actually reported by whisper.cpp, without changing runtime precedence.
- Continue Phase 5 quality and host-matrix work before preferring Vulkan or another accelerator.
- Stop the resident broker for the measured Iris Xe Base and Turbo identities: neither clears both the 25 percent and 300 ms thresholds.
- Retain an on-demand, TTL-bounded broker as a future conditional tier for identities that do pass.
- Test whisper.cpp CUDA before considering CTranslate2 on NVIDIA. Treat CTranslate2 as a distinct engine and model representation, never a transparent fallback.

## Definition of done

- Normal telemetry distinguishes requested metadata from observed backend and device truth.
- CPU, Vulkan, CUDA, ROCm, OpenVINO, software rasterizer, unknown output, and multiple-device logs have parser fixtures.
- The complete quality corpus and host matrix decide acceleration per exact identity.
- First-use shader compilation is reported separately from steady one-shot latency.
- Residency is compared against the best warmed one-shot path on the same identity and stops unless it wins by at least 25 percent and 300 ms.
- Every accelerated or resident failure permits at most one same-model managed CPU retry and reports the full attempt cost.
- No package bundles host drivers, Vulkan ICDs, or several large accelerator stacks by default.

## Phases

1. **Observed runtime truth.** Parse successful whisper.cpp stderr into backend and selected-device telemetry. Preserve candidate ordering and all execution behavior.
2. **Corpus and host matrix.** Run at least twenty licensed fixtures and ten randomized pairs per fixture on CPU-only, Intel Vulkan, AMD Vulkan, NVIDIA Vulkan, and NVIDIA CUDA hosts where available.
3. **Conditional same-engine acceleration.** Cache passed identities, invalidate on binary/library/model/VAD/device/driver/ICD/tuning change, quarantine failures, and add one same-model managed CPU retry.
4. **First-use warm-up policy.** Only after a managed accelerator passes Phase 3, test an idle/on-AC shader warm-up. Never block first dictation or ship a driver-specific cache.
5. **Conditional residency.** Build the cross-process broker only for an identity that independently clears latency, quality, lifecycle, and memory gates. Use one worker, one request at a time, managed leases, destructive uncertain cancellation, and bounded idle exit.
6. **Specialized backends.** Test OpenVINO after Vulkan on Intel and whisper.cpp CUDA after Vulkan on NVIDIA. Consider CTranslate2 only if it beats the best whisper.cpp path by another 20 percent and 500 ms with equivalent quality.

Phases are stop-gated. Phase 1 may merge because it changes observation only. Phase 3 cannot change selection before Phase 2 passes. Phase 5 does not begin for the measured Iris Xe identities.

## Phase 2 result

The first host slice is implemented with a pinned twenty-recording FLEURS subset and a same-binary CPU control. Through the optimized Echo binary, warmed Base Q5_1 on Iris Xe Vulkan is 54.2 percent and 761 ms faster at the paired median with lower p95, but Dutch WER regresses from 33.78 to 43.24 percent. The exact observed identity stops on quality. Corpus coverage, fresh-cache and reset repeats, explicit driver/ICD identity, Turbo, and AMD/NVIDIA hosts remain pending rather than inferred.

## Verification

See [testing.md](testing.md). Research and comparator evidence are in [research.md](research.md); the selected system shape is in [architecture.md](architecture.md).
