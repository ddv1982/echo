# Research notes

## Whisper

OpenAI describes Turbo as Large v3 with the decoder pruned from 32 layers to 4 and fine-tuned for multilingual transcription. The published table reports about 8x relative speed versus Large, compared with about 4x for Small, with minor overall quality degradation. Turbo is intended for transcription, not translation.

- OpenAI model card: https://github.com/openai/whisper/blob/main/model-card.md
- OpenAI repository and model table: https://github.com/openai/whisper
- Turbo release discussion: https://github.com/openai/whisper/discussions/2363

whisper.cpp publishes these relevant artifacts:

| Model | Disk |
| --- | ---: |
| Small | 466 MiB |
| Large v3 Turbo Q5_0 | 547 MiB |
| Large v3 Q5_0 | 1.1 GiB |
| Large v3 | 2.9 GiB |

Q5 models use less memory and disk and can run faster depending on hardware. Echo already pins the 574,041,195-byte Large v3 Turbo Q5_0 artifact and its SHA-256. The model the user called “Large V5” is therefore most likely Large v3 Turbo with Q5_0 quantization, not a Whisper v5 family.

- whisper.cpp model inventory: https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md
- whisper.cpp runtime and optimizations: https://github.com/ggml-org/whisper.cpp

Echo already enables Silero VAD when installed. Flash Attention can help GPU-backed Metal or CUDA builds, but upstream reports that it may degrade CPU performance. The managed Linux archive is a CPU runtime, so this plan does not enable it blindly.

whisper.cpp defaults to four computation threads. Reports show diminishing returns and hardware-specific regressions above the useful core count. This plan puts thread tuning in the benchmark path instead of assigning an unmeasured global value.

## Parakeet

NVIDIA Parakeet TDT 0.6B v3 is a 600M-parameter multilingual model with automatic language detection across 25 European languages. Its official card shows strong results for several high-resource languages and materially weaker results for some others. “25 languages” is a capability statement, not a uniform quality guarantee.

- NVIDIA model card: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3

The pinned sherpa-onnx offline CLI prints a JSON result containing `text`, tokens, timestamps, and other fields. Echo currently treats all successful stdout as transcript text. The adapter must parse that protocol at its subprocess boundary.

- sherpa-onnx Parakeet v3 example: https://github.com/k2-fsa/sherpa/blob/master/docs/source/onnx/pretrained_models/offline-transducer/code-nemo/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.txt

Neither Parakeet nor Whisper receives screen contents, window text, history, or the clipboard. Both receive only normalized 16 kHz mono PCM, with language and dictionary hints added only where supported. Screen text appearing in the target application therefore points to injection state, not OCR.

## Clipboard delivery

X11 selections are served by the owning process when a target requests them. A successful synthetic Ctrl+V command does not prove that the target has already fetched the selection. Wayland `wl-copy --paste-once` similarly exits after a request, but its own manual warns that paste-once can break XWayland clients.

- xclip repository and selection ownership: https://github.com/astrand/xclip
- wl-clipboard manual: https://manpages.debian.org/bookworm/wl-clipboard/wl-copy.1.en.html

Echo currently restores the old clipboard immediately after the key command exits. Worse, it reports `ClipboardOnly` after placing the transcript on the clipboard and then immediately restores the old value. Leaving the transcript in the clipboard after fallback is the smallest portable correctness invariant.
