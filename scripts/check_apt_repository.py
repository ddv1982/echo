#!/usr/bin/env python3
"""Build a signed Echo APT repo from a fixture .deb and verify InRelease."""

from __future__ import annotations

import argparse
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

from build_apt_repository import (
    DEFAULT_COMPONENT_ID,
    DEFAULT_ORIGIN,
    DEFAULT_SETUP_PACKAGE_NAME,
    APPLICATION_POOL_DIR,
    build_arg_parser as build_repository_arg_parser,
    build_repository,
    write_deb_archive,
)
from guard_apt_publication import repository_identity

FIXTURE_VERSION = "1.0.0"
FIXTURE_ARCH = "amd64"
FIXTURE_PACKAGE = "echo"
FIXTURE_CONTROL = f"""Package: {FIXTURE_PACKAGE}
Version: {FIXTURE_VERSION}
Architecture: {FIXTURE_ARCH}
Maintainer: Douwe de Vries <douwe.de.vries.82@gmail.com>
Section: utils
Priority: optional
Homepage: https://github.com/ddv1982/echo
Description: Echo dictation
 Local-only voice dictation for the desktop.
"""
FIXTURE_DESKTOP = f"""[Desktop Entry]
Name=Echo
Exec=/usr/bin/echo-desktop
Icon={DEFAULT_COMPONENT_ID}
Type=Application
Categories=Utility;AudioVideo;
"""
FIXTURE_METAINFO = f"""<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{DEFAULT_COMPONENT_ID}</id>
  <name>Echo</name>
  <summary>Local-only voice dictation</summary>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <description>
    <p>Echo is local-only voice dictation for the desktop.</p>
  </description>
  <url type="homepage">https://github.com/ddv1982/echo</url>
  <launchable type="desktop-id">{DEFAULT_COMPONENT_ID}.desktop</launchable>
  <provides>
    <binary>echo-desktop</binary>
  </provides>
  <releases>
    <release version="{FIXTURE_VERSION}" date="2026-09-07"/>
  </releases>
</component>
"""


def write_fixture_deb(output: pathlib.Path) -> pathlib.Path:
    write_deb_archive(
        output,
        {"./control": (FIXTURE_CONTROL.encode("utf-8"), 0o644)},
        {
            f"./usr/share/metainfo/{DEFAULT_COMPONENT_ID}.metainfo.xml": (
                FIXTURE_METAINFO.encode("utf-8"),
                0o644,
            ),
            f"./usr/share/applications/{DEFAULT_COMPONENT_ID}.desktop": (
                FIXTURE_DESKTOP.encode("utf-8"),
                0o644,
            ),
        },
    )
    return output


def run_checked(command: list[str], env: dict[str, str], **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        env=env,
        check=True,
        text=True,
        stdout=kwargs.get("stdout", subprocess.PIPE),
        stderr=kwargs.get("stderr", subprocess.PIPE),
        input=kwargs.get("input"),
    )


def gpg_env(homedir: pathlib.Path) -> dict[str, str]:
    env = os.environ.copy()
    env["GNUPGHOME"] = str(homedir)
    env["LC_ALL"] = "C"
    return env


def kill_agent(homedir: pathlib.Path) -> None:
    subprocess.run(
        ["gpgconf", "--kill", "gpg-agent"],
        env=gpg_env(homedir),
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def prepare_gnupg_home(homedir: pathlib.Path) -> None:
    homedir.mkdir(parents=True, exist_ok=True)
    os.chmod(homedir, 0o700)
    (homedir / "gpg-agent.conf").write_text("allow-loopback-pinentry\n", encoding="utf-8")
    env = gpg_env(homedir)
    run_checked(["gpgconf", "--launch", "gpg-agent"], env)
    run_checked(["gpg-connect-agent", "/bye"], env)


def read_first_fingerprint(output: str) -> str:
    for line in output.splitlines():
        fields = line.split(":")
        if fields and fields[0] == "fpr" and len(fields) > 9 and fields[9]:
            return fields[9]
    raise RuntimeError("Could not find generated APT check signing key fingerprint.")


def generate_signing_key(homedir: pathlib.Path, parameters_path: pathlib.Path) -> str:
    parameters_path.write_text(
        """%no-protection
Key-Type: eddsa
Key-Curve: ed25519
Name-Real: Echo APT Check
Name-Email: apt-check@echo.local
Expire-Date: 0
%commit
""",
        encoding="utf-8",
    )
    env = gpg_env(homedir)
    run_checked(
        [
            "gpg",
            "--quiet",
            "--batch",
            "--homedir",
            str(homedir),
            "--pinentry-mode",
            "loopback",
            "--generate-key",
            str(parameters_path),
        ],
        env,
    )
    listed = run_checked(
        ["gpg", "--batch", "--homedir", str(homedir), "--with-colons", "--list-secret-keys"],
        env,
    )
    return read_first_fingerprint(listed.stdout)


def verify_signatures(verification_home: pathlib.Path, public_key: pathlib.Path, repo_root: pathlib.Path) -> None:
    env = gpg_env(verification_home)
    prepare_gnupg_home(verification_home)
    run_checked(
        ["gpg", "--quiet", "--batch", "--homedir", str(verification_home), "--import", str(public_key)],
        env,
    )
    dists = repo_root / "dists" / "stable"
    run_checked(
        [
            "gpg",
            "--quiet",
            "--batch",
            "--homedir",
            str(verification_home),
            "--trust-model",
            "always",
            "--verify",
            str(dists / "Release.gpg"),
            str(dists / "Release"),
        ],
        env,
    )
    run_checked(
        [
            "gpg",
            "--quiet",
            "--batch",
            "--homedir",
            str(verification_home),
            "--trust-model",
            "always",
            "--verify",
            str(dists / "InRelease"),
        ],
        env,
    )


def assert_repository_layout(repo_root: pathlib.Path, public_key: pathlib.Path, setup_package: pathlib.Path) -> None:
    pool_deb = repo_root / APPLICATION_POOL_DIR / f"{FIXTURE_PACKAGE}_{FIXTURE_VERSION}_{FIXTURE_ARCH}.deb"
    if not pool_deb.is_file():
        raise RuntimeError(f"missing pool package: {pool_deb}")
    release = (repo_root / "dists" / "stable" / "Release").read_text(encoding="utf-8")
    if f"Origin: {DEFAULT_ORIGIN}" not in release:
        raise RuntimeError("Release Origin does not match echo-stable-main")
    if "Label: Echo" not in release:
        raise RuntimeError("Release Label does not match Echo")
    if not (repo_root / "dists" / "stable" / "InRelease").is_file():
        raise RuntimeError("missing InRelease")
    if not public_key.is_file() or public_key.stat().st_size == 0:
        raise RuntimeError("exported archive keyring is missing or empty")
    if not setup_package.is_file():
        raise RuntimeError("setup package was not written")
    if setup_package.name != f"{DEFAULT_SETUP_PACKAGE_NAME}_1.0_all.deb":
        raise RuntimeError(f"unexpected setup package name: {setup_package.name}")


def check_repository(deb_path: pathlib.Path | None) -> None:
    gpg = shutil.which("gpg")
    if not gpg:
        raise RuntimeError("gpg is required to verify a signed APT repository")

    work = pathlib.Path(tempfile.mkdtemp(prefix="echoapt-"))
    gnupg_home = pathlib.Path(tempfile.mkdtemp(prefix="echoag-"))
    verification_home = pathlib.Path(tempfile.mkdtemp(prefix="echoav-"))
    try:
        prepare_gnupg_home(gnupg_home)
        fingerprint = generate_signing_key(gnupg_home, work / "key-parameters")
        if deb_path is None:
            deb_path = write_fixture_deb(work / f"{FIXTURE_PACKAGE}_{FIXTURE_VERSION}_{FIXTURE_ARCH}.deb")
        output_dir = work / "repository"
        public_key = work / "echo-archive-keyring.pgp"
        setup_package = work / f"{DEFAULT_SETUP_PACKAGE_NAME}_1.0_all.deb"
        args = build_repository_arg_parser().parse_args(
            [
                str(deb_path),
                "--output",
                str(output_dir),
                "--gpg-key",
                fingerprint,
                "--gpg-homedir",
                str(gnupg_home),
                "--public-key-out",
                str(public_key),
                "--setup-package-out",
                str(setup_package),
                "--clean",
            ]
        )
        build_repository(args)
        assert_repository_layout(output_dir, public_key, setup_package)
        verify_signatures(verification_home, public_key, output_dir)
        identity = repository_identity(
            lambda path: (output_dir / path).read_bytes(), public_key, verify_packages=True,
        )
        if identity["version"] != FIXTURE_VERSION or set(identity["packages"]) != {FIXTURE_ARCH}:
            raise RuntimeError("signed publication identity does not match the fixture package")
    finally:
        kill_agent(gnupg_home)
        kill_agent(verification_home)
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(gnupg_home, ignore_errors=True)
        shutil.rmtree(verification_home, ignore_errors=True)


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Verify a signed Echo APT repository from a fixture .deb.")
    parser.add_argument("--deb", help="optional application .deb; a synthetic fixture is used when omitted")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="synthesize a fixture .deb and verify InRelease (default when --deb is omitted)",
    )
    return parser


def main() -> int:
    parser = build_arg_parser()
    args = parser.parse_args()
    deb_path = None if args.self_test or not args.deb else pathlib.Path(args.deb)
    try:
        check_repository(deb_path)
    except Exception as error:  # noqa: BLE001 - CLI should surface concise failures
        print(f"error: {error}", file=sys.stderr)
        return 1
    print("Signed APT repository check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
