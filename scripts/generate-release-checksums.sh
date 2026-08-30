#!/usr/bin/env bash
set -euo pipefail

TMPROOT=$(mktemp -d)
trap 'rm -rf "$TMPROOT"' EXIT

fail() {
    printf 'generate-release-checksums: %s\n' "$1" >&2
    return 1
}

collect_assets() {
    local publish=$1 require_manifest=$2 entry base allowed asset
    if [ ! -d "$publish" ]; then
        fail "no such publish directory: $publish"
        return 1
    fi

    shopt -s nullglob
    local debs=("$publish"/*.deb)
    local rpms=("$publish"/*.rpm)
    local appimages=("$publish"/*.AppImage)
    if [ "${#debs[@]}" -ne 1 ] || [ "${#rpms[@]}" -ne 1 ] ||
        [ "${#appimages[@]}" -ne 1 ] || [ ! -f "$publish/echo-desktop" ] ||
        [ -L "$publish/echo-desktop" ] || [ ! -f "$publish/echo-desktop.cdx.json" ] ||
        [ -L "$publish/echo-desktop.cdx.json" ] || [ ! -f "$publish/LICENSE-MIT" ] ||
        [ -L "$publish/LICENSE-MIT" ]; then
        fail "expected the MIT license, one deb, one rpm, one AppImage, echo-desktop, and its SBOM"
        return 1
    fi

    ASSETS=(
        "$(basename "${appimages[0]}")"
        "$(basename "${debs[0]}")"
        "LICENSE-MIT"
        "echo-desktop"
        "echo-desktop.cdx.json"
        "$(basename "${rpms[0]}")"
    )
    mapfile -d '' -t ASSETS < <(printf '%s\0' "${ASSETS[@]}" | LC_ALL=C sort -z)

    while IFS= read -r -d '' entry; do
        if [ ! -f "$entry" ] || [ -L "$entry" ]; then
            fail "staged entry is not a regular file: $(basename "$entry")"
            return 1
        fi
        base=$(basename "$entry")
        allowed=0
        for asset in "${ASSETS[@]}"; do
            if [ "$base" = "$asset" ]; then
                allowed=1
                break
            fi
        done
        if [ "$base" = SHA256SUMS ]; then
            allowed=1
        fi
        if [ "$allowed" -ne 1 ]; then
            fail "unexpected staged entry: $base"
            return 1
        fi
    done < <(find "$publish" -mindepth 1 -maxdepth 1 -print0)

    if [ "$require_manifest" -eq 1 ] && [ ! -f "$publish/SHA256SUMS" ]; then
        fail "SHA256SUMS is missing"
        return 1
    fi
}

write_manifest() {
    local publish=$1 temp_manifest
    collect_assets "$publish" 0 || return
    temp_manifest=$(mktemp "$publish/.SHA256SUMS.XXXXXXXX")
    if ! (cd "$publish" && sha256sum -- "${ASSETS[@]}") >"$temp_manifest"; then
        rm -f "$temp_manifest"
        return 1
    fi
    mv "$temp_manifest" "$publish/SHA256SUMS"
}

verify_manifest() {
    local publish=$1 expected
    collect_assets "$publish" 1 || return
    expected=$(mktemp "$TMPROOT/expected.XXXXXXXX")
    (cd "$publish" && sha256sum -- "${ASSETS[@]}") >"$expected"
    if ! cmp -s "$expected" "$publish/SHA256SUMS"; then
        rm -f "$expected"
        fail "SHA256SUMS does not match the sorted staged asset set"
        return 1
    fi
    rm -f "$expected"
    (cd "$publish" && sha256sum --check --strict SHA256SUMS)
}

self_test() {
    local root publish first
    root="$TMPROOT/self-test"
    publish="$root/publish"
    mkdir -p "$publish"
    printf 'appimage\n' >"$publish/echo_1.0.0_amd64.AppImage"
    printf 'deb\n' >"$publish/echo_1.0.0_amd64.deb"
    printf 'binary\n' >"$publish/echo-desktop"
    printf '{"bomFormat":"CycloneDX"}\n' >"$publish/echo-desktop.cdx.json"
    printf 'rpm\n' >"$publish/echo-1.0.0-1.x86_64.rpm"
    printf 'mit\n' >"$publish/LICENSE-MIT"

    write_manifest "$publish"
    first=$(sha256sum "$publish/SHA256SUMS" | cut -d' ' -f1)
    write_manifest "$publish"
    if [ "$(sha256sum "$publish/SHA256SUMS" | cut -d' ' -f1)" != "$first" ]; then
        fail "writing the same staged set changed SHA256SUMS"
        return 1
    fi
    verify_manifest "$publish" >/dev/null

    rm "$publish/echo-desktop.cdx.json"
    if verify_manifest "$publish" >/dev/null 2>&1; then
        fail "a staged set with no SBOM passed verification"
        return 1
    fi
    printf '{"bomFormat":"CycloneDX"}\n' >"$publish/echo-desktop.cdx.json"
    write_manifest "$publish"

    printf 'changed\n' >>"$publish/echo-desktop"
    if verify_manifest "$publish" >/dev/null 2>&1; then
        fail "a changed asset passed verification"
        return 1
    fi
    printf 'binary\n' >"$publish/echo-desktop"
    write_manifest "$publish"

    sed -n '1p' "$publish/SHA256SUMS" >>"$publish/SHA256SUMS"
    if verify_manifest "$publish" >/dev/null 2>&1; then
        fail "a duplicate checksum entry passed verification"
        return 1
    fi
    write_manifest "$publish"

    printf 'extra\n' >"$publish/extra.txt"
    if verify_manifest "$publish" >/dev/null 2>&1; then
        fail "an extra staged file passed verification"
        return 1
    fi
    rm "$publish/extra.txt" "$publish/echo_1.0.0_amd64.AppImage"
    if verify_manifest "$publish" >/dev/null 2>&1; then
        fail "a missing AppImage passed verification"
        return 1
    fi

    printf 'generate-release-checksums: self-test passed\n'
}

main() {
    case "${1:-}" in
        --self-test)
            if [ "$#" -ne 1 ]; then
                fail "usage: generate-release-checksums.sh --self-test"
                return 1
            fi
            self_test
            ;;
        --verify)
            if [ "$#" -ne 2 ]; then
                fail "usage: generate-release-checksums.sh --verify <publish-dir>"
                return 1
            fi
            verify_manifest "$2"
            ;;
        '')
            fail "usage: generate-release-checksums.sh <publish-dir> | --verify <publish-dir> | --self-test"
            return 1
            ;;
        *)
            if [ "$#" -ne 1 ]; then
                fail "usage: generate-release-checksums.sh <publish-dir>"
                return 1
            fi
            write_manifest "$1"
            ;;
    esac
}

main "$@"
