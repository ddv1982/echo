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

## Ship it as an alpha

Echo is not 1.0 and it should not pretend to be. Everything this release produces is marked alpha, in three places, because a user who downloads a binary decides how much to trust it from the label.

**Mark the GitHub Release as a pre-release.** `softprops/action-gh-release` takes `prerelease: true`. Derive it from the tag rather than hardcoding it, so a later `v1.0.0` does not silently ship as an alpha because nobody remembered to flip a flag (**principle-encode-lessons-in-structure**). Tags are `v0.1.0-alpha.1`, `v0.1.0-alpha.2`, and so on.

**Label the nightly artifacts distinctly from the tagged ones.** A nightly is not even an alpha; it is whatever was on `main`. Name it with the short SHA so a bug report identifies a commit.

**Say it in the app.** The version and the alpha label belong in Settings, so a user reporting a transcription bug can identify their build without running a command. That is a frontend change and it lands with the transparency panel in phase 15, not here; this phase only establishes the version string it will display.

**One packaging risk to verify before choosing a version string.** A semver prerelease suffix uses `-`, and `-` is a field separator in both Debian and RPM version formats. RPM's `Version` field cannot contain `-` at all. So `0.1.0-alpha.1` may be rejected or silently mangled by the deb and rpm bundlers. The conventional encodings differ per format, `0.1.0~alpha.1` for Debian and a split `Version` plus `Release` for RPM, and this repo has never run `cargo tauri build`, so treat this as unverified.

Try the suffixed version first and read the produced filenames and control metadata. If either bundler rejects or mangles it, the fallback is to keep the manifest version plain (`0.1.0`) and carry the alpha label in the git tag, the GitHub Release title, the `prerelease` flag, and the artifact filenames only. That still tells every user what they have, and it does not fight two packaging formats for a cosmetic win.

**Version currently lives in two places** and they will drift. `Cargo.toml:10` sets `workspace.package.version = "0.1.0"` and `src-tauri/tauri.conf.json:41` sets `"version": "0.1.0"` independently. Collapse them to one source. Tauri can inherit the version from the crate when the config field is absent, but confirm that rather than assuming it; the documented alternative source is a `package.json` path. If inheritance does not work, add a CI step asserting the two strings match, which is the cheap version of the same guarantee.

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
4. Push a `v0.1.0-alpha.1` tag to a scratch branch. Confirm the Release is created **marked as a pre-release**, and read `dpkg-deb -f` and the rpm metadata to see what each bundler did with the `-alpha.1` suffix. Report the exact strings in the PR. This is the unverified packaging risk above, and reading the produced metadata is the only way to settle it.

Record in the PR which of the three bundle targets actually produced a file. The investigation read the bundler's target filtering from source and confirmed Linux resolves `"all"` to exactly deb, rpm, and AppImage, but nothing in this repo has ever run it.

## Note for phase 6

The deb installs its icon as `echo-desktop.png` and its desktop entry as `Echo.desktop`, both derived from `mainBinaryName` and `productName`. `packaging/echo.desktop` uses `Icon=echo`. A user who installs the deb and also follows the README's manual steps ends up with two menu entries pointing at two differently-named icons. Do not fix it here. Phase 6 owns the naming collision, and this phase's artifact is the evidence for it.
