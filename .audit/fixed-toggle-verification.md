# Fixed toggle verification

Verified on 2026-08-23 from `/Users/vriesd/projects/echo`.

## Rerunnable gate

Command:

```sh
scripts/verify-fixed-toggle.sh
```

Result: exit 0.

- Frontend build and TypeScript check passed.
- Frontend lint passed.
- Frontend tests passed with Node 22. Four files and 68 tests passed.
- Workspace Clippy passed with warnings denied.
- `echo-desktop` tests passed. The main binary had 17 passing tests and 4 host-dependent ignored tests. All CLI and desktop-entry integration tests passed.
- Direct rustfmt checks passed for every touched Rust file.
- `evdev` is absent from the `echo` dependency tree.
- Removed-shortcut references are limited to the three obsolete-config regression assertions.
- `rec --hold` exits 2 and prints `usage: echo-desktop rec --once|--toggle`.
- `git diff --check` passed.

## Workspace baseline comparison

Command:

```sh
cargo test --workspace
```

Result: exit 101 with 84 library tests passing and these two unchanged macOS-host failures:

- `rec::tests::toggle_starts_stops_and_can_restart`
- `upgrade::tests::path_scan_finds_installs_in_path_order_and_stale_ones_differ`

Before the change, the same host also failed `rec::tests::managed_claim_does_not_stop_an_existing_session`. That test and its managed hold implementation were intentionally deleted with push-to-talk.

## Live browser verification

Recording: `/Users/vriesd/.t3/userdata/browser-artifacts/browser-recording-mt52zges.mp4`

The recording exercises the active portal preview, ready GNOME presentation, and manual compositor presentation. It shows the fixed chord, the status-only Settings row, GNOME readiness across Home and sidebar, and manual setup remaining unverified.

## Host limits

This macOS host cannot exercise Linux portal consent, real X11 grabs, or real GNOME dconf writes. The private portal test compiles but is ignored because `dbus-daemon` is unavailable. The X11 and live GNOME tests remain explicitly ignored with their required host tools named in the test output.
