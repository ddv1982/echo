# Phased remediation plan

This plan fixes the code quality and release hygiene issues from the review without mixing unrelated work into one large change. Each phase ends in a checkable state. The detailed pstack checklist is in `overview.md`; use this file as the human control plan.

## Definition of done

The work is done when these statements are true.

- Managed payload verification does not trust a persistent stamp beside mutable payload files.
- Rust owns the IPC contract, and TypeScript consumes generated or checked types.
- Frontend polling and async subscriptions cannot overlap or write state after disposal.
- `App.tsx`, `src-tauri/src/main.rs`, `frontend/src/tauri.ts`, and `crates/echo/src/install/mod.rs` are split by responsibility.
- Releases publish the MIT license, checksums, and provenance for supported assets.
- Historical release cleanup is scripted, dry-run first, and destructive only with an explicit reviewed manifest.
- Retired plans, qualification artifacts, and `.audit` data are removed from the active tree after required evidence is archived.
- `README.md` is a product README, not a build log, architecture note, and release audit in one file.

## Phase 0. Baseline and guardrails

Purpose. Capture the current state before changing behavior.

Work.

- Record the current commit, CI status, release inventory, largest files, tracked `.audit` size, and README line count.
- Run the current verification commands that already pass locally.
- Save the pstack checker output for `overview.md`, but treat it as a shape check only.
- Create `docs/plans/18-code-quality-and-release-hygiene/decisions.tsv` when execution starts.

Verification.

- `git status --short` shows only planned files.
- `node /home/vriesd/.codex/plugins/cache/cursor-plugins/pstack/0.14.5/skills/poteto-mode/scripts/check-plan.mjs docs/plans/18-code-quality-and-release-hygiene/overview.md` runs and its output is archived.
- The baseline note lists commands that could not run and why.

## Phase 1. Fix correctness and trust first

Purpose. Remove the highest-risk gap before refactors make the code move around.

PR 18.1 fixes managed payload verification.

- Delete trust in the on-disk `verified.json` stamp.
- Keep a process-local verification cache only after a hash succeeds.
- Add tests for same-size mutation with restored mtime and forged legacy stamps.
- Update README wording if it still claims stronger tamper resistance than Echo provides.

PR 18.9 adds license and checksum release basics.

- Add `LICENSE-MIT` and publish Echo under MIT.
- Generate and verify `SHA256SUMS` from the staged publish directory.
- Make the release verifier reject missing supported assets.
- Make AppImage a required, verified release asset because supported releases already publish it.

PR 18.10 pins release workflows and adds provenance.

- Pin third-party GitHub Actions by full commit SHA.
- Add a check that rejects floating `uses` references.
- Add Dependabot updates for GitHub Actions.
- Add GitHub artifact attestations for GitHub-built release assets.
- Add an SBOM for Cargo and npm dependencies. Keep the Vulkan runtime under its separate checksum contract.

Verification.

- Rust install tests cover the restored-mtime bypass.
- `scripts/verify-release-artifacts.sh --self-test` passes.
- The release workflow publishes a checksum manifest in a dry-run or staged run.
- A fixture with a floating action fails the workflow-pinning check.

## Phase 2. Stabilize app boundaries

Purpose. Make the frontend and Tauri boundary safe before splitting files.

PR 18.2 generates or checks IPC types from Rust.

- Move public DTOs and enums into a focused Rust IPC module.
- Generate TypeScript into `frontend/src/generated/ipc.ts`, or add a deterministic contract check if generation cannot cover Echo cleanly.
- Remove handwritten optional fields where Rust always serializes nullable fields.

PR 18.3 serializes polling and async subscriptions.

- Replace interval polling with a loop that starts the next request only after the previous one settles.
- Ignore late async results after unmount.
- Ensure late subscription setup still runs its unlisten callback.
- Route microphone-test failures to visible Settings error state.

PR 18.4 separates real Tauri transport from preview fixtures.

- Define one `DesktopApi`.
- Keep real `invoke` and `listen` calls in one production adapter.
- Move browser preview data, mutable fixtures, and seed helpers into a preview adapter.
- Add an import check so production cannot import preview fixtures.

Verification.

- Frontend tests prove no overlapping polls, no stale state write, no leaked listener, and visible microphone-test errors.
- TypeScript typecheck and lint pass.
- Editing a Rust IPC enum without regenerating TypeScript fails the contract check.
- Production bundle inspection shows no preview fixture import path.

## Phase 3. Split the large modules

Purpose. Reduce reader load after behavior has tests around it.

PR 18.5 splits frontend features out of `App.tsx`.

- Move Home, History, Dictionary, Settings, Setup, and Shortcuts into focused feature modules.
- Keep `App.tsx` as composition and top-level routing.
- Move feature tests beside the code where practical.
- Do not redesign UI in this PR.

PR 18.6 splits the managed installer.

- Move types, payload verification, store lifecycle, and filesystem cleanup into separate files under `crates/echo/src/install`.
- Keep public installer behavior unchanged.
- Keep `mod.rs` as module declarations and re-exports.

PR 18.7 extracts the desktop shortcut subsystem.

- Move portal, X11, GNOME repair, retry, state, and shutdown logic out of `src-tauri/src/main.rs`.
- Keep Tauri command names and frontend-visible error strings stable.

PR 18.8 reduces `src-tauri/src/main.rs` to desktop composition.

- Move settings projection, status projection, and command groups into focused modules.
- Keep `main.rs` responsible for CLI dispatch, desktop builder, tray, single instance, and command registration.

Verification.

- Existing UI tests and Rust tests pass after each PR, not only at the end.
- `cargo fmt --all --check` and workspace Clippy pass in CI.
- `App.tsx`, `main.rs`, and `install/mod.rs` hit the target roles stated above.
- Moved-code review shows no opportunistic product change.

## Phase 4. Clean release history and retired evidence

Purpose. Remove active-tree noise without rewriting public history.

PR 18.11 archives retired qualification machinery.

- Move CI-required `.audit/pr16-1-evidence` fixtures into a maintained fixture path.
- Update `scripts/verify-whisper-runtime-archive.sh`.
- Archive raw audit evidence outside the active tree with a manifest and SHA-256 digest.
- Delete retired plans, obsolete qualification scripts, and raw `.audit` files only after the archive verifies.

PR 18.12 prepares GitHub release cleanup.

- Add a read-only release-history audit script.
- Make apply mode idempotent and locked to an exact manifest digest.
- After the stack lands, let the operator delete only the obsolete `qualification-*` drafts named by the reviewed manifest.
- After the stack lands, let the operator add a superseded warning to `v0.12.6`.
- Preserve all published releases, all published assets, and tags that have no GitHub Release.
- Leave repository tag ruleset changes as manual operator steps.

Verification.

- Current CI no longer reads `.audit`.
- Dead-reference scan finds no live references to retired commands or deleted plan paths.
- Release audit dry run reports the exact drafts, orphan tags, and release-note edit before any write.
- Apply mode refuses to run without the reviewed digest and token.

## Phase 5. Rewrite documentation last

Purpose. Make the README match the final product and final release process.

PR 18.13 rewrites public docs.

- Keep `README.md` focused on what Echo does, supported Linux install paths, first dictation, privacy, one CLI example, short source build, troubleshooting links, maintainer links, and license.
- Move CLI reference to `docs/cli.md`.
- Move architecture notes to `docs/architecture.md`.
- Move GPU runtime details to `docs/gpu-runtime.md`.
- Move troubleshooting to `docs/troubleshooting.md`.
- Move QA and historical evidence to `docs/qa/README.md` and `docs/history`.
- Delete the first-build plan link, retired QA evidence table, and unfinished Flathub brand note from README.

Verification.

- README stays under 120 lines unless a specific section earns the extra length.
- Link checker passes.
- Every documented command that is safe in a clean checkout runs.
- Rendered README shows install and first use before maintainer details.

## Recommended order

Run the work in this order.

1. 18.1, 18.9, and 18.10. These fix user trust and release trust.
2. 18.2, 18.3, and 18.4. These make the app boundary stable.
3. 18.5, 18.6, 18.7, and 18.8. These split the large modules.
4. 18.11 and 18.12. These clean history and releases once the active paths are stable.
5. 18.13. Rewrite docs after the code and release policy stop moving.

## What not to do

- Do not delete or rewrite published releases.
- Do not rewrite tags.
- Do not delete `.audit` until current CI no longer depends on it.
- Do not combine frontend, Tauri, installer, release, and docs cleanup in one PR.
- Do not accept pstack checker success as proof that the plan is safe. The checker verifies headings, lane counts, and wording. It does not verify dependencies, commands, external permissions, or whether a perf gate makes sense.
