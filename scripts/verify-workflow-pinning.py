#!/usr/bin/env python3
import argparse
from pathlib import Path
import re
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
FULL_SHA = re.compile(r"[0-9a-f]{40}")
USES = re.compile(
    r"^\s*(?:-\s*)?(?:\"uses\"|'uses'|uses)\s*:\s*(.+?)\s*$"
)
FLOW_USES = re.compile(
    r"(?:^|[{,])\s*(?:\"uses\"|'uses'|uses)\s*:\s*([^,}]+)"
)
DOCKER_DIGEST = re.compile(r"docker://.+@sha256:[0-9a-f]{64}")
MAPPING = re.compile(r"^(\s*)([^:#][^:]*?):(?:\s*(.*))?$")
RUN = re.compile(
    r"^(\s*)(?:-\s*)?(?:\"run\"|'run'|run)\s*:\s*(.*)$"
)
REF_NAME = re.compile(r"\$\{\{\s*github\.ref_name\s*\}\}")
PR_EXCLUSION = "github.event_name != 'pull_request'"

CHECK_PERMISSIONS = {
    "policy": {"contents": "read"},
    "frontend": {"contents": "read"},
    "rust": {"contents": "read"},
    "assets": {"contents": "read"},
    "check": {},
}
RELEASE_PERMISSIONS = {
    "release-policy": {"actions": "read", "contents": "read"},
    "linux-packages": {"contents": "read"},
    "appimage": {"contents": "read"},
    "release-assets": {"contents": "read"},
    "attest-assets": {
        "actions": "read",
        "attestations": "write",
        "contents": "read",
        "id-token": "write",
    },
    "github-release": {"contents": "write"},
}
CHECK_SHARDS = {"policy", "frontend", "rust", "assets"}


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


def _indent(line):
    return len(line) - len(line.lstrip(" "))


def _unquote(value):
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def _plain_value(value):
    return _unquote(re.split(r"\s+#", value, maxsplit=1)[0].strip())


def _mapping_entry(line):
    if not line.strip() or line.lstrip().startswith("#"):
        return None
    match = MAPPING.match(line)
    if match is None:
        return None
    return len(match.group(1)), _unquote(match.group(2)), match.group(3) or ""


def _block_value(lines, index, indent, marker):
    values = []
    for line in lines[index + 1 :]:
        if line.strip() and _indent(line) <= indent:
            break
        if line.strip():
            values.append(line.strip())
        elif values:
            values.append("")
    separator = " " if marker.startswith(">") else "\n"
    return separator.join(values)


def _field_value(lines, field):
    index, indent, raw = field
    value = _plain_value(raw)
    if value.startswith(("|", ">")):
        return _block_value(lines, index, indent, value)
    return value


def _workflow_jobs(lines):
    jobs_index = None
    jobs_indent = None
    for index, line in enumerate(lines):
        entry = _mapping_entry(line)
        if entry is not None and entry[0] == 0 and entry[1] == "jobs":
            jobs_index = index
            jobs_indent = entry[0]
            break
    if jobs_index is None:
        return {}

    starts = []
    job_indent = jobs_indent + 2
    for index in range(jobs_index + 1, len(lines)):
        line = lines[index]
        if line.strip() and _indent(line) <= jobs_indent:
            break
        entry = _mapping_entry(line)
        if entry is not None and entry[0] == job_indent:
            starts.append((entry[1], index))

    jobs = {}
    workflow_end = len(lines)
    for index in range(jobs_index + 1, len(lines)):
        if lines[index].strip() and _indent(lines[index]) <= jobs_indent:
            workflow_end = index
            break
    for position, (name, start) in enumerate(starts):
        end = starts[position + 1][1] if position + 1 < len(starts) else workflow_end
        fields = {}
        for index in range(start + 1, end):
            entry = _mapping_entry(lines[index])
            if entry is not None and entry[0] == job_indent + 2:
                fields[entry[1]] = (index, entry[0], entry[2])
        jobs[name] = {
            "line": start + 1,
            "start": start,
            "end": end,
            "fields": fields,
        }
    return jobs


def _permissions(lines, field):
    index, indent, raw = field
    value = _plain_value(raw)
    if value:
        if not (value.startswith("{") and value.endswith("}")):
            return None
        inside = value[1:-1].strip()
        if not inside:
            return {}
        permissions = {}
        for item in inside.split(","):
            key, separator, permission = item.partition(":")
            if not separator:
                return None
            permissions[_unquote(key)] = _plain_value(permission)
        return permissions

    permissions = {}
    for line in lines[index + 1 :]:
        if line.strip() and _indent(line) <= indent:
            break
        entry = _mapping_entry(line)
        if entry is not None and entry[0] == indent + 2:
            permissions[entry[1]] = _plain_value(entry[2])
    return permissions or None


def _top_level_field(lines, name):
    for index, line in enumerate(lines):
        entry = _mapping_entry(line)
        if entry is not None and entry[0] == 0 and entry[1] == name:
            return index, entry[0], entry[2]
    return None


def _run_scalars(lines, start=0, end=None):
    if end is None:
        end = len(lines)
    scalars = []
    index = start
    while index < end:
        match = RUN.match(lines[index])
        if match is None:
            index += 1
            continue
        indent = len(match.group(1))
        raw = _plain_value(match.group(2))
        if raw.startswith(("|", ">")):
            value_lines = []
            cursor = index + 1
            while cursor < end:
                line = lines[cursor]
                if line.strip() and _indent(line) <= indent:
                    break
                value_lines.append(line)
                cursor += 1
            scalars.append((index + 1, "\n".join(value_lines)))
            index = cursor
        else:
            scalars.append((index + 1, _unquote(raw)))
            index += 1
    return scalars


def _needs(lines, field):
    index, indent, raw = field
    value = _plain_value(raw)
    if value.startswith("[") and value.endswith("]"):
        return {
            _unquote(item.strip())
            for item in value[1:-1].split(",")
            if item.strip()
        }
    if value:
        return {_unquote(value)}
    needs = set()
    for line in lines[index + 1 :]:
        if line.strip() and _indent(line) <= indent:
            break
        match = re.match(r"^\s*-\s*([^#]+?)\s*(?:#.*)?$", line)
        if match is not None:
            needs.add(_unquote(match.group(1)))
    return needs


def _dependency_closure(lines, jobs, name):
    closure = set()
    pending = [name]
    while pending:
        dependency = pending.pop()
        job = jobs.get(dependency)
        if job is None:
            continue
        fields = job["fields"]
        direct = _needs(lines, fields["needs"]) if "needs" in fields else set()
        for item in direct - closure:
            closure.add(item)
            pending.append(item)
    return closure


def _child_fields(lines, field):
    index, indent, raw = field
    if _plain_value(raw):
        return {}
    fields = {}
    for child_index in range(index + 1, len(lines)):
        line = lines[child_index]
        if line.strip() and _indent(line) <= indent:
            break
        entry = _mapping_entry(line)
        if entry is not None and entry[0] == indent + 2:
            fields[entry[1]] = (child_index, entry[0], entry[2])
    return fields


def _sequence_values(lines, field):
    index, indent, raw = field
    value = _plain_value(raw)
    if value.startswith("[") and value.endswith("]"):
        return {
            _unquote(item.strip())
            for item in value[1:-1].split(",")
            if item.strip()
        }
    if value:
        return set()
    values = set()
    for line in lines[index + 1 :]:
        if line.strip() and _indent(line) <= indent:
            break
        match = re.match(r"^\s*-\s*([^#]+?)\s*(?:#.*)?$", line)
        if match is not None:
            values.add(_plain_value(match.group(1)))
    return values


def _release_triggers(lines):
    field = _top_level_field(lines, "on")
    if field is None:
        return {}, set()
    value = _plain_value(field[2])
    if value.startswith("[") and value.endswith("]"):
        return {}, {
            _unquote(item.strip())
            for item in value[1:-1].split(",")
            if item.strip()
        }
    return _child_fields(lines, field), set()


def _has_pull_request_trigger(lines):
    fields, inline = _release_triggers(lines)
    return "pull_request" in fields or "pull_request" in inline


def _aggregate_verifies_shards(run_scalars):
    success_comparison = re.compile(r"(?:==|=|!=)\s*['\"]?success['\"]?")
    for _, scalar in run_scalars:
        if not success_comparison.search(scalar):
            continue
        if all(
            re.search(rf"needs\.{re.escape(shard)}\.result\b", scalar)
            for shard in CHECK_SHARDS
        ):
            return True
    return False


def _aggregate_fails_on_mismatch(run_scalars):
    result = r'''(?:"\$result"|'\$result'|\$result)'''
    success = r'''(?:"success"|'success'|success)'''
    failure_guards = (
        re.compile(
            rf"\[\s*{result}\s*=\s*{success}\s*\]\s*\|\|\s*exit\s+1\b"
            rf"[ \t]*(?:#[^\n]*)?(?:\n|$)"
        ),
        re.compile(
            rf"\[\s*{result}\s*!=\s*{success}\s*\]\s*&&\s*exit\s+1\b"
            rf"[ \t]*(?:#[^\n]*)?(?:\n|$)"
        ),
        re.compile(
            rf"\bif\s+\[\s*{result}\s*!=\s*{success}\s*\]\s*;\s*then\b"
            rf"(?:(?!\bfi\b).)*?(?:^|\n)[ \t]*exit\s+1\b"
            rf"[ \t]*(?:#[^\n]*)?(?:\n|$)",
            re.DOTALL,
        ),
    )
    success_comparison = re.compile(r"(?:==|=|!=)\s*['\"]?success['\"]?")
    for _, scalar in run_scalars:
        if not success_comparison.search(scalar):
            continue
        if not all(
            re.search(rf"needs\.{re.escape(shard)}\.result\b", scalar)
            for shard in CHECK_SHARDS
        ):
            continue
        if any(pattern.search(scalar) for pattern in failure_guards):
            return True
    return False


def _check_policy(lines, jobs):
    findings = []
    missing = set(CHECK_PERMISSIONS) - set(jobs)
    if missing:
        findings.append(
            (1, f"check workflow is missing jobs: {', '.join(sorted(missing))}")
        )

    aggregate = jobs.get("check")
    if aggregate is not None:
        fields = aggregate["fields"]
        if_field = fields.get("if")
        if if_field is None or _field_value(lines, if_field) != "always()":
            findings.append(
                (aggregate["line"], "aggregate check must use if: always()")
            )
        needs = _needs(lines, fields["needs"]) if "needs" in fields else set()
        if needs != CHECK_SHARDS:
            findings.append(
                (
                    aggregate["line"],
                    "aggregate check must need policy, frontend, rust, and assets",
                )
            )
        runs = _run_scalars(lines, aggregate["start"], aggregate["end"])
        if not _aggregate_verifies_shards(runs):
            findings.append(
                (
                    aggregate["line"],
                    "aggregate check must verify every shard result is success",
                )
            )
        elif not _aggregate_fails_on_mismatch(runs):
            findings.append(
                (
                    aggregate["line"],
                    "aggregate check must fail unless every shard result is success",
                )
            )

    assets = jobs.get("assets")
    if assets is not None:
        fields = assets["fields"]
        needs = _needs(lines, fields["needs"]) if "needs" in fields else set()
        if "rust" in needs:
            findings.append((assets["line"], "assets must not depend on rust"))
        runs = _run_scalars(lines, assets["start"], assets["end"])
        commands = "\n".join(value for _, value in runs)
        icon_paths = ("src-tauri/icons", "frontend/public", "assets/icons")
        checks_icon_drift = (
            re.search(r"\bgit\s+diff\b[^\n]*--exit-code\b", commands) is not None
            and all(path in commands for path in icon_paths)
        )
        if "xtask" not in commands or not checks_icon_drift:
            findings.append(
                (assets["line"], "assets must run xtask and check icon drift")
            )
        release_build = re.search(
            r"\bcargo\s+build\b(?:[^\n]*\\\n)*[^\n]*"
            r"(?:--release|-r(?:\s|$))",
            commands,
        )
        if release_build:
            findings.append(
                (assets["line"], "assets must not run cargo build --release")
            )
        validation_commands = "\n".join(
            value for _, value in runs if "desktop-file-validate" in value
        )
        required_desktops = {"packaging/Echo.desktop"}
        required_desktops.update(
            path.relative_to(ROOT).as_posix()
            for path in (ROOT / "src-tauri" / "templates").glob("*.desktop")
        )
        has_template_glob = "src-tauri/templates/*.desktop" in validation_commands
        missing_desktops = {
            path
            for path in required_desktops
            if path not in validation_commands
            and not (path.startswith("src-tauri/templates/") and has_template_glob)
        }
        if missing_desktops:
            findings.append(
                (
                    assets["line"],
                    "assets desktop validation is missing: "
                    + ", ".join(sorted(missing_desktops)),
                )
            )

    rust = jobs.get("rust")
    if rust is not None:
        runs = _run_scalars(lines, rust["start"], rust["end"])
        compile_lines = [
            line_number
            for line_number, command in runs
            if re.search(r"\bcargo\s+(?:clippy|test|build)\b", command)
        ]
        prepares_dist = [
            line_number
            for line_number, command in runs
            if re.search(
                r"(?:^|\n)\s*mkdir\s+-p\s+(?:--\s+)?['\"]?frontend/dist['\"]?(?:\s|$)",
                command,
            )
        ]
        if compile_lines and not any(
            line_number < min(compile_lines) for line_number in prepares_dist
        ):
            findings.append(
                (
                    rust["line"],
                    "rust must create frontend/dist before compiling Tauri targets",
                )
            )
    return findings


def _release_policy(lines, jobs):
    findings = []
    missing = set(RELEASE_PERMISSIONS) - set(jobs)
    if missing:
        findings.append(
            (1, f"release workflow is missing jobs: {', '.join(sorted(missing))}")
        )
    if not _has_pull_request_trigger(lines):
        findings.append((1, "release workflow must retain its pull_request trigger"))
    triggers, inline_triggers = _release_triggers(lines)
    if (
        "workflow_dispatch" not in triggers
        and "workflow_dispatch" not in inline_triggers
    ):
        findings.append(
            (1, "release workflow must retain its workflow_dispatch trigger")
        )
    schedule = triggers.get("schedule")
    schedules = _sequence_values(lines, schedule) if schedule is not None else set()
    if not any(item.startswith("cron:") for item in schedules):
        findings.append((1, "release workflow must retain its schedule trigger"))
    push = triggers.get("push")
    push_fields = _child_fields(lines, push) if push is not None else {}
    branches = (
        _sequence_values(lines, push_fields["branches"])
        if "branches" in push_fields
        else set()
    )
    tags = (
        _sequence_values(lines, push_fields["tags"])
        if "tags" in push_fields
        else set()
    )
    if "main" not in branches:
        findings.append((1, "release workflow must retain its main branch push trigger"))
    if "v*" not in tags:
        findings.append((1, "release workflow must retain its v* tag push trigger"))

    for name in ("linux-packages", "appimage", "release-assets", "attest-assets"):
        job = jobs.get(name)
        if job is None:
            continue
        field = job["fields"].get("if")
        condition = _field_value(lines, field) if field is not None else ""
        if " ".join(condition.split()) != PR_EXCLUSION:
            findings.append(
                (job["line"], f"{name} must explicitly exclude pull_request")
            )

    for name in ("linux-packages", "appimage"):
        job = jobs.get(name)
        if job is None:
            continue
        fields = job["fields"]
        needs = _needs(lines, fields["needs"]) if "needs" in fields else set()
        if "release-policy" not in needs:
            findings.append((job["line"], f"{name} must need release-policy"))

    release_assets = jobs.get("release-assets")
    if release_assets is not None:
        fields = release_assets["fields"]
        needs = _needs(lines, fields["needs"]) if "needs" in fields else set()
        if not {"linux-packages", "appimage"}.issubset(needs):
            findings.append(
                (
                    release_assets["line"],
                    "release-assets must need linux-packages and appimage",
                )
            )
        if "release-policy" not in _dependency_closure(
            lines, jobs, "release-assets"
        ):
            findings.append(
                (
                    release_assets["line"],
                    "release-assets must depend on release-policy",
                )
            )

    attest_assets = jobs.get("attest-assets")
    if attest_assets is not None:
        fields = attest_assets["fields"]
        needs = _needs(lines, fields["needs"]) if "needs" in fields else set()
        if "release-assets" not in needs:
            findings.append(
                (attest_assets["line"], "attest-assets must need release-assets")
            )

    github_release = jobs.get("github-release")
    if github_release is not None:
        fields = github_release["fields"]
        needs = _needs(lines, fields["needs"]) if "needs" in fields else set()
        if not {"release-assets", "attest-assets"}.issubset(needs):
            findings.append(
                (
                    github_release["line"],
                    "github-release must need release-assets and attest-assets",
                )
            )
        field = fields.get("if")
        condition = _field_value(lines, field) if field is not None else ""
        clauses = [clause.strip(" ()") for clause in condition.split("&&")]
        if "||" in condition or "github.ref_type == 'tag'" not in clauses:
            findings.append(
                (github_release["line"], "github-release must require a tag")
            )

    policy = jobs.get("release-policy")
    if policy is not None and "if" in policy["fields"]:
        findings.append(
            (policy["line"], "release-policy must remain runnable on pull_request")
        )
    return findings


def workflow_policy_findings(path):
    if path.stem not in {"check", "release"}:
        return []
    lines = path.read_text().splitlines()
    jobs = _workflow_jobs(lines)
    findings = []

    top_permissions = _top_level_field(lines, "permissions")
    if top_permissions is None or _permissions(lines, top_permissions) != {}:
        line = top_permissions[0] + 1 if top_permissions is not None else 1
        findings.append((line, "top-level permissions must be empty"))

    allowlist = CHECK_PERMISSIONS if path.stem == "check" else RELEASE_PERMISSIONS
    for name, job in jobs.items():
        fields = job["fields"]
        timeout = (
            _field_value(lines, fields["timeout-minutes"])
            if "timeout-minutes" in fields
            else ""
        )
        try:
            positive_timeout = int(timeout) > 0
        except ValueError:
            positive_timeout = False
        if not positive_timeout:
            findings.append(
                (
                    job["line"],
                    f"job {name} must have a direct positive timeout-minutes",
                )
            )

        expected = allowlist.get(name)
        if expected is None:
            findings.append(
                (job["line"], f"job {name} has no permissions allowlist")
            )
            continue
        field = fields.get("permissions")
        actual = _permissions(lines, field) if field is not None else None
        if actual != expected:
            findings.append(
                (
                    job["line"],
                    f"job {name} permissions must be exactly {expected}, got {actual}",
                )
            )

    for line_number, scalar in _run_scalars(lines):
        if REF_NAME.search(scalar):
            findings.append(
                (
                    line_number,
                    "shell run scalar must not contain ${{ github.ref_name }}",
                )
            )

    if path.stem == "check":
        findings.extend(_check_policy(lines, jobs))
    else:
        findings.extend(_release_policy(lines, jobs))
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
        for line_number, finding in workflow_policy_findings(path):
            print(
                f"verify-workflow-pinning: {path}:{line_number}: {finding}",
                file=sys.stderr,
            )
            failed = True
    return not failed


CHECK_FIXTURE = """\
name: check
on: [push, pull_request]
permissions: {}
jobs:
  policy:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    permissions:
      contents: read
    steps:
      - run: python3 scripts/verify-workflow-pinning.py
  frontend:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    permissions:
      contents: read
    steps:
      - run: npm test
  rust:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    permissions:
      contents: read
    steps:
      - run: mkdir -p frontend/dist
      - run: cargo test
  assets:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    permissions:
      contents: read
    steps:
      - run: cargo run --package xtask
      - run: git diff --exit-code -- src-tauri/icons frontend/public assets/icons
      - run: desktop-file-validate packaging/Echo.desktop src-tauri/templates/*.desktop
  check:
    if: always()
    needs: [policy, frontend, rust, assets]
    runs-on: ubuntu-latest
    timeout-minutes: 5
    permissions: {}
    steps:
      - name: Require successful shards
        run: |
          for result in \\
            "${{ needs.policy.result }}" \\
            "${{ needs.frontend.result }}" \\
            "${{ needs.rust.result }}" \\
            "${{ needs.assets.result }}"; do
            [ "$result" = success ] || exit 1
          done
"""

RELEASE_FIXTURE = """\
name: release
on:
  workflow_dispatch:
  pull_request:
  push:
    branches:
      - main
    tags:
      - "v*"
  schedule:
    - cron: "17 4 * * *"
permissions: {}
jobs:
  release-policy:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    permissions:
      actions: read
      contents: read
    steps:
      - run: echo policy
  linux-packages:
    if: github.event_name != 'pull_request'
    needs: release-policy
    runs-on: ubuntu-latest
    timeout-minutes: 45
    permissions:
      contents: read
    steps:
      - run: echo packages
  appimage:
    if: github.event_name != 'pull_request'
    needs: release-policy
    runs-on: ubuntu-latest
    timeout-minutes: 45
    permissions:
      contents: read
    steps:
      - run: echo appimage
  release-assets:
    if: github.event_name != 'pull_request'
    needs: [linux-packages, appimage]
    runs-on: ubuntu-latest
    timeout-minutes: 15
    permissions:
      contents: read
    steps:
      - run: echo assets
  attest-assets:
    if: github.event_name != 'pull_request'
    needs: release-assets
    runs-on: ubuntu-latest
    timeout-minutes: 5
    permissions:
      actions: read
      attestations: write
      contents: read
      id-token: write
    steps:
      - run: echo attest
  github-release:
    if: >-
      github.ref_type == 'tag' &&
      needs.release-assets.result == 'success' &&
      needs.attest-assets.result == 'success'
    needs: [release-assets, attest-assets]
    runs-on: ubuntu-latest
    timeout-minutes: 10
    permissions:
      contents: write
    steps:
      - run: echo release
"""


def _replace(fixture, old, new):
    if fixture.count(old) != 1:
        raise RuntimeError(f"self-test replacement is not unique: {old!r}")
    return fixture.replace(old, new)


def _expect_policy_finding(path, fixture, expected):
    path.write_text(fixture)
    findings = [message for _, message in workflow_policy_findings(path)]
    if not any(expected in message for message in findings):
        raise RuntimeError(f"expected policy finding {expected!r}, got {findings}")


def self_test():
    fixtures = ROOT / "scripts" / "fixtures" / "workflow-pinning"
    if floating_references(fixtures / "pinned.yml"):
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

    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory)
        pinning = temporary / "pinning.yml"
        pinning.write_text(
            """\
jobs:
  pins:
    steps:
      - uses: owner/repository@v1.2.3
      - uses: owner/repository@main
      - uses: owner/repository@ABCDEF0123456789ABCDEF0123456789ABCDEF01
      - uses: owner/repository@${{ github.sha }}
      - uses: actions/checkout@v7
      - uses: docker://alpine:latest
      - uses: docker://alpine@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
      - uses: owner/repository@0123456789abcdef0123456789abcdef01234567
"""
        )
        bad_pins = {reference for _, reference in floating_references(pinning)}
        expected_bad_pins = {
            "owner/repository@v1.2.3",
            "owner/repository@main",
            "owner/repository@ABCDEF0123456789ABCDEF0123456789ABCDEF01",
            "owner/repository@${{ github.sha }}",
            "actions/checkout@v7",
            "docker://alpine:latest",
        }
        if bad_pins != expected_bad_pins:
            raise RuntimeError(f"inline pin findings differ: {bad_pins}")

        for replacement in (
            '      - "uses": actions/checkout@v7',
            "      - {'uses': actions/checkout@v7}",
        ):
            pinning.write_text(
                _replace(CHECK_FIXTURE, "      - run: npm test", replacement)
            )
            quoted_key_findings = {
                reference for _, reference in floating_references(pinning)
            }
            if quoted_key_findings != {"actions/checkout@v7"}:
                raise RuntimeError(
                    "quoted uses key findings differ: "
                    f"{quoted_key_findings} for {replacement!r}"
                )

        check = temporary / "check.yml"
        release = temporary / "release.yml"
        check.write_text(CHECK_FIXTURE)
        release.write_text(RELEASE_FIXTURE)
        if workflow_policy_findings(check):
            raise RuntimeError("the valid check policy fixture was rejected")
        if workflow_policy_findings(release):
            raise RuntimeError("the valid release policy fixture was rejected")

        _expect_policy_finding(
            check,
            _replace(CHECK_FIXTURE, "      - run: mkdir -p frontend/dist\n", ""),
            "rust must create frontend/dist before compiling Tauri targets",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                '            [ "$result" = success ] || exit 1',
                '            [ "$result" = success ]',
            ),
            "aggregate check must fail unless every shard result is success",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  linux-packages:\n    if: github.event_name != 'pull_request'\n",
                "  linux-packages:\n"
                "    if: github.event_name != 'pull_request' || always()\n",
            ),
            "linux-packages must explicitly exclude pull_request",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  attest-assets:\n    if: github.event_name != 'pull_request'\n",
                "  attest-assets:\n",
            ),
            "attest-assets must explicitly exclude pull_request",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "    needs: [linux-packages, appimage]",
                "    needs: linux-packages",
            ),
            "release-assets must need linux-packages and appimage",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "    needs: [release-assets, attest-assets]",
                "    needs: attest-assets",
            ),
            "github-release must need release-assets and attest-assets",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "      github.ref_type == 'tag' &&\n"
                "      needs.release-assets.result == 'success' &&\n",
                "      needs.release-assets.result == 'success' &&\n",
            ),
            "github-release must require a tag",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  appimage:\n    if: github.event_name != 'pull_request'\n"
                "    needs: release-policy\n",
                "  appimage:\n    if: github.event_name != 'pull_request'\n",
            ),
            "appimage must need release-policy",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  attest-assets:\n    if: github.event_name != 'pull_request'\n"
                "    needs: release-assets\n",
                "  attest-assets:\n    if: github.event_name != 'pull_request'\n",
            ),
            "attest-assets must need release-assets",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  attest-assets:\n    if: github.event_name != 'pull_request'\n",
                "  attest-assets:\n"
                "    if: github.event_name != 'pull_request' || always()\n",
            ),
            "attest-assets must explicitly exclude pull_request",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "      needs.attest-assets.result == 'success'",
                "      needs.attest-assets.result == 'success' || always()",
            ),
            "github-release must require a tag",
        )
        for trigger, source in (
            ("workflow_dispatch", "  workflow_dispatch:\n"),
            ("pull_request", "  pull_request:\n"),
            ("schedule", "  schedule:\n    - cron: \"17 4 * * *\"\n"),
        ):
            _expect_policy_finding(
                release,
                _replace(RELEASE_FIXTURE, source, ""),
                f"release workflow must retain its {trigger} trigger",
            )
        _expect_policy_finding(
            release,
            _replace(RELEASE_FIXTURE, "    branches:\n      - main\n", ""),
            "release workflow must retain its main branch push trigger",
        )
        _expect_policy_finding(
            release,
            _replace(RELEASE_FIXTURE, '    tags:\n      - "v*"\n', ""),
            "release workflow must retain its v* tag push trigger",
        )

        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n",
                "  policy:\n    runs-on: ubuntu-latest\n",
            ),
            "direct positive timeout-minutes",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n    permissions:\n"
                "      contents: read\n",
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n",
            ),
            "job policy permissions",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "permissions: {}\njobs:\n  policy:\n",
                "permissions:\n  contents: read\njobs:\n  policy:\n",
            ).replace(
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n    permissions:\n"
                "      contents: read\n",
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n",
                1,
            ),
            "job policy permissions",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n    permissions:\n"
                "      contents: read\n",
                "  policy:\n    runs-on: ubuntu-latest\n"
                "    timeout-minutes: 5\n    permissions:\n"
                "      contents: write\n",
            ),
            "job policy permissions",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  linux-packages:\n    if: github.event_name != 'pull_request'\n",
                "  linux-packages:\n",
            ),
            "linux-packages must explicitly exclude pull_request",
        )
        _expect_policy_finding(
            release,
            _replace(
                RELEASE_FIXTURE,
                "  release-policy:\n    runs-on:",
                "  release-policy:\n"
                "    if: github.event_name != 'pull_request'\n    runs-on:",
            ),
            "release-policy must remain runnable",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "      - run: npm test",
                "      - run: echo ${{ github.ref_name }}",
            ),
            "shell run scalar",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "      - run: npm test",
                "      - run: |\n          echo start\n"
                "          echo ${{ github.ref_name }}",
            ),
            "shell run scalar",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "      - run: npm test",
                '      - "run": echo ${{ github.ref_name }}',
            ),
            "shell run scalar",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "      - run: npm test",
                "      - 'run': |\n          echo start\n"
                "          echo ${{ github.ref_name }}",
            ),
            "shell run scalar",
        )
        _expect_policy_finding(
            check,
            _replace(CHECK_FIXTURE, "    if: always()", "    if: success()"),
            "aggregate check must use if: always()",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                '            "${{ needs.frontend.result }}" \\\n',
                "",
            ),
            "aggregate check must verify every shard result",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "  assets:\n    runs-on:",
                "  assets:\n    needs: rust\n    runs-on:",
            ),
            "assets must not depend on rust",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                "      - run: cargo run --package xtask",
                "      - run: cargo build --release\n"
                "      - run: ./target/release/xtask",
            ),
            "assets must not run cargo build --release",
        )
        _expect_policy_finding(
            check,
            _replace(
                CHECK_FIXTURE,
                " src-tauri/templates/*.desktop",
                " src-tauri/templates/Echo.desktop",
            ),
            "assets desktop validation is missing",
        )

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
