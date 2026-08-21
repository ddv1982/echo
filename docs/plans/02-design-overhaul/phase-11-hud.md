[Back to overview](overview.md)

# Phase 11: HUD palette alignment

## Goal

The X11 recording capsule is the only Echo pixel most users see while dictating, and it still speaks the old palette (`BG 0x141821`, cyan wave `0x6ec1e4`). Align it with the new language: near-black neutral capsule, red recording dot, grayscale wave.

## Changes

- `crates/echo/src/ui/hud.rs`. Update the color constants in `draw_recording_frame` (`BG`, `RED`, `RED_DARK`, `CYAN`, `CYAN_DIM`) to the new palette: neutral near-black background matching the dark theme page surface, recording red matching `--recording`, and the wave bars in two grayscale steps replacing the cyan pair. Rename the constants to what they now are. No behavior, shape, or timing changes.
- The pairing between these constants and `frontend/src/styles/tokens.css` values is stated in a short comment at the constants, because the Rust side cannot read CSS and the next editor must know where the numbers come from (a non-obvious why, the sanctioned comment case).

## Data structures

Five u32 color constants. No new types.

## Verification

Static: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`.

Runtime: no control skill covers raw X11 surfaces; this gap is flagged per the plan playbook. Fallback: run `echo-desktop --hud-demo` under Xvfb (`xvfb-run`), capture the screen with `import` or `xwd`, and compare the capsule against the dark-theme app screenshot side by side. The existing `cli_hud_demo` integration test must stay green.
