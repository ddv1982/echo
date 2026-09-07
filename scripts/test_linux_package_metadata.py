#!/usr/bin/env python3
import io
import pathlib
import tarfile
import tempfile
import unittest

import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from validate_linux_package_metadata import (
    DEFAULT_BINARY,
    DEFAULT_COMPONENT_ID,
    DEFAULT_DESKTOP_ID,
    DEFAULT_LICENSE,
    DEFAULT_RPM_ARCH,
    DEFAULT_RPM_PACKAGE_NAME,
    require_installed_icon,
    validate_package,
)


def _tar_gz(files):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name, content in files.items():
            payload = content.encode("utf-8") if isinstance(content, str) else content
            member = tarfile.TarInfo(name)
            member.size = len(payload)
            member.mode = 0o644
            archive.addfile(member, io.BytesIO(payload))
    return output.getvalue()


def _ar_entry(name, payload):
    header = (
        f"{name + '/':<16}{0:<12}{0:<6}{0:<6}{'100644':<8}{len(payload):<10}`\n"
    ).encode("ascii")
    assert len(header) == 60
    return header + payload + (b"\n" if len(payload) % 2 else b"")


def _write_deb(path, *, desktop_exec=DEFAULT_BINARY):
    metainfo = f"""<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{DEFAULT_COMPONENT_ID}</id>
  <metadata_license>{DEFAULT_LICENSE}</metadata_license>
  <project_license>{DEFAULT_LICENSE}</project_license>
  <launchable type="desktop-id">{DEFAULT_DESKTOP_ID}</launchable>
  <provides><binary>{DEFAULT_BINARY}</binary></provides>
  <releases><release version="1.0.0" date="2026-01-01"/></releases>
</component>
"""
    files = {
        f"usr/share/metainfo/{DEFAULT_COMPONENT_ID}.metainfo.xml": metainfo,
        f"usr/share/applications/{DEFAULT_DESKTOP_ID}": (
            "[Desktop Entry]\n"
            "Type=Application\n"
            "Name=Echo\n"
            f"Exec={desktop_exec}\n"
            "Icon=echo-desktop\n"
        ),
        "usr/share/icons/hicolor/128x128/apps/echo-desktop.png": b"png",
        "usr/share/doc/echo/copyright": "License: MIT\n",
    }
    data_archive = _tar_gz(files)
    path.write_bytes(b"!<arch>\n" + _ar_entry("debian-binary", b"2.0\n") + _ar_entry("data.tar.gz", data_archive))


class LinuxPackageMetadataTests(unittest.TestCase):
    def validate_deb(self, path):
        return validate_package(
            package_path=path,
            expected_component_id=DEFAULT_COMPONENT_ID,
            expected_license=DEFAULT_LICENSE,
            expected_binary=DEFAULT_BINARY,
            expected_desktop_id=DEFAULT_DESKTOP_ID,
            expected_rpm_package_name=DEFAULT_RPM_PACKAGE_NAME,
            expected_rpm_arch=DEFAULT_RPM_ARCH,
            run_external_tools=False,
            require_external_tools=False,
        )

    def test_missing_desktop_icon_records_error(self):
        with tempfile.TemporaryDirectory() as temp:
            errors = []
            paths = require_installed_icon(errors, pathlib.Path(temp), "echo-desktop")

        self.assertEqual(paths, [])
        self.assertEqual(
            errors,
            ["desktop Icon does not resolve to an installed hicolor/pixmaps icon: 'echo-desktop'"],
        )

    def test_hicolor_icon_satisfies_desktop_icon(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            icon = root / "usr/share/icons/hicolor/128x128/apps/echo-desktop.png"
            icon.parent.mkdir(parents=True)
            icon.write_bytes(b"png")
            errors = []
            paths = require_installed_icon(errors, root, "echo-desktop")

        self.assertEqual(errors, [])
        self.assertEqual(paths, ["/usr/share/icons/hicolor/128x128/apps/echo-desktop.png"])

    def test_valid_deb_fixture_exercises_package_metadata_contract(self):
        with tempfile.TemporaryDirectory() as temp:
            package = pathlib.Path(temp) / "echo_1.0.0_amd64.deb"
            _write_deb(package)
            report = self.validate_deb(package)

        self.assertTrue(report.ok, report.errors)
        self.assertEqual(report.package_format, "deb")
        self.assertEqual(report.component_id, DEFAULT_COMPONENT_ID)
        self.assertEqual(report.binary, DEFAULT_BINARY)
        self.assertEqual(report.launchable, DEFAULT_DESKTOP_ID)
        self.assertEqual(report.license_path, "/usr/share/doc/echo/copyright")

    def test_deb_fixture_rejects_desktop_binary_drift(self):
        with tempfile.TemporaryDirectory() as temp:
            package = pathlib.Path(temp) / "echo_1.0.0_amd64.deb"
            _write_deb(package, desktop_exec="wrong-binary")
            report = self.validate_deb(package)

        self.assertFalse(report.ok)
        self.assertIn("desktop Exec binary mismatch: expected 'echo-desktop', got 'wrong-binary'", report.errors)


if __name__ == "__main__":
    unittest.main()
