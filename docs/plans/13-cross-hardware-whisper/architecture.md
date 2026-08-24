# Architecture

## Principle

Execution policy is an evidence lookup, not a platform guess. `Linux`, `Vulkan installed`, or `NVIDIA libraries present` are insufficient keys. Exact runtime, libraries, model, VAD, backend, physical device, driver/ICD, tuning, protocol, and cache state define an execution identity.

## Layers

1. **Universal floor:** managed CPU one-shot with the selected model.
2. **Observed candidate:** a compatible system runtime may run under existing precedence, but telemetry comes from successful runtime output rather than its requested label.
3. **Proven accelerator:** only a passed identity can outrank managed CPU. A failure quarantines that identity and permits one same-model managed CPU retry.
4. **Proven resident identity:** only a passed accelerator or CPU identity may use an on-demand broker with a bounded TTL.
5. **Specialized engine:** OpenVINO encoder assets, whisper.cpp CUDA, and CTranslate2 remain separate candidates and packaging decisions.

## Phase 1 shape

Phase 1 adds a pure stderr boundary:

```rust
struct WhisperRuntimeObservation {
    backend: WhisperRuntimeBackend,
    device: Option<String>,
}
```

On a successful request, the parser recognizes the backend token selected by `whisper_backend_init_gpu`, binds Vulkan indices to their enumerated physical description, recognizes CUDA and ROCm device records, recognizes CPU fallback, and leaves unknown output unknown. OpenVINO remains a legacy scalar telemetry value until encoder and main-compute backends are modeled separately.

`WhisperRuntimeTelemetry` gains an optional device field with a serde default, so old history remains readable. Advanced diagnostics show the observed device. Declared runtime source is preserved. A known declared backend remains the fallback only when the runtime emitted no usable observation.

This phase does not add a probe subprocess, cache, quarantine, fallback, runtime download, warm-up, broker, or setting. It cannot make an accelerated runtime win selection.

## Future resident shape

If an identity passes, Echo borrows Sona's lifecycle mechanics while preserving Echo's existing plan:

- First client starts one hidden user-scoped broker under an exclusive state lock.
- Broker starts the pinned server on loopback with an OS-selected port and random path.
- Readiness is a verified handshake owned by that child, never a sleep.
- One worker key names runtime/libraries/model/VAD/backend/device/driver/tuning/generations.
- One request runs at a time; active work holds an activity and managed-generation lease.
- Same key is idempotent. A changed key replaces the worker after old work completes.
- Uncertain cancellation kills the server and removes state.
- Idle timeout exits the full worker and releases memory and leases.
- Cold same-model CPU remains the only retry.

No permanent system service is installed.

## Packaging matrix

| Hardware | Floor | Conditional candidate | Packaging boundary |
| --- | --- | --- | --- |
| Unknown or CPU-only | Managed CPU | None | Current component |
| Intel/AMD Vulkan | Managed CPU | System, then possibly managed Vulkan | Host owns driver/ICD |
| NVIDIA Vulkan | Managed CPU | Identity-proven Vulkan | Do not infer from installed libraries |
| NVIDIA CUDA | Managed CPU | whisper.cpp CUDA after direct bakeoff | Separate runtime; host owns driver |
| Intel OpenVINO | Best proven CPU/Vulkan | Encoder experiment | Runtime plus model-specific IR/cache |
| NVIDIA CTranslate2 | Best proven whisper.cpp | Separate engine | Separate runtime and converted model |

Deb, RPM, AppImage, and raw binary share policy. Large accelerator runtimes remain independently downloadable managed components.
