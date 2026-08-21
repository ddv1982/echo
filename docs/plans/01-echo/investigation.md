# How the video's dictation app works

throughput checkpoint: n/a, read-only investigation

Primary source. [https://www.youtube.com/watch?v=IMQw3aHjf2Q](https://www.youtube.com/watch?v=IMQw3aHjf2Q), fetched this session. Quotes below are from that transcript.

## Overview

The video is a "build this with Claude Code" walkthrough. The ask they type is simple. Look up push-to-talk dictation. Recommend architecture for this machine. Start with a skeleton, then a real macOS app.

Claude's plan is the architecture we should keep at the pipeline level.

1. Shell. The app process.
2. HUD. Waveform overlay while recording.
3. Hotkey. `CGEventTap`, Fn then Right Control in the demo.
4. Audio. Microphone while the key is held.
5. STT. Apple `SpeechAnalyzer` / `SpeechTranscriber` on macOS 26. Parakeet as backup, pulled from Hugging Face with curl.
6. Cleanup. Optional small local model for punctuation and ums.
7. Injection. Put text where the cursor is.

They name the app Murmur, then Murmur YouTube, because an older build already occupied the Accessibility permission slot. They grant Microphone and Accessibility. The prototype transcribes. It does not type into Cursor. Claude's logs say injection succeeded. The creator says, "That's the classic AX silent failure." After the fix, it types into Cursor and a Google Doc. They add an engine comparison window, pick Apple as the default because it needs no model download, then grow a history window, a dictionary, and an 1980s tape-recorder look.

That is a Mac app. We need Linux too. The pipeline stays. The shell does not.

## Key concepts

**Hold-to-talk, not a recorder.** The unit of work is one key-down, one utterance, one insert. Release is the commit. That is why the session is a state machine and not a file pipeline.

**Local STT is the privacy pitch.** The video wants audio to never leave the machine. Apple's new model does that on macOS 26. [WWDC25 session 277](https://developer.apple.com/videos/play/wwdc2025/277/) describes `SpeechAnalyzer` as an on-device session you add modules to. `SpeechTranscriber` is the long-form module that now powers Notes and Voice Memos. `AssetInventory` downloads language packs into system storage, not into your app bundle. `DictationTranscriber` is the short-form cousin of old `SFSpeechRecognizer`. Blake Crosley's writeup of the split is the important one for a dictionary. `AnalysisContext.contextualStrings` lands on the short-form path. Long-form `SpeechTranscriber` does not take those phrases. See [speech-framework-vs-sfspeechrecognizer](https://blakecrosley.com/blog/speech-framework-vs-sfspeechrecognizer).

**Parakeet is the portable engine.** The video downloads NVIDIA Parakeet when Apple is not enough, and promises a separate repo for non-Mac users. Linux needs that portable engine on day one. [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) already ships INT8 ONNX builds of `parakeet-tdt-0.6b-v3`, about 680 MB, 25 languages, fine on CPU. [whisper.cpp](https://github.com/ggml-org/whisper.cpp) is the fallback that also runs on both OSes.

**Injection is the product.** Transcription that stays on the clipboard is a toy. The video learns this in Cursor. Other local dictation apps hit the same wall. Chrome often returns `kAXErrorNoValue` for the focused element, so a strict AX-only path falls through to clipboard-only. [VaulType's TEXT_INJECTION.md](https://github.com/vaultype/VaulType/blob/main/docs/features/TEXT_INJECTION.md) describes the working cascade. Try a real text field through Accessibility. Fall back to clipboard plus a synthetic paste. Restore the user's clipboard. Treat AX "success" as a rumor.

**Linux injection is a different rumor.** Wayland compositors do not give you `CGEventTap`. Tools that work today pick from libei / the emulated-input portal, `ydotool` over uinput, `wtype` on wlroots, `xdotool` on X11, and clipboard paste. [flowvoice](https://github.com/GOJO-SENPA1/flowvoice) auto-selects `wtype` / `ydotool` / `xdotool`. Layout-unaware uinput keystrokes become garbage unless you go through XKB.

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

The video's comparison window records once and scores Apple against Parakeet and a third closed app they already had installed. That third number is dirty. They read another process's local database for timing, so the clock includes IPC they do not own. They still conclude the gap is small and pick Apple for zero-setup. That pick is correct for a Mac-only demo on macOS 26. It is wrong as Echo's default. Linux has no SpeechAnalyzer. The shared default has to be Parakeet or whisper.cpp.

Dictionary in the video is a post-pass. They add "Claude Code", speak "clawed code", and the UI flashes Corrected. That design ports. Feed the same list into Apple contextual strings later if we add the short-form adapter. Do not wait for Apple to make the dictionary useful on Linux.

## Where things live

Target tree for this repo. Two crates. Platform code is modules, not a crate per OS.

```
crates/echo-core/          session, PCM newtype, Engine, Injector, stores
crates/echo/               binary
  src/audio.rs             cpal capture
  src/stt/                 whisper.cpp and sherpa-onnx adapters
  src/inject/linux.rs
  src/inject/macos.rs
  src/hotkey/linux.rs
  src/hotkey/macos.rs
  src/ui/                  HUD, history, tray
  src/main.rs
```

What the video put in a Swift target maps like this.

| Video piece | Echo home | Linux | macOS |
| --- | --- | --- | --- |
| Shell | `crates/echo` | tray + window | tray + window |
| HUD | `src/ui` | layer-shell or keep-above | click-through `NSPanel` equivalent via winit |
| Hotkey | `src/hotkey` | evdev, plus a CLI so compositors can bind | `CGEventTap` |
| Audio | `src/audio` | PipeWire / ALSA via cpal | Core Audio via cpal |
| STT | `src/stt` | Parakeet, whisper.cpp | same, optional SpeechAnalyzer later |
| Cleanup | later phase | local model over a stdin protocol | same |
| Injection | `src/inject` | EI, ydotool, xdotool, paste | AX, Cmd+V, pasteboard restore |
| History / dictionary | `echo-core` stores | files under `$XDG_DATA_HOME/echo` | `~/Library/Application Support/Echo` |

Prior art to read, not to vendor.

- Video architecture. Swift apps such as [janisbelozerovs-dev/murmur](https://github.com/janisbelozerovs-dev/murmur) and [daviddao/murmur](https://github.com/daviddao/murmur).
- Linux loop. [flowvoice](https://github.com/GOJO-SENPA1/flowvoice), [paretoimproved/murmur](https://github.com/paretoimproved/murmur).
- Portable engines. [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), [whisper.cpp](https://github.com/ggml-org/whisper.cpp).

## Gotchas

**Logs lie.** The video's AX path returned success in Electron. Any inject method that cannot read back the target field must be marked `Unconfirmed`. The comparison harness and the Linux widget test are how we refuse that lie.

**Apple is not the default.** `SpeechTranscriber` is fast on a Mac that already has the assets. It does not exist on Linux. Custom vocabulary on the long-form model is weak. Use it as a macOS bonus after Parakeet and whisper.cpp insert text on both OSes.

**Wayland will waste a week if we pretend it is X11.** `xdotool` does nothing useful on GNOME or KDE Wayland. `wtype` is wlroots. GNOME wants the portal or uinput. Design the cascade, then test on one X11 session and one Wayland session.

**uinput is a permission product.** `ydotool` usually needs a root daemon and a socket the user can write. First-run UX is "here is the exact command," not a silent fail.

**Clipboard paste must restore.** Overwriting the pasteboard and leaving the transcript there is how people lose a copied password. Save, paste, wait, restore. Document the race.

**Do not start in Swift and port.** The video's own next step was "a separate GitHub repo" for non-Mac users. That split is how this kind of app dies. Rust from phase 1 is the whole point of this plan.

**Cleanup is later on purpose.** Without a reliable insert, a prettier sentence still sits on the clipboard. Ship the ugly honest loop first.
