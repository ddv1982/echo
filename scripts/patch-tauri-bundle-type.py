#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import shutil
import sys
import tempfile
from pathlib import Path

from whisper_release_common import BUNDLE_MARKER, BUNDLE_TOKENS, bundle_variant


def patch(source: Path, destination: Path, bundle_type: str) -> None:
    if destination.exists():
        raise ValueError(f"destination already exists: {destination}")
    contents = source.read_bytes()
    patched = bundle_variant(contents, bundle_type)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.write_bytes(patched)
    os.chmod(destination, source.stat().st_mode)


def verify(source: Path, candidate: Path, bundle_type: str) -> None:
    expected = bundle_variant(source.read_bytes(), bundle_type)
    if candidate.read_bytes() != expected:
        raise ValueError("candidate differs outside the Tauri bundle marker")


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        source = root / "source"
        source.write_bytes(b"before" + BUNDLE_MARKER + b"after")
        for bundle_type, token in BUNDLE_TOKENS.items():
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
    parser.add_argument("--type", choices=tuple(BUNDLE_TOKENS))
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
