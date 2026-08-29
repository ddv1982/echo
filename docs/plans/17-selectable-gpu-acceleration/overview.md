# Selectable GPU acceleration

## Context

Echo measured a real win and cannot deliver it. On Intel Iris Xe with Mesa 25.2.8, `.audit/pr16-2-evidence/summary.json` records Whisper Small at 4545 ms on CPU against 2444 ms accelerated, and Large v3 Turbo at 19270 ms against 6064 ms, over 280 runs per candidate with every language quality gate passing and no new hallucinations. That evidence is sound.

The delivery around it is not. The shipped `io.github.ddv1982.echo_0.12.6_amd64.deb` contains 23 files: the binary, five icon sizes, and a desktop entry. Releases 0.12.2, 0.12.3, and 0.12.4 shipped 152 to 240 files including a bundled runtime. The payload only ever reached a package through an operator running `cargo tauri bundle --config` against a generated overlay from `scripts/compose-whisper-admission-set.py`, uploading the result to a `qualification-$commit` draft, and letting the tag workflow substitute those assets for its own build. Commit `fc7ed1f` turned a missing draft from a hard failure into a silent downgrade, so 0.12.6 published with nothing and no check objected. The changelog still promises "Auto uses a receipt-verified local Vulkan device when one enumerates."

The machinery built to make that payload trustworthy is now the obstacle. `scripts/` holds 18,711 lines of qualification Python out of 19,037 total. `crates/echo/src/stt/` adds roughly 3,500 lines of admission, identity, and portable-package Rust. A full qualification is around five hours of measured child time for a deb and rpm pair, two physical reboots, and a licensed eight-class corpus audit. Evidence carries a 30-day fuse. The one real qualification, measured 2026-08-27 at commit `8e39dee`, was invalidated roughly 24 hours later when the next PR moved the behavior projection digest. Total reach across the project's history is one GPU: vendor `0x8086`, device `0x46a6`, drm `i915`.

This plan keeps the capability and deletes the apparatus. The patched whisper.cpp receipt selector, UUID device pinning, the scrubbed child environment, and quarantine with one CPU recovery are roughly 1,800 lines and they are what makes GPU execution provable. Everything built to transfer a maintainer's verdict onto machines nobody owns goes. In its place the user makes the choice, on a control that names the GPU it will use.

## Scope

**Included**

- A two-state `Whisper acceleration` control offering CPU and GPU, defaulting to CPU on a fresh install.
- A GPU device picker listing every enumerated Vulkan device by name, pinned by its `deviceUUID` and `driverUUID` pair.
- Delivery of the Vulkan runtime as a managed component downloaded on demand, replacing the bundled package.
- Deletion of both runtime acceleration gates, the identity algebra, the portable package format, and the behavior projection digest chain.
- Removal of the release-time qualification obligation, and a release check that fails when a tagged package cannot do what the changelog claims.

**Excluded**

- The Whisper model default. `crates/echo/src/stt/cache.rs:170` ranks by measured WER, so `Auto` picks the largest installed model, and Large on CPU at 19270 ms is a worse default than Small on CPU at 4545 ms. That is a larger UX win than this plan delivers and it deserves its own plan.
- Any new hardware qualification. No AMD or NVIDIA measurement is required to land this, because CPU remains the default and GPU is an explicit choice.
- Deleting the qualification Python. `scripts/sweep-whisper-admission.py`, `promote`, `compose`, and the benchmark tooling stay in the tree as research instruments. They stop being referenced by CI, releases, and the runtime.
- Shadow calibration against user dictation. Recorded in Alternatives as the option to reach for if a real quality complaint appears.

## Constraints

- CPU transcription must keep working unchanged at every phase boundary, including the phases where acceleration does not exist at all.
- A user who never selects GPU must never download the Vulkan runtime and must never execute a Vulkan code path.
- The `deviceUUID` and `driverUUID` pair is the only stable device identity. `selectedIndex` reorders across reboots and driver updates, which is why `StableVulkanReceipt` already omits it.
- A pinned device that no longer enumerates falls back to CPU and says so. It never silently selects a different GPU.
- Every accelerated run keeps proving which backend executed it. A transcript without a matching Vulkan receipt is a failure, not a result.
- Decode tuning is pinned to beam 3, best-of 5, temperature fallback enabled. That is the configuration with 400 transcriptions of zero WER delta across five languages in `.audit/whisper-phase5-small-v192-b3/decision.md`, and it is close enough to upstream defaults that the CPU fallback is not a downgrade.
- Editing `crates/echo-core/src/engine.rs` currently trips the behavior contract guard, so the digest chain has to go before the enum changes.

## Alternatives

### Keep the admission gate and re-qualify per release

Reject. Its reach is one GPU after five plan cycles, its evidence expires monthly, and `stage-qualified-whisper-release.py` writes `productionReady: False` at three sites with no code path writing `True`, while `release.yml` passes `--require-production-ready`. The reusable path cannot publish and the expensive path costs one to two working days per host.

### Bundle the Vulkan runtime in the deb and rpm

Reject. The tree is 58 MB, `libggml-vulkan.so` accounts for about 42 MB of it, and every user pays that download whether or not they own a usable GPU. `whisper_identity_v3.py:341` also restricts package types to deb and rpm, so AppImage users could never be reached. Bundling was only necessary because `whisper_portable.rs:407` binds the package to `echoBinarySha256`, and this plan removes that binding.

### Choose: managed component downloaded on demand

The `whisper-runtime` component at `crates/echo/src/install/catalog.rs:65` already delivers the CPU whisper.cpp build this way, with archive and per-file digest verification, range resume, symlink target pinning, and generation-named install directories. A Vulkan runtime is the same shape. Users who pick GPU fetch it, users who do not never see it, and AppImage works because delivery no longer runs through the package manager.

### Shadow calibration against user dictation

Defer. Adjudicating GPU against CPU on the user's own speech is the only mechanism that could catch a per-language quality regression on hardware nobody measured, and `CalibrationObservation`, `validate_new_calibration`, the job queue, and the leases already exist as dead code in `whisper_accel_cache.rs`. It is deferred because CPU is the default here, so the quality risk reaches nobody who did not choose it, and because this subsystem's failure mode has been too much machinery rather than too little rigor.

## Applicable skills

Implementers should invoke the **how** skill over `crates/echo/src/stt/` before the deletion phases, the **tdd** skill for the receipt-validation and device-pinning behavior, `/deslop` over every diff before commit, and the **unslop** skill on the changelog and settings copy. Phases 3 through 5 delete large amounts of code and warrant the **show-me-your-work** skill for a decision trail.

## Phases

1. [Retire the behavior projection contract](phase-1-retire-behavior-contract.md).
2. [Extract the quarantine primitives](phase-2-extract-quarantine.md).
3. [Delete the exact-host admission gate](phase-3-delete-admission-gate.md).
4. [Delete the planner and route store](phase-4-delete-planner.md).
5. [Delete the portable package gate](phase-5-delete-portable-gate.md).
6. [Make acceleration a two-state force](phase-6-two-state-preference.md).
7. [Deliver the Vulkan runtime as a managed component](phase-7-managed-vulkan-runtime.md).
8. [Enumerate GPU devices over IPC](phase-8-enumerate-devices.md).
9. [Add the GPU device picker](phase-9-device-picker.md).
10. [Run Whisper on the selected device](phase-10-gpu-execution.md).
11. [Report what actually ran](phase-11-acceleration-readout.md).
12. [Make releases assert what they ship](phase-12-release-honesty.md).

Phases 1 through 5 are pure subtraction and leave a CPU-only application that behaves exactly as 0.12.6 does today. Phase 6 is the last phase that can land without any GPU present. Phases 10 and 11 are the only ones requiring a Vulkan device to verify, and both fall back to CPU when none exists.

## Verification

Per-phase commands and thresholds live in [testing.md](testing.md). Project level:

```bash
npm run build --prefix frontend
npm run lint --prefix frontend
npm run test --prefix frontend
npm run test:responsive --prefix frontend
cargo clippy --workspace --all-targets -- -D warnings
xvfb-run -a cargo test --workspace
```

A green build proves the CPU path and the settings plumbing. It proves nothing about GPU execution, because no automated test in this repo drives a real device. `crates/echo/src/stt/backend/vulkan.rs:466` silently returns when `ECHO_TEST_VULKAN_PROBE` is unset and reports a pass it did not earn. Phase 1 converts it to an explicit skip.

## Implementation guidance

Land phases in order. Each is independently shippable and each ends with the application working. Do not begin phase 6 before phase 1 has landed, because the behavior contract guard forces a projection digest change for any edit to `crates/echo-core/src/engine.rs` and that digest chains into two fixtures.

Commit liberally within a phase and rebase into small ordered commits before opening a PR. Run `/deslop` over each diff. After opening a PR, use the **babysit** skill. Treat any automated review comment sceptically and dismiss noise with a stated reason.

The deletion phases will surface dead imports and orphaned tests in files this plan does not name. Remove them in the phase that orphans them rather than leaving them for a cleanup pass.
