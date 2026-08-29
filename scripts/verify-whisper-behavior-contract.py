#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import json
import subprocess
import tempfile
from pathlib import Path

from whisper_identity_v3 import strict_json_file, strict_json_loads
from whisper_v3_contract import (
    BEHAVIOR_PATH,
    IDENTITIES_PATH,
    validate_behavior,
    validate_current,
    verify_measured_inference_contract,
)

REPO_ROOT = Path(__file__).resolve().parent.parent


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
    behavior = validate_current(REPO_ROOT)
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
    head = git_output("rev-parse", "HEAD").strip()
    contract = strict_json_file(REPO_ROOT / IDENTITIES_PATH)["cases"][
        "inferenceContract"
    ]["input"]
    with tempfile.TemporaryDirectory() as temporary:
        contract_path = Path(temporary) / "contract.json"
        contract_path.write_text(json.dumps(contract), encoding="utf-8")
        verify_measured_inference_contract(
            repo_root=REPO_ROOT,
            measured_commit=head,
            contract_path=contract_path,
            model_sha256=contract["modelSha256"],
            vad_sha256=contract["vadSha256"],
            tuning=contract["tuning"],
        )
        for field, mutate in (
            (
                "behavior",
                lambda value: value["behavior"].update(projectionSha256="0" * 64),
            ),
            (
                "claim scope",
                lambda value: value.update(claimScope="product-stt-corpus-v2"),
            ),
        ):
            changed = copy.deepcopy(contract)
            mutate(changed)
            contract_path.write_text(json.dumps(changed), encoding="utf-8")
            try:
                verify_measured_inference_contract(
                    repo_root=REPO_ROOT,
                    measured_commit=head,
                    contract_path=contract_path,
                    model_sha256=contract["modelSha256"],
                    vad_sha256=contract["vadSha256"],
                    tuning=contract["tuning"],
                )
            except ValueError:
                pass
            else:
                raise AssertionError(f"changed {field} reused measured evidence")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate and enforce the Whisper inference behavior contract"
    )
    parser.add_argument("--base-ref")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    behavior = validate_current(REPO_ROOT)
    if args.self_test:
        self_test()
    if args.base_ref:
        enforce_base(args.base_ref, behavior)
    print("verify-whisper-behavior-contract: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
