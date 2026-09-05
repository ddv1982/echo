# Complexity containment plan and new-chat handoff

Status: code changes and automated verification complete. Platform and compatibility paths are retained after review. Physical desktop and routine-use acceptance remain open. See the [implementation record](22-complexity-containment/implementation.md). The investigation below records the original baseline.

Baseline: `main` at `a00be5e6697d58ba701d53b965d4e2c3ee359b61`, released as `v0.14.18`. Investigation date: 2026-09-05. Recheck the baseline before implementing in a later chat.

## Objective

Reduce the amount of platform, engine, and compatibility knowledge a maintainer must carry across unrelated modules. Preserve current capabilities unless the user explicitly chooses a narrower support contract.

Necessary complexity cannot disappear while Echo supports different desktop protocols, speech engines, external CLI processes, and recovery paths. The useful reductions are fewer duplicate observations, fewer synchronized copies of state, and fewer modules independently interpreting the same facts. A smaller support scope could remove more code, but that is a product decision rather than an automatic refactor.

The recommended first implementation is small: remove the legacy `HEALTH` mirror while retaining `HealthCacheState` and its existing freshness rules.

## Completed work the next chat must preserve

[Plan 21](21-recording-coordination.md) has already been implemented and released. Read its [implementation record](21-recording-coordination/implementation.md), rather than restarting its original research phases.

The existing implementation already provides:

- `PublishedSession` around the existing dictation state machine and lease.
- `Injecting` publication before the insertion effect.
- Scoped active-status validation against the matching live owner record.
- Central explicit control/token/receipt policy and `recordingObservation` acceptance rules.
- Maintained actual-CLI recovery tests and a native Tauri/WebKit verification script.
- CI checks for native contracts, isolated X11 registration, and private-bus portal routing.

Do not recreate those abstractions, replace the recording transport by default, or remove their ordering guards. Physical-desktop and elapsed routine-use sign-off are still open. Green synthetic checks do not close those product-acceptance items.

## Findings from the current source

### 1. One health value is stored in two places

[`status.rs`](https://github.com/ddv1982/echo/blob/a00be5e6697d58ba701d53b965d4e2c3ee359b61/src-tauri/src/status.rs#L114) declares `HEALTH` alongside `HealthCacheState.cached`. Normal health reads use `HealthCacheState`. The mirror is read once to seed initialization, written after successful collection, and cleared during invalidation. GNOME test fixtures write directly to the mirror through a test-only import in `main.rs`.

This is a concrete deletion candidate, not a claim that `HEALTH` is unused. Remove the redundant storage after migrating those fixtures to the actual cache. Preserve generation invalidation and asynchronous refresh behavior.

The one-second source-check interval and ten-second health TTL are different policies. They must not be removed merely because the mirror is removed.

### 2. A settings response repeats speech/model observations

The request path runs through queued settings commands, readiness collection, `settings::snapshot`, `read_from_file`, and [`speech::snapshot`](https://github.com/ddv1982/echo/blob/a00be5e6697d58ba701d53b965d4e2c3ee359b61/src-tauri/src/speech.rs#L44).

`read_from_file` obtains a language catalog. Speech projection separately resolves the next run and builds model/engine availability views. [`language_catalog` and `resolve_next_run_for_process`](https://github.com/ddv1982/echo/blob/a00be5e6697d58ba701d53b965d4e2c3ee359b61/crates/echo/src/transcribe.rs#L467) each construct process/runtime observations. `ModelCache::inventory` reads the model directory. Setup readiness also needs managed and external component information.

Repeated collection within one response is a consolidation candidate. It is not evidence that every scan performs expensive hashing, that all inventories are interchangeable, or that a new global cache is needed. The existing collectors have different jobs. Measure and trace their shared inputs before changing their interfaces.

### 3. Platform observation and platform policy are different responsibilities

[`DesktopSession`](https://github.com/ddv1982/echo/blob/a00be5e6697d58ba701d53b965d4e2c3ee359b61/crates/echo/src/hotkey.rs#L7) already centralizes a narrow environment classification. Shortcut selection adds live portal/version observations and registration results. GNOME repair is a separate fallback policy.

The injector consumes the desktop session too, but its goal is focused text insertion rather than global shortcut registration. [`LinuxInjector::type_text` and `detection_summary`](https://github.com/ddv1982/echo/blob/a00be5e6697d58ba701d53b965d4e2c3ee359b61/crates/echo/src/inject.rs#L217) are useful places to review how execution choices and readiness descriptions stay aligned. Tool presence, focus availability, permission, and actual insertion success are different facts. No mismatch is claimed as a reproduced bug by this investigation.

Share genuinely identical observations where that removes duplication. Keep shortcut and insertion decisions separate. Do not introduce one universal backend registry to conceal their different failure modes.

### 4. Compatibility and engine recovery account for real branches

Current recording controls still recognize legacy lock payloads and matching flat stop/cancel intents. Portal Registry absence does not imply that GlobalShortcuts is absent. An X11 installation and an older GNOME setup can still need different paths.

Whisper GPU execution also has deliberate complexity: discovery, CPU fallback availability, qualified plan/receipt validation, device choice, leases, and quarantine/recovery. Parakeet has different model and language semantics. These distinctions are not duplicated implementations of one interchangeable operation.

Deleting these branches requires either proving they are redundant under the retained contract or explicitly changing that contract. Tests can prove mixed-version behavior; they cannot prove that no deployed user has an older binary. Do not require new remote telemetry just to justify a support decision.

## Starter support ledger

Keep this ledger descriptive. It is not a proposed runtime registry or a complete support certification.

| Retained variation | Why it exists | Source to inspect | Gate for changing/removing it |
| --- | --- | --- | --- |
| Desktop Home and standalone CLI owners | Recording can be controlled across processes | `crates/echo/src/rec.rs`, `src-tauri/src/cli.rs` | Preserve mutual exclusion, session attribution, stale controls, and CLI behavior |
| X11 and portal shortcuts | Registration protocols and permissions differ | `crates/echo/src/hotkey.rs`, `src-tauri/src/shortcuts.rs` | Verify the affected backend; dropping one requires a user-approved support change |
| Older GNOME/portal fallback | Registry and GlobalShortcuts availability are distinct | `src-tauri/src/shortcuts/gnome.rs` | State the minimum supported portal contract and migration behavior before removal |
| Text insertion fallbacks | Focus, clipboard, tool availability, and permissions differ | `crates/echo/src/inject.rs` | Preserve focus safety, fallback order, clipboard handling, and no duplicate insertion |
| Legacy recording files and PID-only payloads | Older installed CLI/shortcut processes can coexist | `crates/echo/src/rec.rs`, `crates/echo/src/process_identity.rs` | Explicit compatibility cutoff plus mixed-version and upgrade tests |
| Whisper CPU/GPU recovery | Accelerator failure must retain a valid recovery path | `crates/echo/src/stt/whisper_plan.rs`, `whisper_gpu.rs`, `whisper_recovery.rs` | Preserve plan identity, receipts, leases, bounded fallback, and failure reporting |
| Parakeet versus Whisper | Model formats and language selection differ | `crates/echo/src/stt/parakeet.rs`, `crates/echo/src/transcribe.rs` | Keep distinct behavior while both engines remain supported |

## Phased implementation

### Phase 1: remove redundant health state

This is the first PR. No new runtime abstraction is needed.

1. Trace all `HEALTH` reads/writes again at the implementation head.
2. Move the GNOME test setup and ignored latency-probe fixture onto a narrow test-only seed operation for `HealthCacheState`, or an equivalent existing test seam.
3. Make fixture seeding work whether the `OnceLock` has already initialized or not. Keep tests isolated from other pending refreshes.
4. Delete `HEALTH`, the one-time mirror seed, mirror publication/clear operations, and the test-only re-export through `main.rs`.
5. Remove the mirror-specific publication callback if it no longer serves a real caller. Preserve the stale-generation checks it currently surrounds.

Primary files are `src-tauri/src/status.rs`, `src-tauri/src/main.rs`, and `src-tauri/src/shortcuts/gnome/tests.rs`.

Exit gate:

- One authoritative cached `Health` value remains.
- Existing cache reuse, background refresh, source-fingerprint, invalidation, and stale-generation tests pass.
- Test seeding does not depend on process initialization order.
- Settings changes, setup completion, and stale-install removal still invalidate health.
- Native status remains responsive. No new queue, clock, production fixture switch, or cache is introduced.

Expected size is one focused PR. Budget roughly one engineer-day initially; revise if the fixture dependencies differ from this baseline.

### Phase 2: reuse facts within a settings response

First capture the current call path and a native settings-read baseline. Count collection calls, not just method names. Separate model-directory enumeration, runtime discovery, provenance validation, and UI projection.

Then pass one request-scoped observation through the projections that actually consume the same facts. Prefer extending the current collector/projection boundaries to adding a new public service. Keep requested preferences, executable next-run resolution, and installation readiness distinct.

Primary files are `src-tauri/src/settings.rs`, `src-tauri/src/speech.rs`, `src-tauri/src/setup.rs`, `crates/echo/src/transcribe.rs`, and `crates/echo/src/stt/runtime.rs`.

Exit gate:

- A settings response no longer independently recollects the same facts without a documented reason.
- No lasting cache or new invalidation owner is added for this optimization.
- Environment overrides, missing/configured models, English-only Whisper, multilingual Whisper, Parakeet, and unavailable-runtime views retain their existing semantics.
- Setup changes and externally added/removed models appear in subsequent observations.
- Execution still validates its inputs when it starts. A presentation snapshot is not a permanent authorization to execute a stale runtime.
- Native settings/configuration-ordering checks pass, and the before/after record states exactly which repeated collections were removed.

Do not merge `get_readiness` into `get_settings` or remove snapshot revisions as a shortcut. Their callers and lifetimes differ. Request-scoped reuse is not a filesystem transaction and must not be described as an atomic view of concurrent external installation changes.

Budget one or two focused PRs, roughly two to four engineer-days after the trace confirms the seam.

### Phase 3: contain platform and engine variation

Use the support ledger to trace one scenario at a time. Identify the observations, the subsystem that interprets them, and the error/result exposed to callers.

Start with insertion readiness versus actual insertion policy, or a single speech-projection path. Extract a shared fact or pure decision only when it removes two genuinely equivalent implementations. Keep command attempts fallible and preserve their recovery behavior.

Do not turn shortcut registration, target focus, insertion, and GPU qualification into interchangeable implementations of a generic "backend" interface. A useful boundary should let its caller ask a smaller question without learning the implementation details it hides.

Exit gate for each unit:

- The PR names the duplicated decision it removes and its remaining owner.
- Backend-specific branches stay inside the responsible subsystem.
- Existing success and failure behavior is tested through the affected adapter, not only through a new helper.
- Adding a descriptor or interface also removes repeated policy; a table that merely lists existing branches does not count as a simplification.
- No capability, fallback, or safety check is silently removed.

This phase is conditional on a verified duplication. Stop with a documented "keep" decision where different behavior is justified. Estimate the selected unit after tracing it, rather than reserving an open-ended rewrite.

### Phase 4: make compatibility retirement explicit

For each legacy path, record its protocol shape, known consumer, existing test, support promise, and proposed migration/removal condition. Do not invent a minimum supported GNOME, portal, or Echo version from comments or file names.

The default result can be retention with a clear boundary. If removal would drop a capability or exclude older installations, obtain the user's product decision before implementing it. Then update support documentation and mixed-version tests in the same change.

Exit gate:

- Every proposed deletion has an explicit supported-version/protocol rule.
- Older shortcuts and upgrade handoff have a documented migration or failure behavior.
- Tests cover both retained and rejected formats at the boundary.
- A compatibility bridge has an owner and an actual end condition; it does not become another permanent authority.

Fixture success is correctness evidence, not deployed-version usage evidence. No broad deletion of legacy files, timestamp validation, X11, portal fallback, CPU recovery, or an engine is authorized by this plan alone.

### Phase 5: consolidate verification and prove the result

Keep a small contract suite organized by public behavior: recording controls/recovery, settings snapshots, shortcut registration, insertion, engine qualification, and upgrade boundaries. Reuse existing fixtures only when they serve the same contract. Avoid a single test framework with a switch for every subsystem.

For each completed phase, record:

- Production state stores, policy implementations, and collection sites removed.
- New state, lifetimes, invalidation rules, and dependencies introduced.
- The affected public-path tests and their results.
- Native observations measured under the same configuration before and after.
- Manual platform coverage that was performed, and coverage still missing.

Use line counts only to explain the diff. Do not target a percentage reduction, maximum file length, or a new architecture score. The acceptance question is whether fewer independently maintained decisions produce the same supported behavior.

Complete the real-desktop and routine-use work still open from Plan 21. Isolated X11 grabs, a mock portal, synthetic audio, and a native WebView do not prove physical shortcut delivery, microphone behavior, or insertion into actual applications.

## Verification starting points

Use Rust 1.89 and the repository's pinned frontend tooling. Recheck package/workflow versions at the implementation head.

For the first health-cache unit:

```sh
rg -n '\bHEALTH\b|health_cache_state|health_invalidate' src-tauri/src
xvfb-run -a cargo test -p echo-desktop --bin echo-desktop status::tests::
xvfb-run -a cargo test -p echo-desktop --bin echo-desktop shortcuts::gnome::tests::
python3 scripts/verify-recording-native.py --output /tmp/echo-containment-native.json
```

The default test commands leave environment-dependent ignored cases ignored. Do not run all ignored native tests against a user's active desktop indiscriminately. The repository already has isolated X11/portal commands in `.github/workflows/check.yml`.

Before review, run the checks appropriate to the changed area, then the repository's required checks. Preserve generated IPC verification, frontend typecheck/lint/tests, and native contract validation. A global refactor is not needed to run them.

The native probe uses `target/recording-native-probe`. Coordinate build/run access where necessary and never reuse a build target across deliberately mutated source variants. Record the tested source and binary identity. Do not create sibling Echo worktrees for this handoff by default; the previous run's extra worktrees were cleaned up at the user's request.

## Delivery and scope

This plan is source-backed investigation, not a benchmark result or a new bug report. No new performance claim or physical-desktop validation was produced in this turn. The release checks for `v0.14.18` are historical evidence for that exact release, not proof of future edits.

Start with Phase 1 and reassess after its deletion is verified. Deliver subsequent units as focused, reviewable PRs. Do not merge unrelated cleanup into them. Preserve the existing file lease, session identity, receipt/completion distinction, owner revisions, command epoch, FIFO configuration owner, private storage rules, runtime provenance, and single-attempt insertion protections.

The overall amount of removable complexity is not yet known. A valid outcome is to remove the health mirror and repeated observations while deliberately retaining the platform and compatibility branches. That would reduce maintenance burden without changing Echo's product promise.

## Copy into a new chat

> Work in `/home/vriesd/projects/echo`. Read `docs/plans/22-complexity-containment.md` and `docs/plans/21-recording-coordination/implementation.md` first. The researched baseline is `a00be5e`, released as `v0.14.18`; check current Git state and changes since that baseline before editing. Plan 21 is already implemented. Start Plan 22 with the focused `HEALTH` mirror removal, migrating its test fixtures onto the real `HealthCacheState` and preserving refresh/invalidation/generation behavior. Verify that unit before continuing to request-scoped speech observations and any justified platform-policy consolidation. Preserve existing capabilities, recording safety, and compatibility by default. Do not add a daemon, universal backend framework, global cache, or extra worktrees without a demonstrated need. Compatibility retirement requires an explicit support decision. Keep a short evidence/deletion record and report manual acceptance gaps honestly. Prepare reviewable changes; do not infer merge or release authorization from the old chat's completed releases.
