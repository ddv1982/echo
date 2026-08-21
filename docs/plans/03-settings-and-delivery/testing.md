[Back to overview](overview.md)

# Testing

## Static, every phase

Order matters. `tauri-build` resolves `frontendDist` at compile time, so a missing `frontend/dist` panics `cargo check`, `clippy`, `test`, and `build` alike. The npm build is not optional even for a Rust-only phase.

```sh
npm ci --prefix frontend
npm run build --prefix frontend            # tsc --noEmit, then vite build
npm run test --prefix frontend             # vitest + jsdom
npm run lint --prefix frontend
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`--all-targets` matters. `src-tauri/Cargo.toml` is the only workspace member missing `[lints] workspace = true`, so the workspace's `clippy::all = deny` and `unsafe_code = forbid` do not reach the crate holding the desktop shell. Phase 1 adds the missing lints stanza.

From phase 1 onward CI runs exactly this list, so a green PR is the static check.

## Environment prerequisites

The README's `apt install` line is incomplete and one gap is a hard build failure. Verified by reproducing the panic with pkg-config blinded:

```sh
sudo apt install build-essential pkg-config libasound2-dev \
  libwebkit2gtk-4.1-dev libdbus-1-dev libayatana-appindicator3-dev xdotool
```

`libdbus-1-dev` is required by `libdbus-sys` via `dbus` via `tao`. The `vendored` feature that would sidestep it is not enabled by the resolved feature set, so the pkg-config probe is unconditional. `libayatana-appindicator3-1` is loaded by `dlopen` at runtime and its absence kills the tray; the loader sits behind a `once_cell` `Lazy`, so today it panics rather than degrading.

Rust 1.88 or newer for `rust-version`, and Cargo 1.85 or newer separately, to parse `edition2024` in a transitive dependency.

Measured cold timings on a 4-core machine, for sizing CI: 67 s for a cold `cargo check`, 74 s for `cargo test --workspace`, 150 s for a warm-cache release build.

## Runtime surfaces and their control skills

| Surface | How | Skill |
| --- | --- | --- |
| Tauri webview | `cargo run -p echo-desktop` | **control-ui** |
| CLI subcommands | `echo-desktop rec --once/--toggle/--hold` | **control-cli** |
| X11 HUD | `xvfb-run ./target/release/echo-desktop --hud-demo`, capture with `xwd` | none, manual fallback |
| GTK tray | screenshot a real desktop panel | none, manual only |

The last two gaps are flagged per the plan playbook. The HUD fallback is the one [02-design-overhaul/testing.md](../02-design-overhaul/testing.md) already established. The tray is drawn into the panel by libappindicator and cannot be driven at all, which is why phase 6 verifies it by screenshotting at 22 px and 24 px on a real GNOME session.

The Vite dev server is for styling iteration only. Outside Tauri, `frontend/src/tauri.ts` serves in-memory preview fixtures and never starts the Rust backend, so it proves nothing about the input-to-output chain.

## The two-process rule

**Every phase that changes behaviour must be verified through both recording paths.** They are not the same code path and this is the failure mode most likely to ship.

The GUI record button spawns a thread **in-process** (`src-tauri/src/main.rs:175-183`), so it sees the desktop app's own environment. A compositor shortcut bound to `echo-desktop rec --toggle` spawns a **fresh process** that inherits the desktop session environment and can never see anything the GUI set in its own environment. The only channel the two share is the filesystem under `data_dir()`, plus the config file phases 7 and 8 add.

An environment-only implementation of any setting passes every test you would think to write through the GUI and silently falls back through the bound shortcut. Bind the shortcut once and keep it bound for the rest of the program.

## Test conventions to follow

**Rust unit tests** live in `#[cfg(test)] mod tests` at the bottom of the file they test and never touch hardware or a model.

**Rust integration tests needing hardware or models are `#[ignore]`d and gated twice**: the attribute carries a reason string naming what to set, and the test body asserts the environment variable at runtime. So `cargo test -- --ignored` fails loudly instead of quietly grabbing a microphone. Copy the shape at `crates/echo/tests/record_once.rs:5-12`.

**Assertions on transcripts stay loose.** `has_known_word` against `["claude", "code", "clawed"]` (`crates/echo/tests/transcribe_fixture.rs:10-15`), never an exact string.

**Frontend tests** use `render(<App />)`, then `await screen.findByRole(...)` to clear the deliberate 0 ms initial-fetch timer, then synchronous `fireEvent`. There is no `vi.mock`; the bridge mocks itself because `isTauri()` checks `window.__TAURI_INTERNALS__`, which jsdom never defines. **A new IPC wrapper with no preview branch will fail its test**, because it reaches `invoke` with no Tauri present. Give every new control an accessible name; a bare `<select>` leaves `getByRole('combobox')` as the only handle and that breaks at the second one.

## Fixtures

`crates/echo/tests/fixtures/claude_code.wav` is the only audio fixture today, roughly 400 ms at 16 kHz mono. Phase 3 adds a silent WAV beside it, which every later blank-audio and VAD check needs.

Two JSON fixture sets get committed: whisper-cli `-oj` output in phase 13 and `sherpa-onnx-offline` stdout in phase 16. Both formats are log-adjacent with no compatibility guarantee, so record the upstream version that produced each one.

Phase 20 needs a real dictation corpus, roughly 20 utterances with hand-written references. Two words of `claude_code.wav` cannot separate two engines. Cover normal speech, fast speech, a technical sentence with identifiers, a quiet utterance, one with background noise, and several non-English languages inside Parakeet's 25. Commit the audio, or a manifest plus a fetch script if it is large.

`crates/echo/tests/compare_engines.rs` becomes that harness. Today it asserts nothing and swallows errors at `:16-24`, which is why the phase 16 Parakeet bug survived. At minimum it must fail when an engine returns empty text for a speech fixture.

Note that `ECHO_AUDIO_FIXTURE` suppresses the HUD as a side effect (`crates/echo/src/ui/hud.rs:39`), intentionally. The fixture path and the HUD can never be exercised in the same run.

## The non-speech marker inventory

Phase 3's filter is tested against output actually observed from `ggml-base.bin` across 27 languages and four non-speech fixtures, not against anecdotes.

**Bracketed, square.** `[BLANK_AUDIO]`, `[MUSIK]`, `[MÚSICA]`, `[Música]`, `[MÚSICA DE FUNDO]`, `[Musique]`, `[muzyka]`, `[MUZIĘ]`, `[MÜZİK ÇALIYOR]`, `[音楽]`, `[MUSIC]`.

**Bracketed, round.** `(music)`, `(música)`, `(音楽)`, `(blender whirring)`, `(plastic crinkling)`, `(crunching)`, `(grunts)`.

**Asterisk-delimited**, which a `\[.*\]` filter misses entirely. `* Musik *`, `* Spannungsvolle Musik *`.

**Bare glyphs.** `♪`, `♪♪`, `...`.

**Negative cases that must survive.** `Open (paren) here`, `Rate it 5 stars *`, `He said "music" loudly`. Real dictation contains parentheses, which is why the filter is whole-segment only.

Match case-insensitively; the lowercase `[blank_audio]` form is reported downstream.

**What no filter catches**, recorded here so nobody mistakes phase 3 for the fix. `you` (English, silence). `TV GELDERLAND 2020.` (Dutch, all four fixtures). `Редактор субтитров А.Синецкая Корректор А.Егорова` (Russian, silence). `Det är ju en av det här.` (Swedish, silence). These are memorised subtitle boilerplate. They look exactly like dictated text. Phase 4's VAD is the only measure that stops them, because it stops the decoder seeing the audio at all.

## Flags that were measured and rejected

Do not reach for these when tuning phase 3 or 4. Measured at whisper.cpp `45f1593`:

- `-sns` / `--suppress-nst` turned `[BLANK_AUDIO]` into `I'm going to put the water in the pot.` on white noise, and into a fluent fabricated German sentence with `-l de`. It bans the bracket tokens, so the model emits prose instead. Strictly worse for dictation.
- `--suppress-regex '\[.*|\*.*|\(.*'` produced repetition loops: `[Música]` 70-plus times, `...` 100-plus times.
- `-nth` / `--no-speech-thold` did nothing on digital silence, because the model is confidently emitting a real token so `no_speech_prob` stays under the threshold. Two upstream issues covering this are still open.
- `--suppress-blank` is not a CLI flag at all. It is a library parameter defaulting to true, and it only masks EOT and the space token at the initial decoding position.

## End-to-end acceptance, after phase 19

The gate for the program as a whole. Each item maps to one of the five original complaints.

1. **Identity.** Install the phase 2 deb on a clean GNOME session. One menu entry. Recognisable icon in the panel at default and 200% scale, in the titlebar, in the dock, and as the webview favicon. **Check the panel on a dark theme and a light theme**, because one background cannot prove an alpha channel: the current icon looks right on a light panel and shows a white box on a dark one.
2. **Microphone.** Two input devices attached. Pin the non-default one in Settings, restart, and dictate through both the GUI button and the bound compositor shortcut. Both use the pinned device.
3. **Transparency.** Read the model path off the Settings transparency panel and find that exact file on disk. Swap the model, dictate, and watch the panel follow.
4. **Silence.** Two seconds of silence through a real microphone, then `ECHO_CLEANUP=off`, then again with the VAD model removed. Nothing typed in any of the three.
5. **Languages.** Download `ggml-small.bin` from inside the app, pick German, dictate German, get German. Switch to Auto, dictate German, and see German reported as detected.
6. **Delivery.** All of the above using only artifacts downloaded from GitHub Actions, with no local toolchain. The Release is marked as a pre-release, and the version shown in Settings matches the tag it was built from.

Item 6 is the one that proves the rest. A green CI badge is not evidence; a working download is (**principle-prove-it-works**).

Phase 20 is deliberately outside this gate. Its output is a measured verdict, so its acceptance is a reproducible table and a written decision, not a working feature.
