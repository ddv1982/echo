# Third-party components

Echo's managed component catalog installs the following runtime code and model
weights only when a user selects them. These downloads are not embedded in the
Echo binary, AppImage, Debian package, or RPM. The release CycloneDX SBOM still
records each catalog entry's URL, SHA-256 digest, license provenance, supplier,
distributor, origin, applicable modifications, kind, and source link. Where the
upstream record supports it, the SBOM also records an evidence-scope note and
evidence link without claiming a stronger supply-chain attestation.

This notice is maintained with
`crates/echo/src/install/catalog.rs`. It identifies the managed components and
their direct provenance, but does not reproduce every transitive third-party
license text. Upstream distributions remain subject to all notices and license
terms that accompany them.

## Runtime code

- **`whisper-runtime` (Whisper runtime 1.9.2):** supplied by ggml-org and
  derived from
  [whisper.cpp commit `306c88f4d1286aec1bf96e544632897886af5501`](https://github.com/ggml-org/whisper.cpp/tree/306c88f4d1286aec1bf96e544632897886af5501),
  under its [MIT license](https://github.com/ggml-org/whisper.cpp/blob/306c88f4d1286aec1bf96e544632897886af5501/LICENSE).
  Echo downloads the [upstream Linux CPU runtime
  archive](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.2) on
  demand.
- **`whisper-vulkan-runtime` (Whisper GPU runtime 1.9.2-vulkan):** supplied and
  distributed by Echo as a Vulkan-enabled build derived from the same
  [whisper.cpp commit](https://github.com/ggml-org/whisper.cpp/tree/306c88f4d1286aec1bf96e544632897886af5501)
  under the same [MIT license](https://github.com/ggml-org/whisper.cpp/blob/306c88f4d1286aec1bf96e544632897886af5501/LICENSE).
  Echo [publishes this custom build
  separately](https://github.com/ddv1982/echo/releases/tag/whisper-vulkan-runtime-1.9.2)
  and installs it on demand. It is not an application release asset.
- **`sherpa-runtime` (sherpa-onnx 1.13.6):** supplied and distributed by k2-fsa
  from the
  [v1.13.6 release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.6)
  whose tag resolves to commit
  [`1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911`](https://github.com/k2-fsa/sherpa-onnx/tree/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911).
  The tag-triggered [`.github/workflows/linux.yaml` at that immutable
  commit](https://github.com/k2-fsa/sherpa-onnx/blob/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/.github/workflows/linux.yaml)
  builds, names, and uploads
  `sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2`; the catalog's release
  asset SHA-256 digest identifies the exact downloaded bytes.
  sherpa-onnx source is [Apache-2.0](https://github.com/k2-fsa/sherpa-onnx/blob/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/LICENSE),
  but Echo downloads the upstream static, no-TTS distribution, which
  incorporates dependencies governed by their own terms. The binary as a whole
  is therefore not represented as exclusively Apache-2.0. Consult the pinned
  [dependency declarations](https://github.com/k2-fsa/sherpa-onnx/tree/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/cmake)
  which declare static ONNX Runtime 1.27.1, and the notices supplied or
  referenced by the upstream source and release. Bundled dependencies retain
  their own terms.

## Model weights

- **`whisper-base-q5-1` (Whisper Base multilingual Q5_1),
  `whisper-small` (Whisper Small multilingual), and
  `whisper-large-v3-turbo-q5-0` (Whisper Large v3 Turbo Q5_0):** GGML model
  artifacts converted and distributed by ggerganov from OpenAI Whisper model
  weights under the [MIT license stated by the pinned model
  repository](https://huggingface.co/ggerganov/whisper.cpp/blob/5359861c739e955e79d9a303bcbc70fb988958b1/README.md).
  Echo downloads each named GGML model file on demand from the
  [ggerganov/whisper.cpp distribution at revision
  `5359861c739e955e79d9a303bcbc70fb988958b1`](https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1).
- **`silero-vad` (Silero VAD 6.2.0):** originated by the Silero Team from
  [Silero VAD v6.2](https://github.com/snakers4/silero-vad/tree/v6.2) under its
  [MIT license](https://github.com/snakers4/silero-vad/blob/v6.2/LICENSE).
  Echo downloads the ggml-org-converted and distributed GGML artifact on demand
  from the [ggml-org/whisper-vad distribution pinned at
  `9ffd54a1e1ee413ddf265af9913beaf518d1639b`](https://huggingface.co/ggml-org/whisper-vad/tree/9ffd54a1e1ee413ddf265af9913beaf518d1639b).
- **`parakeet-tdt-06b-v3-int8` (Parakeet TDT 0.6B v3 INT8):** supplied,
  converted, and distributed by k2-fsa from NVIDIA model weights under
  [CC-BY-4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode),
  with the original [NVIDIA model card pinned at
  `575de92b31b2f60855bca9b70968bde5afb069ba`](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/blob/575de92b31b2f60855bca9b70968bde5afb069ba/README.md).
  sherpa-onnx converted the model to ONNX and INT8-quantized it. These are
  modifications to the NVIDIA weights. Echo downloads that modified model from
  the [sherpa-onnx ASR model distribution](https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models)
  on demand. The merged [k2-fsa PR 2500/export
  record](https://github.com/k2-fsa/sherpa-onnx/pull/2500) associates
  `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` with NVIDIA origin, ONNX
  conversion, and INT8 quantization. Its conversion script references an
  unpinned NVIDIA model, so Hugging Face revision
  `575de92b31b2f60855bca9b70968bde5afb069ba` is an attribution and license
  snapshot only. It does not independently attest exact source-revision byte
  lineage.
