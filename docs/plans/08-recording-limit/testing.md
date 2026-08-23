# Recording-limit verification

## Predicate

The feature passes when default, config, environment, timed capture, toggle capture, status, Settings, Home, preview, and documentation all agree on one effective 1..600-second limit whose default is 600.

## Focused checks

- Core policy table covers invalid environment text, precedence, 0, 1, 60, 61, 90, 599, 600, 601, `u32::MAX`, and `u64::MAX`.
- Recorder structure resolves the limit once before timed and toggle capture share the same call. Tests cover stop-only behavior, token-scoped cancellation, fixture cancellation, and fixture truncation at the snapped limit.
- Status tests cover a live 600-second snapshot, old files without the field, malformed values, and dead writers.
- Audio tests compare direct conversion with the previous two-stage algorithm on short mono, stereo, and multichannel inputs. A calculation-only test records the ten-minute 48 kHz stereo budget as 230.4 MB native plus 19.2 MB output without allocating them.
- Tauri tests cover default 600, values above the old cap, environment lock, maximum clamping, and active status precedence.
- Frontend tests cover the visible General control, required labels, default reset, custom old values, environment lock, Home `10:00`, preview snapshot, and shortcut-test cleanup.

## Rerunnable proof

`scripts/verify-recording-limit.sh` runs the focused Rust and Node 22 tests and rejects stale production references to the old fixed toggle limit or three-second default. It uses fixtures and pure deadline assertions. No test waits ten minutes or opens host audio.

The full gate adds frontend build, lint, all tests, workspace clippy, workspace tests, fixed-toggle, first-run and transcription verifiers, release build, icon drift, package assembly, exact-head PR/main checks, annotated tag validation, and published release asset inspection.

## Claim boundary

Automated tests prove policy, fixture limit and cancellation behavior, session-scoped stop requests, status projection, and capture conversion equivalence. Source inspection confirms that the live capture path takes ownership of native samples and converts directly into final PCM; the arithmetic test records the expected buffer budget but does not instrument allocations. The suite does not prove model quality or inference speed on a real ten-minute recording. The release notes must not claim live transcription performance that was not measured.
