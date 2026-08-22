# Phase 7: the HUD

Back to [overview](overview.md).

## Goal

A recording HUD that looks designed, tells the truth about the microphone, and stays for the whole session. Today's capsule (`crates/echo/src/ui/hud.rs`) has hard 1-bit SHAPE edges, thirteen bars animated by a sine wave that knows nothing about the mic, and a lifetime that ends at `drop(recording_hud)` in `crates/echo/src/rec.rs:100`, before transcription starts. The longest wait in the session gets no indicator at all.

## Changes

**Real levels, from the capture callback.** `crates/echo/src/audio.rs` gains a `LevelMeter`: an `Arc<AtomicU32>` holding f32 RMS bits, published per cpal callback buffer when a sink is attached. The callback already touches every buffer (`audio.rs:264-266`); computing a sum of squares there is a few instructions per sample and touches no lock. `rec.rs` creates the meter, hands one clone to the capture and one to the HUD. When `ECHO_AUDIO_FIXTURE` is set, the fixture path publishes RMS computed from the WAV chunks it plays, so demo and CI screenshots show truthful bars.

**The bars follow the meter, smoothed like a broadcast meter.** Fast attack, slow release: each frame the displayed level moves toward the measured RMS by a large factor when rising and about 0.80 retention when falling, the behavior [open-wispr](https://github.com/human37/open-wispr/pull/66) adopted and [sflow](https://github.com/daniel-carreon/sflow) publishes constants for. A rolling history of recent levels drives fourteen bars with rounded caps. The sine wave is deleted. A dead or silent mic now reads as a flat line, which is exactly the diagnostic Superwhisper's [recording window docs](https://superwhisper.com/docs/get-started/interface-rec-window) describe, and exactly what a fake animation hides.

**The whole session, not just Recording.** The HUD lives until injection finishes and renders four states, following the pill-state pattern [sflow's README](https://github.com/daniel-carreon/sflow) tabulates:

- **Recording.** Pulsing red dot (the one functional accent, per plan 02), live bars.
- **Transcribing.** The bars collapse into three dots with a traveling shimmer. Neutral color; the mic is no longer live.
- **Done.** A brief hold, about 300 ms, then the capsule fades.
- **Failed.** The dot goes steady red for about a second. No text; the desktop app carries the detail.

`rec.rs` moves the drop to after injection and drives transitions from the session machine it already has. No font dependency: state is communicated through motion and color, and if an elapsed-seconds readout is wanted it is drawn from an embedded 3x5 bitmap digit set, not a font stack.

**Per-pixel alpha.** The window requests a 32-bit ARGB visual with its own colormap when a compositor owns the `_NET_WM_CM_S0` selection, and each frame is rasterized client-side into an RGBA buffer with tiny-skia and pushed with `PutImage`. That buys anti-aliased capsule edges, a translucent background fill, a hairline border, and a soft glow behind the dot, none of which the 1-bit SHAPE mask can express. When no compositor answers, the code keeps the current SHAPE path with opaque colors, because ARGB transparency without a compositor renders black, per the [xorg mailing list](https://lists.x.org/archives/xorg/2017-December/059097.html). Both paths draw the same layout; only the edge quality and translucency differ.

**tiny-skia joins `crates/echo`.** It is already in the workspace lockfile through `xtask`, so the HUD and the icon generator share one rasterizer. The rejected alternatives are raw XRender trapezoid drawing (no anti-aliased curves without mask pictures, far more protocol code) and a WebKitGTK overlay window (rejected in the overview on four open upstream bugs and a second-process architecture).

**Click-through and always-on-top stay as they are.** The input shape is already set to an empty region and `_NET_WM_STATE_ABOVE` is already set; both survive the rewrite.

**`--hud-demo` cycles the states.** Two seconds of recording bars driven by the fixture or a synthetic source, then transcribing, then done. One command produces every screenshot the PR needs.

## Data structures

```
LevelMeter(Arc<AtomicU32>)          // f32 RMS bits, written per audio callback
HudState { Recording, Transcribing, Done, Failed }
FrameBuffer { width, height, pixels: Vec<u8> }   // premultiplied RGBA, one per frame
```

`HudState` is set from the session machine in `rec.rs`; the HUD thread reads it each frame. The frame buffer is reused across frames rather than reallocated.

## Verification

**Static.** `cargo test --workspace`. Unit tests cover the level-to-bar mapping (silence flattens, full scale saturates), the smoothing constants (attack faster than release), state transitions, and the compositor-detection fallback (no `_NET_WM_CM_S0` owner selects the SHAPE path).

**Runtime.** Under Xvfb, run `echo-desktop --hud-demo` with `ECHO_AUDIO_FIXTURE` set and screenshot each state; attach the four frames to the PR. Then one live check on real hardware: `rec --toggle`, speak, stop, and confirm the bars tracked speech, the transcribing state appeared, and the capsule faded after injection. If CI can run xcompmgr or picom under Xvfb, add the compositor-present screenshot there; otherwise the ARGB path is verified manually and the fallback is what CI sees. The acceptance gate is visual: the capsule edges are smooth in the compositor screenshot, and the bars move with the fixture's actual loudness, which the sine wave could never do.
