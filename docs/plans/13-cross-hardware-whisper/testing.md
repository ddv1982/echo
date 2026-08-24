# Testing

## Phase 1 parser matrix

- CPU model placement plus `no GPU found` resolves CPU.
- Selected `VulkanN` binds only to `ggml_vulkan: N = ...`, including multiple-device logs.
- Selected `CUDAN` and `ROCmN` resolve their backend and matching `Device N` label.
- OpenVINO evidence resolves only under the documented scalar compatibility rule.
- A software Vulkan device remains observable but never becomes selection evidence.
- Merely enumerating a GPU, printing `use gpu = 1`, or loading a backend without selecting it remains unknown.
- Malformed, partial, conflicting, and unrelated stderr cannot invent a backend.
- Known declared metadata is retained only when no stronger runtime observation exists.
- Old history without `device` deserializes unchanged.

Drive the real file CLI with the pinned Vulkan runtime and confirm JSON plus Advanced diagnostics report `vulkan` and the Iris Xe description. Drive the fake system runtime and managed CPU fixtures to confirm compatibility.

## Acceleration gate

- At least twenty licensed fixtures covering required languages, silence, nonspeech, short and long dictation.
- At least ten randomized pairs per fixture.
- Fresh-cache first use and populated-cache steady state remain separate.
- Repeat after reboot or power-policy reset.
- Every row resolves actual backend, physical device, driver, and ICD; CPU control resolves CPU.
- At least 20 percent and 500 ms lower paired median with lower p95.
- Per-language WER/CER regression no greater than 0.5 absolute points, no new hallucination, no failure, same model and decoding policy.

Stop and quarantine on unknown backend, software rasterizer, driver change, p95 or quality regression, or a win that disappears after reset.

## Residency gate

Compare with the best warmed one-shot runtime for the same identity:

- At least 25 percent and 300 ms lower warm median, plus lower p95.
- At least three fresh worker cycles and ten warm observations per fixture overall.
- No quality or failure regression.
- Cancellation completes or kills the worker within one second; next request converges through at most one same-model cold retry.
- Two clients converge to one broker and server.
- `MemAvailable` stays above the greater of 1.5 GiB or 20 percent of RAM on the minimum-memory host; swap-in/out and memory-pressure stalls do not materially increase.
- TTL removes worker RSS/state and releases leases within ten seconds after expiry.

Missing either latency threshold is an immediate stop. The current Iris Xe Base and Turbo identities stop before broker implementation.

## Project gate

```text
cargo clippy --workspace --all-targets -- -D warnings
xvfb-run -a cargo test --workspace
npm run typecheck --prefix frontend
npm run lint --prefix frontend
npm run test --prefix frontend
npm run build --prefix frontend
npm run test:responsive --prefix frontend
./scripts/verify-transcribe-cli.sh
./scripts/verify-stt-benchmark.sh
./scripts/verify-whisper-acceleration.sh
./scripts/verify-whisper-runtime-archive.sh
cargo build --release
```
