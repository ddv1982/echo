# First-run readiness grounding

## Exit condition

Echo v0.7.0 is ready when all of these predicates hold:

- A Linux x86_64 user with an empty Echo cache and no speech runtime on `PATH` can press Recommended setup and reach Ready without installing a package or running a terminal command.
- The recommended plan installs one pinned Whisper runtime, one hardware-appropriate multilingual model, and Silero VAD. Each component has an independent visible state.
- One-click Parakeet setup installs the pinned sherpa-onnx offline runtime and all four required Parakeet model files.
- A cancelled or interrupted transfer leaves a resumable partial outside active paths. Retry continues it. A checksum failure never activates it.
- Repair verifies managed components and replaces missing or corrupt ones. Removal deletes only Echo-managed files.
- System runtimes and manually imported models remain usable and are labelled as external rather than managed.
- Settings shows every input device as a clear, stable choice, reports the system default and available metadata, persists the selected ID, exposes missing-device fallback, refreshes after hot-plug, and announces the selected device's test result.

## Current microphone path

Echo uses CPAL 0.15 and reduces each device to `{ name, is_default }`. The persisted setting and runtime lookup both use an exact display-name match. Duplicate names select the first handle, a missing choice silently tests the fallback device, and the UI renders the whole choice as a narrow native select. Settings enumerates only on mount. `microphoneReady` proves that a handle resolves, not that capture works.

The source path is `crates/echo/src/audio.rs`, projected through `src-tauri/src/main.rs`, then rendered in `frontend/src/App.tsx`. The baseline browser recording is `/Users/vriesd/.t3/userdata/browser-artifacts/browser-recording-mt5eqsr1.mp4`.

CPAL 0.18.2 adds `DeviceId` plus `DeviceDescription` fields for user-facing name, manufacturer, device type, interface type, address, driver, and extended details. Its native PipeWire backend maps `node.description`, BlueZ, USB, PCI, icon role, address, and driver properties. ALSA still supplies a stable PCM ID, a readable first description line, driver ID, and extended lines. This is enough to replace name-only identity without inventing metadata.

PipeWire documents `node.name` as the unique linking name and `node.description` as the human-readable GUI description. Microsoft, Apple, and GNOME all present connected input devices as explicit selectable rows and pair selection with visible input-level testing.

Primary sources:

- [CPAL 0.18.2 traits](https://github.com/RustAudio/cpal/blob/v0.18.2/src/traits.rs)
- [CPAL structured device descriptions](https://github.com/RustAudio/cpal/blob/v0.18.2/src/device_description.rs)
- [CPAL 0.18 migration guide](https://github.com/RustAudio/cpal/blob/v0.18.2/UPGRADING.md)
- [PipeWire identifying properties](https://github.com/pipewire/pipewire/blob/master/doc/dox/config/pipewire-props.7.md#identifying-properties)
- [GNOME microphone selection](https://help.gnome.org/gnome-help/sound-usemic.html)
- [Microsoft microphone selection and testing](https://support.microsoft.com/en-us/windows/hardware/drivers/how-to-set-up-and-test-microphones-in-windows)
- [Apple input-device selection](https://support.apple.com/guide/mac-help/change-the-sound-input-settings-mchlp2567/mac)

## Current download path

Echo has four direct-file offers. It downloads to a process-specific temp file, verifies SHA-1, renames the file into the model directory, emits progress, and deletes every partial on cancellation or failure. It cannot resume. It does not check free space, manage runtimes or Parakeet, distinguish manual from managed files, repair or remove components, or prevent duplicate downloads for one ID. Installed UI state checks only `is_file`, so a corrupt offer can hide its own repair action.

The useful base is the activation rule in `crates/echo/src/stt/fetch.rs`: scanners never see temp names, and the final rename is the only activation point. The new installer should keep that rule while replacing offer-specific SHA-1 logic with typed SHA-256 components and stable resumable partials.

## Pinned upstream artifacts

The runtime and Parakeet archives are official, versioned GitHub release assets. GitHub publishes their SHA-256 digests.

| Component | Artifact | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Whisper runtime 1.9.2 | `whisper-bin-ubuntu-x64.tar.gz` | 9,497,583 | `46811a3ecf584307480a220b9ef5ff81b7b22dc41577cbc274ce3afc61f753b1` |
| sherpa-onnx runtime 1.13.6 | `sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2` | 361,356,492 | `ba2c35a3f6ca889e6c31fe12eba292fb13eeca5cb13687e6b04ccdc23649c954` |
| Parakeet TDT 0.6b v3 INT8 | `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` | 487,170,055 | `5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf` |

The Whisper archive contains `whisper-cli` plus its `$ORIGIN` shared libraries. The sherpa archive contains a 34 MiB `bin/sherpa-onnx-offline` whose remaining dependencies are standard glibc libraries. The Parakeet model provides `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx`, and `tokens.txt` for 25 European languages.

Whisper model and VAD SHA-256 values come from their Hugging Face LFS object IDs:

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `ggml-base-q5_1.bin` | 59,707,625 | `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898` |
| `ggml-base.en-q5_1.bin` | 59,721,011 | `4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f` |
| `ggml-small.bin` | 487,601,967 | `1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b` |
| `ggml-large-v3-turbo-q5_0.bin` | 574,041,195 | `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2` |
| `ggml-silero-v6.2.0.bin` | 885,098 | `2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987` |

Primary sources:

- [whisper.cpp v1.9.2](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.2)
- [sherpa-onnx v1.13.6](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.6)
- [sherpa-onnx ASR models](https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models)
- [Parakeet v3 model documentation](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html#sherpa-onnx-nemo-parakeet-tdt-0-6b-v3-int8-25-european-languages)
- [whisper.cpp model repository](https://huggingface.co/ggerganov/whisper.cpp/tree/main)
- [whisper.cpp VAD repository](https://huggingface.co/ggml-org/whisper-vad/tree/main)

## Scope and rigor

Expected change area is six to eight Rust modules, four frontend files, installer tests, UI tests, one rerunnable verifier, README, release notes, and this plan. The work is high rigor because it downloads executable code, mutates hundreds of megabytes of user data, and adds removal. The installer will support managed Linux x86_64 artifacts in v0.7.0. Unsupported platforms get an explicit status. System runtimes and manual files remain read-only external inputs.
