# v0.8 adversarial review

## Intent

Echo v0.8 replaces the unexplained fixed 60-second recording watchdog with one configurable recording-limit policy shared by timed and toggle capture. The default and ceiling are ten minutes, while the capture conversion avoids the previous native clone and full mono intermediate buffer.

## Review panel

- `gpt-5.6-sol`: 6 findings
- `gpt-5.5`: no findings
- `gpt-5.4`: 2 findings
- `gpt-5.6-luna`: 7 findings

All four reviewers inspected `origin/main...HEAD` and the surrounding source with the same pstack rubric. The lead pass reproduced or traced each accepted execution path before changing code.

## Acted on

1. Shortcut verification could report success after cleanup failed, or leave a recording active on timeout or unmount. Cleanup now runs on every active test exit, success waits until status is no longer Recording, and failures leave the shortcut unverified. Tests cover rejection, timeout, and unmount.
2. Fixture capture bypassed both the snapped deadline and toggle cancellation. Fixture playback now truncates at the limit, watches the same token-scoped stop request as device capture, and returns only the played samples. Tests cover the limit and early toggle stop.
3. Stop files were unscoped and could cancel a replacement session. Each fully initialized lock now carries a unique token, stop requests carry the observed token, and watchers ignore stale tokens. Atomic hard-link publication removes the empty-lock window. A regression test recreates the replacement-session race.
4. A live legacy status without a limit snapshot was displayed as ten minutes. The desktop DTO preserves the unknown value and Home shows elapsed time without inventing a maximum. Rust and frontend tests cover the legacy path.
5. The memory and routing proof language was stronger than the tests. The test plan now distinguishes structural source evidence, exact conversion-equivalence tests, fixture limit tests, and calculation-only memory budgeting.

## Considered

- Make the stop-only command return true only during active capture. The command is a session-scoped cleanup request and the lock intentionally lives through transcription. UI correctness now depends on the observed Recording transition, not the boolean alone.
- Add a fake `AudioCapture` abstraction solely to assert the duration argument. Both modes resolve the limit before entering one shared capture call, so an interface layer would add code without creating stronger behavioral evidence. Fixture tests exercise both deadline and cancellation semantics.

## Dismissed

- An explicit 600-second file or environment value leaves the Settings select blank. The option builder already inserts the effective value for both sources; a focused frontend test now fixes that fact in place.
- The missing v0.8 version during review. Version and changelog finalization remain the last implementation commit before the PR and tag checks.

## Agreement map

Three reviewers independently identified shortcut cleanup risk. Two identified fixture bypass, two identified the token race, and two asked for narrower test claims. The legacy-status mismatch and explicit-600 Settings report were single-reviewer findings; direct tests accepted the former and disproved the latter.

## Live proof

- Default ten-minute Settings selection, active Home `10:00` snapshot, dark recording state, and light 1024 px Settings layout: `/Users/vriesd/.t3/userdata/browser-artifacts/browser-recording-mt5p8ck5.mp4`
- Focused implementation gate: `scripts/verify-recording-limit.sh`

## Decision-trail audit

`gpt-5.4` checked the TSV against the final diff and reran focused evidence. It found one low-severity wording mismatch: the video proves visible UI states but not console state. The TSV now limits that row to what the artifact visibly proves. No implementation or test claim was disputed.
