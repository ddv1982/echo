# Phase 3: non-speech residue filter

Back to [overview](overview.md).

## Goal

Stop `[BLANK_AUDIO]` and its siblings from reaching the user's cursor. This ships before VAD because it needs no new model download, so it fixes the reported symptom on the user's existing setup.

Be honest about what this phase is. It is a backstop, not the fix. A 27-language sweep across four non-speech fixtures found that roughly half the failures are unbracketed prose the model memorised from subtitles: `TV GELDERLAND 2020.` in Dutch, a fabricated Russian subtitle-editor credit, and a bare `you` in English on digital silence. No filter catches those. Phase 4 is the actual fix.

## Changes

**`crates/echo-core/src/engine.rs` or a new `crates/echo-core/src/nonspeech.rs`.** One pure function taking the raw transcript and returning it with whole-segment non-speech markers removed. Pure, no I/O, unit-testable, and it belongs in `echo-core` beside the `Transcript` type it operates on.

**`crates/echo/src/stt/whisper.rs` and `crates/echo/src/stt/parakeet.rs`.** Apply it where each engine builds its `Transcript`, alongside the existing `.trim()`.

The placement matters and it is the one design decision in this phase. This is an engine artifact, not a cleanup preference, so it does not go in `RulesCleanup`. `ECHO_CLEANUP=off` must not reintroduce `[BLANK_AUDIO]` (**principle-boundary-discipline**: guard at the boundary where the untrusted data enters).

**Match rules, derived from observed output rather than from issue threads.** Case-insensitive, whole-segment only:

- Leading `[` or `(`.
- Leading and trailing `*`, which catches `* Musik *` and `* Spannungsvolle Musik *`. A `\[.*\]` filter misses these entirely, which is why the rule list is not just brackets.
- Strings composed only of the musical glyphs `♪♫♬♩♭♮♯` and whitespace.
- Strings composed only of `.`, `…`, and whitespace.

**Whole-segment only, deliberately.** Do not strip brackets mid-segment. Real dictation contains parentheses, and a user who says "open paren note to self close paren" should get them.

No `regex` crate. There is none in the tree today and these rules are four `starts_with`/`chars().all()` checks. Adding a dependency for that would not earn its place (**principle-laziness-protocol**).

## Data structures

`fn strip_nonspeech(raw: &str) -> &str`. Borrowed in, borrowed out, since every rule either returns the input unchanged or returns empty.

## Verification

**Static.** `cargo test --workspace` with a table-driven unit test over the observed marker inventory. The full list is in [testing.md](testing.md); it includes `[BLANK_AUDIO]`, `[MÚSICA]`, `(music)`, `* Musik *`, `♪♪`, and `...`, plus negative cases proving `Open (paren) here` and `Rate it 5 stars *` survive untouched.

**Runtime.** Via **control-cli**. Record two seconds of silence with a real microphone, or point `ECHO_AUDIO_FIXTURE` at a silent WAV, and run `echo-desktop rec --once` with `ECHO_SKIP_INJECT=1`. The status file's `last=` must be empty rather than carrying a marker. Then run the same thing with `ECHO_CLEANUP=off` and confirm it is still empty, which is the assertion that proves the filter is at the engine boundary and not in the cleanup pass.

## Note

Add a silent WAV fixture next to `crates/echo/tests/fixtures/claude_code.wav` in this phase. Phase 4 needs it, and today a blank-audio regression has no test that could catch it, because both engine integration tests are `#[ignore]`d and neither asserts anything about unwanted tokens.
