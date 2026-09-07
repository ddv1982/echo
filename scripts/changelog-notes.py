#!/usr/bin/env python3
import re
import sys
from pathlib import Path


HEADING = re.compile(r"^## (?P<tag>\S+)(?: - (?P<date>\d{4}-\d{2}-\d{2}))?$")


def parse_heading(line):
    match = HEADING.fullmatch(line)
    if not match:
        return None
    return match.group("tag"), match.group("date")


def section(text, tag):
    heading = "## " + tag
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        parsed = parse_heading(line)
        if parsed and parsed[0] == tag:
            start = i + 1
            break
    if start is None:
        raise ValueError("missing changelog heading: " + heading)
    end = len(lines)
    for i in range(start, len(lines)):
        if lines[i].startswith("## "):
            end = i
            break
    body = "\n".join(lines[start:end]).strip()
    if not body:
        raise ValueError("empty changelog section: " + heading)
    return body


def heading_date(text, tag):
    for line in text.splitlines():
        parsed = parse_heading(line)
        if parsed and parsed[0] == tag:
            return parsed[1]
    raise ValueError("missing changelog heading: ## " + tag)


def main(argv):
    if len(argv) != 2:
        print("usage: changelog-notes.py <tag>", file=sys.stderr)
        print("       changelog-notes.py --self-test", file=sys.stderr)
        return 1
    arg = argv[1]
    if arg == "--self-test":
        two = "# Changelog\n\n## v1.0.0\n\nFirst.\n\n## v0.9.0\n\nOlder.\n"
        if section(two, "v1.0.0") != "First.":
            print("self-test: first section extract failed", file=sys.stderr)
            return 1
        dated = "# Changelog\n\n## v1.0.0 - 2026-01-02\n\nDated notes.\n\n## v0.9.0\n\nOlder.\n"
        if section(dated, "v1.0.0") != "Dated notes.":
            print("self-test: dated heading extract failed", file=sys.stderr)
            return 1
        if heading_date(dated, "v1.0.0") != "2026-01-02":
            print("self-test: dated heading date extract failed", file=sys.stderr)
            return 1
        if heading_date(two, "v1.0.0") is not None:
            print("self-test: undated heading should have no date", file=sys.stderr)
            return 1
        cases = (
            (two, "v9.9.9"),
            ("# Changelog\n\n## v1.0.0\n\n## v0.9.0\n\nOlder.\n", "v1.0.0"),
            ("# Changelog\n\n## v1.0.0 - date\n\nNotes.\n", "v1.0.0"),
        )
        for text, tag in cases:
            try:
                section(text, tag)
            except ValueError:
                continue
            print("self-test: expected failure for " + tag, file=sys.stderr)
            return 1
        return 0
    try:
        sys.stdout.write(
            section(Path("CHANGELOG.md").read_text(encoding="utf-8"), arg) + "\n"
        )
    except ValueError as exc:
        print(exc, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
