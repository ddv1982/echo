# Echo. Local hold-to-talk dictation for Linux

## Context

Echo is a system-wide push-to-talk app. Hold a key, speak, and cleaned text lands at the cursor in whatever app is focused. Audio stays on the machine. v1 is Linux only. X11 and Wayland.

The video at [https://www.youtube.com/watch?v=IMQw3aHjf2Q](https://www.youtube.com/watch?v=IMQw3aHjf2Q) builds that loop as a Swift Mac app. The pipeline is still the right one. Shell, HUD, hotkey, audio, STT, optional cleanup, cursor injection. The first real bug in the video is the one that matters on Linux too. Insertion reports success and types nothing. We prove insert by reading text back from a widget we own.

This plan keeps the pipeline and throws out the Mac shell, Apple speech APIs, and any `cfg(target_os = "macos")` stubs. The language is Rust. Go was the other candidate and loses on STT bindings and OS input.

## Scope

Included.

- Hold-to-talk dictation on Linux, X11 and Wayland.
- Local STT only. Audio never leaves the machine.
- A shared engine protocol. Default engines are NVIDIA Parakeet via sherpa-onnx and whisper.cpp.
- An injection cascade that types into the focused app, including Electron, Chromium, terminals, and native toolkits.
- A click-through waveform HUD, a tray presence, transcription history, and a personal dictionary.
- An engine comparison harness like the one in the video.

Excluded.

- macOS and Windows. A later plan can add them behind the same traits. Do not leave empty adapters in the tree.
- Cloud STT and cloud cleanup.
- Mobile.
- A notes or meeting-recorder product.
- Snippets, tone profiles, and team sync. Add those after the loop is honest.

## Constraints

- This Cloud Agent host is Linux. Every v1 phase must be provable here.
- Native overlay UI has no `control-ui` or `control-cli` skill. Runtime proof is a scripted log plus a focused text field we spawn.
- Wayland will not let a normal app type into another window without help. Plan on libei / the input portal, then `ydotool` (uinput), then clipboard paste. wlroots `wtype` is a compositor-specific extra, not the default.
- Do not ship Chromium to draw a waveform.
- Permissions are part of the product. Microphone plus uinput or portal access. First-run UX is an exact command, not a silent fail.

## Alternatives

**A. Follow the video and start in Swift.** That is a Mac app. Rejected. v1 is Linux.

**B. Go daemon plus CGo.** One static binary is attractive. whisper.cpp and sherpa-onnx become CGo or subprocesses. evdev and libei bindings are thinner than Rust's. A GC in the audio callback is a problem we would spend the first month dancing around. Rejected.

**C. Electron or Tauri.** Cross-platform HUD for free. We would also ship a browser and make insert harder. Rejected for the app shell. The engine list is still the right one. Parakeet through [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) and Whisper through [whisper.cpp](https://github.com/ggml-org/whisper.cpp).

**D. Rust workspace, two crates, Linux modules only.** Shared session machine and engine traits. Inject and hotkey are Linux files, not `cfg` soup. This is the choice. Linux dictation tools that already work, such as [flowvoice](https://github.com/GOJO-SENPA1/flowvoice), call out to the same injectors we will use.

**Update (2026-08).** The desktop UI overhaul revisited C for the app shell: the settings and history surfaces now ship as a Tauri app (`src-tauri` plus `frontend/`), a third workspace member alongside the two crates above. The GTK shell that predated it is deleted. Injection still goes through the external-tool cascade, not the webview. libei never got an implementation and is out of the cascade; the order is ydotool or wtype on Wayland, xdotool on X11, then clipboard.

## Applicable skills

- **how** over libei / ydotool, evdev, and whisper.cpp or sherpa-onnx before changing those subsystems.
- **interrogate** on the injection cascade and the engine protocol before those ship.
- **unslop** on every prose surface. `/deslop` on every diff before commit.
- **show-me-your-work** for the engine and injection decisions.
- No control skill exists for a native HUD. Call that out in each phase that draws pixels.

## Phases

1. [Session machine](./phase-1-session-machine.md)
2. [Audio capture](./phase-2-audio.md)
3. [STT engines](./phase-3-stt-engines.md)
4. [Injection](./phase-4-inject.md)
5. [Hotkeys](./phase-5-hotkey.md)
6. [HUD](./phase-6-hud.md)
7. [Dictionary](./phase-7-dictionary.md)
8. [App shell](./phase-8-app-shell.md)
9. [Cleanup pass](./phase-9-cleanup.md)

Project-level checks live in [testing.md](./testing.md). The video read lives in [investigation.md](./investigation.md).

## Verification

Static. `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` on this Linux host.

Runtime. Injection must put text into a real focused widget we spawn, not only log `injected=true`. Repeat in a terminal and in Chromium when doing the manual checklist. The comparison harness records one WAV and runs every enabled engine on it.

## Implementation guidance

Implementers apply these before coding a phase.

- Read this overview and the matching phase file. Then run the **how** skill on any subsystem that is new to the thread.
- Keep the session machine in `echo-core`. Do not let platform modules own dictation state.
- Do not add `macos.rs` or Apple engine variants. That is a later plan.
- `/deslop` each diff. Apply **unslop** to commits, comments, and any markdown you add.
- Use **interrogate** on injection and the `Engine` trait before calling those done.
- Keep a **show-me-your-work** trail for engine picks and inject fallbacks.
- Cursor's built-in babysit skill after the implementation PR opens.
- Do not add a crate or a trait for a single caller. Two crates is the ceiling until a third has a real second consumer.
