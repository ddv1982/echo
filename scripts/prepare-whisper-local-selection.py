#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from whisper_identity_v3 import verify_acceleration_set
from whisper_portable_selection import (
    CALIBRATION_FIXTURE_RELATIVE,
    CALIBRATION_FIXTURE_SOURCE,
    build_binding,
    canonical_digest,
    portable_execution_record,
    verify_legacy_exact_index,
    verify_portable_filesystem,
    verify_portable_selection,
)
from whisper_release_common import (
    read_json_strict,
    sha256_file,
    verify_contained_symlinks,
)


def prepare(
    *,
    runtime: Path,
    reusable_evidence: Path,
    echo_binary: Path,
    output: Path,
    package_type: str,
    version: str,
    commit: str,
) -> None:
    runtime = runtime.resolve()
    reusable = reusable_evidence.resolve() / "whisper-acceleration"
    echo_binary = echo_binary.resolve()
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    acceleration_set = read_json_strict(
        reusable / "acceleration-set.v3.json", "source acceleration set"
    )
    verify_acceleration_set(acceleration_set)
    subprocess.run(
        [
            str(Path(__file__).with_name("verify-whisper-vulkan-runtime.sh")),
            "--verify",
            "--require-vulkan",
            str(runtime),
        ],
        check=True,
    )
    output.mkdir(parents=True)
    shutil.copytree(runtime, output / "runtime", symlinks=True)
    fixture = output / CALIBRATION_FIXTURE_RELATIVE
    fixture.parent.mkdir()
    shutil.copy2(CALIBRATION_FIXTURE_SOURCE, fixture)
    portable = {
        "schemaVersion": 1,
        "executionArtifact": portable_execution_record(output / "runtime"),
        "inferenceContracts": acceleration_set["inferenceContracts"],
        "calibrationFixture": {
            "relativePath": CALIBRATION_FIXTURE_RELATIVE,
            "sha256": sha256_file(fixture),
        },
    }
    legacy = {
        "schemaVersion": 1,
        "executionArtifactId": portable["executionArtifact"]["id"],
        "records": [],
    }
    verify_portable_selection(portable)
    verify_legacy_exact_index(legacy, portable)
    binding = build_binding(
        portable=portable,
        legacy=legacy,
        source_acceleration_set_sha256=canonical_digest(acceleration_set),
        package_type=package_type,
        version=version,
        echo_commit=commit,
        echo_binary_sha256=sha256_file(echo_binary),
    )
    for name, value in (
        ("portable-selection.v1.json", portable),
        ("legacy-exact-index.v1.json", legacy),
        ("portable-selection-binding.v1.json", binding),
    ):
        (output / name).write_text(
            json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    verify_contained_symlinks(output)
    verify_portable_filesystem(output)


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        runtime = Path(temporary) / "runtime"
        runtime.mkdir()
        (runtime / "whisper-cli").write_bytes(b"runtime")
        (runtime / "echo-whisper-runtime-probe").write_bytes(b"probe")
        (runtime / "libwhisper.so").write_bytes(b"library")
        (runtime / "build-receipt.json").write_text(
            json.dumps({"artifactId": "1" * 64}), encoding="utf-8"
        )
        first = portable_execution_record(runtime)
        assert len(first["id"]) == 64
        (runtime / "echo-whisper-runtime-probe").write_bytes(b"changed")
        second = portable_execution_record(runtime)
        assert first["id"] != second["id"]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--reusable-evidence", type=Path)
    parser.add_argument("--echo-binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--package-type", choices=("deb", "rpm"), default="deb")
    parser.add_argument("--version")
    parser.add_argument("--commit")
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
            return 0
        if any(
            value is None
            for value in (
                args.runtime,
                args.reusable_evidence,
                args.echo_binary,
                args.output,
                args.version,
                args.commit,
            )
        ):
            parser.error("package preparation requires every path, version, and commit")
        prepare(
            runtime=args.runtime,
            reusable_evidence=args.reusable_evidence,
            echo_binary=args.echo_binary,
            output=args.output,
            package_type=args.package_type,
            version=args.version,
            commit=args.commit,
        )
    except (
        KeyError,
        OSError,
        TypeError,
        ValueError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"prepare-whisper-local-selection: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
