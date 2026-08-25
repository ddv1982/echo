#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import shutil
import sys
import tempfile
from pathlib import Path

UNKNOWN = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
TOKENS = {
    "deb": b"__TAURI_BUNDLE_TYPE_VAR_DEB",
    "rpm": b"__TAURI_BUNDLE_TYPE_VAR_RPM",
    "appimage": b"__TAURI_BUNDLE_TYPE_VAR_APP",
}


def patch(source: Path, destination: Path, bundle_type: str) -> None:
    if destination.exists():
        raise ValueError(f"destination already exists: {destination}")
    contents = source.read_bytes()
    if contents.count(UNKNOWN) != 1:
        raise ValueError("source must contain exactly one unknown Tauri bundle marker")
    token = TOKENS[bundle_type]
    if len(token) != len(UNKNOWN):
        raise ValueError("Tauri bundle marker lengths differ")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.write_bytes(contents.replace(UNKNOWN, token, 1))
    os.chmod(destination, source.stat().st_mode)


def verify(source: Path, candidate: Path, bundle_type: str) -> None:
    expected = source.read_bytes()
    if expected.count(UNKNOWN) != 1:
        raise ValueError("source must contain exactly one unknown Tauri bundle marker")
    expected = expected.replace(UNKNOWN, TOKENS[bundle_type], 1)
    if candidate.read_bytes() != expected:
        raise ValueError("candidate differs outside the Tauri bundle marker")


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        source = root / "source"
        source.write_bytes(b"before" + UNKNOWN + b"after")
        for bundle_type, token in TOKENS.items():
            output = root / bundle_type
            patch(source, output, bundle_type)
            assert output.read_bytes() == b"before" + token + b"after"
            verify(source, output, bundle_type)
    print("patch-tauri-bundle-type: self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Create or verify an exact Tauri bundle-type ELF variant"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--type", choices=tuple(TOKENS))
    parser.add_argument("--source", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.type is None or args.source is None or args.output is None:
            parser.error("--type, --source, and --output are required")
        elif args.verify:
            verify(args.source, args.output, args.type)
        else:
            patch(args.source, args.output, args.type)
    except (OSError, ValueError) as error:
        print(f"patch-tauri-bundle-type: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
