#!/usr/bin/env python3
import argparse
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
FULL_SHA = re.compile(r"[0-9a-f]{40}")
USES = re.compile(r"^\s*(?:-\s*)?uses\s*:\s*(.+?)\s*$")
FLOW_USES = re.compile(r"(?:^|[{,])\s*uses\s*:\s*([^,}]+)")
DOCKER_DIGEST = re.compile(r"docker://.+@sha256:[0-9a-f]{64}")


def action_reference(line):
    if line.lstrip().startswith("#"):
        return None
    match = USES.match(line)
    if match is None:
        match = FLOW_USES.search(line)
        if match is None:
            return None
    value = re.split(r"\s+#", match.group(1), maxsplit=1)[0].strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        value = value[1:-1]
    return value


def floating_references(path):
    findings = []
    for line_number, line in enumerate(path.read_text().splitlines(), start=1):
        reference = action_reference(line)
        if reference is None or reference.startswith("./"):
            continue
        if reference.startswith("docker://"):
            if DOCKER_DIGEST.fullmatch(reference) is None:
                findings.append((line_number, reference))
            continue
        _, separator, revision = reference.rpartition("@")
        if not separator or FULL_SHA.fullmatch(revision) is None:
            findings.append((line_number, reference))
    return findings


def workflow_paths(arguments):
    if arguments:
        return [Path(argument) for argument in arguments]
    directory = ROOT / ".github" / "workflows"
    return sorted((*directory.glob("*.yml"), *directory.glob("*.yaml")))


def verify(paths):
    failed = False
    for path in paths:
        if not path.is_file():
            print(f"verify-workflow-pinning: no such workflow: {path}", file=sys.stderr)
            failed = True
            continue
        for line_number, reference in floating_references(path):
            print(
                f"verify-workflow-pinning: {path}:{line_number}: "
                f"{reference} is not pinned to a full commit SHA",
                file=sys.stderr,
            )
            failed = True
    return not failed


def self_test():
    fixtures = ROOT / "scripts" / "fixtures" / "workflow-pinning"
    if not verify([fixtures / "pinned.yml"]):
        raise RuntimeError("the pinned fixture was rejected")
    findings = floating_references(fixtures / "floating.yml")
    references = {reference for _, reference in findings}
    expected = {
        "actions/checkout@v7",
        "docker://alpine:latest",
        "owner/repository/path@main",
    }
    if references != expected:
        raise RuntimeError(f"floating fixture findings differ: {references}")
    print("verify-workflow-pinning: self-test passed")


def main():
    parser = argparse.ArgumentParser(
        description="Reject GitHub Actions references that can move without review."
    )
    parser.add_argument("workflows", nargs="*")
    parser.add_argument("--self-test", action="store_true")
    arguments = parser.parse_args()
    if arguments.self_test:
        if arguments.workflows:
            parser.error("--self-test does not accept workflow paths")
        self_test()
        return
    if not verify(workflow_paths(arguments.workflows)):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
