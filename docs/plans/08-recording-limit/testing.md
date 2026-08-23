# Recording-limit verification

## Predicate

The feature passes when default, config, environment, timed capture, toggle capture, status, Settings, Home, preview, and documentation all agree on one effective 1..600-second limit whose default is 600.

## Focused checks

- Core policy table covers invalid environment text, precedence, 0, 1, 60, 61, 90, 599, 600, 601, `u32::MAX`, and `u64::MAX`.
- Recorder tests prove timed and toggle modes receive the same snapped limit and stop-only never starts a session.
- Status tests cover a live 600-second snapshot, old files without the field, malformed values, and dead writers.
- Audio tests compare direct conversion with the previous two-stage algorithm on short mono, stereo, and multichannel inputs. A calculation-only test proves ten-minute 48 kHz stereo logical buffers are 230.4 MB native plus 19.2 MB output without allocating them.
- Tauri tests cover default 600, values above the old cap, environment lock, maximum clamping, and active status precedence.
- Frontend tests cover the visible General control, required labels, default reset, custom old values, environment lock, Home `10:00`, preview snapshot, and shortcut-test cleanup.

## Rerunnable proof

`scripts/verify-recording-limit.sh` runs the focused Rust and Node 22 tests and rejects stale production references to the old fixed toggle limit or three-second default. It uses fixtures and pure deadline assertions. No test waits ten minutes or opens host audio.

The full gate adds frontend build, lint, all tests, workspace clippy, workspace tests, fixed-toggle, first-run and transcription verifiers, release build, icon drift, package assembly, exact-head PR/main checks, annotated tag validation, and published release asset inspection.

## Claim boundary

Automated tests prove policy, routing, capture conversion equivalence, and memory shape. They do not prove model quality or inference speed on a real ten-minute recording. The release notes must not claim live transcription performance that was not measured.
