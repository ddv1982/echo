# Code quality and release hygiene plan

This program fixes the integrity gap, shrinks the largest modules, makes IPC contracts authoritative, hardens releases, archives retired machinery, and rewrites the public documentation. Users get clearer installation and release trust. Maintainers get smaller modules and checks that stop the same debt returning. PRs 18.1 through 18.13 land in order as one reviewed stack. No task may change product behavior unless its section says so.

Start with [phases.md](phases.md) for the human control plan. Use this file during execution and verification.

## How to read this

One box is one unit of work. Every box names the evidence that checks it. A nested box is a sub-step of the box above it. Check a box only when its evidence exists, a file, a log line, a screenshot, a test run, or a SHA. The body is a how-to. The appendices explain and record.

The program runs the installed `pstack/skills/poteto-mode/playbooks/autopilot-stack.md` playbook. Owners stop at merge-ready. The operator reviews the Graphite stack and lands it.

Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

## Program checklist

### Arm the program

- [ ] State the protocol and this plan to the operator, then stop. Start execution only on their explicit go.
- [ ] On their go, arm a `/goal` with this exact text. "Run `docs/plans/18-code-quality-and-release-hygiene/overview.md` from PR 18.1 through PR 18.13 under autopilot-stack. Verify every PR with unit, live, and perf evidence. Owners never merge. Done means every PR is stack-ready, every operator gate is resolved, the decision trail is current, and the working tree is clean."
- [ ] Read the active installed `autopilot-stack`, `swarm`, `control-ui`, `control-cli`, `opening-a-pr`, `show-me-your-work`, `deslop`, and `no-comments` skill resources at program start. Re-read them at every tick.
  - [ ] Confirm trunk with `git show origin/main:README.md >/dev/null` before spawning owners.
  - [ ] Record the installed pstack version and each selected skill path in `docs/plans/18-code-quality-and-release-hygiene/decisions.tsv`.
- [ ] Arm the 30-minute audit tick. In a local session, use a real terminal `/loop`. In a cloud root, use a cloud-sleeper wake chain. Never leave the cadence to memory.
- [ ] Use this tick prompt verbatim. "Re-read the execution playbook and the armed /goal. Audit the operation against both and fix drift in this tick. Probe every active lane and judge progress by side effects only. Stand down a stuck lane and dispatch its replacement now. Then send the operator a status message, whether or not anything changed, with the queue table of PR, owner, state, and head SHA, the verdicts since the last tick, what merged, open operator gates, and blockers."
- [ ] On the operator's hold or stand-down, send every owner a zero-writes order at once.

### Spawn owners

- [ ] Spawn one owner per PR with the full lifecycle from `autopilot-stack.md`.
- [ ] Follow this dependency graph.
  - [ ] Start PR 18.1, PR 18.2, PR 18.3, and PR 18.9 from `main`.
  - [ ] Start PR 18.4 after PR 18.2 and PR 18.3.
  - [ ] Start PR 18.5 after PR 18.4.
  - [ ] Start PR 18.6 after PR 18.1.
  - [ ] Start PR 18.7 after PR 18.2.
  - [ ] Start PR 18.8 after PR 18.7.
  - [ ] Start PR 18.10 after PR 18.9.
  - [ ] Start PR 18.11 after PR 18.6 and PR 18.10.
  - [ ] Start PR 18.12 after PR 18.10 and after the PR 18.11 archive manifest exists.
  - [ ] Start PR 18.13 after PR 18.1, PR 18.5, PR 18.8, PR 18.9, PR 18.10, PR 18.11, and PR 18.12.
- [ ] Hold the file boundaries named in each PR section. An owner touches no file outside its section without logging the reason and receiving a root countersign.
- [ ] Hold the review gate. PR 18.3, PR 18.9, PR 18.11, PR 18.12, and PR 18.13 wait for the operator's review in chat with screenshots and a video before they enter the stack.

### PR mechanics, for every PR

- [ ] Open the PR ready, never draft, with `gh pr create` and `draft: false`, or with Graphite `gt` for the stack.
- [ ] Run the repository lint and typecheck once before the PR-facing push. Push with hooks on.
- [ ] Run `/deslop` before each commit and `/no-comments` before review.
- [ ] Triage every Bugbot and security-reviewer comment with the active pstack bug-triage guidance.
- [ ] Rebase onto current trunk before babysit and again before the stack-ready report.
- [ ] Append each decision and checkpoint to `docs/plans/18-code-quality-and-release-hygiene/decisions.tsv` with evidence.

### Verdict and merge, for every PR

- [ ] At the stack-ready head SHA, run the swarm from the active `swarm` skill. Use one gates lane, the ten live lanes from the PR's live block, the perf lane from its perf block, and one audit lane that distrusts the PR body.
- [ ] Mark a PR clean only when every lane reports `PASS`. Return findings to the owner. Run a fresh swarm for every new head SHA.
- [ ] On a clean verdict, append the PR to the Graphite stack. Never merge, arm auto-merge, or close it.
- [ ] After a restack, compare patch IDs. Re-run the verdict for every PR whose patch changed.

### Boot recipe, for every live lane

Each live lane runs on its own VM at the PR head. Use `control-ui` for the desktop and browser preview. Use `control-cli` for build, installer, release, and documentation commands.

- [ ] Run `git fetch origin <head-branch> && git checkout <head-SHA>`.
- [ ] Install the native packages from `.github/workflows/check.yml`, then run `npm ci --prefix frontend`.
- [ ] For UI lanes, start the Vite preview or `echo-desktop` and wait for the first status response. For CLI lanes, start the named command and capture its exit status.
- [ ] Deliver input only through the selected control skill. Use `git diff`, process listings, and generated manifests as read-only diagnostics.
- [ ] Save every screenshot to `/tmp/swarm-<pr-id>/worker-<n>/<slug>.png` and return the paths with the report.

## Detect managed payload changes reliably (PR 18.1)

**Depends on.** None.

**Files.**

- [ ] Edit `crates/echo/src/install/mod.rs`.
- [ ] Edit `crates/echo/src/install/tests.rs`.
- [ ] Edit `crates/echo/Cargo.toml` only if the restored-mtime test needs a test dependency.
- [ ] Edit `README.md` only to make the current integrity claim accurate.

**Build.**

- [ ] Delete trust in the persistent `verified.json` cache. Keep only a process-local cache keyed by file type, mode, size, device, inode, ctime, mtime, and symlink target.
- [ ] Treat an old on-disk verification stamp as a cache miss. Hash every regular file on a cold process cache and after any fingerprint change.
- [ ] Keep `ManagedStore::verify` as a forced full hash. Prime only the process cache after install verification succeeds.
- [ ] State the actual boundary. Echo detects persistent payload mutation but does not defend against an active writer running as the same account.

**You see.**

- [ ] A same-size payload edit with its mtime restored changes the component state to `needs-repair`.
- [ ] A forged legacy `verified.json` file cannot make a cold process accept changed content.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Extend `crates/echo/src/install/tests.rs` with equal-size restored-mtime and forged-stamp cases. Run `cargo test -p echo install::tests`.
- [ ] Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Start with a healthy managed CPU runtime. Save `cpu-ready.png`. Pass when Settings reports the runtime ready.
- [ ] Lane 2. Restart Echo with a cold process cache. Save `cold-ready.png`. Pass when the runtime remains ready after one full verification.
- [ ] Lane 3. Change one runtime byte without changing its size. Save `runtime-repair.png`. Pass when Settings offers repair.
- [ ] Lane 4. Restore the changed file's mtime. Save `mtime-repair.png`. Pass when Settings still offers repair.
- [ ] Lane 5. Add a forged legacy verification stamp. Save `forged-stamp.png`. Pass when Echo still rejects the changed payload.
- [ ] Lane 6. Run explicit Verify on a healthy model. Save `model-verify.png`. Pass when it completes without changing the active generation.
- [ ] Lane 7. Run explicit Verify on a changed model. Save `model-corrupt.png`. Pass when it reports corruption.
- [ ] Lane 8. Repair the changed runtime. Save `runtime-repaired.png`. Pass when a new verified generation becomes active.
- [ ] Lane 9. Start a CPU transcription after repair. Save `cpu-transcription.png`. Pass when the transcript completes.
- [ ] Lane 10. Start a GPU transcription after repair. Save `gpu-transcription.png`. Pass when telemetry names Vulkan or gives an explicit CPU fallback reason.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure cold and warm `ManagedStore::status(false)` time per active component.
- [ ] Probe. Add an ignored read-only timing test that takes `ECHO_MANAGED_ROOT`. Run it three times on trunk and three times at the head.
- [ ] Baseline. Record trunk metadata-check time and the direct SHA-256 reference time. The 2026-08-30 all-payload reference was 0.93 seconds cold and 0.59 seconds warm on this host.
- [ ] Rule. Fail when a warm status call exceeds 10 ms per component or when a cold full verification exceeds 1.25 times the direct SHA-256 reference.

**Review gate.** None. PR 18.1 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.1 to the Graphite stack. The operator lands the stack later.

## Generate the frontend IPC contract from Rust (PR 18.2)

**Depends on.** None.

**Files.**

- [ ] Create `src-tauri/src/ipc.rs`.
- [ ] Create `frontend/src/generated/ipc.ts`.
- [ ] Edit `src-tauri/src/main.rs`, `src-tauri/src/setup.rs`, `frontend/src/types.ts`, and `frontend/src/tauri.ts`.
- [ ] Add the selected generator and its deterministic drift check to the Rust workspace.

**Build.**

- [ ] Run a bounded spike against every current command and setup event. Choose the smallest generator that emits all tagged unions without unsafe casts or a second handwritten mirror.
- [ ] Move public transport DTOs and enums into `ipc.rs`. Keep core-domain types private and project them at the IPC boundary.
- [ ] Generate TypeScript during an explicit checked command. Fail CI when generated output differs from the committed file.
- [ ] Remove optional fields in TypeScript where Rust always serializes a nullable field.

**You see.**

- [ ] Editing a Rust IPC enum and skipping regeneration makes the contract check fail.
- [ ] The frontend imports command and event shapes only from `frontend/src/generated/ipc.ts`.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add Rust serialization fixtures for every tagged enum variant. Run `cargo test -p echo-desktop ipc`.
- [ ] Run the generator twice, compare output bytes, then run `npm run typecheck --prefix frontend`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Load Idle status from the real desktop. Save `status-idle.png`. Pass when the generated type accepts the response.
- [ ] Lane 2. Start recording. Save `status-recording.png`. Pass when the generated phase value renders Recording.
- [ ] Lane 3. Open Settings. Save `settings-read.png`. Pass when every generated settings field renders.
- [ ] Lane 4. Save one setting. Save `settings-write.png`. Pass when the read-back value matches.
- [ ] Lane 5. Enumerate microphones. Save `microphones.png`. Pass when every selection variant renders without a cast.
- [ ] Lane 6. Enumerate GPU devices. Save `gpu-devices.png`. Pass when device IDs survive the boundary unchanged.
- [ ] Lane 7. Start a setup operation. Save `setup-progress.png`. Pass when the generated progress event updates the row.
- [ ] Lane 8. Cancel setup. Save `setup-cancelled.png`. Pass when the generated cancelled event clears activity.
- [ ] Lane 9. Exercise a shortcut failure. Save `shortcut-failed.png`. Pass when the tagged failure variant renders.
- [ ] Lane 10. Load a last run with no performance data. Save `nullable-performance.png`. Pass when the required nullable field renders without an exception.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure clean contract-generation time and generated TypeScript bytes.
- [ ] Probe. Run the generator five times on trunk's spike and five times at the PR head.
- [ ] Baseline. Record the selected spike's median generation time and current handwritten contract bytes.
- [ ] Rule. Fail when generation takes more than 5 seconds or generated contract bytes exceed the handwritten contract by more than 25 percent.

**Review gate.** None. PR 18.2 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.2 after PR 18.1 in the Graphite stack.

## Serialize frontend polling and subscriptions (PR 18.3)

**Depends on.** None.

**Files.**

- [ ] Create `frontend/src/hooks/useSerialPoll.ts` and `frontend/src/hooks/useAsyncSubscription.ts`.
- [ ] Edit `frontend/src/App.tsx` and `frontend/src/App.test.tsx`.

**Build.**

- [ ] Schedule the next status poll only after the previous request settles. Ignore late results after disposal.
- [ ] Make async subscription cleanup call a listener that resolves after unmount.
- [ ] Route Settings microphone-test rejection and readiness refresh rejection to the visible error state.
- [ ] Migrate status polling, level polling, SetupChecklist, and Settings setup events to the shared hooks.

**You see.**

- [ ] Status requests never overlap, navigation does not leak setup listeners, and a rejected microphone test produces one visible error.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add deferred-promise tests for one request at a time, stale-result refusal, late unlisten, and rejected microphone tests. Run `npm run test --prefix frontend`.
- [ ] Run `npm run typecheck --prefix frontend && npm run lint --prefix frontend`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Leave Home visible for 30 seconds. Save `home-poll.png`. Pass when at most one status request is outstanding.
- [ ] Lane 2. Hide the window for 10 seconds. Save `hidden-window.png`. Pass when status polling pauses.
- [ ] Lane 3. Restore the window. Save `visible-window.png`. Pass when one polling chain resumes.
- [ ] Lane 4. Navigate Home to Settings 20 times. Save `settings-navigation.png`. Pass when setup-listener count returns to one.
- [ ] Lane 5. Leave Settings open for 15 seconds. Save `microphone-refresh.png`. Pass when microphone refreshes do not overlap.
- [ ] Lane 6. Reject microphone enumeration. Save `microphone-enumeration-error.png`. Pass when one error banner appears.
- [ ] Lane 7. Reject a microphone test. Save `microphone-test-error.png`. Pass when busy clears and the error appears.
- [ ] Lane 8. Resolve a setup subscription after Settings closes. Save `late-subscription.png`. Pass when the returned unlisten runs once.
- [ ] Lane 9. Start and stop recording during a slow status response. Save `slow-status.png`. Pass when the UI ends in the newest phase.
- [ ] Lane 10. Run shortcut verification while status polling continues. Save `shortcut-verification.png`. Pass when neither polling loop starves the other.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure maximum concurrent status calls, calls per 1.2 seconds, and idle renderer CPU.
- [ ] Probe. Use the preview adapter to delay status responses and count calls on trunk and at the head.
- [ ] Baseline. Record trunk concurrency and idle CPU before changing the hooks.
- [ ] Rule. Fail when concurrency exceeds one, visible calls exceed four per 1.2 seconds, or idle CPU grows by more than 5 percentage points.

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 7 and lane 9 screenshots into `docs/plans/18-code-quality-and-release-hygiene/media/18-3-review-errors.png` and `docs/plans/18-code-quality-and-release-hygiene/media/18-3-review-status.png`.
- [ ] Record a 30 to 60 second video of slow polling, navigation, and microphone failure. Save it as `docs/plans/18-code-quality-and-release-hygiene/media/18-3-review.mp4`.
- [ ] Post the screenshots and video in chat. Stop at stack-ready. Wait for the operator's review.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.3 after PR 18.2 in the Graphite stack.

## Separate the real and preview desktop adapters (PR 18.4)

**Depends on.** PR 18.2 and PR 18.3.

**Files.**

- [ ] Create `frontend/src/api/DesktopApi.ts`, `frontend/src/api/tauriDesktopApi.ts`, and `frontend/src/api/previewDesktopApi.ts`.
- [ ] Move preview fixtures and seed helpers out of `frontend/src/tauri.ts`.
- [ ] Edit frontend tests to construct the adapter they exercise.

**Build.**

- [ ] Define one `DesktopApi` from the generated IPC contract.
- [ ] Keep real `invoke` and `listen` calls in `tauriDesktopApi.ts`.
- [ ] Keep mutable preview state, timers, and test seeders in `previewDesktopApi.ts`.
- [ ] Select the adapter once at the application composition point. Prevent the production entry from importing preview fixtures.

**You see.**

- [ ] `frontend/src/tauri.ts` no longer mixes production transport with mutable browser fixtures, and Vite preview still works.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add a contract suite for deterministic adapter methods and exact Tauri command names. Run `npm run test --prefix frontend`.
- [ ] Add an import-boundary check that rejects preview imports from the production entry.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Start `npm run dev --prefix frontend`. Save `preview-home.png`. Pass when rich preview data renders.
- [ ] Lane 2. Toggle recording in preview. Save `preview-recording.png`. Pass when the preview timer changes phase.
- [ ] Lane 3. Save Settings in preview. Save `preview-settings.png`. Pass when the adapter returns projected values.
- [ ] Lane 4. Run preview setup. Save `preview-setup.png`. Pass when progress and completion events render.
- [ ] Lane 5. Run the real desktop Home view. Save `real-home.png`. Pass when `get_app_status` supplies data.
- [ ] Lane 6. Save a real setting. Save `real-setting.png`. Pass when disk read-back matches.
- [ ] Lane 7. Enumerate real microphones. Save `real-microphones.png`. Pass when the real adapter returns devices.
- [ ] Lane 8. Enumerate real GPU devices. Save `real-gpu.png`. Pass when the real adapter returns or explains no devices.
- [ ] Lane 9. Receive a real setup event. Save `real-setup-event.png`. Pass when the listener updates one row.
- [ ] Lane 10. Inspect the production module graph. Save `production-imports.png`. Pass when no preview module is reachable.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure initial production JavaScript bytes and adapter initialization time.
- [ ] Probe. Run `npm run build --prefix frontend` on trunk and at the head, then inspect the initial bundle and a performance mark around adapter creation.
- [ ] Baseline. Record trunk's initial bundle bytes and adapter startup median.
- [ ] Rule. Fail when the initial bundle grows by more than 3 percent or adapter startup exceeds 5 ms.

**Review gate.** None. PR 18.4 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.4 after PR 18.3 in the Graphite stack.

## Split frontend features out of App.tsx (PR 18.5)

**Depends on.** PR 18.4.

**Files.**

- [ ] Create focused modules under `frontend/src/app`, `frontend/src/home`, `frontend/src/history`, `frontend/src/dictionary`, `frontend/src/settings`, and `frontend/src/shortcuts`.
- [ ] Reduce `frontend/src/App.tsx` to composition and top-level routing.
- [ ] Move tests beside their feature or keep one integration suite for full-app behavior.

**Build.**

- [ ] Move Home, History, and Dictionary first without changing markup.
- [ ] Move Settings state into `useSettingsController` and leave presentational controls free of IPC calls.
- [ ] Move setup and shortcut verification into focused state owners.
- [ ] Keep one name for each action and delete comments that only narrate prior bugs or plan phases.

**You see.**

- [ ] `App.tsx` is a short composition root, each feature owns its async state, and rendered behavior matches trunk.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Move existing tests by behavior and add controller tests for write ordering and stale refresh. Run `npm run test --prefix frontend`.
- [ ] Run `npm run typecheck --prefix frontend && npm run lint --prefix frontend`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Open Home while idle. Save `home-idle.png`. Pass when status, setup, and recent history match trunk.
- [ ] Lane 2. Start and stop recording. Save `home-recording.png`. Pass when the timer and level bars work.
- [ ] Lane 3. Search History. Save `history-search.png`. Pass when grouping and filtering match trunk.
- [ ] Lane 4. Copy a transcript. Save `history-copy.png`. Pass when copied feedback appears.
- [ ] Lane 5. Add a dictionary entry. Save `dictionary-add.png`. Pass when the row appears once.
- [ ] Lane 6. Remove a dictionary entry. Save `dictionary-remove.png`. Pass when the row disappears.
- [ ] Lane 7. Change each General setting. Save `settings-general.png`. Pass when every value persists.
- [ ] Lane 8. Change CPU and GPU controls. Save `settings-advanced.png`. Pass when prerequisite and device states match trunk.
- [ ] Lane 9. Run microphone and shortcut tests. Save `settings-tests.png`. Pass when success and failure copy match trunk.
- [ ] Lane 10. Complete first-run setup. Save `first-run.png`. Pass when the checklist disappears at the same state as trunk.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure initial JavaScript bytes, Home render time, and background IPC calls.
- [ ] Probe. Capture Vite build output and a 30-second control-ui trace on trunk and at the head.
- [ ] Baseline. Record trunk's bundle bytes, Home render median, and idle IPC count.
- [ ] Rule. Fail when bundle bytes or render time grow by more than 5 percent, or when idle IPC count increases.

**Review gate.** None. PR 18.5 is a behavior-preserving move and is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.5 after PR 18.4 in the Graphite stack.

## Split the managed installer by responsibility (PR 18.6)

**Depends on.** PR 18.1.

**Files.**

- [ ] Create `crates/echo/src/install/types.rs`, `payload.rs`, `store.rs`, and `filesystem.rs`.
- [ ] Reduce `crates/echo/src/install/mod.rs` to module declarations and re-exports.
- [ ] Keep `crates/echo/src/install/installer.rs` as orchestration.

**Build.**

- [ ] Move DTOs and leases to `types.rs`.
- [ ] Move catalogue projection, extraction plans, hashing, and the process cache to `payload.rs`.
- [ ] Move `ManagedStore` lifecycle and locking to `store.rs`.
- [ ] Move containment, receipt-name validation, cleanup, and resume calculations to `filesystem.rs`.
- [ ] Preserve the public API and delete pass-through helpers that no longer earn their place.

**You see.**

- [ ] `install/mod.rs` is a composition root under 150 lines and every installer behavior remains unchanged.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run `cargo test -p echo install::tests` and the ignored pinned-archive tests when fixtures exist.
- [ ] Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Install the managed CPU runtime. Save `install-runtime.png`. Pass when one generation activates.
- [ ] Lane 2. Resume an interrupted model download. Save `resume-model.png`. Pass when received bytes resume above zero.
- [ ] Lane 3. Cancel a download. Save `cancel-download.png`. Pass when partial bytes remain resumable.
- [ ] Lane 4. Repair a missing file. Save `repair-file.png`. Pass when a new valid generation activates.
- [ ] Lane 5. Verify a healthy component. Save `verify-component.png`. Pass when the active generation stays unchanged.
- [ ] Lane 6. Remove a managed component. Save `remove-component.png`. Pass when only receipt-owned paths disappear.
- [ ] Lane 7. Run recovery with stale staging. Save `recover-staging.png`. Pass when safe stale paths are removed.
- [ ] Lane 8. Present an unsafe receipt path. Save `unsafe-receipt.png`. Pass when deletion is refused.
- [ ] Lane 9. Hold a runtime lease during remove. Save `leased-runtime.png`. Pass when removal waits or refuses without breaking the run.
- [ ] Lane 10. Complete a transcription after reinstall. Save `post-install-transcription.png`. Pass when the managed runtime executes.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure status, verify, install activation, and cleanup duration.
- [ ] Probe. Run the installer timing test on the same managed-root copy on trunk and at the head.
- [ ] Baseline. Record five trunk samples for each operation.
- [ ] Rule. Fail when any median grows by more than 5 percent or any allocation count grows by more than 10 percent.

**Review gate.** None. PR 18.6 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.6 after PR 18.5 in the Graphite stack.

## Extract the desktop shortcut subsystem (PR 18.7)

**Depends on.** PR 18.2.

**Files.**

- [ ] Create `src-tauri/src/shortcuts.rs`.
- [ ] Move GNOME repair, portal, X11, retry, state, and shutdown logic out of `src-tauri/src/main.rs`.
- [ ] Move `src-tauri/src/portal_runtime_tests.rs` under the shortcut module.

**Build.**

- [ ] Expose one crate-local shortcut facade for status, repair, retry, reconcile, and shutdown.
- [ ] Keep `FixedShortcut`, native state, listeners, and GNOME ownership rules together.
- [ ] Preserve Tauri command names and every current error string consumed by the frontend.

**You see.**

- [ ] `main.rs` delegates shortcut ownership to one module and every portal, X11, and GNOME test still passes.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run `cargo test -p echo-desktop shortcuts` and `cargo test -p echo-desktop portal_runtime_tests`.
- [ ] Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Register a portal shortcut. Save `portal-active.png`. Pass when Settings reports the effective trigger.
- [ ] Lane 2. Activate and deactivate the portal shortcut. Save `portal-toggle.png`. Pass when one recording toggles.
- [ ] Lane 3. Change the portal trigger. Save `portal-changed.png`. Pass when status shows the new effective trigger.
- [ ] Lane 4. Close the portal session. Save `portal-retry.png`. Pass when retry enters the expected state.
- [ ] Lane 5. Register the X11 shortcut in Xephyr. Save `x11-active.png`. Pass when another focused app still receives inserted text.
- [ ] Lane 6. Create an X11 conflict. Save `x11-conflict.png`. Pass when registration fails clearly.
- [ ] Lane 7. Inspect a ready GNOME binding. Save `gnome-ready.png`. Pass when Echo makes no write.
- [ ] Lane 8. Repair a stale GNOME binding. Save `gnome-repair.png`. Pass when only Echo-owned fields change.
- [ ] Lane 9. Present a conflicting GNOME binding. Save `gnome-conflict.png`. Pass when repair is refused.
- [ ] Lane 10. Quit the desktop. Save `shortcut-shutdown.png`. Pass when listeners unregister and no worker remains.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure desktop startup to shortcut-ready and idle shortcut worker CPU.
- [ ] Probe. Record ten launches on trunk and at the head under the same X11 or portal fixture.
- [ ] Baseline. Record trunk median ready time and 30-second idle CPU.
- [ ] Rule. Fail when ready time grows by more than 5 percent or idle CPU grows by more than 0.5 percentage points.

**Review gate.** None. PR 18.7 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.7 after PR 18.6 in the Graphite stack.

## Reduce main.rs to desktop composition (PR 18.8)

**Depends on.** PR 18.7.

**Files.**

- [ ] Create `src-tauri/src/settings.rs`, `status.rs`, and focused modules under `src-tauri/src/commands`.
- [ ] Reduce `src-tauri/src/main.rs` to CLI dispatch, desktop builder, tray, single-instance setup, and command registration.
- [ ] Move settings and status tests beside their owners.

**Build.**

- [ ] Move environment and file setting projection, validation, and locked writes to `settings.rs`.
- [ ] Move AppStatus, health caching, recording projection, and last-run telemetry to `status.rs`.
- [ ] Keep command handlers thin and group library, recording, device, and setup commands by owner.
- [ ] Preserve health invalidation after every config write and preserve all IPC names.

**You see.**

- [ ] `main.rs` is under 500 production lines and contains no GNOME parsing, settings mapping, or status projection.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run `cargo test -p echo-desktop` and `xvfb-run -a cargo test --workspace`.
- [ ] Run `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Launch the desktop with no arguments. Save `desktop-launch.png`. Pass when Home opens.
- [ ] Lane 2. Launch a second instance. Save `single-instance.png`. Pass when the existing window focuses.
- [ ] Lane 3. Close the window with a tray available. Save `tray-hide.png`. Pass when Echo stays running.
- [ ] Lane 4. Quit from the tray. Save `tray-quit.png`. Pass when the process exits cleanly.
- [ ] Lane 5. Read and save Settings. Save `settings-roundtrip.png`. Pass when every value round-trips.
- [ ] Lane 6. Read status through IPC. Save `status-ipc.png`. Pass when health and recording fields render.
- [ ] Lane 7. Add and remove a dictionary entry. Save `dictionary-ipc.png`. Pass when both commands persist.
- [ ] Lane 8. Read history after transcription. Save `history-ipc.png`. Pass when the latest row appears.
- [ ] Lane 9. Run `echo-desktop transcribe` without a desktop. Save `transcribe-cli.png`. Pass when the CLI exits with text.
- [ ] Lane 10. Replace the on-disk binary and launch again. Save `upgrade-restart.png`. Pass when the existing process hands over once.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure release binary bytes, desktop startup time, and `get_app_status` latency.
- [ ] Probe. Build release binaries and record twenty startup and IPC samples on trunk and at the head.
- [ ] Baseline. Record trunk binary bytes and median timings.
- [ ] Rule. Fail when binary bytes grow by more than 1 percent or either median timing grows by more than 5 percent.

**Review gate.** None. PR 18.8 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.8 after PR 18.7 in the Graphite stack.

## Publish complete release assets and license terms (PR 18.9)

**Depends on.** None.

**Files.**

- [ ] Create `LICENSE-MIT` and `LICENSE-APACHE`.
- [ ] Edit `.github/workflows/release.yml`, `scripts/verify-release-artifacts.sh`, `docs/RELEASING.md`, and the release section of `README.md`.
- [ ] Create a deterministic `SHA256SUMS` generator with a self-test.

**Build.**

- [ ] State that GitHub Releases, not every Git tag, are supported downloads.
- [ ] Make AppImage a required release input and remove its best-effort policy, because supported releases already publish it.
- [ ] Verify the final AppImage desktop entry and bundled executable. Do not compare its patched internal binary byte-for-byte with the unbundled binary.
- [ ] Generate and verify `SHA256SUMS` from the exact staged publish directory before upload.
- [ ] Run the complete staged-asset contract on pull requests and tag builds.

**You see.**

- [ ] Every supported release has two license texts, four application assets, and a verified checksum manifest.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run `scripts/verify-release-artifacts.sh --self-test` and the checksum-generator self-test.
- [ ] Run `cargo metadata --no-deps --format-version 1` and verify both license files exist.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Build the Debian package. Save `deb-built.png`. Pass when metadata and contents verify.
- [ ] Lane 2. Install the Debian package in a clean VM. Save `deb-installed.png`. Pass when `echo-desktop --version` matches.
- [ ] Lane 3. Build the RPM. Save `rpm-built.png`. Pass when metadata and contents verify.
- [ ] Lane 4. Extract the RPM through the normal path. Save `rpm-normal.png`. Pass when the expected files appear.
- [ ] Lane 5. Extract the RPM through the 7z fallback. Save `rpm-fallback.png`. Pass when the same files appear.
- [ ] Lane 6. Build the AppImage. Save `appimage-built.png`. Pass when its desktop entry and executable verify.
- [ ] Lane 7. Run the AppImage in extraction mode. Save `appimage-version.png`. Pass when its version matches the workspace.
- [ ] Lane 8. Verify the raw binary. Save `binary-version.png`. Pass when its version matches the workspace.
- [ ] Lane 9. Check `SHA256SUMS`. Save `checksums.png`. Pass when every staged asset verifies and no extra asset is present.
- [ ] Lane 10. Remove one staged asset. Save `missing-asset.png`. Pass when publication fails before upload.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure Linux package-job duration and checksum-generation time.
- [ ] Probe. Run the same release workflow on trunk and at the head with the GitHub Actions timing API.
- [ ] Baseline. Record trunk median from the last five successful main runs.
- [ ] Rule. Fail when checksum work adds more than 30 seconds or package-job duration grows by more than 10 percent without an explained runner variance.

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 6 and lane 9 screenshots into `docs/plans/18-code-quality-and-release-hygiene/media/18-9-review-appimage.png` and `docs/plans/18-code-quality-and-release-hygiene/media/18-9-review-checksums.png`.
- [ ] Record a 30 to 60 second video that downloads the staged assets and verifies `SHA256SUMS`. Save it as `docs/plans/18-code-quality-and-release-hygiene/media/18-9-review.mp4`.
- [ ] Post the screenshots and video in chat. Stop at stack-ready. Wait for the operator's review.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.9 after PR 18.8 in the Graphite stack.

## Pin workflows and attest release outputs (PR 18.10)

**Depends on.** PR 18.9.

**Files.**

- [ ] Edit `.github/workflows/check.yml` and `.github/workflows/release.yml`.
- [ ] Create `scripts/verify-workflow-pinning.py` and `.github/dependabot.yml`.
- [ ] Edit `docs/RELEASING.md`.

**Build.**

- [ ] Pin every `uses` reference to a reviewed full commit SHA. Add a check that rejects floating references.
- [ ] Let Dependabot propose GitHub Actions updates for review.
- [ ] Generate GitHub artifact attestations for final GitHub-built assets with the narrow required permissions.
- [ ] Generate an SBOM that covers Cargo and npm dependencies. Do not claim that it covers the separately operator-built Vulkan archive.
- [ ] Add a tag-policy check and document that a repository tag ruleset is the real protection against update or deletion.

**You see.**

- [ ] A release asset verifies against its checksum, GitHub attestation, and complete desktop SBOM.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run `python3 scripts/verify-workflow-pinning.py` against passing and failing fixtures.
- [ ] Validate both workflows and run `scripts/verify-release-artifacts.sh --self-test`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Inspect every workflow action reference. Save `action-pins.png`. Pass when all use full SHAs.
- [ ] Lane 2. Add a floating action to a fixture. Save `floating-action.png`. Pass when the policy check fails.
- [ ] Lane 3. Run Dependabot configuration validation. Save `dependabot.png`. Pass when GitHub accepts the file.
- [ ] Lane 4. Build the Cargo SBOM portion. Save `cargo-sbom.png`. Pass when workspace packages appear.
- [ ] Lane 5. Build the npm SBOM portion. Save `npm-sbom.png`. Pass when production and build dependencies appear.
- [ ] Lane 6. Combine the SBOM. Save `desktop-sbom.png`. Pass when both ecosystems remain identifiable.
- [ ] Lane 7. Create an artifact attestation in a non-release workflow. Save `attestation-created.png`. Pass when GitHub reports success.
- [ ] Lane 8. Verify the attestation. Save `attestation-verified.png`. Pass when `gh attestation verify` accepts the asset.
- [ ] Lane 9. Change one asset after attestation. Save `attestation-tamper.png`. Pass when verification fails.
- [ ] Lane 10. Simulate a repeated tag ref in the policy fixture. Save `tag-policy.png`. Pass when the guard refuses it and directs the maintainer to a new patch version.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure SBOM and attestation job duration.
- [ ] Probe. Run five workflow-dispatch builds without publication and compare job timings.
- [ ] Baseline. Record the PR 18.9 workflow median before adding provenance.
- [ ] Rule. Fail when provenance adds more than 90 seconds to the release critical path.

**Review gate.** None. PR 18.10 is not review-gated.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.10 after PR 18.9 in the Graphite stack.

## Archive retired qualification machinery (PR 18.11)

**Depends on.** PR 18.6 and PR 18.10.

**Files.**

- [ ] Move the three CI fixtures from `.audit/pr16-1-evidence` to `scripts/fixtures/whisper-runtime-performance` and update `scripts/verify-whisper-runtime-archive.sh`.
- [ ] Create `docs/history/README.md` and `docs/history/evidence-2026-08-30.md`.
- [ ] Delete retired qualification-only scripts after the archive exists.
- [ ] Remove plans 01 through 17, retired QA reports, and raw `.audit` data only after the archive manifest resolves.

**Build.**

- [ ] Create a deterministic external evidence archive with a SHA-256 manifest and source commit. Keep it separate from application releases.
- [ ] Make current CI independent of `.audit` before deleting any audit path.
- [ ] Delete only scripts unreachable from current CI, runtime, release, and maintained research commands.
- [ ] Distill current architecture facts before removing historical plans. Preserve Git history as the complete source record.

**You see.**

- [ ] Current CI has no `.audit` dependency, retired qualification commands are absent, and the external archive reproduces the removed evidence inventory.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run `verify-stt-benchmark.sh`, `verify-stt-corpus.sh`, and `verify-whisper-runtime-archive.sh` after the fixture move.
- [ ] Run a dead-reference scan for deleted scripts, `.audit`, `qualification-`, and `docs/plans/01` through `docs/plans/17`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. List the pre-change audit inventory. Save `audit-before.png`. Pass when the manifest count matches trunk.
- [ ] Lane 2. Build the evidence archive. Save `archive-built.png`. Pass when it is deterministic across two runs.
- [ ] Lane 3. Verify the archive checksum. Save `archive-checksum.png`. Pass when the recorded SHA-256 matches.
- [ ] Lane 4. Extract the archive in a clean directory. Save `archive-extracted.png`. Pass when every manifest path exists.
- [ ] Lane 5. Run runtime-performance verification from the new fixture path. Save `fixture-verification.png`. Pass when the result matches trunk.
- [ ] Lane 6. Run the benchmark verifier. Save `benchmark-verifier.png`. Pass when its current checks pass.
- [ ] Lane 7. Run the corpus verifier. Save `corpus-verifier.png`. Pass when its current checks pass.
- [ ] Lane 8. Run the runtime archive verifier. Save `runtime-verifier.png`. Pass when both pinned archives install.
- [ ] Lane 9. Search current code and docs for retired commands. Save `dead-reference-scan.png`. Pass when only historical manifest text remains.
- [ ] Lane 10. Clone the PR head into a clean directory. Save `clean-clone.png`. Pass when all maintained checks find their fixtures.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure tracked bytes, clone checkout bytes, and maintained verification duration.
- [ ] Probe. Record `git ls-files` sizes and the three verifier timings on trunk and at the head.
- [ ] Baseline. Record trunk's 5.4 MB tracked `.audit` size and current verifier medians.
- [ ] Rule. Fail when tracked bytes do not fall by at least 4 MB or any maintained verifier grows by more than 5 percent.

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 2, lane 3, and lane 4 screenshots into `docs/plans/18-code-quality-and-release-hygiene/media/18-11-review-archive.png`, `docs/plans/18-code-quality-and-release-hygiene/media/18-11-review-checksum.png`, and `docs/plans/18-code-quality-and-release-hygiene/media/18-11-review-extract.png`.
- [ ] Record a 30 to 60 second video that rebuilds, checks, and extracts the archive. Save it as `docs/plans/18-code-quality-and-release-hygiene/media/18-11-review.mp4`.
- [ ] Post the screenshots, video, archive URL, manifest, and checksum in chat. Stop at stack-ready. Wait for the operator's review before deleting tracked evidence.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA and verifies the external archive before accepting deletions.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.11 after PR 18.10 in the Graphite stack.

## Prepare safe historical release cleanup (PR 18.12)

**Depends on.** PR 18.10 and the PR 18.11 archive manifest.

**Files.**

- [ ] Create `scripts/audit-release-history.sh` with read-only default behavior and an explicit apply mode.
- [ ] Create `docs/history/releases.md`.
- [ ] Edit `docs/RELEASING.md` with the remote-cleanup policy.

**Build.**

- [ ] Inventory every tag, published release, draft, asset, and asset digest into a reviewable manifest.
- [ ] Make apply mode idempotent. It may delete only the five named obsolete qualification drafts and add a warning to v0.12.6.
- [ ] Preserve every published tag, published asset, and tag without a GitHub Release.
- [ ] Require an exact manifest digest and an operator confirmation token before apply mode performs any GitHub write.
- [ ] Leave repository tag ruleset changes as named operator steps after PR 18.10 lands.

**You see.**

- [ ] Dry run reports five deletable drafts, one release-note edit, four preserved tags without releases, and no published deletion.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Test the audit and apply logic against captured GitHub API fixtures, including repeated runs and changed remote state.
- [ ] Run the script in dry-run mode against `ddv1982/echo` and compare its manifest with `docs/history/releases.md`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. List all published releases. Save `published-releases.png`. Pass when no release is marked for deletion.
- [ ] Lane 2. List all drafts. Save `draft-releases.png`. Pass when exactly five qualification drafts are marked.
- [ ] Lane 3. List tags without releases. Save `orphan-tags.png`. Pass when four tags are preserved.
- [ ] Lane 4. Inspect v0.12.6. Save `v0126-before.png`. Pass when the proposed warning names v0.13.0 and changes no asset.
- [ ] Lane 5. Change one fixture asset digest. Save `manifest-drift.png`. Pass when apply mode refuses the stale manifest.
- [ ] Lane 6. Run apply mode without a token. Save `missing-token.png`. Pass when no API write occurs.
- [ ] Lane 7. Run apply mode twice against a mock server. Save `idempotent-apply.png`. Pass when the second run has no mutation.
- [ ] Lane 8. Present an extra draft. Save `unexpected-draft.png`. Pass when the script refuses broad deletion.
- [ ] Lane 9. Present a published release as a delete target. Save `published-protected.png`. Pass when validation rejects it.
- [ ] Lane 10. Re-run the real dry run immediately before operator review. Save `final-dry-run.png`. Pass when its digest matches the reviewed manifest.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure GitHub API calls and dry-run duration.
- [ ] Probe. Run five dry runs against captured fixtures and two read-only runs against GitHub.
- [ ] Baseline. Record the minimum calls needed to list releases, tags, and assets once.
- [ ] Rule. Fail when the script makes more than 10 API calls or a read-only run exceeds 30 seconds without a GitHub rate-limit response.

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 2, lane 4, and lane 10 screenshots into `docs/plans/18-code-quality-and-release-hygiene/media/18-12-review-drafts.png`, `docs/plans/18-code-quality-and-release-hygiene/media/18-12-review-v0126.png`, and `docs/plans/18-code-quality-and-release-hygiene/media/18-12-review-dry-run.png`.
- [ ] Record a 30 to 60 second video of the dry run and refusal paths. Save it as `docs/plans/18-code-quality-and-release-hygiene/media/18-12-review.mp4`.
- [ ] Post the screenshots, video, manifest digest, and exact planned GitHub writes in chat. Stop at stack-ready. Wait for the operator's review.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.12 after PR 18.11 in the Graphite stack. The script stays in dry-run mode until the operator separately authorizes remote apply.

## Rebuild the public documentation (PR 18.13)

**Depends on.** PR 18.1, PR 18.5, PR 18.8, PR 18.9, PR 18.10, PR 18.11, and PR 18.12.

**Files.**

- [ ] Rewrite `README.md`.
- [ ] Create or update `docs/architecture.md`, `docs/cli.md`, `docs/troubleshooting.md`, `docs/gpu-runtime.md`, and `docs/qa/README.md`.
- [ ] Keep `docs/RELEASING.md` as the maintainer release how-to.

**Build.**

- [ ] Keep the README focused on purpose, install, first dictation, privacy, one CLI example, short source build, troubleshooting links, maintainer links, and license.
- [ ] Move CLI reference, shortcut internals, managed-runtime details, QA evidence, source cleanup, status-file behavior, injection details, and live hardware checks to their owning documents.
- [ ] Remove the first-build plan link, retired QA evidence table, and unfinished Flathub brand note.
- [ ] Check every command, path, release claim, and internal link against the final stack.

**You see.**

- [ ] A new user can find a supported package, complete a first dictation, and understand local data handling without reading implementation history.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Run the Markdown link checker and every documented command that is safe in a clean checkout.
- [ ] Run `npm run build --prefix frontend`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.6-fast-xhigh` at the PR head, per the boot recipe.

- [ ] Lane 1. Render the README at desktop width. Save `readme-desktop.png`. Pass when install and first use appear before developer material.
- [ ] Lane 2. Render the README at mobile width. Save `readme-mobile.png`. Pass when headings, tables, and code blocks do not overflow.
- [ ] Lane 3. Follow the GitHub Releases link. Save `readme-release-link.png`. Pass when it reaches the supported release page.
- [ ] Lane 4. Follow the CLI link. Save `cli-doc.png`. Pass when every current subcommand is documented once.
- [ ] Lane 5. Follow the troubleshooting link. Save `troubleshooting-doc.png`. Pass when shortcut, microphone, model, and stale-install cases are findable.
- [ ] Lane 6. Follow the architecture link. Save `architecture-doc.png`. Pass when current module ownership matches the final tree.
- [ ] Lane 7. Follow the GPU runtime link. Save `gpu-runtime-doc.png`. Pass when CPU default and on-demand GPU behavior are accurate.
- [ ] Lane 8. Follow the release link. Save `releasing-doc.png`. Pass when AppImage, checksums, attestations, and immutable tags are accurate.
- [ ] Lane 9. Follow the license links. Save `license-links.png`. Pass when both license texts render.
- [ ] Lane 10. Search rendered docs for retired qualification terms. Save `retired-terms.png`. Pass when none appear outside the historical release record.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure README bytes, broken-link count, and clean-checkout documentation command duration.
- [ ] Probe. Run the link checker and documented-command script on trunk and at the head.
- [ ] Baseline. Record trunk's 186 README lines, current bytes, and current link-check duration.
- [ ] Rule. Fail when the README exceeds 120 lines, any internal link is broken, or documentation checks take more than 60 seconds excluding Rust compilation.

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 1, lane 2, and lane 3 screenshots into `docs/plans/18-code-quality-and-release-hygiene/media/18-13-review-desktop.png`, `docs/plans/18-code-quality-and-release-hygiene/media/18-13-review-mobile.png`, and `docs/plans/18-code-quality-and-release-hygiene/media/18-13-review-release.png`.
- [ ] Record a 30 to 60 second video that navigates from README to install, CLI, troubleshooting, release, and license pages. Save it as `docs/plans/18-code-quality-and-release-hygiene/media/18-13-review.mp4`.
- [ ] Post the screenshots and video in chat. Stop at stack-ready. Wait for the operator's review.

**Merge.**

- [ ] Root records a clean verdict at the exact head SHA.
- [ ] Bugbot and security-reviewer triage is complete.
- [ ] Rebase onto current trunk after the verdict and prove the patch ID is unchanged.
- [ ] Root appends PR 18.13 after PR 18.12 in the Graphite stack. The operator reviews and lands the full stack.

## Close the program

- [ ] Check every box above against its evidence.
- [ ] Run the full check workflow and the full staged release contract at the final stack tip.
- [ ] Confirm the repository has no unexpected working-tree changes and the decision trail resolves every evidence pointer.
- [ ] Apply the approved PR 18.12 GitHub release cleanup only after the repository stack lands and the operator gives the exact apply token.
- [ ] Enable the GitHub full-SHA action policy and immutable `v*` tag ruleset only after PR 18.10 lands and the operator approves those repository settings.
- [ ] Reply with the final Graphite stack links, one verdict per PR, external changes made, archived artifacts, and any parked work.

## Appendix A. Prototype evidence

The managed-payload hashing probe ran at trunk SHA `5fb579b`. It hashed every active managed payload under the local Echo cache three times. The measured wall times were 0.93 seconds, 0.59 seconds, and 0.59 seconds. Always hashing more than 1 GB on the status path is too slow. PR 18.1 therefore hashes once on a cold process cache and after a strong file-identity change. It never trusts the payload-adjacent stamp.

The polling defect needs no behavior prototype. `App.tsx` starts a new request every 400 ms without waiting for the prior request. PR 18.3 begins with a deferred-promise regression test.

The Rust-to-TypeScript generator remains unproven against Echo's DTO graph. PR 18.2 starts with a bounded spike. The spike must cover every command and event, preserve every tagged union, produce deterministic output, and require no unsafe cast or handwritten mirror. Reject any tool that misses one predicate.

## Appendix B. Alternatives rejected

Do not hash every model on every readiness poll. The local probe measured 0.59 to 0.93 seconds for the active payload set.

Do not retain `verified.json` as a trust source. It lives beside user-owned payloads and the same writer can change both.

Do not rewrite the frontend and backend monoliths in one PR. Move one owner at a time and keep behavior green after each stack link.

Do not publish retrospective binaries for tags without GitHub Releases. Preserve those tags as source-history markers.

Do not delete raw audit evidence before a deterministic external archive and in-repository manifest exist.

Do not keep AppImage as a sometimes-supported attachment. Existing releases already present it to users, so make it a required verified asset.

## Appendix C. Risks

PR 18.1 can add cold-start latency. Measure real managed roots and keep warm checks under 10 ms per component.

PR 18.2 can choose a generator that leaks framework types through the application. Keep DTO ownership in `ipc.rs` and keep core types private.

PR 18.3 changes timing. Test late responses, hidden windows, unmount, and rejected calls before changing component boundaries.

PR 18.5 and PR 18.8 can become unreadable move diffs. Preserve markup and behavior, enable moved-code review, and reject opportunistic redesign.

PR 18.9 can make AppImage failures block releases. Prove the complete contract on pull requests before making the job required.

PR 18.10 attestations cover GitHub-built assets only. The operator-built Vulkan runtime keeps a separate receipt and checksum contract.

PR 18.11 deletes the largest body of historical material. Verify the external archive twice and keep its digest in the repository.

PR 18.12 changes public GitHub state. Dry run is the default. The operator supplies the only apply token after reviewing the exact manifest digest.

PR 18.13 can make docs shorter but less useful. Move detail to one clear owner document before deleting it from README.

## Appendix D. Links and reading list

Read `crates/echo/src/install/mod.rs`, `src-tauri/src/main.rs`, `src-tauri/src/setup.rs`, `frontend/src/App.tsx`, `frontend/src/tauri.ts`, and `frontend/src/types.ts` before editing their owners.

Read `.github/workflows/check.yml`, `.github/workflows/release.yml`, `scripts/verify-release-artifacts.sh`, `scripts/verify-whisper-runtime-archive.sh`, `docs/RELEASING.md`, `README.md`, and `CHANGELOG.md` before release or documentation work.

PR 18.1, PR 18.2, PR 18.3, PR 18.6, PR 18.7, PR 18.8, PR 18.9, PR 18.10, and PR 18.12 use the pstack `how` skill before implementation. PR 18.2, PR 18.9, PR 18.10, and PR 18.12 use `interrogate` before stack-ready.

The root keeps `docs/plans/18-code-quality-and-release-hygiene/decisions.tsv` under `show-me-your-work`. Each owner keeps its local uncommitted `decisions.tsv` and returns it with the stack-ready report. The root trail is append-only and records each accepted PR SHA, verdict, operator gate, archive digest, and GitHub write.
