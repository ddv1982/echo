#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
from pathlib import Path

from whisper_identity_v3 import (
    canonical_json_bytes,
    sha256_bytes,
    strict_json_file,
    strict_json_loads,
    verify_fixture,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
BEHAVIOR_PATH = Path("crates/echo/tests/fixtures/whisper-behavior-v3.json")
IDENTITIES_PATH = Path("crates/echo/tests/fixtures/whisper-v3-identities.json")
BEHAVIOR_FIELDS = {"projection", "projectionSha256", "schemaVersion", "watchedPaths"}


def validate_behavior(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != BEHAVIOR_FIELDS:
        raise ValueError("behavior contract has unknown or missing fields")
    if value["schemaVersion"] != 3:
        raise ValueError("behavior contract has the wrong schema version")
    projection = value["projection"]
    if not isinstance(projection, dict) or set(projection) != {
        "decode",
        "launch",
        "receipt",
        "recovery",
        "telemetry",
    }:
        raise ValueError("behavior projection has unknown or missing sections")
    expected = sha256_bytes(canonical_json_bytes(projection))
    if value["projectionSha256"] != expected:
        raise ValueError("behavior projection digest differs from its canonical values")
    watched = value["watchedPaths"]
    if (
        not isinstance(watched, list)
        or not watched
        or watched != sorted(set(watched))
        or not all(isinstance(path, str) and path.endswith(".rs") for path in watched)
    ):
        raise ValueError("behavior watched paths are not sorted, unique Rust paths")
    missing = [path for path in watched if not (REPO_ROOT / path).is_file()]
    if missing:
        raise ValueError(f"behavior watched paths do not exist: {missing}")
    return value


def validate_current() -> dict[str, object]:
    behavior = validate_behavior(strict_json_file(REPO_ROOT / BEHAVIOR_PATH))
    identities = verify_fixture(REPO_ROOT / IDENTITIES_PATH)
    contract_digest = identities["cases"]["inferenceContract"]["input"]["behavior"][
        "projectionSha256"
    ]
    if contract_digest != behavior["projectionSha256"]:
        raise ValueError("inference contract does not bind the behavior projection")
    return behavior


def enforce_changed_paths(
    changed_paths: set[str],
    old_behavior: dict[str, object],
    new_behavior: dict[str, object],
) -> None:
    watched = set(old_behavior["watchedPaths"]) | set(new_behavior["watchedPaths"])
    changed_behavior = sorted(changed_paths & watched)
    if (
        changed_behavior
        and old_behavior["projectionSha256"] == new_behavior["projectionSha256"]
    ):
        paths = ", ".join(changed_behavior)
        raise ValueError(
            "Whisper inference behavior changed without a new projection digest: "
            f"{paths}"
        )


def git_output(*arguments: str) -> str:
    return subprocess.run(
        ["git", *arguments],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def enforce_base(base_ref: str, behavior: dict[str, object]) -> None:
    merge_base = git_output("merge-base", base_ref, "HEAD").strip()
    changed = set(
        filter(
            None, git_output("diff", "--name-only", f"{merge_base}...HEAD").splitlines()
        )
    )
    try:
        old_raw = git_output("show", f"{merge_base}:{BEHAVIOR_PATH.as_posix()}")
    except subprocess.CalledProcessError:
        return
    old_behavior = validate_behavior(strict_json_loads(old_raw))
    enforce_changed_paths(changed, old_behavior, behavior)


def self_test() -> None:
    behavior = validate_current()
    same = dict(behavior)
    changed = dict(behavior)
    changed["projectionSha256"] = "0" * 64
    watched_path = behavior["watchedPaths"][0]
    enforce_changed_paths({"Cargo.toml"}, behavior, same)
    enforce_changed_paths({watched_path}, behavior, changed)
    try:
        enforce_changed_paths({watched_path}, behavior, same)
    except ValueError:
        pass
    else:
        raise AssertionError("watched behavior change reused the old projection digest")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate and enforce the Whisper inference behavior contract"
    )
    parser.add_argument("--base-ref")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    behavior = validate_current()
    if args.self_test:
        self_test()
    if args.base_ref:
        enforce_base(args.base_ref, behavior)
    print("verify-whisper-behavior-contract: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
