# Phase 2: CI release artifacts

Back to [overview](overview.md).

## Goal

Download a working Echo build from GitHub without a local toolchain. Linux only.

## Changes

**`.github/workflows/release.yml`, new.** Reuses phase 1's setup steps, then bundles. Triggered three ways: `workflow_dispatch` for on-demand builds, `push` on tags matching `v*`, and a nightly `schedule` so the build path does not rot between releases.

`cargo-tauri` is nowhere in this repo. No `@tauri-apps/cli` in `frontend/package.json`, no `tauri-cli` dev-dependency in any of the three manifests, no npm script, no cargo alias. `cargo tauri build` has therefore never run here, which makes this the highest-uncertainty phase in the plan. Install it explicitly with `cargo install tauri-cli --version "^2" --locked`, cached by the rust-cache step.

Bundle targets split by risk:

- **`--bundles deb,rpm` in the required job.** Both are produced by pure-Rust code paths with no external tooling. `rpm` uses the `rpm` crate, so no `rpmbuild` is needed.
- **AppImage in a separate job marked `continue-on-error`.** `linuxdeploy.rs` downloads `AppRun-{arch}` and `linuxdeploy-{arch}.AppImage` from `github.com/tauri-apps/binary-releases` on first use and needs a FUSE-capable runner. Do not let a network fetch of a third-party binary block a release. Promote it to required once it has passed ten consecutive nightlies.

Upload with `actions/upload-artifact` on every run, and attach to a GitHub Release with `softprops/action-gh-release` on tag pushes only.

**Also upload the bare `target/release/echo-desktop` binary.** It is 10.7 MB and it is the fastest thing for a user to try. The README's install instructions already assume a loose binary on `PATH`.

**`README.md`.** Add a "Download" section above "Build", pointing at the Releases page and the nightly artifact.

## Data structures

None.

## Verification

**Static.** Phase 1's check workflow still green.

**Runtime.** This phase is verified by using the output, not by reading the log (**principle-prove-it-works**).

1. Download the `.deb` from the run's artifacts. `dpkg-deb -c` it and confirm the three expected paths: `usr/bin/echo-desktop`, `usr/share/applications/Echo.desktop`, and at least one icon under `usr/share/icons/hicolor/`.
2. `sudo dpkg -i` it on a clean Ubuntu container, run `echo-desktop --hud-demo`, and confirm it starts.
3. Download the loose binary, `chmod +x`, and confirm `echo-desktop rec --once` reaches `Transcribing` with `ECHO_AUDIO_FIXTURE` set to the repo's `claude_code.wav`.

Record in the PR which of the three bundle targets actually produced a file. The investigation read the bundler's target filtering from source and confirmed Linux resolves `"all"` to exactly deb, rpm, and AppImage, but nothing in this repo has ever run it.

## Note for phase 6

The deb installs its icon as `echo-desktop.png` and its desktop entry as `Echo.desktop`, both derived from `mainBinaryName` and `productName`. `packaging/echo.desktop` uses `Icon=echo`. A user who installs the deb and also follows the README's manual steps ends up with two menu entries pointing at two differently-named icons. Do not fix it here. Phase 6 owns the naming collision, and this phase's artifact is the evidence for it.
