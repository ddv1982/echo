# Echo. A local Wispr Flow clone for Linux and macOS

## Context

[Wispr Flow](https://wisprflow.ai/) is a system-wide push-to-talk dictation app. Hold a key, speak, and cleaned text lands at the cursor in whatever app is focused. The company sells that loop on Mac, Windows, iPhone, and Android. There is no Linux build. Pricing starts free at 2,000 words per week, then Pro for unlimited use.

The video at [https://www.youtube.com/watch?v=IMQw3aHjf2Q](https://www.youtube.com/watch?v=IMQw3aHjf2Q) clones that loop on a Mac in a few Claude Code turns. The creator names the app Murmur YouTube. The working architecture is a Swift shell, a waveform HUD, a `CGEventTap` hotkey, microphone capture, Apple `SpeechAnalyzer` / `SpeechTranscriber` on macOS 26, optional NVIDIA Parakeet, optional cleanup, and cursor injection. The first real bug is the one that matters. Accessibility insertion reports success inside Cursor and types nothing. That is the Electron AX silent-failure case.

This plan keeps the video's pipeline and throws out the Mac-only shell. Echo has to run on Linux and macOS. The language is Rust. Go was the other candidate and loses on STT bindings and OS input.

## Scope

Included.

- Hold-to-talk dictation on Linux (X11 and Wayland) and macOS.
- Local STT only. Audio never leaves the machine.
- A shared engine protocol. Default engines are NVIDIA Parakeet via sherpa-onnx and whisper.cpp. Both run on both OSes.
- A platform injection cascade that actually types into the focused app, including Electron and Chromium.
- A click-through waveform HUD, a tray presence, transcription history, and a personal dictionary.
- An engine comparison harness like the one in the video.

Excluded.

- Windows. Add it later behind the same traits. Do not block v1 on it.
- Cloud STT and cloud cleanup.
- Mobile.
- A notes or meeting-recorder product.
- Wispr Flow branding, trademarks, or a copy of their UI.
- Snippets, tone profiles, and team sync. Those are Wispr differentiators we can add after the loop is honest.

## Constraints

- This Cloud Agent host is Linux. Core types, audio, STT, and the Linux inject/hotkey path can be proven here. macOS adapters need a Mac.
- Native macOS UI has no `control-ui` or `control-cli` skill. Runtime proof on Mac is a scripted log plus a human checking TextEdit and Cursor. Runtime proof on Linux is a scripted log plus a focused text field we control.
- Wayland will not let a normal app type into another window without help. Plan on libei / the input portal, then `ydotool` (uinput), then clipboard paste. wlroots `wtype` is a compositor-specific extra, not the default.
- Apple `SpeechAnalyzer` exists only on macOS 26+ / iOS 26+. It is an optional later adapter, not the default engine. [WWDC25 session 277](https://developer.apple.com/videos/play/wwdc2025/277/) is the source for that API.
- Do not ship Chromium to draw a waveform. The video's first failure is an Electron accessibility bug. We are not becoming that target.
- Permissions are part of the product. Linux needs microphone plus uinput or portal access. macOS needs Microphone, Accessibility, and often Input Monitoring.

## Alternatives

**A. Native Swift on macOS, separate Linux app.** This is what the video did, plus the "Windows Parakeet repo" they promised and did not build. Two codebases. Dictionary, history, and engine comparison would drift on day one. Rejected because Linux is a v1 requirement, not a port.

**B. Go daemon plus CGo.** One static binary is attractive. whisper.cpp and sherpa-onnx become CGo or subprocesses. `CGEventTap`, evdev, and libei bindings are thinner than Rust's. A GC in the audio callback is a problem we would spend the first month dancing around. Rejected.

**C. Electron or Tauri, like [OpenWhispr](https://github.com/OpenWhispr/openwhispr).** Cross-platform HUD for free. We would also ship a browser and re-enter the injection mess the video hit in Cursor. Rejected for the app shell. We can still steal their engine list. They already run Parakeet through sherpa-onnx and Whisper through whisper.cpp on Mac, Windows, and Linux. See [their local-models guide](https://docs.openwhispr.com/guides/local-models).

**D. Rust workspace, two crates, platform modules behind `cfg`.** Shared session machine and engine traits. Linux and macOS adapters at the edges. This is the choice. The Linux dictation tools that already work ([whisrs](https://github.com/y0sif/whisrs), [xhisper-rs](https://github.com/PrivateGER/xhisper-rs), [flowvoice](https://github.com/GOJO-SENPA1/flowvoice)) are mostly Rust or call out to the same injectors we will use.

## Applicable skills

- **how** over SpeechAnalyzer, `CGEventTap`, libei / ydotool, and whisper.cpp or sherpa-onnx before changing those subsystems.
- **interrogate** on the injection cascade and the engine protocol before those ship.
- **unslop** on every prose surface. `/deslop` on every diff before commit.
- **show-me-your-work** for the engine and injection decisions.
- No control skill exists for a native HUD. Call that out in each phase that draws pixels.

## Phases

1. [Session machine](./phase-1-session-machine.md)
2. [Audio capture](./phase-2-audio.md)
3. [STT engines](./phase-3-stt-engines.md)
4. [Linux injection](./phase-4-inject-linux.md)
5. [macOS injection](./phase-5-inject-macos.md)
6. [Hotkeys](./phase-6-hotkey.md)
7. [HUD](./phase-7-hud.md)
8. [Dictionary](./phase-8-dictionary.md)
9. [App shell](./phase-9-app-shell.md)
10. [Cleanup pass](./phase-10-cleanup.md)

Project-level checks live in [testing.md](./testing.md). The video read lives in [investigation.md](./investigation.md).

## Verification

Static. `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` on Linux in this environment.

Runtime. Linux phases must insert text into a real focused widget we spawn, not only log `injected=true`. macOS phases must do the same in TextEdit and in an Electron app (Cursor or VS Code). The comparison harness records one WAV and runs every enabled engine on it.

## Implementation guidance

Implementers apply these before coding a phase.

- Read this overview and the matching phase file. Then run the **how** skill on any subsystem that is new to the thread.
- Keep the session machine in `echo-core`. Do not let platform modules own dictation state.
- Linux is the first proving ground because this repo's agents run there. Do not start with the Apple adapter.
- `/deslop` each diff. Apply **unslop** to commits, comments, and any markdown you add.
- Use **interrogate** on injection and the `Engine` trait before calling those done.
- Keep a **show-me-your-work** trail for engine picks and inject fallbacks.
- Cursor's built-in babysit skill after the implementation PR opens.
- Do not add a crate or a trait for a single caller. Two crates is the ceiling until a third has a real second consumer.
