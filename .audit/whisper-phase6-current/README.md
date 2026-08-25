# Current launch-contract smoke

These JSON files preserve two real `echo-desktop transcribe` results after the child environment hardening from the independent trail review.

The managed run used the normal managed Small model and CPU runtime. The system run isolated `ECHO_MODEL_DIR`, put the locally built v1.9.2 Vulkan runtime first on `PATH`, and invoked the parent with `LD_LIBRARY_PATH`, `VK_DRIVER_FILES`, `VK_ICD_FILENAMES`, `MESA_VK_DEVICE_SELECT`, and `DRI_PRIME` unset. Both used `crates/echo/tests/fixtures/claude_code.wav`, `--engine whisper --model small --language en --format json`.

Observed results:

- Managed CPU: 307 ms, managed source, adjacent library path, composite identity `dca15bd35fe0ebbe1995ba825ed99c9f626a4fe08b25ddd24954fb8be2d554b5`.
- System Vulkan: 1377 ms, Intel Iris Xe, adjacent library path, composite identity `39b940671c4b8496747842c12f8f8e5699197f75705239da45710d5823cc6fc5`.

This is runtime smoke evidence, not admission evidence. The fixture produced an empty transcript for both runs, and the full Phase 5 corpus was not rerun on this code state.
