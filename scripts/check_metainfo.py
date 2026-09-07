#!/usr/bin/env python3
"""Fail if AppStream metainfo version/date drift from Cargo.toml or CHANGELOG."""

from __future__ import annotations

import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
METAINFO_PATH = ROOT / "packaging" / "io.github.ddv1982.echo.metainfo.xml"
CARGO_TOML_PATH = ROOT / "Cargo.toml"
CHANGELOG_PATH = ROOT / "CHANGELOG.md"

HEADING = re.compile(r"^## (?P<tag>\S+)(?: - (?P<date>\d{4}-\d{2}-\d{2}))?$")


def workspace_package_version(text: str) -> str:
    in_workspace_package = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_workspace_package = stripped == "[workspace.package]"
            continue
        if in_workspace_package and stripped.startswith("version"):
            _, _, value = stripped.partition("=")
            version = value.strip().strip('"').strip("'")
            if not version:
                break
            return version
    raise ValueError("Cargo.toml must declare [workspace.package] version")


def first_release(root: ET.Element) -> tuple[str | None, str | None]:
    for child in root.iter():
        if child.tag.rsplit("}", 1)[-1] == "release":
            return child.attrib.get("version"), child.attrib.get("date")
    return None, None


def changelog_heading_date(text: str, tag: str) -> str | None:
    for line in text.splitlines():
        match = HEADING.fullmatch(line)
        if match and match.group("tag") == tag:
            return match.group("date")
    raise ValueError(f"missing changelog heading: ## {tag}")


def check(
    cargo_toml: str,
    metainfo_xml: str,
    changelog: str,
    metainfo_label: str = str(METAINFO_PATH),
) -> None:
    version = workspace_package_version(cargo_toml)
    root = ET.fromstring(metainfo_xml.encode("utf-8"))
    release_version, release_date = first_release(root)
    if not release_version or not release_date:
        raise ValueError(f"{metainfo_label} must contain a <release version=\"...\" date=\"...\"> entry")
    if release_version != version:
        raise ValueError(
            f"{metainfo_label} latest release version {release_version} does not match "
            f"workspace.package.version {version}"
        )
    heading_date = changelog_heading_date(changelog, f"v{version}")
    if heading_date is not None and heading_date != release_date:
        raise ValueError(
            f"{metainfo_label} release date {release_date} does not match CHANGELOG.md date {heading_date}"
        )


def main() -> int:
    try:
        check(
            CARGO_TOML_PATH.read_text(encoding="utf-8"),
            METAINFO_PATH.read_text(encoding="utf-8"),
            CHANGELOG_PATH.read_text(encoding="utf-8"),
        )
    except (OSError, ET.ParseError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    print(f"metainfo ok for v{workspace_package_version(CARGO_TOML_PATH.read_text(encoding='utf-8'))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
