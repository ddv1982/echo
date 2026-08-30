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
#   scripts/verify-release-artifacts.sh --self-test
set -euo pipefail

DESKTOP_ENTRY=io.github.ddv1982.echo.desktop
BINARY=usr/bin/echo-desktop

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

# An installed tree is correct when it holds the launcher, the desktop entry
# that points at it, and the exact binary CI produced. The digest is what
# separates "a build" from "this build".
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

extract_rpm() {
    local package=$1 into=$2
    package=$(readlink -f "$package")
    mkdir -p "$into"
    if command -v rpm2cpio >/dev/null 2>&1; then
        (cd "$into" && rpm2cpio "$package" | cpio -idmu --quiet)
    elif command -v 7z >/dev/null 2>&1; then
        # p7zip unwraps the rpm to a cpio, which cpio then unpacks. This is
        # the same fallback the packaging smoke test uses.
        local staging="$into/.7z"
        mkdir -p "$staging"
        7z x -y -o"$staging" "$package" >/dev/null
        local payload
        payload=$(find "$staging" -maxdepth 1 -type f | head -1)
        (cd "$into" && cpio -idmu --quiet <"$payload")
        rm -rf "$staging"
    else
        fail "no rpm2cpio and no 7z to read $package"
        return 1
    fi
}

verify_publish_dir() {
    local publish=$1 work expected
    shopt -s nullglob
    local debs=("$publish"/*.deb)
    local rpms=("$publish"/*.rpm)
    local appimages=("$publish"/*.AppImage)

    if [ "${#debs[@]}" -ne 1 ] || [ "${#rpms[@]}" -ne 1 ] || [ ! -f "$publish/echo-desktop" ]; then
        ls -l "$publish" >&2
        fail "expected one deb, one rpm, and the binary; found ${#debs[@]} deb and ${#rpms[@]} rpm"
        return 1
    fi
    expected=$(sha256sum "$publish/echo-desktop" | cut -d' ' -f1)

    work=$(scratch_dir)

    dpkg-deb -x "${debs[0]}" "$work/deb"
    check_tree "$(basename "${debs[0]}")" "$work/deb" "$expected"

    extract_rpm "${rpms[0]}" "$work/rpm"
    check_tree "$(basename "${rpms[0]}")" "$work/rpm" "$expected"

    # The AppImage relocates and patches its binary, so its digest is
    # legitimately its own. Presence is what can be asserted there.
    if [ "${#appimages[@]}" -gt 0 ]; then
        (cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 "$(readlink -f "${appimages[0]}")" --appimage-extract >/dev/null)
        local member
        for member in "$work/squashfs-root/$BINARY" "$work/squashfs-root/$DESKTOP_ENTRY"; do
            if [ ! -e "$member" ]; then
                find "$work/squashfs-root" -maxdepth 3 >&2 || true
                fail "the AppImage is missing ${member#"$work/squashfs-root/"}"
                return 1
            fi
        done
    fi

    printf 'every published artefact carries CI build %s\n' "$expected"
}

self_test() {
    local root tree expected
    root=$(scratch_dir)
    tree="$root/tree"
    mkdir -p "$tree/usr/bin" "$tree/usr/share/applications"
    printf 'the binary CI built' >"$tree/$BINARY"
    printf 'Exec=/usr/bin/echo-desktop\n' >"$tree/usr/share/applications/$DESKTOP_ENTRY"
    expected=$(sha256sum "$tree/$BINARY" | cut -d' ' -f1)

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

    local publish="$root/publish"
    mkdir -p "$publish"
    printf 'the binary CI built' >"$publish/echo-desktop"
    if verify_publish_dir "$publish" 2>/dev/null; then
        echo "self-test: a publish set with no packages was accepted" >&2
        return 1
    fi

    echo "verify-release-artifacts: self-test passed"
}

main() {
    if [ "${1:-}" = "--self-test" ]; then
        self_test
        return
    fi
    if [ "$#" -ne 1 ]; then
        fail "usage: verify-release-artifacts.sh <publish-dir> | --self-test"
        return 1
    fi
    verify_publish_dir "$1"
}

main "$@"
