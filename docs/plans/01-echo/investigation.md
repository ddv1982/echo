# How the video's dictation app works

throughput checkpoint: n/a, read-only investigation

Primary source. [https://www.youtube.com/watch?v=IMQw3aHjf2Q](https://www.youtube.com/watch?v=IMQw3aHjf2Q), fetched this session. Quotes below are from that transcript.

## Overview

The video is a "build this with Claude Code" walkthrough of a Mac app. The ask they type is simple. Look up push-to-talk dictation. Recommend architecture for this machine. Start with a skeleton, then a real app.

Claude's plan is the architecture we keep at the pipeline level. Echo implements it on Linux only.

1. Shell. The app process.
2. HUD. Waveform overlay while recording.
3. Hotkey. In the video, `CGEventTap`. On Linux, evdev plus a CLI bind.
4. Audio. Microphone while the key is held.
5. STT. The video used Apple's on-device transcriber, with Parakeet as backup. Echo uses Parakeet and whisper.cpp.
6. Cleanup. Optional small local model for punctuation and ums.
7. Injection. Put text where the cursor is.

They name the app Murmur, then Murmur YouTube, because an older build already occupied a permission slot. The prototype transcribes. It does not type into Cursor. Claude's logs say injection succeeded. The creator says, "That's the classic AX silent failure." After the fix, it types into Cursor and a Google Doc. They add an engine comparison window, then a history window, a dictionary, and an 1980s tape-recorder look.

That is a Mac app. Echo is not. The pipeline stays. The shell, hotkey, and engines do not.

## Key concepts

**Hold-to-talk, not a recorder.** The unit of work is one key-down, one utterance, one insert. Release is the commit. That is why the session is a state machine and not a file pipeline.

**Local STT is the privacy pitch.** The video wants audio to never leave the machine. Echo does the same with portable engines. [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) ships INT8 ONNX builds of `parakeet-tdt-0.6b-v3`, about 680 MB, 25 languages, fine on CPU. [whisper.cpp](https://github.com/ggml-org/whisper.cpp) is the fallback.

**Injection is the product.** Transcription that stays on the clipboard is a toy. The video learns this in Cursor. Linux has the same class of lie. A backend can report success and type into the void. [VaulType's TEXT_INJECTION.md](https://github.com/vaultype/VaulType/blob/main/docs/features/TEXT_INJECTION.md) is still the right cascade idea. Try a real insert. Fall back to clipboard plus a synthetic paste. Restore the user's clipboard. Treat "success" as a rumor until a widget we own echoes the text back.

**Linux injection is compositor policy.** Wayland will not give you a Mac-style event tap. Tools that work today pick from libei / the emulated-input portal, `ydotool` over uinput, `wtype` on wlroots, `xdotool` on X11, and clipboard paste. [flowvoice](https://github.com/GOJO-SENPA1/flowvoice) auto-selects `wtype` / `ydotool` / `xdotool`. Layout-unaware uinput keystrokes become garbage unless you go through XKB.

**Cleanup is what makes dictation usable.** The video treats punctuation and um-stripping as optional, added after raw insert works. [paretoimproved/murmur](https://github.com/paretoimproved/murmur) is the Linux version of that sentence. Raw ASR is not the product. A local rewrite pass is.

## How it works

End-to-end, one utterance.

```
hotkey down
    -> session Idle to Recording
    -> HUD visible, mic buffers PCM
hotkey up
    -> session Recording to Transcribing
    -> engine.transcribe(pcm_16k_mono)
    -> optional cleanup
    -> optional dictionary rewrite
    -> session Injecting
    -> injector.deliver(text, focused_target)
    -> history append
    -> session Idle
```

Failure is a state, not a log line. Missing mic permission, missing inject permission, engine download in flight, no focused target. The HUD should say which one.

The video's comparison window records once and scores a few engines. One of those timings came from another process's local database, so the clock includes IPC they do not own. Echo's harness records one WAV and runs Parakeet and whisper.cpp on it. No foreign databases.

Dictionary in the video is a post-pass. They add "Claude Code", speak "clawed code", and the UI flashes Corrected. That design ports as a string rewrite after STT. It does not depend on a vendor vocabulary API.

## Where things live

Target tree for this repo. Two crates. Linux only.

```
crates/echo-core/          session, PCM newtype, Engine, Injector, stores
crates/echo/               binary
  src/audio.rs             cpal capture
  src/stt/                 whisper.cpp and sherpa-onnx adapters
  src/inject.rs            libei, ydotool, xdotool, paste
  src/hotkey.rs            evdev plus CLI
  src/ui/                  HUD, history, tray
  src/main.rs
```

What the video put in a Swift target maps like this.

| Video piece | Echo home on Linux |
| --- | --- |
| Shell | `crates/echo`, tray + window |
| HUD | `src/ui`, layer-shell or keep-above |
| Hotkey | `src/hotkey.rs`, evdev, plus a CLI so compositors can bind |
| Audio | `src/audio.rs`, PipeWire / ALSA via cpal |
| STT | `src/stt`, Parakeet and whisper.cpp |
| Cleanup | later phase, local model over a stdin protocol |
| Injection | `src/inject.rs`, EI, ydotool, xdotool, paste |
| History / dictionary | `echo-core` stores under `$XDG_DATA_HOME/echo` |

Prior art to read, not to vendor.

- Video architecture, for the pipeline only. Swift apps such as [janisbelozerovs-dev/murmur](https://github.com/janisbelozerovs-dev/murmur) and [daviddao/murmur](https://github.com/daviddao/murmur).
- Linux loop. [flowvoice](https://github.com/GOJO-SENPA1/flowvoice), [paretoimproved/murmur](https://github.com/paretoimproved/murmur).
- Portable engines. [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), [whisper.cpp](https://github.com/ggml-org/whisper.cpp).

## Gotchas

**Logs lie.** The video's insert path returned success in Electron. Any inject method that cannot read back the target field must be marked `Unconfirmed`. The comparison harness and the widget test are how we refuse that lie.

**Wayland will waste a week if we pretend it is X11.** `xdotool` does nothing useful on GNOME or KDE Wayland. `wtype` is wlroots. GNOME wants the portal or uinput. Design the cascade, then test on one X11 session and one Wayland session.

**uinput is a permission product.** `ydotool` usually needs a root daemon and a socket the user can write. First-run UX is "here is the exact command," not a silent fail.

**Clipboard paste must restore.** Overwriting the pasteboard and leaving the transcript there is how people lose a copied password. Save, paste, wait, restore. Document the race.

**Do not leave a Mac half-port in the tree.** Empty `macos.rs` files are not a head start. They are noise. When macOS is actually in scope, write a new plan.

**Cleanup is later on purpose.** Without a reliable insert, a prettier sentence still sits on the clipboard. Ship the ugly honest loop first.
