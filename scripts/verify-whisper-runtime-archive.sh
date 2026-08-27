#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
archive_path=${1:-}
scratch_dir=

"$repo_dir/scripts/verify-whisper-vulkan-runtime.sh" --self-test
"$repo_dir/scripts/verify-whisper-runtime-performance.py" --self-test
"$repo_dir/scripts/verify-whisper-runtime-performance.py" --verify \
  "$repo_dir/.audit/pr16-1-evidence/performance-runs.json" \
  "$repo_dir/.audit/pr16-1-evidence/performance-summary.json" \
  "$repo_dir/.audit/pr16-1-evidence/interleaved.tsv"

if [ -z "$archive_path" ]; then
  scratch_dir=$(mktemp -d /tmp/echo-whisper-runtime.XXXXXX)
  trap 'rm -rf "$scratch_dir"' EXIT
  archive_path="$scratch_dir/whisper-bin-ubuntu-x64.tar.gz"
  curl --fail --location --retry 3 \
    --output "$archive_path" \
    https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-ubuntu-x64.tar.gz
else
  archive_dir=$(cd "$(dirname "$archive_path")" && pwd)
  archive_path="$archive_dir/$(basename "$archive_path")"
fi

test -f "$archive_path"
cd "$repo_dir"
ECHO_PINNED_WHISPER_ARCHIVE="$archive_path" \
  cargo test -p echo install::tests::pinned_whisper_runtime_archive_installs -- --ignored --exact
printf '%s\n' 'verify-whisper-runtime-archive: ok'
