# Large Turbo acceleration grounding

## Current product boundary

Echo does not enable a GPU because Vulkan exists. It starts with a managed CPU plan and replaces that plan only when one packaged admission matches the executable, runtime, model, VAD, decoding policy, language policy, device receipt, DRM device, ICD files, and Mesa cache seed.

The existing identity already includes `modelSha256`. Quarantine and user cache directories use the full identity key. Those types can keep Small and Large failures separate without a new model enum.

The installed package is the limiting boundary. Production reads one `whisper-acceleration/admission.json`. A Small record rejects Large by hash and a Large record would reject Small. Shipping both models requires a multi-record package and exact record selection.

Automatic language detection and non-empty recognition hints remain outside this phase. Current evidence uses pinned language and an empty prompt.

## Existing evidence tools

The current sweep already runs the shipping `echo-desktop transcribe` boundary, randomizes CPU and Vulkan order, replays every raw observation, checks five languages and eight product classes, rejects new silence hallucinations, and requires both a 20 percent and 500 ms median reduction plus lower p95.

Large Turbo can use the sweep without a Small-specific branch. The model name must match the filename stem `large-v3-turbo-q5_0`.

Two gaps matter before a Large claim:

- One-shot observations do not record peak process RSS, minimum host memory availability, or swap growth.
- Cache reset evidence includes the model digest. The Small two-boot cycle cannot authorize Large.

Repeated successful observations provide useful crash, timeout, exit, and parse stability evidence. They do not prove memory safety by themselves.

## Upstream evidence

The official model list reports `large-v3-turbo-q5_0` at 547 MiB. It is multilingual and quantized. The file size does not establish runtime memory use. [whisper.cpp model list](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md#available-models)

whisper.cpp documents that quantization reduces disk and memory use and may improve processing efficiency depending on hardware. That is a reason to test Q5_0, not a performance claim for this laptop. [whisper.cpp quantization](https://github.com/ggml-org/whisper.cpp#quantization)

The upstream `whisper-bench` tool times only the encoder on random audio. Echo must keep its product corpus because users also pay model load, VAD, decoding, process, and parsing costs. [whisper.cpp benchmark tool](https://github.com/ggml-org/whisper.cpp/tree/master/examples/bench)

Upstream integrated-GPU work and user reports show that Vulkan results depend on the exact driver, device, model, and build. Those reports support measuring this host. They do not transfer a speedup to Echo. [integrated GPU support](https://github.com/ggml-org/whisper.cpp/pull/3492), [hardware-dependent Large Turbo report](https://github.com/ggml-org/whisper.cpp/issues/3304)

## Host budget

At framing time this laptop reported:

- 33,358,905,344 bytes of RAM.
- 19,543,138,304 bytes available.
- 7,234,322,432 bytes of swap already used.
- 23,146,434,560 bytes free on the workspace filesystem.
- Intel Iris Xe `8086:46a6` with the `i915` DRM driver.

Existing swap use makes an absolute swap threshold misleading. The screen must record per-run deltas and fail on new sustained swap growth, OOM, timeout, nonzero exit, or malformed output.

## Constraints

- Small remains the recommended setup and keeps its own admission.
- Large qualification never broadens to other GPUs or drivers.
- The runtime, VAD, pinned language, empty prompt, and decoding values remain exact.
- A package may share an identical runtime and probe, but cache seeds and admission records remain identity-specific.
- Normal CI runs deterministic self-tests and package fixtures. Hardware qualification remains an explicit Linux-host operation.
- A real second boot is required for Large reset evidence.
