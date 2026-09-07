#!/usr/bin/env python3
"""Fixture tests for Echo APT control parsing, setup sources, and pool paths."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tempfile
import unittest

import build_apt_repository as apt
import check_apt_installer
from check_apt_repository import write_fixture_deb
from deb_common import parse_desktop_file, read_ar_entries

ROOT = pathlib.Path(__file__).resolve().parent.parent
INSTALLER = ROOT / "scripts" / "install-apt-repo.sh"
PLACEHOLDER = check_apt_installer.PLACEHOLDER
SAMPLE_FINGERPRINT = check_apt_installer.SAMPLE_FINGERPRINT


class ControlParsingTests(unittest.TestCase):
    def test_parse_control_reads_package_version_architecture(self):
        fields = apt.parse_control(
            b"Package: echo\nVersion: 1.0.0\nArchitecture: amd64\nMaintainer: Test\nDescription: Echo\n"
        )
        self.assertEqual(fields["Package"], "echo")
        self.assertEqual(fields["Version"], "1.0.0")
        self.assertEqual(fields["Architecture"], "amd64")

    def test_fixture_deb_control_and_desktop(self):
        with tempfile.TemporaryDirectory() as tmp:
            deb = write_fixture_deb(pathlib.Path(tmp) / "echo_1.0.0_amd64.deb")
            fields, metainfo, desktop = apt.deb_control_metainfo_and_desktop(deb)
            self.assertEqual(fields["Package"], "echo")
            self.assertIn(b"<id>io.github.ddv1982.echo</id>", metainfo)
            self.assertEqual(desktop["Exec"], "/usr/bin/echo-desktop")
            self.assertEqual(parse_desktop_file(b"[Desktop Entry]\nName=Echo\n")["Name"], "Echo")


class SetupSourcesTests(unittest.TestCase):
    def test_setup_sources_pin_signed_by_echo_keyring(self):
        args = argparse.Namespace(
            repository_url=apt.DEFAULT_REPOSITORY_URL,
            suite=apt.DEFAULT_SUITE,
            component=apt.DEFAULT_COMPONENT,
        )
        text = apt.setup_sources_text(args, ["amd64"], apt.DEFAULT_SETUP_KEYRING_PATH)
        self.assertIn("Types: deb", text)
        self.assertIn(f"URIs: {apt.DEFAULT_REPOSITORY_URL}", text)
        self.assertIn("Suites: stable", text)
        self.assertIn("Components: main", text)
        self.assertIn(f"Signed-By: {apt.DEFAULT_SETUP_KEYRING_PATH}", text)

    def test_setup_package_installs_echo_sources_with_signed_by(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            deb = write_fixture_deb(tmp_path / "echo_1.0.0_amd64.deb")
            keyring = tmp_path / "echo-archive-keyring.pgp"
            keyring.write_bytes(b"dummy-echo-keyring\n")
            setup_out = tmp_path / "echo-repository-setup_1.0_all.deb"
            repo = tmp_path / "repo"
            args = apt.build_arg_parser().parse_args(
                [
                    str(deb),
                    "--output",
                    str(repo),
                    "--unsigned",
                    "--clean",
                    "--setup-package-out",
                    str(setup_out),
                    "--setup-public-key",
                    str(keyring),
                ]
            )
            apt.build_repository(args)
            self.assertTrue(setup_out.is_file())
            entries = read_ar_entries(setup_out)
            data_name = next(name for name in entries if name.startswith("data.tar"))
            files = apt.extract_tar_member(data_name, entries[data_name], lambda name: name.endswith("echo.sources"))
            sources_name = "etc/apt/sources.list.d/echo.sources"
            self.assertIn(sources_name, files)
            sources = files[sources_name].decode("utf-8")
            self.assertIn(f"Signed-By: {apt.DEFAULT_SETUP_KEYRING_PATH}", sources)
            self.assertIn("Types: deb", sources)


class FingerprintRenderingTests(unittest.TestCase):
    def test_template_contains_placeholder(self):
        template = INSTALLER.read_text(encoding="utf-8")
        self.assertIn(PLACEHOLDER, template)
        check_apt_installer.validate_template(template)

    def test_rendered_installer_uses_expected_fingerprint(self):
        template = INSTALLER.read_text(encoding="utf-8")
        rendered = template.replace(PLACEHOLDER, SAMPLE_FINGERPRINT)
        check_apt_installer.validate_rendered_installer(rendered, SAMPLE_FINGERPRINT)

    def test_refuses_unsubstituted_placeholder(self):
        template = INSTALLER.read_text(encoding="utf-8")
        with self.assertRaisesRegex(ValueError, PLACEHOLDER):
            check_apt_installer.validate_rendered_installer(template, SAMPLE_FINGERPRINT)

    def test_check_script_rejects_template_as_rendered(self):
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts" / "check_apt_installer.py"),
                "--rendered-installer",
                str(INSTALLER),
                "--expected-fingerprint",
                SAMPLE_FINGERPRINT,
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(PLACEHOLDER, result.stderr)


class PoolPathTests(unittest.TestCase):
    def test_pool_path_is_echo_deb_under_main_e_echo(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            deb = write_fixture_deb(tmp_path / "echo_1.0.0_amd64.deb")
            repo = tmp_path / "repo"
            args = apt.build_arg_parser().parse_args(
                [
                    str(deb),
                    "--output",
                    str(repo),
                    "--unsigned",
                    "--clean",
                ]
            )
            apt.build_repository(args)
            pooled = repo / apt.APPLICATION_POOL_DIR / "echo_1.0.0_amd64.deb"
            self.assertTrue(pooled.is_file(), pooled)
            packages = (repo / "dists" / "stable" / "main" / "binary-amd64" / "Packages").read_text(encoding="utf-8")
            self.assertIn("Package: echo", packages)
            self.assertIn("Filename: pool/main/e/echo/echo_1.0.0_amd64.deb", packages)
            dep11 = (repo / "dists" / "stable" / "main" / "dep11" / "Components-amd64.yml").read_text(encoding="utf-8")
            self.assertIn("ID: 'io.github.ddv1982.echo'", dep11)
            self.assertIn("Package: 'echo'", dep11)

    def test_glob_ignores_setup_debs(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            write_fixture_deb(tmp_path / "echo_1.0.0_amd64.deb")
            (tmp_path / "echo-repository-setup_1.0_all.deb").write_bytes(b"not-a-real-deb")
            selected = apt.select_application_debs(sorted(tmp_path.glob("*.deb")))
            self.assertEqual([path.name for path in selected], ["echo_1.0.0_amd64.deb"])


if __name__ == "__main__":
    unittest.main()
