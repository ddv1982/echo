# Model quality and reliability

## Outcome

Echo should give a new user one strong multilingual default, keep Parakeet as an explicit local alternative, and never confuse runtime output or old clipboard contents with dictated speech.

The default for machines with at least 8 GiB RAM becomes Whisper Large v3 Turbo Q5_0. Base Q5_1 remains the low-memory fallback. Existing Small, system-installed, and manually imported models remain usable, but Small stops being the recommended setup.

There is no new quality setting and no config migration. `whisper_model` remains an exact optional pin.

## Phases

### Phase 1: correct the two text boundaries

- Parse the pinned sherpa-onnx JSON response and pass only its `text` field into Echo.
- Reject malformed JSON-shaped output instead of dictating it literally.
- Keep legacy successful plain-text output compatible.
- Stop restoring the previous clipboard after fallback paste. A fallback leaves the dictated text available, including when Echo can only offer `ClipboardOnly`.
- Prefer the Wayland clipboard pair in Wayland sessions and the X11 pair in X11 sessions.

### Phase 2: choose a stronger managed default

- Recommend Large v3 Turbo Q5_0 at 8 GiB RAM and above.
- Recommend Base Q5_1 below 8 GiB or when memory cannot be measured.
- Keep Small as an advanced and compatibility option.
- Keep Engine Auto behavior unchanged. The Recommended setup explicitly activates Whisper, while existing users who chose Auto do not silently move away from Parakeet.
- Do not enable Flash Attention or override whisper.cpp thread counts without measurements on the managed Linux runtime.

### Phase 3: make Settings truthful

- Rename `Model quality` to `Speech model`.
- Use the backend-produced language mode to project the engine that Auto actually selected.
- Show the Whisper selector for Whisper.
- Show `Parakeet TDT 0.6B v3` as a fixed model for Parakeet instead of hiding the row. The managed activation version retains the INT8 artifact identity; manual full-precision files do not inherit it.
- Clear a dormant file-backed Whisper model when a user explicitly selects or installs Parakeet.
- Name Turbo as the recommended balance and Small as a faster, lower-accuracy option.

### Phase 4: build the benchmark lever

- Add a manifest-driven script that invokes the shipping `echo-desktop transcribe --format json` boundary.
- Accept Whisper, Parakeet, and Fake candidates.
- Emit raw JSON Lines plus a Markdown summary.
- Report per-language word error rate, inference real-time factor, and nonempty output on silence.
- Fail on missing candidates or inference errors.
- Verify the script with Fake in CI. Real model runs stay opt-in because this host has no Linux runtime or model cache.

### Phase 5: prove and release

- Run focused regressions before and after each fix.
- Run Rust, frontend, responsive, CLI, formatting, lint, and release checks.
- Review the completed diff from independent correctness and maintainability angles.
- Open and babysit the PR, merge only after required checks and review are clear, then tag and verify the release assets.

## Non-goals

- No managed full Large v3 download in this release.
- No Fast, Balanced, or Best persistent enum.
- No third Parakeet runtime.
- No phrase blacklist for hallucinations.
- No arbitrary clipboard restore delay.
- No claim that one aggregate English score represents multilingual quality.
