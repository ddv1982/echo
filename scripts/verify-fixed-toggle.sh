#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

npm run build --prefix frontend
npm run lint --prefix frontend
(
  cd frontend
  npx -y node@22 node_modules/vitest/vitest.mjs run
)

cargo clippy --workspace --all-targets -- -D warnings
cargo test -p echo-desktop
rustfmt --edition 2021 --check \
  crates/echo-core/src/config.rs \
  crates/echo/src/hotkey.rs \
  crates/echo/src/rec.rs \
  crates/echo/src/status.rs \
  src-tauri/src/main.rs \
  src-tauri/src/portal_runtime_tests.rs \
  src-tauri/tests/cli_rec.rs

if cargo tree -p echo | rg -q 'evdev'; then
  echo 'evdev remains in the echo dependency tree' >&2
  exit 1
fi

forbidden_matches="$(
  rg -n 'hold_key|toggle_shortcut|ECHO_HOLD_KEY|ECHO_TOGGLE_SHORTCUT|push-to-talk|rec --hold|evdev' \
    crates src-tauri frontend/src README.md packaging \
    --glob '!**/*.test.*' \
    --glob '!src-tauri/tests/**' || true
)"
forbidden_total="$(printf '%s\n' "$forbidden_matches" | rg -c '.' || true)"
obsolete_fixture_total="$(printf '%s\n' "$forbidden_matches" | rg -c '^crates/echo-core/src/config.rs:' || true)"
if [[ "$forbidden_total" != 3 || "$obsolete_fixture_total" != 3 ]]; then
  printf '%s\n' "$forbidden_matches" >&2
  echo 'unexpected removed-shortcut reference remains' >&2
  exit 1
fi

set +e
cli_output="$(./target/debug/echo-desktop rec --hold 2>&1)"
cli_status=$?
set -e
if [[ $cli_status -ne 2 || "$cli_output" != 'usage: echo-desktop rec --once|--toggle' ]]; then
  printf '%s\n' "$cli_output" >&2
  echo "removed rec --hold returned $cli_status" >&2
  exit 1
fi

git diff --check
