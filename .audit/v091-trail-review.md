# v0.9.1 decision-trail review

The trail review was dispatched with `gpt-5.4`; the reviewer identified itself generically as `gpt-5`.

## Flags and resolutions

- Two verification rows used prose bundles instead of durable evidence. `.audit/v091-local-verification.md` now records the commands, counts, red result, and remaining Linux CI boundary.
- The final verification row was still in the working tree. The trail and this review ship in the next audit commit.
- The TDD row did not point at durable red evidence. Commit `17ebe85` now serves as the before-fix artifact, with `c9decf0` as the green fix.
- The frame and G55 dropout rows used session labels. `grounding.md` and `architecture.md` now serve as the durable framing and synthesis records.

## Attention

Local macOS cannot execute the Linux Whisper runtime. Exact-head Ubuntu CI must run `CommandRuntimeProbe` before merge. The PR, main, tag, and release checks remain authoritative for that claim.
