#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
scratch_dir=$(mktemp -d /tmp/echo-first-run.XXXXXX)
trap 'rm -rf "$scratch_dir"' EXIT

export ECHO_CONFIG_DIR="$scratch_dir/config"
export ECHO_DATA_DIR="$scratch_dir/data"
export ECHO_MODEL_DIR="$scratch_dir/models"
export PATH="$scratch_dir/bin:$PATH"
mkdir -p "$ECHO_CONFIG_DIR" "$ECHO_DATA_DIR" "$ECHO_MODEL_DIR" "$scratch_dir/bin"

cd "$repo_dir"
cargo test -p echo install::
cargo test -p echo microphone::tests
cargo test -p echo stt::runtime::tests

cd "$repo_dir/frontend"
npx -y node@22 node_modules/vitest/vitest.mjs run src/App.test.tsx

test ! -e "$ECHO_CONFIG_DIR/config.json"
test ! -e "$ECHO_DATA_DIR/history.json"
printf '%s\n' 'verify-first-run-readiness: ok'
