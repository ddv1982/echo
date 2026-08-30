#!/usr/bin/env python3
import re
from pathlib import Path
import sys
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parent.parent
LINK = re.compile(r"!?(?:\[[^]]*\])\(([^)]+)\)")
HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*$")


def slug(value):
    value = re.sub(r"[`*_~]", "", value.strip().lower())
    value = re.sub(r"[^\w\s-]", "", value)
    return re.sub(r"[-\s]+", "-", value).strip("-")


def headings(path):
    return {
        slug(match.group(1))
        for line in path.read_text().splitlines()
        if (match := HEADING.match(line))
    }


def public_documents():
    paths = [ROOT / "README.md"]
    paths.extend(sorted((ROOT / "docs").glob("*.md")))
    paths.extend(sorted((ROOT / "docs" / "history").glob("*.md")))
    paths.append(ROOT / "docs" / "qa" / "README.md")
    return [path for path in paths if path.exists()]


def check(path):
    failures = []
    for line_number, line in enumerate(path.read_text().splitlines(), 1):
        for raw_target in LINK.findall(line):
            target = raw_target.strip().strip("<>").split(maxsplit=1)[0]
            if target.startswith(("https://", "http://", "mailto:")):
                continue
            relative, _, fragment = target.partition("#")
            destination = path if not relative else path.parent / unquote(relative)
            if not destination.exists():
                failures.append(f"{path.relative_to(ROOT)}:{line_number}: missing {relative}")
            elif fragment and destination.suffix.lower() == ".md":
                if unquote(fragment).lower() not in headings(destination):
                    failures.append(
                        f"{path.relative_to(ROOT)}:{line_number}: missing heading #{fragment}"
                    )
    return failures


def main():
    failures = [failure for path in public_documents() for failure in check(path)]
    if failures:
        print("\n".join(failures), file=sys.stderr)
        raise SystemExit(1)
    print(f"checked local links in {len(public_documents())} public documents")


if __name__ == "__main__":
    main()
