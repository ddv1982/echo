# Code review remediation

The immutable Flow plan is the source of truth. This checklist records phased
delivery progress in the repository.

## Phase 1: user-visible and security fixes

- [x] Move blocking desktop commands off Tauri's async runtime and cache the last-run projection.
- [x] Reject unsafe private-data and managed-runtime path fallbacks.
- [x] Make text injection single-attempt, option-safe, session-consistent, and clipboard-preserving.

## Phase 2: correctness and resilience

- [x] Make History and Dictionary updates cross-process safe and failure-atomic, and remove the ctime test race.
- [x] Correct WAV/sample handling, non-speech filtering, capture accounting, and recoverable audio locks.
- [x] Prevent stale frontend snapshots and post-unmount updates, and complete affected accessibility behavior.
- [x] Retry cold-start shortcut failures, support legacy recording stops, and recover safe desktop state after poisoned locks.
- [x] Bound installer reads and surfaced engine errors, refresh externally changed health state, and pin model sources.

## Phase 3: delivery and maintainability

- [x] Parallelize and harden CI, validate shipped desktop entries, remove dead build identity, and make packaging main/tag-only.
- [x] Add third-party attribution and managed-component SBOM coverage.
- [x] Remove confirmed dead or duplicated contracts and split only frontend files with clear existing feature boundaries.
- [x] Prepare version 0.14.13 and user-facing release notes.

## Publication

- [ ] Open a focused pull request and address actionable review comments.
- [ ] Merge only after required pull-request checks pass.
- [ ] Tag the green merged `main` commit with annotated `v0.14.13` and verify the release workflow and published assets.

## Non-goals

- Splitting Rust modules solely because they exceed a line threshold.
- Relocating the research scripts or historical evidence without a maintained ownership/archive contract.
- Replacing every desktop `Result<_, String>` in this release; UI-visible errors will instead be sanitized at affected boundaries.
- Adding speculative abstractions or compatibility paths without a persisted or external consumer.
