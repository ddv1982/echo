# v0.7 adversarial review

## Intent

Echo v0.7 must expose stable, recognizable microphone devices and let users switch and test them. A fresh Linux x86_64 user must also be able to install a pinned local Whisper runtime or one-click Parakeet, choose a hardware-aware recommended model and VAD, see truthful readiness, and safely download, cancel, resume, repair, verify, and remove managed files without changing system runtimes or manual models.

## Review panel

- `gpt-5.6-sol`: 10 findings
- `gpt-5.5`: 6 findings
- `gpt-5.4`: 4 findings
- `gpt-5.6-luna`: 6 findings

All four reviewers inspected `origin/main...HEAD` and surrounding source with the same pstack rubric. The lead pass reproduced each accepted execution path before changing code.

## Acted on

1. Cancellation could still activate after download. Three reviewers found missing checks during hashing, direct copies, archive members, runtime probing, and activation. The installer now checks each boundary, reads response bodies on a cancellable channel, bounds connection and header waits, and has tests for a stalled body, copy cancellation, and pre-activation cancellation.
2. Same-size corruption could remain ready. All four reviewers found the quick metadata path. Installation and explicit Verify write a durable fingerprint stamp after full SHA-256 verification. Fresh shortcut processes compare the small stamp and file metadata instead of rehashing every model. Metadata changes force a new hash, failures persist as repair markers, and corrupt managed models fall back to manual models.
3. Recovery and config writes could race. Two reviewers found that a second desktop process could remove live staging. Three found unsynchronized whole-file config updates. Managed operations now hold a cross-process shared lease while recovery requires an exclusive lease. Microphone, Settings, and setup config writes share one transaction lock, and terminal setup refreshes Settings state.
4. Readiness could disagree with actual selection and capacity. Reviewers found duplicate manual and managed model names, cumulative disk undercounting, an enabled low-space Parakeet button, and progress-triggered readiness storms. Managed models now replace same-name manual entries, plan space includes earlier retained payloads, each plan controls its own button and reason, backend progress is throttled, and the frontend applies progress locally.
5. The installer module crossed 1,000 lines. The orchestration and tests moved into `installer.rs` and `tests.rs`; `mod.rs` is no longer over 1,000 lines.
6. Atomic writes did not sync before rename. The shared atomic writer now syncs the temporary file and parent directory, so the activation pointer and receipt are durable before success is reported.

## Considered

- Validate manually imported model contents before calling them ready. Runtime executability is now checked on Unix. Manual model validation remains an advanced-user boundary because the current cache and fake-runtime tests intentionally accept arbitrary model fixtures.
- Persist cancelled and failed UI outcomes across process restarts. Terminal errors remain visible in the current session, while resumable bytes and repair markers provide the durable state that changes the next action.
- Extract the new React sections from the existing `App.tsx` monolith. The file was already over 1,000 lines. This release keeps the feature local instead of adding a broad frontend refactor after behavior is proven.

## Dismissed

- Origin-qualified model IDs. A managed-first deduplication rule fixes the reachable bug without migrating config and every CLI caller.
- A local same-user symlink attacker inside Echo's managed cache. This does not cross a privilege boundary. The closed archive extractor still rejects traversal, special members, changed symlinks, unknown payloads, and unpinned files.
- A new network-heavy end-to-end release harness. CI already runs the complete Rust workspace, all frontend tests, lint, release build, and package workflows. The focused first-run script uses deterministic in-memory boundary adapters instead of downloading 850 MB of production artifacts in CI.
- The missing v0.7 version during review. Version and changelog finalization are intentionally the last commit before the PR and tag policy check.

## Agreement map

Cancellation and corruption had the strongest agreement and blocked release. Recovery/config concurrency and duplicate model precedence had multi-reviewer support and concrete traces. Capacity, Parakeet admission, and progress rescans were single-reviewer findings confirmed by direct calculation and live preview. Suggestions for a config-wide model identity migration and a production-archive CI download were larger than the bugs they addressed.

## Live proof

- Final light, dark, narrow, earbud-selection, microphone-test, and guided-setup recording: `/Users/vriesd/.t3/userdata/browser-artifacts/browser-recording-mt5kbmzt.mp4`
- Focused hardening gate: `scripts/verify-first-run-readiness.sh`

## PR review follow-up

The automated review on PR 48 found two P2 issues at exact head `1658c56`. Home refreshed the microphone snapshot but not readiness after a device change, and process-local fingerprint caching still hashed every managed model in each shortcut process. The `fix: address v0.7 PR review` commit refreshes both snapshots with a regression test and persists verified fingerprints inside each immutable release. Review comments: `discussion_r3838136035` and `discussion_r3838136038`.
