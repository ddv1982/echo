# Portable Whisper acceleration plan

Echo will replace exact per-laptop GPU admission with portable runtime packages, receipt-verified local selection, and managed CPU recovery.
The program ships Vulkan first on compatible Linux x86_64 hardware.
PR 16.1 through PR 16.5 form the required sequence.
PR 16.6 measures persistent model reuse and stops unless it earns implementation.

## How to read this

One box is one unit of work. Every box names the evidence that checks it. Check a box only when its evidence exists, such as a file, log line, screenshot, test run, or SHA. The body is a how-to. The appendices explain and record.

The program runs `pstack/skills/poteto-mode/playbooks/autopilot-full.md`. One owner carries each PR through review and merge after the root gives a clean verdict. PR 16.4 stops for the operator's product review before merge.

Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

## Program checklist

### Arm the program

- [ ] State the protocol and this plan to the operator, then stop. Start execution only on her explicit go.
- [ ] On her go, arm a `/goal` with this exact text. "Run docs/plans/16-portable-whisper-acceleration/overview.md in PR 16.1 through PR 16.6 order. A PR is complete only when its unit, live, and perf evidence exists. Owners merge after a clean root verdict. PR 16.4 waits for operator review. Done means PR 16.5 ships portable cross-hardware Vulkan with CPU recovery, and PR 16.6 records a measured persistence decision."
- [ ] Read these from trunk at program start. Re-read them at every tick.
  - [ ] Run `git show origin/main:AGENTS.md` only when `git cat-file -e origin/main:AGENTS.md` succeeds. Otherwise record the user-supplied AGENTS instructions and the missing repo file.
  - [ ] `git show origin/main:docs/RELEASING.md`
  - [ ] Read `autopilot-full.md`, `opening-a-pr.md`, `pstack:swarm`, `cursor-team-kit:control-cli`, `cursor-team-kit:control-ui`, `pstack:interrogate`, and `pstack:show-me-your-work` from their installed plugin paths.
- [ ] Inventory the required whisper.cpp source, Small and Large models, VAD model, free disk, local Vulkan device, and external Intel, AMD, NVIDIA, CPU-only, and dual-GPU hosts. Record owners and availability before spawning implementation owners.
- [ ] Verify that `grok-4.6-fast-xhigh` and quota for ten simultaneous live lanes are available. If either is missing, stop and run `pstack:setup-pstack` to record an available lane model before revising and rechecking this plan.
- [ ] Arm the 30-minute audit tick. Use a real local `/loop`, or a cloud wake chain. Do not rely on memory.
- [ ] Use this tick prompt verbatim. "Re-read the execution playbook from trunk and the armed /goal. Audit the operation against both and fix drift in this tick. Probe every active lane and judge progress by side effects only. Stand down a stuck lane and dispatch its replacement now. Then send the operator a status message, whether or not anything changed, with the queue table of PR, owner, state, and head SHA, the verdicts since the last tick, what merged, open operator gates, and blockers."
- [ ] On the operator's hold or stand-down, send every owner a zero-writes order at once.

### Spawn owners

- [ ] Spawn one owner per PR with the full lifecycle that `autopilot-full.md` names.
- [ ] Follow this dependency graph. PR 16.1 starts from `main`. PR 16.2 follows PR 16.1. PR 16.3 follows PR 16.2. PR 16.4 follows PR 16.3. PR 16.5 follows PR 16.4. PR 16.6 follows PR 16.5.
- [ ] Hold the file boundaries that each PR lists. Do not edit a later PR's files early unless the plan lists the file in both PRs.
- [ ] Hold the review gate. PR 16.4 changes the acceleration setting and runtime status. It waits for the operator's review with screenshots and a video.
- [ ] Do not spawn the PR 16.4 owner until physical Intel, AMD, NVIDIA, CPU-only, and dual-GPU lanes are reserved. A missing lane is a blocker, not a pass.

### PR mechanics, for every PR

- [ ] Open the PR ready, never draft, with `gh pr create` and `draft: false`.
- [ ] Run the repo lint and typecheck once before the PR-facing push. Push with hooks on.
- [ ] Run `/deslop` before each commit and `/no-comments` before review.
- [ ] Run `pstack:interrogate` after product verification. Fix valid findings and record dismissals with evidence.
- [ ] Triage every Bugbot and security-reviewer comment by evidence.
- [ ] Rebase onto current trunk before babysit and again before the merge-ready report.

### Verdict and merge, for every PR

- [ ] At the merge-ready head SHA, run the swarm per `pstack/skills/swarm/SKILL.md`. Run one gates lane, the ten live lanes from the PR's **Verify, live** block, one perf lane, and one audit lane that distrusts the PR body.
- [ ] Run the full product verification commands from the repo at the exact head SHA. A targeted test run is not a substitute.
- [ ] Mark the PR clean only when every lane reports `PASS`. Send findings back to the owner. A new head gets a fresh verdict.
- [ ] Use `pstack:babysit` semantics after opening the PR. Fix CI and valid review comments. The owner squash-merges only after the root's clean verdict. PR 16.4 also requires the operator's review.

### Boot recipe, for every live lane

Each live lane runs on its own Linux VM or physical host at the PR head. Drive the CLI with `cursor-team-kit:control-cli`. Drive Settings with `cursor-team-kit:control-ui` under Xvfb.

- [ ] Run `git fetch origin <head-branch> && git checkout <head SHA>`.
- [ ] Build the exact release runtime and `echo-desktop`. Install the package in a disposable VM when the lane checks packaging.
- [ ] Deliver audio and settings only through the matching control skill. Use read-only logs, receipts, and package extraction for diagnosis.
- [ ] Save every screenshot to `/tmp/swarm-<pr-id>/worker-<n>/<slug>.png` and return the paths with the report.

## Build a portable managed runtime (PR 16.1)

**Depends on.** None.

**Files.**

- [ ] Edit `scripts/build-whisper-vulkan-receipt.sh` and the runtime archive verifier.
- [ ] Create a runtime build receipt and a portable CPU dispatch verifier under `scripts/`.
- [ ] Edit package resource configuration only as needed to include the CPU backend variants.

**Build.**

- [ ] Build the pinned whisper.cpp runtime with explicit `GGML_NATIVE=OFF`, `GGML_BACKEND_DL=ON`, `GGML_CPU_ALL_VARIANTS=ON`, `GGML_VULKAN=ON`, and `SOURCE_DATE_EPOCH`.
- [ ] Record the upstream revision, patch digest, CMake cache digest, compiler identity, CPU variants, and private ELF dependencies in the build receipt.
- [ ] Make managed CPU execution select a compatible optimized variant at runtime and always disable GPU.

**You see.**

- [ ] The verifier prints the selected CPU variant, `gpuDisabled=true`, the runtime artifact ID, and `portable=true`.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add self-tests that reject `GGML_NATIVE=ON`, a missing baseline variant, a missing optimized variant, an unpinned revision, and an external private library. Run `./scripts/verify-whisper-runtime-archive.sh`.
- [ ] Run `cargo test -p echo stt::runtime` and `cargo test -p echo stt::whisper`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head.

- [ ] Lane 1. Run CPU transcription with Vulkan hidden. Save `cpu-only.png`. Pass when the receipt names CPU, GPU is disabled, and the transcript parses.
- [ ] Lane 2. Run CPU transcription on this laptop. Save `cpu-variant.png`. Pass when the selected optimized variant matches detected features.
- [ ] Lane 3. Run the baseline binary under an x86_64-v2 CPU mask. Save `cpu-baseline.png`. Pass when no illegal instruction occurs.
- [ ] Lane 4. Remove one CPU variant from a copied bundle. Save `missing-variant.png`. Pass when verification fails before launch.
- [ ] Lane 5. Change one runtime library byte. Save `runtime-drift.png`. Pass when the artifact ID check fails.
- [ ] Lane 6. Resolve private libraries with `ldd`. Save `elf-bindings.png`. Pass when every Whisper and ggml library resolves inside the bundle.
- [ ] Lane 7. Run the runtime probe twice. Save `receipt-repeat.png`. Pass when both artifact IDs and CPU receipts agree.
- [ ] Lane 8. Install the Debian package in a VM. Save `deb-cpu.png`. Pass when managed CPU transcription succeeds.
- [ ] Lane 9. Install the RPM package in a VM. Save `rpm-cpu.png`. Pass when managed CPU transcription succeeds.
- [ ] Lane 10. Run with no compatible optimized module. Save `cpu-generic.png`. Pass when the portable baseline runs and the receipt reports the baseline variant.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Median and p95 managed CPU latency for Small and Large Turbo, plus runtime package bytes.
- [ ] Probe. Run ten interleaved trunk and head transcriptions per model with the same fixture, tuning, VAD, and CPU mask.
- [ ] Baseline. Record trunk median, p95, and package bytes before building the head runtime.
- [ ] Rule. Fail when head median or p95 is more than 10 percent slower, or when package growth exceeds the sum of the verified CPU variant files by more than 5 percent.

**Review gate.** None. PR 16.1 is not review-gated.

**Merge.**

- [ ] Record the exact runtime artifact ID and CPU dispatch receipt in the PR.
- [ ] Root gives a clean verdict at the exact head SHA, CI is green, and the owner squash-merges.

## Separate runtime evidence from app release identity (PR 16.2)

**Depends on.** PR 16.1.

**Files.**

- [ ] Edit `crates/echo/src/stt/whisper_admission.rs` and `crates/echo/src/stt/whisper_plan.rs`.
- [ ] Edit `scripts/sweep-whisper-admission.py`, `scripts/promote-whisper-admission.py`, `scripts/stage-qualified-whisper-release.py`, and `scripts/whisper_release_common.py`.
- [ ] Create the v3 runtime package manifest, inference contract, performance evidence, and release binding schemas.

**Build.**

- [ ] Define `ExecutionArtifactId`, `InferenceContractId`, `LocalEnvironmentKey`, and `PerformanceEvidenceId` as separate content-addressed values.
- [ ] Exclude the Echo commit, Echo ELF digest, Debian marker, and RPM marker from inference and hardware keys.
- [ ] Keep the exact Echo commit and ELF digest in a release binding that points to a verified runtime package and inference contract.
- [ ] Add a CI guard that requires an inference contract change when launch, decode, receipt, telemetry, or recovery behavior changes.

**You see.**

- [ ] A version-only Debian or RPM build reuses the same inference evidence and prints `physicalRequalificationRequired=false`.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add cross-language canonical hash fixtures for all four IDs. Run the Rust and Python schema self-tests.
- [ ] Add staging tests for app-only reuse and every behavior-changing invalidation. Run `./scripts/verify-whisper-acceleration.sh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head.

- [ ] Lane 1. Change only the workspace version. Save `version-reuse.png`. Pass when staging reuses evidence and binds the new ELF.
- [ ] Lane 2. Change only the Debian marker. Save `deb-marker-reuse.png`. Pass when staging reuses evidence.
- [ ] Lane 3. Change only the RPM marker. Save `rpm-marker-reuse.png`. Pass when staging reuses evidence.
- [ ] Lane 4. Change the runtime library. Save `runtime-invalidates.png`. Pass when evidence reuse is refused.
- [ ] Lane 5. Change the model digest. Save `model-invalidates.png`. Pass when the inference contract changes.
- [ ] Lane 6. Change the VAD digest. Save `vad-invalidates.png`. Pass when the inference contract changes.
- [ ] Lane 7. Change tuning. Save `tuning-invalidates.png`. Pass when the inference contract changes.
- [ ] Lane 8. Change request policy. Save `policy-invalidates.png`. Pass when the inference contract changes.
- [ ] Lane 9. Change receipt or recovery schema. Save `abi-invalidates.png`. Pass when the CI guard refuses the old contract.
- [ ] Lane 10. Extract fresh Debian and RPM packages. Save `release-binding.png`. Pass when each exact ELF points to the unchanged runtime package ID.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Cold and warm release-staging duration for unchanged inference evidence.
- [ ] Probe. Stage trunk with the current exact admission flow, then stage the head twice with the same runtime evidence.
- [ ] Baseline. Record trunk staging wall time and every physical command that it requires.
- [ ] Rule. Fail when the head still requires a hardware sweep for an app-only change, or when warm staging is slower than trunk package verification.

**Review gate.** None. PR 16.2 is not review-gated.

**Merge.**

- [ ] Record a truth table of changes that do and do not require physical requalification.
- [ ] Root gives a clean verdict at the exact head SHA, CI is green, and the owner squash-merges.

## Add receipt-driven local selection (PR 16.3)

**Depends on.** PR 16.2.

**Files.**

- [ ] Edit `crates/echo/src/stt/whisper_acceleration.rs`, `whisper_probe.rs`, `whisper_plan.rs`, and `whisper_recovery.rs`.
- [ ] Create `crates/echo/src/stt/whisper_accel_cache.rs` and `crates/echo/src/stt/backend/vulkan.rs`.
- [ ] Extend the packaged runtime probe to enumerate devices and prove explicit selectors.

**Build.**

- [ ] Keep one deep planner interface that accepts `Auto`, `Gpu`, or `Cpu` and returns managed CPU or GPU-then-CPU.
- [ ] Key compatibility, selection, shader cache, and quarantine by runtime, model, VAD, tuning, backend receipt, device UUID, driver UUID, and pipeline cache UUID.
- [ ] Store local observations as immutable per-key files. Do not ship host ICD paths or shader cache seeds.
- [ ] On an Auto cache miss, return CPU for the current request and schedule bounded background CPU and GPU calibration. Never block first dictation on calibration.
- [ ] Require ready and result receipts. Quarantine a failed key for 24 hours and permit exactly one managed CPU recovery.

**You see.**

- [ ] A new compatible host transcribes on CPU immediately, then reports a cached local Auto decision after background calibration.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add planner, key invalidation, immutable state, selector, receipt, quarantine, and one-retry tests. Run `cargo test -p echo stt::whisper_acceleration`.
- [ ] Run `cargo test -p echo stt::whisper_recovery` and the runtime probe self-tests.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head.

- [ ] Lane 1. Run Auto with an empty local cache. Save `auto-cold.png`. Pass when user audio uses CPU and calibration starts after the result.
- [ ] Lane 2. Run Auto after calibration. Save `auto-warm.png`. Pass when the cached winner and exact receipt are visible.
- [ ] Lane 3. Run GPU with an empty cache. Save `gpu-cold.png`. Pass when a compatible GPU is attempted without a shipped device allowlist.
- [ ] Lane 4. Force a wrong receipt. Save `wrong-receipt.png`. Pass when GPU output is rejected and CPU runs once.
- [ ] Lane 5. Force internal CPU fallback. Save `backend-fallback.png`. Pass when the result is rejected as GPU and CPU runs once.
- [ ] Lane 6. Force a GPU timeout. Save `gpu-timeout.png`. Pass when the process is reaped, the key is quarantined, and CPU runs once.
- [ ] Lane 7. Corrupt the local selection record. Save `corrupt-cache.png`. Pass when Auto uses CPU and preserves the bad record for diagnosis.
- [ ] Lane 8. Change the driver fingerprint. Save `driver-change.png`. Pass when selection and shader paths rotate without deleting old evidence.
- [ ] Lane 9. Reorder two GPU indices. Save `device-reorder.png`. Pass when selection follows a stable UUID or falls back safely.
- [ ] Lane 10. Start two Echo processes. Save `concurrent-state.png`. Pass when both write separate records and neither corrupts shared state.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Foreground planning overhead, background calibration duration, and warm selection overhead.
- [ ] Probe. Measure 100 cached planner calls and one cold calibration while transcribing the same fixture on managed CPU.
- [ ] Baseline. Record trunk `production_whisper_decision` overhead and managed CPU first-result latency.
- [ ] Rule. Fail when cached p95 planning adds more than 25 ms, or when cold Auto delays the user's CPU result by more than 5 percent.

**Review gate.** None. PR 16.3 is not review-gated.

**Merge.**

- [ ] Keep production preference pinned to CPU or the current exact behavior until PR 16.4 passes the hardware matrix.
- [ ] Root gives a clean verdict at the exact head SHA, CI is green, and the owner squash-merges.

## Ship Auto, GPU, and CPU modes with Vulkan (PR 16.4)

**Depends on.** PR 16.3.

**Files.**

- [ ] Edit config, CLI, Settings, status telemetry, and the `prepare_with_config` caller.
- [ ] Edit the Vulkan compatibility policy and hardware matrix workflow.
- [ ] Add scoped release documentation for compatible Linux x86_64 hardware.

**Build.**

- [ ] Make Auto the default. Auto uses GPU only after exact local calibration beats CPU by both 20 percent and 250 ms.
- [ ] Make GPU an explicit preference that tries a receipt-verified compatible GPU even when it is locally slower, with one CPU recovery.
- [ ] Make CPU skip GPU enumeration and always use `--no-gpu`.
- [ ] Keep automatic language and hints on CPU until their own paired release matrix passes. Show that policy in status instead of hiding it.
- [ ] Run the compatibility matrix on Intel Mesa, AMD RADV, NVIDIA Vulkan, CPU-only, and a dual-GPU host before enabling the default.

**You see.**

- [ ] Settings shows Auto, GPU, and CPU, plus the last actual backend, device, fallback reason, and whether calibration is pending.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add config, environment, CLI, IPC, frontend, accessibility, and status-copy tests for all three modes. Run the full frontend and Rust suites.
- [ ] Add matrix-result replay tests that refuse to enable Auto when any required compatibility lane is missing or inconclusive.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head.

- [ ] Lane 1. Select Auto on Intel Vulkan. Save `auto-intel.png`. Pass when the calibrated GPU runs and status names the receipt.
- [ ] Lane 2. Select Auto on AMD RADV. Save `auto-amd.png`. Pass when the calibrated winner runs or CPU remains selected with measured reason.
- [ ] Lane 3. Select Auto on NVIDIA Vulkan. Save `auto-nvidia.png`. Pass when the calibrated winner runs or CPU remains selected with measured reason.
- [ ] Lane 4. Select Auto on CPU-only Linux. Save `auto-cpu-only.png`. Pass when dictation works on CPU without an error dialog.
- [ ] Lane 5. Select GPU on a compatible GPU. Save `gpu-mode.png`. Pass when GPU is attempted and the actual device is shown.
- [ ] Lane 6. Select GPU on CPU-only Linux. Save `gpu-unavailable.png`. Pass when CPU recovery works and the unavailability reason is shown.
- [ ] Lane 7. Select CPU on a GPU host. Save `cpu-mode.png`. Pass when no GPU probe runs and status says CPU.
- [ ] Lane 8. Use automatic language in Auto. Save `auto-language-policy.png`. Pass when CPU runs and status names the policy reason.
- [ ] Lane 9. Use recognition hints in Auto. Save `hint-policy.png`. Pass when CPU runs and every hint reaches the request.
- [ ] Lane 10. Switch modes between recordings. Save `mode-switch.png`. Pass when each next recording follows the new preference without restarting Echo.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Paired CPU and Vulkan median, p95, quality, peak RSS, minimum available memory, swap delta, and failure rate per matrix host and model.
- [ ] Probe. Run the short screen first, then the full multilingual and product corpus only on matrix cells that pass the screen.
- [ ] Baseline. Record the portable managed CPU result first for every host, model, and decode policy.
- [ ] Rule. Auto may select GPU only when median improves by at least 20 percent and 250 ms, p95 improves, quality and hallucination gates pass, every receipt is exact, and resource and stability gates pass. Otherwise Auto selects CPU.

**Review gate.** The operator reviews before merge.

- [ ] Copy the ten Settings and status screenshots into `docs/qa/media/16.4-review-<slug>.png`.
- [ ] Record a 30 to 60 second video that switches Auto, GPU, and CPU and shows actual backend status. Save it as `docs/qa/media/16.4-review.mp4`.
- [ ] Post the screenshots and video in chat. The operator confirms that the labels, pending state, fallback state, and warnings are clear.

**Merge.**

- [ ] Save the complete hardware matrix receipts and the operator's review result.
- [ ] Root gives a clean verdict at the exact head SHA, CI is green, and the owner squash-merges after the operator review.

## Retire exact-machine admission and simplify releases (PR 16.5)

**Depends on.** PR 16.4.

**Files.**

- [ ] Delete the v2 exact admission selector, cache seed copier, composition path, and package-specific physical qualification code.
- [ ] Edit `.github/workflows/release.yml`, `docs/RELEASING.md`, and release staging tools.
- [ ] Update changelog text and package manifests to the v3 runtime package and release binding.

**Build.**

- [ ] Make the tag workflow build the current app, bind it to an unchanged verified runtime package, deep-verify both packages, and publish without a commit-specific qualification draft.
- [ ] Run full physical qualification only when `InferenceContractId`, runtime artifact, model, VAD, tuning, decode suite, or claim scope changes.
- [ ] Keep deterministic package, planner, receipt, fallback, and evidence replay tests in ordinary CI.
- [ ] Remove legacy APIs after every production caller and release path uses v3.

**You see.**

- [ ] An app-only release prints `reusedInferenceEvidence=true` and reaches publish-ready state without a hardware sweep or reboot.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run release-tool self-tests, workflow syntax checks, package extraction fixtures, and the full product suite.
- [ ] Add a repository search check that rejects production references to `AdmissionIdentity`, `admission-set.json`, package cache seeds, and `qualification-$commit`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head.

- [ ] Lane 1. Stage an app-only version bump. Save `release-version-only.png`. Pass when no physical command is requested.
- [ ] Lane 2. Stage a UI-only change. Save `release-ui-only.png`. Pass when evidence is reused.
- [ ] Lane 3. Stage a runtime change. Save `release-runtime-change.png`. Pass when physical qualification is required.
- [ ] Lane 4. Stage a model change. Save `release-model-change.png`. Pass when physical qualification is required.
- [ ] Lane 5. Stage a decode-policy change. Save `release-policy-change.png`. Pass when physical qualification is required.
- [ ] Lane 6. Extract the Debian package. Save `release-deb.png`. Pass when every runtime and release-binding digest matches.
- [ ] Lane 7. Extract the RPM package. Save `release-rpm.png`. Pass when every runtime and release-binding digest matches.
- [ ] Lane 8. Install Debian on a non-admitted GPU. Save `release-new-gpu.png`. Pass when local calibration can try it without a shipped host record.
- [ ] Lane 9. Install RPM on CPU-only Linux. Save `release-cpu-only.png`. Pass when managed CPU works.
- [ ] Lane 10. Run the tag workflow in a test repository. Save `release-workflow.png`. Pass when publish-ready assets need no qualification draft.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Operator wall time, hardware-run count, and package verification time for an app-only release.
- [ ] Probe. Replay the v0.12.4 release procedure and the new procedure with identical runtime evidence and no behavior change.
- [ ] Baseline. Record the four package/model sweeps, reboot evidence, promotion work, and staging time from v0.12.4.
- [ ] Rule. Fail when the new app-only path runs any physical sweep or requires a reboot. Package deep verification must not take longer than the old staging verification by more than 10 percent.

**Review gate.** None. PR 16.5 is not review-gated.

**Merge.**

- [ ] Verify the exact merge commit on `main`, create the requested version tag, babysit tag workflows, and verify every published asset and manifest.
- [ ] Root gives a clean verdict at the exact head SHA, CI is green, and the owner squash-merges before release work starts.

## Measure persistent model reuse before adding it (PR 16.6)

**Depends on.** PR 16.5.

**Files.**

- [ ] Extend the existing resident benchmark and decision artifact under `scripts/` and `docs/plans/16-portable-whisper-acceleration/`.
- [ ] Do not change the production protocol in this PR.
- [ ] Record the result for Small and Large Turbo on CPU and the selected Vulkan device.

**Build.**

- [ ] Compare one-shot against a receipt-verified worker that loads one model once, serializes requests, exits after a bounded idle TTL, and releases all model leases.
- [ ] Measure cold readiness, first request, ten warm requests, memory, cleanup, cancellation, driver crash containment, and transcript parity.
- [ ] Emit `PROCEED`, `STOP`, or `INCONCLUSIVE`. Create an implementation plan only on `PROCEED`.

**You see.**

- [ ] The decision names the absolute and relative warm-latency savings, memory cost, cleanup result, and whether persistence earned implementation.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add deterministic replay tests for the resident observation schema and decision gates. Run the resident benchmark self-test.
- [ ] Run the current one-shot planner, recovery, lease, and package tests to prove no production behavior changed.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head.

- [ ] Lane 1. Run Small CPU one-shot and resident. Save `resident-small-cpu.png`. Pass when receipts and transcripts match.
- [ ] Lane 2. Run Small Vulkan one-shot and resident. Save `resident-small-gpu.png`. Pass when receipts and transcripts match.
- [ ] Lane 3. Run Large CPU one-shot and resident. Save `resident-large-cpu.png`. Pass when receipts and transcripts match.
- [ ] Lane 4. Run Large Vulkan one-shot and resident. Save `resident-large-gpu.png`. Pass when receipts and transcripts match.
- [ ] Lane 5. Change language between warm requests. Save `resident-language.png`. Pass when each transcript follows its request.
- [ ] Lane 6. Change hints between warm requests. Save `resident-hints.png`. Pass when no request state leaks.
- [ ] Lane 7. Cancel an active request. Save `resident-cancel.png`. Pass when the worker stops or returns to a known ready state.
- [ ] Lane 8. Kill the worker during a request. Save `resident-crash.png`. Pass when CPU recovery runs once and quarantine records the GPU key.
- [ ] Lane 9. Wait through the idle TTL. Save `resident-idle.png`. Pass when the worker exits and releases model leases and memory.
- [ ] Lane 10. Run 100 sequential requests. Save `resident-stability.png`. Pass when memory remains within the gate and every response receipt stays exact.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Warm request median and p95, cold readiness plus first request, peak RSS, host available memory, swap delta, and cleanup time.
- [ ] Probe. Interleave ten one-shot and ten resident requests per model and backend after the same cache state.
- [ ] Baseline. Record the PR 16.5 one-shot values first on the same host and inference contract.
- [ ] Rule. `PROCEED` only when warm median improves by at least 25 percent and 300 ms, p95 improves, cold plus first request does not regress by more than 10 percent, transcript and receipt parity pass, and memory returns within 5 percent of baseline after idle exit.

**Review gate.** None. PR 16.6 is not review-gated.

**Merge.**

- [ ] Commit the rerunnable benchmark and the honest decision, including a `STOP` or `INCONCLUSIVE` result.
- [ ] Root gives a clean verdict at the exact head SHA, CI is green, and the owner squash-merges. Do not open a persistence implementation PR unless the result is `PROCEED`.

## Close the program

- [ ] Every box above is checked with its evidence, or a stop gate records why dependent work did not start.
- [ ] Verify the final `main` branch and the published release when PR 16.5 requests a version tag.
- [ ] Reply with the merged PRs, exact SHAs, hardware matrix, measured claims, release asset hashes, and the PR 16.6 persistence verdict.

## Appendix A. Prototype evidence

No new prototype ran during plan authoring. Existing Echo evidence already answers two key questions.

- The current exact selector and release flow work on the admitted Intel device, but app ELF coupling forces repeated package-specific qualification.
- Existing resident tests saved about 137 ms for Base and 186 ms for Large Turbo. Both missed the prior 300 ms gate, so persistence stays behind PR 16.6.

The local background calibration experience remains unproven. PR 16.3 must show that first dictation stays on CPU and does not wait for calibration. PR 16.4 is review-gated because this changes visible Settings and status behavior.

## Appendix B. Alternatives rejected

- Keep exact device records and remove only `echoBinarySha256`. Rejected because unknown compatible GPUs still cannot try acceleration.
- Turn on upstream GPU globally and trust exit status. Rejected because whisper.cpp can keep the CPU backend available, and a successful transcript does not prove which backend ran.
- Ship one universal GPU runtime. Rejected because Vulkan, CUDA, HIP, SYCL, and OpenVINO have different build and driver contracts.
- Ship a persistent worker in the first change. Rejected because Echo's existing resident measurements missed the release threshold.
- Copy one machine's shader cache into every package. Rejected because cache compatibility depends on the live device and driver identity.

## Appendix C. Risks

- Vulkan can return wrong transcripts on some device and driver combinations. PR 16.3 adds a local parity canary. PR 16.4 still requires a representative hardware matrix. The canary reduces risk but cannot prove every utterance on every driver.
- `GGML_NATIVE=OFF` alone is not a complete portable CPU policy. PR 16.1 must prove the actual baseline and runtime variant selection.
- Multi-GPU selection is weak upstream. PR 16.3 must select by stable UUID when possible and reject a receipt that names another device.
- A manual inference contract version can drift. PR 16.2 adds canonical contract fixtures and a watched-file CI guard.
- Background calibration can consume battery and compete with dictation. PR 16.3 must run only when idle and stop immediately when recording starts.
- The Intel laptop cannot prove AMD or NVIDIA support. PR 16.4 needs external physical hosts before Auto becomes the default.

## Appendix D. Links and reading list

- Echo grounding lives in `crates/echo/src/stt/whisper_acceleration.rs`, `whisper_admission.rs`, `whisper_plan.rs`, `whisper_recovery.rs`, and `docs/RELEASING.md`.
- The decision trail lives at `.audit/whisper-accel-overhaul.tsv`.
- The architecture arena artifacts live at `/tmp/echo-accel-arena/candidate-a/design.md`, `/tmp/echo-accel-arena/candidate-b/design.md`, `/tmp/echo-accel-arena/candidate-c/design.md`, and `/tmp/echo-accel-arena/judge.md`.
- The `pstack:how` skill applies to PR 16.1 through PR 16.5. The `pstack:interrogate` skill applies before each merge verdict.
- [whisper.cpp Vulkan support](https://github.com/ggml-org/whisper.cpp/blob/master/README.md#vulkan-gpu-support) documents the cross-vendor build.
- [whisper.cpp backend selection](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/src/whisper.cpp#L1290-L1358) shows that the GPU backend is optional and CPU is always added.
- [whisper.cpp CLI defaults](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/examples/cli/cli.cpp#L77-L80) set GPU use on by default. `--no-gpu` disables it.
- [ggml CPU build options](https://github.com/ggml-org/whisper.cpp/blob/v1.9.2/ggml/CMakeLists.txt) show the native default and dynamic CPU variant options.
- [whisper.cpp Vulkan quality report](https://github.com/ggml-org/whisper.cpp/issues/2400) records hardware-specific wrong-output failures.
- [Buzz runtime build](https://github.com/chidiwilliams/buzz/blob/main/Makefile) builds a portable Vulkan runtime and exposes CPU forcing.
- [Subtitle Edit speech-to-text backends](https://subtitleedit.github.io/subtitleedit/features/speech-to-text.html) expose CPU, Vulkan, and CUDA as selectable runtime choices.
- [CTranslate2 device selection](https://opennmt.net/CTranslate2/python/ctranslate2.Translator.html) exposes CPU, CUDA, and Auto while keeping hardware support tied to its packaged backend.
- [whisper.cpp server source](https://github.com/ggml-org/whisper.cpp/blob/master/examples/server/server.cpp) keeps one loaded model across requests, which PR 16.6 measures before adoption.
