# Research

## Existing Echo evidence

Ten-pair one-shot smoke reports on this i7-12700H / Iris Xe host produced:

| Model | CPU median | Vulkan median | Vulkan gain |
| --- | ---: | ---: | ---: |
| Base multilingual Q5_1 | 1,090 ms | 464 ms | 57.6 percent, 631 ms |
| Large v3 Turbo Q5_0 | 21,515 ms | 5,708 ms | 73.6 percent, 15,811 ms |

The committed fixture produced degenerate nonspeech text, so this proves backend and latency mechanics only. Production acceleration remains gated on corpus quality and reset-repeat evidence.

The resident smoke compared the same warmed Vulkan identities:

| Model | One-shot | Resident warm median | Reduction | Resident gate |
| --- | ---: | ---: | ---: | --- |
| Base multilingual Q5_1 | 463.602 ms | 326.993 ms | 29.5 percent, 136.609 ms | Stop: below 300 ms |
| Large v3 Turbo Q5_0 | 5,708.163 ms | 5,522.501 ms | 3.3 percent, 185.662 ms | Stop: below both thresholds |

Including server readiness makes the first resident request slower than one-shot: 547.708 ms for Base and 6,741.876 ms for Turbo. Median process RSS was 167 MiB for Base and 182 MiB for Turbo even with the Vulkan model loaded, showing that RSS alone cannot account for integrated-GPU or driver allocations.

## How other applications handle it

### Handy

Handy keeps a live in-process transcription `Session`, so repeated dictation reuses the model. It protects active streaming with leases and offers immediate, timed, or never-unload policies; the default is five minutes. Its Linux and Windows x86 builds use dynamic CPU plus Vulkan backends, macOS uses Metal, and the runtime records the backend and device that actually bound. Persisted GPU choice uses a stable device identity rather than a process-local index. [Loaded session and idle watcher](https://github.com/cjpais/Handy/blob/af48dd68a64d58aad128fdbb920492a03da53c79/src-tauri/src/managers/transcription.rs#L179-L350), [bound backend and device](https://github.com/cjpais/Handy/blob/af48dd68a64d58aad128fdbb920492a03da53c79/src-tauri/src/managers/transcription.rs#L548-L625), [stable device selection](https://github.com/cjpais/Handy/blob/af48dd68a64d58aad128fdbb920492a03da53c79/src-tauri/src/managers/transcription.rs#L1946-L2011).

This is effective because Handy's running desktop process owns shortcut requests. Echo's GNOME fallback and file CLI are separate processes, so a desktop-only cache would miss important product paths.

### Vibe and Sona

Vibe moved inference into an app-owned Sona sidecar. Sona binds an OS-selected port, emits machine-readable readiness with version and commit, loads at most one model, skips an identical path/device/GPU-mode reload, serializes transcription, cancels on client disconnect, and unloads after a lease-protected idle timeout. [Sona architecture](https://github.com/thewh1teagle/sona/blob/380a8b3ff9891209c68db7d338231728039c3a25/docs/ARCHITECTURE.md), [idle-unload runtime](https://github.com/thewh1teagle/sona/blob/380a8b3ff9891209c68db7d338231728039c3a25/crates/sona/src/server/unload_timeout.rs), [Vibe lifecycle correction](https://github.com/thewh1teagle/vibe/pull/1259).

This is the closest analogue for a future Echo broker. The useful ideas are exact readiness, explicit ownership, idempotent worker identity, one active request, activity leases, parent/exit cleanup, and TTL-based memory release.

### Speaches and faster-whisper

Speaches dynamically loads requested faster-whisper models and offloads them after inactivity. This validates reference-counted TTL residency for a general API server. Faster-whisper supports CPU and NVIDIA CUDA with different compute types and model assets; its official GPU requirements are CUDA-specific. [Speaches](https://github.com/speaches-ai/speaches), [faster-whisper requirements and benchmarks](https://github.com/SYSTRAN/faster-whisper/blob/master/README.md#benchmark).

That is not a universal Linux backend. It may become a specialized NVIDIA engine, but it adds Python/CTranslate2, CUDA/cuDNN, converted models, and a separate quality surface.

### Buzz

Buzz exposes Whisper.cpp, faster-whisper, and CPU forcing. Live Whisper.cpp recording owns a temporary server and cleans the model afterward; file jobs construct a new transcriber per task. It offers useful escape hatches for old or problematic GPUs, but fixed ports and readiness waits are not lifecycle mechanics Echo should copy. [Buzz backend guidance](https://github.com/chidiwilliams/buzz/blob/master/docs/docs/faq.md), [local server implementation](https://github.com/chidiwilliams/buzz/blob/master/buzz/transcriber/recording_transcriber.py).

## Upstream backend boundary

whisper.cpp supports cross-vendor Vulkan and a separate CUDA build for NVIDIA. OpenVINO accelerates the encoder on supported Intel devices but adds a generated IR and compiled device cache. [Vulkan](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/README.md#vulkan-gpu-support), [CUDA](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/README.md#nvidia-gpu-support), [OpenVINO](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/README.md#openvino-support).

The shared policy is therefore layered: managed CPU for everyone, identity-proven whisper.cpp accelerators next, conditional residency after that, and specialized engines last.

## Independent architecture arena

The judge scored demonstrated benefit, product-path coverage, quality safety, failure correctness, packaging fit, maintainability, and memory behavior:

| Option | Score | Decision |
| --- | ---: | --- |
| Warmed one-shot plus driver shader cache | 92/100 | Current production shape |
| Conditional per-hardware residency | 68/100 | Retain behind an independent gate |
| Same-engine CUDA specialization | 63/100 | Future NVIDIA experiment |
| General cross-process broker | 61/100 | Reject until an identity passes |
| OpenVINO specialization | 56/100 | Conditional Intel experiment |
| App-owned sidecar for every request | 56/100 | Reject as a universal default |
| Always-on service | 47/100 | Reject |
| CTranslate2/faster-whisper | 47/100 | Separate experimental engine only |
| Desktop-only cache | 45/100 | Reject |

The smallest safe next phase is runtime truth without selection: parse actual backend/device evidence, expose it in diagnostics, and leave precedence unchanged.
