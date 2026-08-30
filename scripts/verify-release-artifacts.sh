#!/usr/bin/env bash
# Read every file about to be attached to a GitHub Release and prove it
# carries the binary CI just built.
#
# 0.12.6 published a deb with no acceleration payload and nothing objected,
# because the only check that ran on a tag inspected a hand-built draft
# instead of the artefacts being attached. This opens the artefacts.
#
# Usage:
#   scripts/verify-release-artifacts.sh <publish-dir>
#   scripts/verify-release-artifacts.sh --rpm-fallback <package.rpm>
#   scripts/verify-release-artifacts.sh --self-test
set -euo pipefail

DESKTOP_ENTRY=io.github.ddv1982.echo.desktop
BINARY=usr/bin/echo-desktop
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

# One root for every scratch tree, so cleanup cannot miss one. Allocating
# inside a command substitution loses any variable the subshell sets, which
# is why the root is created once up front rather than tracked in an array.
TMPROOT=$(mktemp -d)
trap 'rm -rf "$TMPROOT"' EXIT

scratch_dir() {
    mktemp -d "$TMPROOT/XXXXXXXX"
}

fail() {
    printf 'verify-release-artifacts: %s\n' "$1" >&2
    return 1
}

# Tauri rewrites a fixed-length marker into the binary as it packages, so the
# canonical build carries _UNK while the copy inside the deb carries _DEB and
# the rpm's carries _RPM. Raw digests therefore never match across the three.
#
# Build the exact variant each package should contain and compare against that,
# rather than normalising the token away. Erasing it would accept a deb that
# carried the rpm's marker. This mirrors the invariant
# scripts/patch-tauri-bundle-type.py already models.
MARKER=__TAURI_BUNDLE_TYPE_VAR

variant_digest() {
    local source=$1 token=$2 markers
    markers=$(grep -c "${MARKER}_UNK" "$source" 2>/dev/null || true)
    if [ "${markers:-0}" -eq 0 ]; then
        fail "$source carries no ${MARKER}_UNK marker to substitute"
        return 1
    fi
    perl -0777 -pe "s/\Q${MARKER}\E_UNK/${MARKER}_${token}/g" "$source" \
        | sha256sum | cut -d' ' -f1
}

# An installed tree is correct when it holds the launcher, the desktop entry
# that points at it, and the binary CI produced. The digest is what separates
# "a build" from "this build".
check_tree() {
    local label=$1 tree=$2 expected=$3 member found
    for member in "$BINARY" "usr/share/applications/$DESKTOP_ENTRY"; do
        if [ ! -f "$tree/$member" ]; then
            # head closing the pipe early would otherwise kill this under
            # pipefail and lose the message the operator actually needs.
            { find "$tree" -type f || true; } | head -50 >&2 || true
            fail "$label is missing $member"
            return 1
        fi
    done
    found=$(sha256sum "$tree/$BINARY" | cut -d' ' -f1)
    if [ "$found" != "$expected" ]; then
        fail "$label ships a binary CI did not build: $found, expected $expected"
        return 1
    fi
}

check_appimage_tree() {
    local label=$1 tree=$2 exec_value
    if [ ! -f "$tree/$BINARY" ] || [ ! -x "$tree/$BINARY" ]; then
        fail "$label is missing executable $BINARY"
        return 1
    fi
    if [ ! -f "$tree/$DESKTOP_ENTRY" ]; then
        fail "$label is missing $DESKTOP_ENTRY"
        return 1
    fi
    if [ ! -f "$tree/AppRun" ] || [ ! -x "$tree/AppRun" ]; then
        fail "$label is missing executable AppRun"
        return 1
    fi
    exec_value=$(sed -n 's/^Exec=//p' "$tree/$DESKTOP_ENTRY")
    if [ "$exec_value" != echo-desktop ]; then
        fail "$label desktop entry has Exec=$exec_value, expected Exec=echo-desktop"
        return 1
    fi
}

# Ubuntu's rpm2cpio has been seen exiting nonzero on an archive it had already
# emitted in full, which is why 7z is a fallback rather than an alternative.
# Pass force=1 to take that fallback deliberately and keep it from rotting.
extract_rpm() {
    local package=$1 into=$2 force=${3:-0} converted staging payload
    package=$(readlink -f "$package")
    mkdir -p "$into"
    if [ "$force" != 1 ] &&
        command -v rpm2cpio >/dev/null 2>&1 &&
        command -v cpio >/dev/null 2>&1; then
        converted=$(mktemp "$TMPROOT/rpm2cpio.XXXXXXXX")
        if rpm2cpio "$package" >"$converted" 2>/dev/null; then
            (cd "$into" && cpio -idmu --quiet <"$converted")
            rm -f "$converted"
            return 0
        fi
        rm -f "$converted"
    fi
    if ! command -v 7z >/dev/null 2>&1; then
        fail "no working rpm2cpio and no 7z to read $package"
        return 1
    fi
    # p7zip unwraps the rpm to a cpio, which cpio then unpacks.
    staging=$(scratch_dir)
    7z x -y -o"$staging" "$package" >/dev/null
    payload=$(find "$staging" -maxdepth 1 -type f | head -1)
    if [ -z "$payload" ]; then
        fail "7z found no cpio archive inside $package"
        return 1
    fi
    (cd "$into" && cpio -idmu --quiet <"$payload")
}

# The release runner has rpm2cpio, so the fallback would never run on the path
# that matters. This exercises it on the real package instead.
verify_rpm_fallback() {
    local package=$1 work
    if [ ! -f "$package" ]; then
        fail "no such package: $package"
        return 1
    fi
    work=$(scratch_dir)
    extract_rpm "$package" "$work" 1
    if [ ! -f "$work/$BINARY" ]; then
        fail "the 7z fallback read no $BINARY out of $(basename "$package")"
        return 1
    fi
    printf 'the 7z rpm fallback reads %s\n' "$(basename "$package")"
}

verify_publish_dir() {
    local publish=$1 work deb_expected rpm_expected package_version version_output
    shopt -s nullglob
    local debs=("$publish"/*.deb)
    local rpms=("$publish"/*.rpm)
    local appimages=("$publish"/*.AppImage)

    "$SCRIPT_DIR/generate-release-checksums.sh" --verify "$publish" >/dev/null || return
    deb_expected=$(variant_digest "$publish/echo-desktop" DEB) || return
    rpm_expected=$(variant_digest "$publish/echo-desktop" RPM) || return

    work=$(scratch_dir)

    dpkg-deb -x "${debs[0]}" "$work/deb" || return
    check_tree "$(basename "${debs[0]}")" "$work/deb" "$deb_expected" || return

    extract_rpm "${rpms[0]}" "$work/rpm" || return
    check_tree "$(basename "${rpms[0]}")" "$work/rpm" "$rpm_expected" || return

    # linuxdeploy patches the relocated executable, so compare its behavior and
    # final desktop layout instead of comparing it byte-for-byte with the raw
    # CI binary.
    install -m 0755 "${appimages[0]}" "$work/image.AppImage"
    package_version=$(python3 -c \
        'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["workspace"]["package"]["version"])' \
        "$SCRIPT_DIR/../Cargo.toml")
    version_output=$(cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 ./image.AppImage --version) || return
    if [ "$version_output" != "echo-desktop $package_version" ]; then
        fail "$(basename "${appimages[0]}") reported $version_output, expected echo-desktop $package_version"
        return 1
    fi
    (cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 ./image.AppImage --appimage-extract >/dev/null) || return
    check_appimage_tree "$(basename "${appimages[0]}")" "$work/squashfs-root" || return

    printf 'verified staged release assets for CI build %s\n' \
        "$(sha256sum "$publish/echo-desktop" | cut -d' ' -f1)"
}

self_test() {
    local root tree image_tree canonical expected
    root=$(scratch_dir)
    tree="$root/tree"
    mkdir -p "$tree/usr/bin" "$tree/usr/share/applications"
    # Shaped like a real build: one bundle marker, which the packager rewrites.
    canonical="$root/echo-desktop"
    printf 'head%s_UNKtail' "$MARKER" >"$canonical"
    printf 'head%s_DEBtail' "$MARKER" >"$tree/$BINARY"
    printf 'Exec=/usr/bin/echo-desktop\n' >"$tree/usr/share/applications/$DESKTOP_ENTRY"
    expected=$(variant_digest "$canonical" DEB)

    if variant_digest "$tree/$BINARY" DEB >/dev/null 2>&1; then
        echo "self-test: an already-packaged binary was accepted as canonical" >&2
        return 1
    fi

    # The case normalising the marker away would have missed.
    if check_tree wrong-variant "$tree" "$(variant_digest "$canonical" RPM)" 2>/dev/null; then
        echo "self-test: a package carrying another type's marker was accepted" >&2
        return 1
    fi

    check_tree good "$tree" "$expected" ||
        { echo "self-test: a correct tree was rejected" >&2; return 1; }

    if check_tree stale "$tree" "0000000000000000000000000000000000000000000000000000000000000000" 2>/dev/null; then
        echo "self-test: a binary CI did not build was accepted" >&2
        return 1
    fi

    rm "$tree/usr/share/applications/$DESKTOP_ENTRY"
    if check_tree no-entry "$tree" "$expected" 2>/dev/null; then
        echo "self-test: a tree with no desktop entry was accepted" >&2
        return 1
    fi

    rm "$tree/$BINARY"
    if check_tree no-binary "$tree" "$expected" 2>/dev/null; then
        echo "self-test: a tree with no binary was accepted" >&2
        return 1
    fi

    image_tree="$root/image-tree"
    mkdir -p "$image_tree/usr/bin"
    printf '#!/bin/sh\n' >"$image_tree/usr/bin/echo-desktop"
    chmod +x "$image_tree/usr/bin/echo-desktop"
    printf '#!/bin/sh\n' >"$image_tree/AppRun"
    chmod +x "$image_tree/AppRun"
    printf '[Desktop Entry]\nExec=echo-desktop\n' >"$image_tree/$DESKTOP_ENTRY"
    check_appimage_tree good-appimage "$image_tree" ||
        { echo "self-test: a valid AppImage tree was rejected" >&2; return 1; }
    chmod -x "$image_tree/usr/bin/echo-desktop"
    if check_appimage_tree non-executable "$image_tree" 2>/dev/null; then
        echo "self-test: a non-executable AppImage binary was accepted" >&2
        return 1
    fi
    chmod +x "$image_tree/usr/bin/echo-desktop"
    printf '[Desktop Entry]\nExec=/usr/bin/echo-desktop\n' >"$image_tree/$DESKTOP_ENTRY"
    if check_appimage_tree wrong-exec "$image_tree" 2>/dev/null; then
        echo "self-test: an AppImage desktop entry with the wrong Exec was accepted" >&2
        return 1
    fi

    local publish="$root/publish"
    mkdir -p "$publish"
    printf 'head%s_UNKtail' "$MARKER" >"$publish/echo-desktop"
    if verify_publish_dir "$publish" 2>/dev/null; then
        echo "self-test: a publish set with no packages was accepted" >&2
        return 1
    fi

    if verify_rpm_fallback "$root/absent.rpm" 2>/dev/null; then
        echo "self-test: a missing rpm was accepted" >&2
        return 1
    fi
    printf 'not an rpm' >"$root/bogus.rpm"
    if verify_rpm_fallback "$root/bogus.rpm" >/dev/null 2>&1; then
        echo "self-test: a file that is not an rpm was accepted" >&2
        return 1
    fi

    echo "verify-release-artifacts: self-test passed"
}

main() {
    if [ "${1:-}" = "--self-test" ]; then
        self_test
        return
    fi
    if [ "${1:-}" = "--rpm-fallback" ]; then
        if [ "$#" -ne 2 ]; then
            fail "usage: verify-release-artifacts.sh --rpm-fallback <package.rpm>"
            return 1
        fi
        verify_rpm_fallback "$2"
        return
    fi
    if [ "$#" -ne 1 ]; then
        fail "usage: verify-release-artifacts.sh <publish-dir> | --rpm-fallback <package.rpm> | --self-test"
        return 1
    fi
    verify_publish_dir "$1"
}

main "$@"
