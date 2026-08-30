#!/usr/bin/env bash
set -euo pipefail

archive_root=echo-evidence-2026-08-30

usage() {
  printf '%s\n' \
    "usage: $0 build <source-commit> <output.tar.gz>" \
    "       $0 verify <archive.tar.gz> <sha256>"
}

sha256_file() {
  sha256sum "$1" | cut -d' ' -f1
}

selected_paths() {
  git ls-tree -r --name-only "$1" | LC_ALL=C sort | awk '
    /^\.audit\// ||
    /^docs\/plans\/(0[1-9]|1[0-7])-/ ||
    /^docs\/qa\/(QA_GATES\.md|phase-14-whisper-acceleration-manual-test-plan\.md)$/ ||
    /^docs\/qa\/(report|runs)\// ||
    /^scripts\/(prepare-whisper-local-selection\.py|verify-pr16-2-evidence\.py|verify-whisper-acceleration\.sh|verify-whisper-behavior-contract\.py|verify-whisper-invalidation\.py|verify-whisper-local-selection\.py|whisper_portable_selection\.py)$/
  '
}

build_archive() {
  local source_commit=$1
  local output=$2
  local source_tree staging content file relative digest size
  source_tree=$(git rev-parse --verify "${source_commit}^{commit}")
  staging=$(mktemp -d /tmp/echo-history-build.XXXXXX)
  trap 'rm -rf "${staging:-}"' EXIT
  content="$staging/$archive_root/content"
  mkdir -p "$content"

  mapfile -t paths < <(selected_paths "$source_tree")
  if [ "${#paths[@]}" -eq 0 ]; then
    printf '%s\n' 'history archive selection is empty' >&2
    exit 1
  fi
  git archive "$source_tree" -- "${paths[@]}" | tar -xf - -C "$content"

  printf '%s\n' "$source_tree" > "$staging/$archive_root/SOURCE_COMMIT"
  printf 'path\tbytes\tsha256\n' > "$staging/$archive_root/INVENTORY.tsv"
  while IFS= read -r -d '' file; do
    relative=${file#"$content/"}
    size=$(stat -c '%s' "$file")
    digest=$(sha256_file "$file")
    printf '%s\t%s\t%s\n' "$relative" "$size" "$digest" \
      >> "$staging/$archive_root/INVENTORY.tsv"
  done < <(find "$content" -type f -print0 | LC_ALL=C sort -z)

  mkdir -p "$(dirname "$output")"
  tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
    --pax-option=delete=atime,delete=ctime \
    -cf - -C "$staging" "$archive_root" | gzip -9 -n > "$output"
  printf '%s  %s\n' "$(sha256_file "$output")" "$(basename "$output")" \
    > "$output.sha256"
}

verify_archive() {
  local archive=$1
  local expected=$2
  local actual staging member file path bytes digest expected_paths actual_paths
  actual=$(sha256_file "$archive")
  if [ "$actual" != "$expected" ]; then
    printf 'archive digest differs: expected %s, got %s\n' "$expected" "$actual" >&2
    exit 1
  fi

  while IFS= read -r member; do
    case "$member" in
      "$archive_root"|"$archive_root"/*) ;;
      *) printf 'archive member escapes root: %s\n' "$member" >&2; exit 1 ;;
    esac
    case "/$member/" in
      */../*) printf 'archive member traverses a parent: %s\n' "$member" >&2; exit 1 ;;
    esac
  done < <(tar -tzf "$archive")

  staging=$(mktemp -d /tmp/echo-history-verify.XXXXXX)
  trap 'rm -rf "${staging:-}"' EXIT
  tar -xzf "$archive" -C "$staging"
  test -s "$staging/$archive_root/SOURCE_COMMIT"
  test -s "$staging/$archive_root/INVENTORY.tsv"
  expected_paths="$staging/expected-paths"
  actual_paths="$staging/actual-paths"
  tail -n +2 "$staging/$archive_root/INVENTORY.tsv" | cut -f1 > "$expected_paths"
  find "$staging/$archive_root/content" -type f -printf '%P\n' \
    | LC_ALL=C sort > "$actual_paths"
  cmp "$expected_paths" "$actual_paths"
  while IFS=$'\t' read -r path bytes digest; do
    [ "$path" = path ] && continue
    file="$staging/$archive_root/content/$path"
    test -f "$file"
    test "$(stat -c '%s' "$file")" = "$bytes"
    test "$(sha256_file "$file")" = "$digest"
  done < "$staging/$archive_root/INVENTORY.tsv"
  printf 'history archive verified: %s\n' "$actual"
}

case ${1:-} in
  build)
    [ "$#" -eq 3 ] || { usage >&2; exit 2; }
    build_archive "$2" "$3"
    verify_archive "$3" "$(sha256_file "$3")"
    ;;
  verify)
    [ "$#" -eq 3 ] || { usage >&2; exit 2; }
    verify_archive "$2" "$3"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
