#!/usr/bin/env python3
"""Validate the Echo APT installer template and rendered fingerprint."""

from __future__ import annotations

import argparse
import hashlib
import os
import pathlib
import re
import stat
import subprocess
import sys
import tempfile

FINGERPRINT_VARIABLE = "ECHO_REPOSITORY_SETUP_SIGNING_KEY_FINGERPRINT"
PLACEHOLDER = "__ECHO_APT_SIGNING_KEY_FINGERPRINT__"
SAMPLE_FINGERPRINT = "0123456789ABCDEF0123456789ABCDEF01234567"
OVERRIDE_FINGERPRINT = "89ABCDEF0123456789ABCDEF0123456789ABCDEF"
NORMALIZATION_LINE = (
    "expected_signing_fingerprint=\"$(printf '%s' \"$expected_signing_fingerprint\" "
    "| tr -d '[:space:]' | tr '[:lower:]' '[:upper:]')\""
)
SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
TEMPLATE_PATH = SCRIPT_DIR / "install-apt-repo.sh"
EFFECTIVE_PREFIX = "ECHO_EFFECTIVE_FINGERPRINT="


def normalize_fingerprint(value: str) -> str:
    return re.sub(r"\s+", "", value).upper()


def is_fingerprint(value: str) -> bool:
    return re.fullmatch(r"[A-F0-9]{40}", value) is not None


def probe_effective_fingerprint(script: str, env_overrides: dict[str, str]) -> str:
    if NORMALIZATION_LINE not in script:
        raise ValueError("Could not find the installer fingerprint normalization line to instrument.")

    injected = (
        f"{NORMALIZATION_LINE}\n"
        f"printf '{EFFECTIVE_PREFIX}%s\\n' \"$expected_signing_fingerprint\"\n"
        "exit 0"
    )
    instrumented = script.replace(NORMALIZATION_LINE, injected, 1)
    with tempfile.TemporaryDirectory(prefix="echo-installer-check-") as tmp:
        script_path = pathlib.Path(tmp) / "install-apt-repo.sh"
        script_path.write_text(instrumented, encoding="utf-8")
        script_path.chmod(script_path.stat().st_mode | stat.S_IXUSR)
        env = os.environ.copy()
        env.pop(FINGERPRINT_VARIABLE, None)
        env.update(env_overrides)
        result = subprocess.run(
            ["sh", str(script_path)],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )
        if result.returncode != 0:
            raise ValueError(
                f"Instrumented installer exited with {result.returncode}: {result.stderr or result.stdout}"
            )
        match = re.search(rf"^{EFFECTIVE_PREFIX}(.*)$", result.stdout, re.MULTILINE)
        if not match:
            raise ValueError(f"Instrumented installer did not print the effective fingerprint. Output: {result.stdout}")
        return match.group(1)


def validate_template(script: str) -> None:
    if PLACEHOLDER not in script:
        raise ValueError(f"scripts/install-apt-repo.sh must contain {PLACEHOLDER} for release-time rendering")

    default_fingerprint = probe_effective_fingerprint(script, {})
    if default_fingerprint != "":
        raise ValueError("The template installer must not trust an unresolved placeholder fingerprint by default.")

    env_fingerprint = probe_effective_fingerprint(
        script,
        {FINGERPRINT_VARIABLE: f" {OVERRIDE_FINGERPRINT.lower()} "},
    )
    if env_fingerprint != OVERRIDE_FINGERPRINT:
        raise ValueError(f"The template installer did not preserve an explicit {FINGERPRINT_VARIABLE} override.")


def validate_rendered_installer(script: str, expected: str) -> None:
    if PLACEHOLDER in script:
        raise ValueError(f"Rendered APT installer still contains {PLACEHOLDER}")

    default_fingerprint = probe_effective_fingerprint(script, {})
    if default_fingerprint != expected:
        raise ValueError(
            f"Rendered APT installer default fingerprint resolved to {default_fingerprint or '<empty>'}, expected {expected}"
        )

    env_fingerprint = probe_effective_fingerprint(
        script,
        {FINGERPRINT_VARIABLE: f" {OVERRIDE_FINGERPRINT.lower()} "},
    )
    if env_fingerprint != OVERRIDE_FINGERPRINT:
        raise ValueError(f"Rendered APT installer did not preserve an explicit {FINGERPRINT_VARIABLE} override.")


def validate_apt_install_staging(script: str) -> None:
    dummy_deb = b"dummy repository setup package\n"
    expected_sha256 = hashlib.sha256(dummy_deb).hexdigest()
    with tempfile.TemporaryDirectory(prefix="echo-installer-apt-check-") as tmp:
        tmp_dir = pathlib.Path(tmp)
        fake_bin = tmp_dir / "bin"
        fake_bin.mkdir()
        dummy_deb_path = tmp_dir / "repository-setup.deb"
        dummy_deb_path.write_bytes(dummy_deb)
        script_path = tmp_dir / "install-apt-repo.sh"
        script_path.write_text(script, encoding="utf-8")
        script_path.chmod(script_path.stat().st_mode | stat.S_IXUSR)

        sudo_path = fake_bin / "sudo"
        sudo_path.write_text(
            """#!/bin/sh
set -eu

mode_for() {
  if stat -c %a "$1" >/dev/null 2>&1; then
    stat -c %a "$1"
  else
    stat -f %Lp "$1"
  fi
}

if [ "$#" -ne 4 ] || [ "$1" != "apt" ] || [ "$2" != "install" ] || [ "$3" != "-y" ]; then
  echo "Unexpected sudo invocation: $*" >&2
  exit 41
fi

deb="$4"
dir="$(dirname "$deb")"
dir_mode="$(mode_for "$dir")"
file_mode="$(mode_for "$deb")"

printf 'ECHO_APT_INSTALL_DEB=%s\\n' "$deb"
printf 'ECHO_APT_INSTALL_DIR_MODE=%s\\n' "$dir_mode"
printf 'ECHO_APT_INSTALL_FILE_MODE=%s\\n' "$file_mode"

case "$deb" in
  */echo-repository-install.*/*)
    ;;
  *)
    echo "APT install did not use the public installer staging directory: $deb" >&2
    exit 42
    ;;
esac

if [ "$dir_mode" != "755" ]; then
  echo "APT installer staging directory mode is $dir_mode, expected 755." >&2
  exit 43
fi

if [ "$file_mode" != "644" ]; then
  echo "APT installer package mode is $file_mode, expected 644." >&2
  exit 44
fi
""",
            encoding="utf-8",
        )
        sudo_path.chmod(sudo_path.stat().st_mode | stat.S_IXUSR)

        curl_path = fake_bin / "curl"
        curl_path.write_text(
            """#!/bin/sh
set -eu

output=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -fsSLo)
      output="$2"
      shift 2
      ;;
    file://*)
      url="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done

if [ -z "$output" ] || [ -z "$url" ]; then
  echo "Unexpected curl invocation" >&2
  exit 51
fi

cp "${url#file://}" "$output"
""",
            encoding="utf-8",
        )
        curl_path.chmod(curl_path.stat().st_mode | stat.S_IXUSR)

        env = os.environ.copy()
        env["PATH"] = f"{fake_bin}{os.pathsep}{env.get('PATH', '')}"
        env["TMPDIR"] = str(tmp_dir)
        env["ECHO_REPOSITORY_SETUP_URL"] = dummy_deb_path.resolve().as_uri()
        env["ECHO_REPOSITORY_SETUP_SHA256"] = expected_sha256
        result = subprocess.run(
            ["sh", str(script_path)],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )
        if result.returncode != 0:
            raise ValueError(
                f"Installer APT staging probe exited with {result.returncode}:\n{result.stdout}{result.stderr}"
            )
        for expected_line in (
            "ECHO_APT_INSTALL_DIR_MODE=755",
            "ECHO_APT_INSTALL_FILE_MODE=644",
        ):
            if expected_line not in result.stdout:
                raise ValueError(
                    f"Installer APT staging probe did not print {expected_line}. Output:\n{result.stdout}"
                )


def load_template() -> str:
    return TEMPLATE_PATH.read_text(encoding="utf-8")


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Check the Echo APT installer fingerprint rendering.")
    parser.add_argument("--rendered-installer", help="path to a fingerprint-substituted installer")
    parser.add_argument("--expected-fingerprint", help="40-character hex fingerprint expected in a rendered installer")
    return parser


def main() -> int:
    parser = build_arg_parser()
    args = parser.parse_args()
    expected = normalize_fingerprint(args.expected_fingerprint or SAMPLE_FINGERPRINT)
    if not is_fingerprint(expected):
        print(
            f"error: expected signing fingerprint must be a 40-character hex fingerprint, got {expected}",
            file=sys.stderr,
        )
        return 1

    try:
        template = load_template()
        validate_template(template)
        if args.rendered_installer:
            rendered = pathlib.Path(args.rendered_installer).read_text(encoding="utf-8")
        else:
            rendered = template.replace(PLACEHOLDER, expected)
        validate_rendered_installer(rendered, expected)
        validate_apt_install_staging(rendered)
    except Exception as error:  # noqa: BLE001 - CLI should surface concise failures
        print(f"error: {error}", file=sys.stderr)
        return 1
    print("APT installer check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
