# Phase 1: CI check workflow

Back to [overview](overview.md).

## Goal

Every push and pull request builds Echo on Ubuntu and runs the full check suite. This lands first because every later phase's claim of "it works" is only as good as the gate that proves it (**principle-sequence-verifiable-units**).

It also fixes a documented lie. A machine set up from the README's `apt install` line cannot build Echo. `libdbus-sys` panics without `libdbus-1-dev`, reproduced by blinding pkg-config and rebuilding that crate alone. Writing the install list into CI is what stops it drifting again (**principle-encode-lessons-in-structure**).

## Changes

**`.github/workflows/check.yml`, new.** One job on `ubuntu-latest`, triggered by `push` and `pull_request`. Steps in a forced order:

1. `apt-get install` the six build packages: `build-essential`, `pkg-config`, `libasound2-dev`, `libwebkit2gtk-4.1-dev`, `libdbus-1-dev`, `xdotool`. Add `libayatana-appindicator3-dev` so the tray's runtime library is present for anything that loads it.
2. Pin the Rust toolchain to the workspace's `rust-version` with `dtolnay/rust-toolchain`, components `clippy` and `rustfmt`.
3. Cache cargo registry and `target/` with `Swatinem/rust-cache`. The measured cold dependency build is 67 seconds for `check` and 150 seconds for a release build, so caching is worth a step.
4. `npm ci --prefix frontend`, then `npm run build --prefix frontend`. **This must precede every cargo step.** `tauri-build` resolves `frontendDist` at compile time and panics when `frontend/dist` is absent, so a cargo-first job fails on `check`, `clippy`, `test`, and `build` alike.
5. `npm run lint --prefix frontend` and `npm run test --prefix frontend`.
6. `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`.
7. `desktop-file-validate packaging/echo.desktop`.

Step 7 fails on the first run, and that is the point. `Categories=Utility;Audio;` is invalid per the menu spec: `Audio` requires `AudioVideo` alongside it, and two main categories make the app appear twice in the menu. Fix `packaging/echo.desktop` to `Categories=Utility;` in this same PR so the step passes.

**`src-tauri/Cargo.toml`.** Add `[lints] workspace = true`. It is the only workspace member missing it, so the workspace's `clippy::all = deny` and `unsafe_code = forbid` do not currently apply to the crate that holds the whole desktop shell. The README's `-D warnings` has been covering that by accident.

**`README.md`.** Correct the `apt install` list to match the workflow. Two packages are missing today and one of them is a hard build failure.

## Data structures

None. This phase is configuration.

## Verification

**Static.** The workflow is the static check. It must go green on its own PR.

**Runtime.** Open the PR and read the Actions run. Confirm four things in the log rather than trusting a green tick: `libdbus-1-dev` appears in the apt output, the npm build step runs before any cargo step, `cargo test` reports 51 passed and 4 ignored, and `desktop-file-validate` exits 0 after the `Categories` fix.

Then prove the package list is the reason it works. Push a throwaway commit removing `libdbus-1-dev` from the workflow, confirm the run fails inside `libdbus-sys`'s build script, and revert. A CI job you have never seen fail has not been tested (**principle-prove-it-works**).
