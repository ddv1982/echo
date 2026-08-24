#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
archive_path=${1:-}
scratch_dir=

if [ -z "$archive_path" ]; then
  scratch_dir=$(mktemp -d /tmp/echo-whisper-runtime.XXXXXX)
  trap 'rm -rf "$scratch_dir"' EXIT
  archive_path="$scratch_dir/whisper-bin-ubuntu-x64.tar.gz"
  curl --fail --location --retry 3 \
    --output "$archive_path" \
    https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-ubuntu-x64.tar.gz
fi

test -f "$archive_path"
cd "$repo_dir"
ECHO_PINNED_WHISPER_ARCHIVE="$archive_path" \
  cargo test -p echo install::tests::pinned_whisper_runtime_archive_installs -- --ignored --exact
printf '%s\n' 'verify-whisper-runtime-archive: ok'
