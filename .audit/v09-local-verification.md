# v0.9 local verification

Production candidate tested at `6a4f0de` plus the audit-only record that adds this file.

| Check | Result |
| --- | --- |
| `scripts/verify-settings-ux.sh` | Passed. Includes 16 microphone tests, setup presenter tests, and pinned Chromium at eight widths in light and dark themes. |
| Frontend Node 22 suite | Passed, 88 tests. |
| Frontend typecheck, ESLint, and Vite production build | Passed. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed. |
| `cargo test --workspace` | Passed. The `echo` crate ran 131 unit tests; ignored tests still require live hardware or cached models. |
| `scripts/verify-fixed-toggle.sh` | Passed. |
| `scripts/verify-first-run-readiness.sh` | Passed. |
| `scripts/verify-recording-limit.sh` | Passed. |
| `scripts/verify-transcribe-cli.sh` | Passed. |
| `cargo build --release` and `cargo run -p xtask` | Passed. |
| `target/release/echo-desktop --version` | Printed `echo-desktop 0.9.0`. |

The local host is macOS. Linux compilation, generated Debian and RPM dependency metadata, AppImage startup, and the real PipeWire/PulseAudio linkage remain CI proof boundaries. A specific Bluetooth device remains a Linux hardware proof boundary.
