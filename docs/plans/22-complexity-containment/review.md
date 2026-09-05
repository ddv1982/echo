# Implementation review record

## Health review

An independent read-only agent reviewed the health-only diff at `ba0edc9`. It found no blocking issue. Seeding advances the generation under the cache mutex, clears the pending probe, and publishes the fixture. Old completion and failure paths cannot overwrite that fixture or clear newer pending work. The `OnceLock` initializes before the seed operation, so cold and warm calls use the same path.

The non-ignored suite has one global cache fixture test. The other fixture users are explicitly ignored desktop probes. The reviewer found no new comments, suppressions, or unnecessary wrappers. It inspected source and did not rerun tests.

## Evidence audit

GPT-5.6-sol independently audited the initial decision trail while settings implementation was still underway. The available review models were all GPT models; this used a separate model, not a different model family.

The audit identified temporary evidence paths, an unlinked source-review result, missing final verification, and the native baseline's dirty-tree marker. The native baseline ran before application edits. The only untracked file at that point was the original Plan 22 document. Its fingerprint therefore represents that working tree, not a clean checkout. The preserved native artifact states this distinction.

The compatibility ledger lists existing tests separately from test execution. In particular, the desktop-mutating GNOME repair test remains ignored. The absent-Registry portal branch also lacks end-to-end coverage in the current private-bus fixture. These are retained coverage gaps.

The audit suggested specifying a telemetry threshold or expiry date for retained compatibility. That suggestion is rejected. This change introduces no compatibility bridge and retires no format. Plan 22 requires an explicit support decision before retirement and prohibits inventing a deployed-version cutoff. The ledger records the decision and migration work that a future retirement would require.

Final verification artifacts supersede the initial temporary log pointers through new decision-trail entries. Physical desktop and elapsed routine-use acceptance remain open.

## Settings review

An independent read-only agent reviewed the settings diff against `ba0edc9` after implementation. It found no blocking issue. External and managed facts stay separate, custom model lookup retains per-path checks, and execution still collects an inventory and leases its selected inputs. The reviewer checked the non-UTF-8 path conversion, unavailable-engine language projections, and the ten-case verifier's source-stability checks. It found no actionable new comments or suppressions. This review did not rerun tests.
