#!/usr/bin/env python3
"""Synthetic package checks for rewrite_linux_package_identity.py."""
from __future__ import annotations

import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "rewrite_linux_package_identity",
    Path(__file__).with_name("rewrite_linux_package_identity.py"),
)
REWRITE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REWRITE)


class RewriteLinuxPackageIdentityTests(unittest.TestCase):
    def test_deb_package_becomes_echo_without_renaming_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = REWRITE.build_synthetic_deb(root)
            rewritten = REWRITE.rewrite_deb(original)
            self.assertEqual(rewritten.name, "echo_1.0.0_amd64.deb")
            self.assertFalse(original.exists())
            package = subprocess.check_output(
                ["dpkg-deb", "-f", str(rewritten), "Package"], text=True
            ).strip()
            self.assertEqual(package, "echo")
            for field in ("Replaces", "Conflicts", "Provides"):
                value = subprocess.check_output(
                    ["dpkg-deb", "-f", str(rewritten), field], text=True
                ).strip()
                self.assertEqual(value, "io.github.ddv1982.echo")
            members = REWRITE.deb_data_members(rewritten)
            self.assertIn(
                "usr/share/applications/io.github.ddv1982.echo.desktop", members
            )
            self.assertNotIn("usr/share/applications/echo.desktop", members)
            self.assertIn("usr/bin/echo-desktop", members)

    @unittest.skipUnless(
        REWRITE.rpm_tools_available(), "rpmrebuild/rpmbuild not installed"
    )
    def test_rpm_name_becomes_echo_keeping_release(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = REWRITE.build_synthetic_rpm(root)
            rewritten = REWRITE.rewrite_rpm(original)
            name = REWRITE.rpm_query(rewritten, "%{NAME}")
            version = REWRITE.rpm_query(rewritten, "%{VERSION}")
            release = REWRITE.rpm_query(rewritten, "%{RELEASE}")
            arch = REWRITE.rpm_query(rewritten, "%{ARCH}")
            self.assertEqual(name, "echo")
            self.assertEqual(release, "1")
            self.assertEqual(
                rewritten.name, f"echo-{version}-{release}.{arch}.rpm"
            )
            for tag in ("Obsoletes", "Conflicts", "Provides"):
                self.assertTrue(
                    REWRITE.rpm_has_relation(rewritten, tag, REWRITE.OLD_PACKAGE),
                    f"missing {tag} {REWRITE.OLD_PACKAGE}",
                )


if __name__ == "__main__":
    raise SystemExit(unittest.main())
