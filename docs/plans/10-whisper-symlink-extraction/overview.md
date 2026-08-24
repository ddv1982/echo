# Whisper runtime symlink extraction hotfix

**Status: implemented and locally verified for v0.9.1; PR pending.**

Echo 0.9.0 cannot install the pinned Whisper 1.9.2 runtime because the archive stores `libwhisper.so` before its immediate symlink target. Runtime extraction incorrectly treats archive order as dependency order. The archive digest, inventory, and immediate link targets are correct.

## Phase 1. Reproduce and ground

- Download the exact pinned artifact and verify its outer SHA-256.
- Trace download, extraction, payload verification, runtime probing, activation, recovery, repair, and removal.
- Compare runtime extraction with the inventory generator's order-independent chain resolution.

Result: complete. The failure is isolated to `extract_tar()` after regular-file extraction and before activation.

## Phase 2. Build the regression harness

- Add a tiny tar with an outer link, an inner link, and a terminal file stored in reverse dependency order.
- Confirm the current extractor fails with `symlink target was not extracted`.
- Add missing, cyclic, escaping, unselected, and flattened-destination mismatch cases.
- Add a rerunnable verifier that downloads the 9.5 MiB pinned artifact and drives the real installer path through activation.

## Phase 3. Validate and materialize the closed link graph

- Keep the streaming archive scan, entry and expanded-byte limits, file writes, modes, and content hashes unchanged.
- Collect selected symlinks in a map keyed by normalized archive source path.
- After the scan, require every selected member to be present.
- Validate each graph edge against both archive-relative source paths and flattened payload destinations.
- Reject missing targets, cycles, unselected members, non-file terminals, and destination mismatches.
- Create every exact immediate link only after the graph is valid, then hash every link after the complete graph exists.
- Check cancellation during graph validation, link creation, and link hashing.

## Phase 4. Recheck lifecycle safety

- Prove the real pinned archive activates and later verifies.
- Re-run cancellation, interruption, checksum failure, failed probe, repair, recovery, and managed-only removal tests.
- Confirm extraction failure never changes an active generation and never removes external files.

## Phase 5. Ship the hotfix

- Prepare v0.9.1 release notes and version metadata.
- Run strict lint, workspace tests, the pinned-artifact verifier, release build, and package workflows.
- Open and babysit the PR until review and CI are clean.
- Merge only after success, verify the exact main commit, create the annotated v0.9.1 tag, and verify release assets and checksums.

## Completion predicate

The synthetic reverse-order chain and real Whisper 1.9.2 installer path pass. Every hostile graph case fails inside staging. Existing atomic activation, repair, cancellation, checksum, recovery, and removal guarantees remain green. The PR, main commit, tag, and release workflows pass on their exact commits.
