#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

(
  cd frontend
  npx -y node@22 node_modules/vitest/vitest.mjs run src/tauri.test.ts src/App.test.tsx
)

cargo test -p echo-core recording::tests::
cargo test -p echo audio::tests::
cargo test -p echo rec::tests::
cargo test -p echo status::tests::
cargo test -p echo-desktop settings_tests::recording
cargo test -p echo-desktop active_recording_limit_snapshot

stale_production="$(
  rg -n \
    'MAX_RECORD_SECONDS|maxRecordSeconds|RECORD_SECOND_PRESETS|recordSeconds: \{ value: null, effective: 3|Math\.min\(60' \
    crates/echo/src/rec.rs \
    src-tauri/src/main.rs \
    frontend/src/App.tsx \
    frontend/src/tauri.ts \
    frontend/src/api/previewDesktopApi.ts \
    frontend/src/types.ts || true
)"
if [[ -n "$stale_production" ]]; then
  printf '%s\n' "$stale_production" >&2
  echo 'stale recording-limit policy remains in production code' >&2
  exit 1
fi

if rg -n 'records for three seconds|up to 60 seconds|after 60 seconds' README.md; then
  echo 'README still describes the old recording limit' >&2
  exit 1
fi

git diff --check
