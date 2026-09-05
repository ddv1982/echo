# Complexity containment implementation

This implements [Plan 22](../22-complexity-containment.md) from `a00be5e6697d58ba701d53b965d4e2c3ee359b61`. [Plan 21's recording coordination](../21-recording-coordination/implementation.md) remains in place. Physical desktop and elapsed routine-use acceptance remain open.

## One health cache

`HealthCacheState.cached` is the sole authoritative cached `Health` value. The change removes `HEALTH`, its initialization copy, its publication and invalidation writes, the mirror publication callback, and the test re-export from `main.rs`.

The test-only seed operation initializes the actual cache, advances its generation, clears pending work, and publishes the fixture. An older probe or refresh cannot overwrite the fixture. Production source freshness remains one second and the full health TTL remains ten seconds. Invalidation callers in settings, setup completion, and stale-install removal are retained.

The existing 19 status tests passed before the edit. All 20 status tests and seven non-ignored GNOME tests passed afterward. The added test reads the real cache, replaces an existing fixture, and delivers stale probe and refresh completions. The ignored desktop-mutating GNOME fixture remains ignored by default.

## Settings observations

The [baseline trace summary](settings-before.json) follows the complete settings response, including readiness. In an isolated process with no installed engine, that response called `SpeechRuntimeInventory::from_cache` six times, enumerated the model directory seven times, and called managed component status 56 times. These are observed calls, not counts inferred from method names.

Readiness independently enumerated external models and prepared an engine. Preferences collected a language catalog. Speech projection then resolved the next run, collected model inventory and engine availability, and collected another catalog for the unavailable-engine fallback. Preparation and runtime discovery account for work beyond the four direct speech projection paths.

Managed status calls do not imply full payload hashing on every call. Runtime launch identity collection does hash a discovered Whisper executable and its adjacent libraries. The empty baseline has no Whisper candidate and therefore does not measure that hashing cost.

One settings request now owns one `SpeechRuntimeInventory`. It retains external model and runtime observations alongside managed component states and the preferred execution assets. The preferences catalog, next-run resolution, engine availability, model projection, and readiness projection borrow these facts. Standalone readiness requests collect their own inventory. `get_readiness`, the configuration queue, and response revisions remain separate.

External and managed installations cannot be collapsed into one list. A managed model can replace an external model with the same name for execution while setup must still report both. The same distinction applies to managed and external Sherpa binaries. The inventory retains those source facts for the different projections.

The internal managed root is now a `PathBuf`. IPC and JSON display projections retain their string representation. This lets runtime discovery reuse the validated root without losing non-UTF-8 path bytes or reading the activation record again. A test covers the typed root, its display serialization, and the managed lease.

The [collection probe](../../../scripts/verify-settings-collections.py) traces filesystem opens from the actual settings response in ten isolated processes. Every case performs one model-directory enumeration and eight managed-status marker checks. It covers unavailable engines, English-only and multilingual Whisper, Parakeet, an English-only model rejecting German, a missing configured model, a custom model override, language and engine overrides, and the fake engine. CI installs `strace`, runs this probe, and preserves its JSON report.

The baseline unavailable response therefore drops from six runtime inventories to one, seven model scans to one, and 56 managed status checks to eight. For a ready Whisper response, the probe still observes two executable opens for identity validation. Initial runtime discovery establishes the candidate identity, and preparation checks the leased selection again. That second validation is intentional. Execution through `prepare_with_config` still collects a fresh inventory and acquires its own leases.

The new state is request-owned data only. No persistent cache, invalidation owner, runtime service, production fixture switch, or production dependency is added. Separate preference/source projection still reads process environment values, and readiness still observes audio, disk, active setup, and History state. Those observations serve distinct fields. A request snapshot is not a filesystem transaction over concurrent external changes.

## Retained variation

Phase 3 ends with a keep decision for insertion. Tool presence, captured target safety, fallible typing, and confirmed paste have different eligibility and success rules. No equivalent complete policy was found for extraction.

Phase 4 retains the supported compatibility paths. The [compatibility record](compatibility.md) names their formats, owners, consumers, tests, and conditions required for future retirement. No minimum Echo, GNOME, or portal release is invented. No capability is removed.

## Contract suite

Existing suites remain organized around public behavior. No new test framework or subsystem selector is introduced.

| Contract | Reproduction command |
| --- | --- |
| Cached status and invalidation | `xvfb-run -a cargo test -p echo-desktop --bin echo-desktop status::tests::` |
| Settings queue and projection | `cargo test -p echo-desktop --bin echo-desktop settings::tests::` and `cargo test -p echo-desktop --bin echo-desktop speech::tests::` |
| Settings collection counts and public projections | `python3 scripts/verify-settings-collections.py --output /tmp/echo-settings-collections.json` |
| Speech resolution and preparation | `cargo test -p echo --lib transcribe::tests::` |
| Recording controls and CLI recovery | `cargo test -p echo --test recording_commands` and `cargo test -p echo-desktop --test cli_rec` |
| Insertion attempts and clipboard safety | `cargo test -p echo --lib inject::tests::` |
| Engine qualification and recovery | `cargo test -p echo --lib stt::whisper_plan::tests::` and `cargo test -p echo --lib stt::whisper_recovery::tests::` |
| X11 registration | `xvfb-run -a cargo test -p echo-desktop --bin echo-desktop x11_runtime_registers_and_releases_the_fixed_grab -- --ignored` |
| Portal registration and routing | `cargo test -p echo-desktop --bin echo-desktop portal_runtime_registers_binds_routes_and_closes -- --ignored` |
| Native settings ordering and recording | `python3 scripts/verify-recording-native.py --output /tmp/echo-containment-native.json` |

Use Rust 1.89 and the pinned frontend dependencies. The native probe records source fingerprints, binary hashes, build profile, and isolated environment. The [native baseline](native-before.json) exercised ten contracts through a real Tauri/WebKit process and left the source unchanged during the run. Application code matched `a00be5e`; the original untracked Plan 22 document accounts for the dirty-tree marker. The baseline current-status lane had 40 samples with p50 and p95 of 1 ms. This debug, fake-engine sample is not a production latency claim or a settings-read speedup measurement.

The [verification record](verification.json) covers application commit `ffb4f176b3d1121243032c9b8c6769a6e600d290`. The workspace passed 530 Rust tests with 17 environment-dependent or helper cases ignored. Frontend build, typecheck, lint, all 275 unit tests, and six browser tests passed. Rust formatting, Clippy, generated IPC, isolated X11 and portal registration, speech benchmark/corpus/archive checks, and the release build also passed.

The [settings collection report](settings-after.json) passed all ten cases. Its source hash matches the committed application source. The [final native probe](native-after.json) passed all ten recording/configuration contracts with no source change during the run. The current-status lane retained a 1 ms p95 under the same debug, fake-engine configuration. Per-lane IPC latency samples are omitted from the committed native reports. Native stage measurements, summaries, and source/binary identities are retained.

The first concurrent workspace run failed the existing GPU fallback test's `elapsed < 3s` assertion. The unchanged test passed in isolation. The complete workspace then passed with two test threads after the other builds finished. The record includes both results; no deadline assertion was weakened.

The [decision trail](decisions.tsv) links decisions to the durable evidence. The [review record](review.md) preserves independent source reviews and the evidence audit. Original temporary count-trace logs were unavailable after the session retry; their recorded aggregate is preserved separately from the final rerunnable collection report. Local full logs remain below `target/containment-checks`, and `verification.json` records their hashes. Only documentation and evidence were added after the tested application commit.

## Remaining acceptance

Synthetic audio, Xvfb grabs, a private-bus portal, and a native WebView do not prove physical microphone behavior, compositor permission handling, or insertion into real applications. This session does not provide one to two weeks of ordinary use. The acceptance procedure and failure gates remain in [Plan 21's implementation record](../21-recording-coordination/implementation.md#remaining-product-acceptance).
