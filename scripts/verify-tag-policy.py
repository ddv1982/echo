#!/usr/bin/env python3
import argparse
import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
FULL_SHA = re.compile(r"[0-9a-f]{40}")


class PolicyError(ValueError):
    pass


def read_runs(path):
    try:
        document = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise PolicyError(f"cannot read workflow runs from {path}: {error}") from error
    pages = document if isinstance(document, list) else [document]
    if not pages or not all(isinstance(page, dict) for page in pages):
        raise PolicyError("workflow run response is not an object or list of objects")
    values = []
    for page in pages:
        page_runs = page.get("workflow_runs")
        if not isinstance(page_runs, list):
            raise PolicyError("workflow run response has no workflow_runs list")
        values.extend(page_runs)
    runs = []
    for index, value in enumerate(values):
        if not isinstance(value, dict):
            raise PolicyError(f"workflow_runs[{index}] is not an object")
        branch = value.get("head_branch")
        sha = value.get("head_sha")
        run_id = value.get("id")
        if branch is not None and not isinstance(branch, str):
            raise PolicyError(f"workflow_runs[{index}].head_branch is invalid")
        if not isinstance(sha, str) or FULL_SHA.fullmatch(sha) is None:
            raise PolicyError(f"workflow_runs[{index}].head_sha is invalid")
        if not isinstance(run_id, int):
            raise PolicyError(f"workflow_runs[{index}].id is invalid")
        runs.append((branch, sha, run_id))
    return runs


def verify_tag_identity(runs, tag, sha):
    moved = [run for run in runs if run[0] == tag and run[1] != sha]
    if not moved:
        return
    previous = ", ".join(f"run {run_id} at {run_sha}" for _, run_sha, run_id in moved)
    raise PolicyError(
        f"tag {tag} previously triggered {previous}; do not move or reuse a tag, "
        "create a new patch version"
    )


def self_test():
    fixtures = ROOT / "scripts" / "fixtures" / "release-provenance"
    sha = "1" * 40
    verify_tag_identity(
        read_runs(fixtures / "tag-runs-same-commit.json"), "v1.2.3", sha
    )
    try:
        verify_tag_identity(read_runs(fixtures / "tag-runs-moved.json"), "v1.2.3", sha)
    except PolicyError as error:
        if "create a new patch version" not in str(error):
            raise RuntimeError(
                "moved-tag failure has no recovery instruction"
            ) from error
    else:
        raise RuntimeError("the moved-tag fixture was accepted")
    print("verify-tag-policy: self-test passed")


def main():
    parser = argparse.ArgumentParser(
        description="Reject a release tag previously observed at another commit."
    )
    parser.add_argument("--runs-json", type=Path)
    parser.add_argument("--tag")
    parser.add_argument("--sha")
    parser.add_argument("--self-test", action="store_true")
    arguments = parser.parse_args()
    if arguments.self_test:
        if arguments.runs_json or arguments.tag or arguments.sha:
            parser.error("--self-test does not accept policy inputs")
        self_test()
        return
    if arguments.runs_json is None or not arguments.tag or not arguments.sha:
        parser.error("--runs-json, --tag, and --sha are required")
    if FULL_SHA.fullmatch(arguments.sha) is None:
        parser.error("--sha must be a 40-character lowercase hexadecimal commit SHA")
    try:
        verify_tag_identity(
            read_runs(arguments.runs_json), arguments.tag, arguments.sha
        )
    except PolicyError as error:
        print(f"verify-tag-policy: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    print(f"tag {arguments.tag} has no workflow history at another commit")


if __name__ == "__main__":
    main()
