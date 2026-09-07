"""Shared Debian archive and AppStream metadata helpers.

Used by build_apt_repository.py and validate_linux_package_metadata.py, which
run as ``python3 scripts/<name>.py`` (so this module resolves as a sibling on
``sys.path``) and are imported the same way by scripts/test_linux_package_metadata.py.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import xml.etree.ElementTree as ET


def read_ar_entries(deb_path: pathlib.Path) -> dict[str, bytes]:
    """Read the top-level members of a Debian ``ar`` archive into memory."""
    entries: dict[str, bytes] = {}
    with deb_path.open("rb") as handle:
        if handle.read(8) != b"!<arch>\n":
            raise ValueError(f"{deb_path} is not a Debian ar archive")

        while True:
            header = handle.read(60)
            if not header:
                break
            if len(header) != 60 or header[58:60] != b"`\n":
                raise ValueError(f"{deb_path} has an invalid ar member header")

            name = header[:16].decode("utf-8", errors="replace").strip().rstrip("/")
            try:
                size = int(header[48:58].decode("ascii").strip())
            except ValueError as error:
                raise ValueError(f"{deb_path} has an invalid ar member size") from error

            data = handle.read(size)
            if len(data) != size:
                raise ValueError(f"{deb_path} has a truncated ar member")
            if size % 2 == 1:
                handle.read(1)
            entries[name] = data
    return entries


def decompress_deb_tar_member(archive_name: str, data: bytes) -> tuple[bytes, str]:
    """Return (tar bytes, tarfile mode) for a control.tar.*/data.tar.* member.

    zstd members are decompressed via the external ``zstd`` tool because the
    stdlib tarfile module cannot read them.
    """
    if archive_name.endswith(".tar.zst"):
        zstd = shutil.which("zstd")
        if not zstd:
            raise ValueError(f"{archive_name} is zstd-compressed; the zstd command is required")
        completed = subprocess.run(
            [zstd, "-dc"],
            input=data,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if completed.returncode != 0:
            raise ValueError(completed.stderr.decode("utf-8", errors="replace"))
        return completed.stdout, "r:"
    return data, "r:*"


def parse_desktop_file(data: bytes) -> dict[str, str]:
    """Parse the ``[Desktop Entry]`` section of a freedesktop .desktop file."""
    fields: dict[str, str] = {}
    in_desktop_entry = False
    for raw_line in data.decode("utf-8", errors="replace").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            in_desktop_entry = line == "[Desktop Entry]"
            continue
        if in_desktop_entry and "=" in line:
            key, value = line.split("=", 1)
            fields[key] = value
    return fields


def strip_ns(name: str) -> str:
    return name.rsplit("}", 1)[-1]


def child_text(element: ET.Element, tag: str) -> str | None:
    for child in element:
        if strip_ns(child.tag) == tag and child.text:
            return child.text.strip()
    return None


def descendant_text(element: ET.Element, tag: str) -> str | None:
    for child in element.iter():
        if strip_ns(child.tag) == tag and child.text:
            return child.text.strip()
    return None


def launchable_desktop_id(element: ET.Element) -> str | None:
    for child in element.iter():
        if strip_ns(child.tag) == "launchable" and child.attrib.get("type") == "desktop-id":
            return (child.text or "").strip() or None
    return None


def first_release(element: ET.Element) -> tuple[str | None, str | None]:
    for child in element.iter():
        if strip_ns(child.tag) == "release":
            return child.attrib.get("version"), child.attrib.get("date")
    return None, None
