#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
archive_path=${1:-}
scratch_dir=

"$repo_dir/scripts/verify-whisper-vulkan-runtime.sh" --self-test
"$repo_dir/scripts/verify-whisper-runtime-performance.py" --self-test
"$repo_dir/scripts/verify-whisper-runtime-performance.py" --verify \
  "$repo_dir/scripts/fixtures/whisper-runtime-performance/performance-runs.json" \
  "$repo_dir/scripts/fixtures/whisper-runtime-performance/performance-summary.json" \
  "$repo_dir/scripts/fixtures/whisper-runtime-performance/interleaved.tsv"

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

# The GPU runtime is published by hand under its own tag, so its catalogue pin
# is the one managed artefact CI never built. A hand-edited digest, a
# re-uploaded asset, or a deleted release would otherwise only surface as a
# failed download on a user's machine the first time they select GPU. The URL
# is read from the catalogue rather than repeated here, so a rotation is
# covered without touching this script.
vulkan_url=$(grep -o 'https://[^"]*echo-whisper-vulkan-runtime.tar.gz' \
  "$repo_dir/crates/echo/src/install/catalog.rs" | head -1)
test -n "$vulkan_url"
vulkan_dir=$(mktemp -d /tmp/echo-whisper-vulkan.XXXXXX)
trap 'rm -rf "${scratch_dir:-}" "$vulkan_dir"' EXIT
vulkan_archive="$vulkan_dir/echo-whisper-vulkan-runtime.tar.gz"
curl --fail --location --retry 3 --output "$vulkan_archive" "$vulkan_url"
ECHO_PINNED_VULKAN_ARCHIVE="$vulkan_archive" \
  cargo test -p echo --lib install::tests::pinned_vulkan_runtime_archive_installs \
  -- --ignored --exact

printf '%s\n' 'verify-whisper-runtime-archive: ok'
