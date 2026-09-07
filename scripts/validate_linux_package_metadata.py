#!/usr/bin/env python3
"""Validate Echo Linux package metadata for repository publishing."""

from __future__ import annotations

import argparse
import glob
import io
import json
import pathlib
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from typing import Any

_SCRIPTS = pathlib.Path(__file__).resolve().parent
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))

from deb_common import (  # noqa: E402
    child_text,
    decompress_deb_tar_member,
    descendant_text,
    first_release,
    launchable_desktop_id,
    parse_desktop_file as parse_desktop_bytes,
    read_ar_entries,
)


DEFAULT_COMPONENT_ID = "io.github.ddv1982.echo"
DEFAULT_LICENSE = "MIT"
DEFAULT_BINARY = "echo-desktop"
DEFAULT_DESKTOP_ID = "io.github.ddv1982.echo.desktop"
DEFAULT_RPM_PACKAGE_NAME = "echo"
DEFAULT_RPM_ARCH = "x86_64"

REQUIRED_DEB_COPYRIGHT_PATH = pathlib.PurePosixPath("usr/share/doc/echo/copyright")
REQUIRED_RPM_LICENSE_PATH = pathlib.PurePosixPath("usr/share/licenses/echo/LICENSE")
CPIO_HEADER_LEN = 110
CPIO_MODE_TYPE_MASK = 0o170000
CPIO_MODE_DIRECTORY = 0o040000
CPIO_MODE_REGULAR_FILE = 0o100000


@dataclass
class ToolResult:
    name: str
    available: bool
    command: list[str] = field(default_factory=list)
    returncode: int | None = None
    stdout: str = ""
    stderr: str = ""

    @property
    def ok(self) -> bool:
        return self.available and self.returncode == 0

    def to_json(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "available": self.available,
            "command": self.command,
            "returncode": self.returncode,
            "stdout": self.stdout,
            "stderr": self.stderr,
        }


@dataclass
class PackageReport:
    package: str
    package_format: str | None = None
    ok: bool = False
    extraction: str | None = None
    metainfo_path: str | None = None
    desktop_path: str | None = None
    license_path: str | None = None
    component_id: str | None = None
    metadata_license: str | None = None
    project_license: str | None = None
    launchable: str | None = None
    binary: str | None = None
    release_version: str | None = None
    release_date: str | None = None
    desktop_fields: dict[str, str] = field(default_factory=dict)
    detected_desktop_ids: list[str] = field(default_factory=list)
    icon_paths: list[str] = field(default_factory=list)
    rpm_header_fields: dict[str, str] = field(default_factory=dict)
    appstream_validation: ToolResult | None = None
    desktop_validation: ToolResult | None = None
    rpm_header_validation: ToolResult | None = None
    errors: list[str] = field(default_factory=list)

    def to_json(self) -> dict[str, Any]:
        return {
            "package": self.package,
            "package_format": self.package_format,
            "ok": self.ok,
            "extraction": self.extraction,
            "metainfo_path": self.metainfo_path,
            "desktop_path": self.desktop_path,
            "license_path": self.license_path,
            "component_id": self.component_id,
            "metadata_license": self.metadata_license,
            "project_license": self.project_license,
            "launchable": self.launchable,
            "binary": self.binary,
            "release_version": self.release_version,
            "release_date": self.release_date,
            "desktop_fields": self.desktop_fields,
            "detected_desktop_ids": self.detected_desktop_ids,
            "icon_paths": self.icon_paths,
            "rpm_header_fields": self.rpm_header_fields,
            "appstream_validation": self.appstream_validation.to_json() if self.appstream_validation else None,
            "desktop_validation": self.desktop_validation.to_json() if self.desktop_validation else None,
            "rpm_header_validation": self.rpm_header_validation.to_json() if self.rpm_header_validation else None,
            "errors": self.errors,
        }


def safe_extract_tar(tar: tarfile.TarFile, destination: pathlib.Path) -> None:
    destination = destination.resolve()
    for member in tar.getmembers():
        output_path = destination / member.name.removeprefix("./")
        try:
            output_path.resolve().relative_to(destination)
        except ValueError as error:
            raise ValueError(f"refusing unsafe tar member path: {member.name}") from error
        if member.isdir():
            output_path.mkdir(parents=True, exist_ok=True)
            continue
        if not member.isreg():
            raise ValueError(f"refusing non-regular tar member: {member.name}")
        output_path.parent.mkdir(parents=True, exist_ok=True)
        source = tar.extractfile(member)
        if source is None:
            raise ValueError(f"missing tar member payload: {member.name}")
        with source, output_path.open("wb") as handle:
            shutil.copyfileobj(source, handle)
        output_path.chmod(member.mode & 0o777)


def extract_data_archive(data_name: str, data: bytes, destination: pathlib.Path) -> str:
    data, mode = decompress_deb_tar_member(data_name, data)
    extraction = "python-ar+zstd+tarfile" if mode == "r:" else "python-ar+tarfile"
    with tarfile.open(fileobj=io.BytesIO(data), mode=mode) as tar:
        safe_extract_tar(tar, destination)
    return extraction


def extract_deb(deb_path: pathlib.Path, destination: pathlib.Path) -> str:
    entries = read_ar_entries(deb_path)
    data_member = next((name for name in entries if name.startswith("data.tar")), None)
    if not data_member:
        raise ValueError("Debian archive does not contain data.tar.*")
    return extract_data_archive(data_member, entries[data_member], destination)


def cpio_pad4(value: int) -> int:
    return (4 - (value % 4)) % 4


def cpio_hex_field(header: bytes, start: int) -> int:
    return int(header[start : start + 8].decode("ascii"), 16)


def safe_cpio_relative_path(raw_name: str) -> pathlib.PurePosixPath:
    name = raw_name.removeprefix("./")
    path = pathlib.PurePosixPath(name)
    if not name or name == "." or path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise ValueError(f"refusing unsafe cpio member path: {raw_name!r}")
    return path


def extract_newc_cpio(data: bytes, destination: pathlib.Path) -> None:
    offset = 0
    seen_paths: set[str] = set()
    while True:
        if offset + CPIO_HEADER_LEN > len(data):
            raise ValueError("truncated newc cpio header")
        header = data[offset : offset + CPIO_HEADER_LEN]
        offset += CPIO_HEADER_LEN
        if header[:6] not in {b"070701", b"070702"}:
            raise ValueError(f"unsupported cpio format: {header[:6]!r}")

        mode = cpio_hex_field(header, 14)
        nlink = cpio_hex_field(header, 38)
        file_size = cpio_hex_field(header, 54)
        name_size = cpio_hex_field(header, 94)
        raw_name = data[offset : offset + name_size]
        offset += name_size
        offset += cpio_pad4(CPIO_HEADER_LEN + name_size)
        if not raw_name.endswith(b"\0"):
            raise ValueError("newc cpio member name is not NUL-terminated")
        member_name = raw_name[:-1].decode("utf-8")
        payload = data[offset : offset + file_size]
        offset += file_size
        offset += cpio_pad4(file_size)

        if member_name == "TRAILER!!!":
            return

        relative_path = safe_cpio_relative_path(member_name)
        path_key = relative_path.as_posix()
        if path_key in seen_paths:
            raise ValueError(f"refusing duplicate cpio member path: {member_name!r}")
        seen_paths.add(path_key)

        mode_type = mode & CPIO_MODE_TYPE_MASK
        output_path = destination.joinpath(*relative_path.parts)
        if mode_type == CPIO_MODE_DIRECTORY:
            output_path.mkdir(parents=True, exist_ok=True)
            continue
        if mode_type != CPIO_MODE_REGULAR_FILE or nlink != 1:
            continue
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_bytes(payload)
        output_path.chmod(mode & 0o777)


def extract_rpm(rpm_path: pathlib.Path, destination: pathlib.Path) -> str:
    errors: list[str] = []
    rpm2cpio = shutil.which("rpm2cpio")
    if rpm2cpio:
        completed = subprocess.run([rpm2cpio, str(rpm_path)], check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if completed.returncode == 0:
            try:
                extract_newc_cpio(completed.stdout, destination)
                return "rpm2cpio+python-newc"
            except Exception as error:  # noqa: BLE001 - keep fallback error context concise
                errors.append(f"rpm2cpio output could not be read as newc cpio: {error}")
        else:
            stderr = completed.stderr.decode("utf-8", errors="replace").strip()
            errors.append(f"rpm2cpio failed: {stderr or f'exit {completed.returncode}'}")
    else:
        errors.append("rpm2cpio is missing")

    bsdtar = shutil.which("bsdtar")
    if bsdtar:
        completed = subprocess.run([bsdtar, "-xf", str(rpm_path), "-C", str(destination)], check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if completed.returncode == 0:
            return "bsdtar"
        stderr = completed.stderr.decode("utf-8", errors="replace").strip()
        errors.append(f"bsdtar failed: {stderr or f'exit {completed.returncode}'}")
    else:
        errors.append("bsdtar is missing")

    raise ValueError("; ".join(errors))


def package_format(package_path: pathlib.Path) -> str:
    if package_path.suffix == ".rpm":
        return "rpm"
    if package_path.suffix == ".deb":
        return "deb"
    raise ValueError(f"unsupported Linux package extension: {package_path.suffix or package_path.name}")


def extract_package(package_path: pathlib.Path, destination: pathlib.Path) -> tuple[str, str]:
    format_name = package_format(package_path)
    if format_name == "rpm":
        return format_name, extract_rpm(package_path, destination)
    return format_name, extract_deb(package_path, destination)


def run_optional_tool(name: str, args: list[str]) -> ToolResult:
    executable = shutil.which(name)
    command = [name, *args]
    if not executable:
        return ToolResult(name=name, available=False, command=command)
    completed = subprocess.run([executable, *args], check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return ToolResult(
        name=name,
        available=True,
        command=command,
        returncode=completed.returncode,
        stdout=completed.stdout.strip(),
        stderr=completed.stderr.strip(),
    )


def query_rpm_header(package_path: pathlib.Path) -> tuple[ToolResult, dict[str, str]]:
    query_format = "\\n".join(["name=%{NAME}", "version=%{VERSION}", "arch=%{ARCH}", "summary=%{SUMMARY}", "license=%{LICENSE}"])
    result = run_optional_tool("rpm", ["-qp", "--queryformat", query_format, str(package_path)])
    fields: dict[str, str] = {}
    if result.ok:
        for line in result.stdout.splitlines():
            if "=" in line:
                key, value = line.split("=", 1)
                fields[key] = value.strip()
    return result, fields


def parse_desktop_file(path: pathlib.Path) -> dict[str, str]:
    return parse_desktop_bytes(path.read_bytes())


def exec_binary(exec_field: str | None) -> str | None:
    if not exec_field:
        return None
    try:
        parts = shlex.split(exec_field)
    except ValueError:
        parts = exec_field.strip().split(maxsplit=1)
    if not parts:
        return None
    binary = parts[0]
    return pathlib.PurePosixPath(binary).name


def installed_icon_paths(root: pathlib.Path, icon_name: str | None) -> list[str]:
    if not icon_name or "/" in icon_name:
        return []
    candidates = []
    for path in (root / "usr/share/icons/hicolor").glob(f"*/apps/{icon_name}.*"):
        if path.is_file():
            candidates.append(path)
    pixmap = root / "usr/share/pixmaps" / f"{icon_name}.png"
    if pixmap.is_file():
        candidates.append(pixmap)
    return sorted(f"/{path.relative_to(root).as_posix()}" for path in candidates)


def require_installed_icon(errors: list[str], root: pathlib.Path, icon_name: str | None) -> list[str]:
    paths = installed_icon_paths(root, icon_name)
    if icon_name and not paths:
        errors.append(
            "desktop Icon does not resolve to an installed hicolor/pixmaps icon: "
            f"{icon_name!r}"
        )
    return paths


def require_equal(errors: list[str], label: str, actual: str | None, expected: str) -> None:
    if actual != expected:
        errors.append(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def require_file(errors: list[str], root: pathlib.Path, relative_path: pathlib.PurePosixPath) -> pathlib.Path:
    path = root / pathlib.Path(*relative_path.parts)
    if not path.is_file():
        errors.append(f"missing required file: /{relative_path}")
    return path


def validate_package(
    package_path: pathlib.Path,
    expected_component_id: str,
    expected_license: str,
    expected_binary: str,
    expected_desktop_id: str,
    expected_rpm_package_name: str,
    expected_rpm_arch: str,
    run_external_tools: bool,
    require_external_tools: bool,
) -> PackageReport:
    report = PackageReport(package=str(package_path))
    metainfo_rel = pathlib.PurePosixPath(f"usr/share/metainfo/{expected_component_id}.metainfo.xml")
    desktop_rel = pathlib.PurePosixPath(f"usr/share/applications/{expected_desktop_id}")

    with tempfile.TemporaryDirectory(prefix="echo-linux-metadata-") as temp:
        extract_root = pathlib.Path(temp)
        try:
            report.package_format, report.extraction = extract_package(package_path, extract_root)
        except Exception as error:  # noqa: BLE001 - surface concise validator failure
            report.errors.append(f"failed to extract {package_path}: {error}")
            return report

        applications_dir = extract_root / "usr/share/applications"
        if applications_dir.is_dir():
            report.detected_desktop_ids = sorted(path.name for path in applications_dir.glob("*.desktop"))

        metainfo_path = require_file(report.errors, extract_root, metainfo_rel)
        desktop_path = require_file(report.errors, extract_root, desktop_rel)
        license_rel = REQUIRED_RPM_LICENSE_PATH if report.package_format == "rpm" else REQUIRED_DEB_COPYRIGHT_PATH
        license_path = require_file(report.errors, extract_root, license_rel)

        if metainfo_path.is_file():
            report.metainfo_path = f"/{metainfo_rel}"
            try:
                root = ET.parse(metainfo_path).getroot()
                report.component_id = child_text(root, "id")
                report.metadata_license = child_text(root, "metadata_license")
                report.project_license = child_text(root, "project_license")
                report.launchable = launchable_desktop_id(root)
                report.binary = descendant_text(root, "binary")
                report.release_version, report.release_date = first_release(root)
            except ET.ParseError as error:
                report.errors.append(f"invalid AppStream XML in /{metainfo_rel}: {error}")

            require_equal(report.errors, "AppStream component id", report.component_id, expected_component_id)
            require_equal(report.errors, "AppStream metadata_license", report.metadata_license, expected_license)
            require_equal(report.errors, "AppStream project_license", report.project_license, expected_license)
            require_equal(report.errors, "AppStream provides binary", report.binary, expected_binary)
            require_equal(report.errors, "AppStream desktop launchable", report.launchable, expected_desktop_id)

            if run_external_tools:
                report.appstream_validation = run_optional_tool("appstreamcli", ["validate", "--no-net", str(metainfo_path)])
                if not report.appstream_validation.available:
                    if require_external_tools:
                        report.errors.append("appstreamcli is required for gate validation but was not found")
                elif not report.appstream_validation.ok:
                    report.errors.append(f"appstreamcli validate --no-net failed for /{metainfo_rel} (exit {report.appstream_validation.returncode})")

        if desktop_path.is_file():
            report.desktop_path = f"/{desktop_rel}"
            report.desktop_fields = parse_desktop_file(desktop_path)
            require_equal(report.errors, "desktop Exec binary", exec_binary(report.desktop_fields.get("Exec")), expected_binary)
            if report.desktop_fields.get("Name") is None:
                report.errors.append(f"desktop file {report.desktop_path} is missing Name")
            if report.desktop_fields.get("Icon") is None:
                report.errors.append(f"desktop file {report.desktop_path} is missing Icon")
            else:
                report.icon_paths = require_installed_icon(report.errors, extract_root, report.desktop_fields.get("Icon"))
            if report.desktop_fields.get("NoDisplay", "").strip().lower() == "true":
                report.errors.append(f"desktop file {report.desktop_path} must be visible")

            if run_external_tools:
                report.desktop_validation = run_optional_tool("desktop-file-validate", [str(desktop_path)])
                if not report.desktop_validation.available:
                    if require_external_tools:
                        report.errors.append("desktop-file-validate is required for gate validation but was not found")
                elif not report.desktop_validation.ok:
                    report.errors.append(f"desktop-file-validate failed for {report.desktop_path} (exit {report.desktop_validation.returncode})")

        if license_path.is_file():
            report.license_path = f"/{license_rel}"
            license_text = license_path.read_text(encoding="utf-8", errors="replace")
            if expected_license not in license_text:
                report.errors.append(f"license metadata is missing `{expected_license}` in /{license_rel}")

        if expected_desktop_id not in report.detected_desktop_ids:
            report.errors.append(
                "required desktop-id contract not found in package desktop files: "
                f"{expected_desktop_id!r}; detected {report.detected_desktop_ids!r}"
            )
        unexpected_desktop_ids = [desktop_id for desktop_id in report.detected_desktop_ids if desktop_id != expected_desktop_id]
        if unexpected_desktop_ids:
            report.errors.append(f"unexpected desktop files found in package: {unexpected_desktop_ids!r}")

        if report.package_format == "rpm" and run_external_tools:
            report.rpm_header_validation, report.rpm_header_fields = query_rpm_header(package_path)
            if not report.rpm_header_validation.available:
                if require_external_tools:
                    report.errors.append("rpm is required for RPM header validation but was not found")
            elif not report.rpm_header_validation.ok:
                report.errors.append(f"rpm -qp --queryformat failed for {package_path} (exit {report.rpm_header_validation.returncode})")
            else:
                require_equal(report.errors, "RPM package name", report.rpm_header_fields.get("name"), expected_rpm_package_name)
                require_equal(report.errors, "RPM license", report.rpm_header_fields.get("license"), expected_license)
                require_equal(report.errors, "RPM architecture", report.rpm_header_fields.get("arch"), expected_rpm_arch)
                if report.release_version is not None:
                    require_equal(report.errors, "RPM version", report.rpm_header_fields.get("version"), report.release_version)

        report.ok = not report.errors
        return report


def expand_package_patterns(patterns: list[str]) -> list[pathlib.Path]:
    paths: list[pathlib.Path] = []
    for pattern in patterns:
        matches = sorted(glob.glob(pattern, recursive=True))
        if matches:
            paths.extend(pathlib.Path(match) for match in matches)
        else:
            candidate = pathlib.Path(pattern)
            if candidate.exists():
                paths.append(candidate)
    return sorted({path.resolve() for path in paths})


def print_summary(report: PackageReport) -> None:
    status = "PASS" if report.ok else "FAIL"
    print(f"[{status}] {report.package}")
    if report.detected_desktop_ids:
        print(f"  detected desktop ids: {', '.join(report.detected_desktop_ids)}")
    if report.metainfo_path:
        print(
            "  AppStream: "
            f"id={report.component_id!r} license={report.project_license!r} "
            f"launchable={report.launchable!r} binary={report.binary!r} "
            f"release={report.release_version!r}"
        )
    if report.desktop_path:
        print(
            "  Desktop: "
            f"Name={report.desktop_fields.get('Name')!r} "
            f"Exec={report.desktop_fields.get('Exec')!r} "
            f"Icon={report.desktop_fields.get('Icon')!r}"
        )
    if report.icon_paths:
        print(f"  icons: {', '.join(report.icon_paths)}")
    if report.rpm_header_fields:
        print(
            "  RPM header: "
            f"name={report.rpm_header_fields.get('name')!r} "
            f"version={report.rpm_header_fields.get('version')!r} "
            f"arch={report.rpm_header_fields.get('arch')!r} "
            f"license={report.rpm_header_fields.get('license')!r}"
        )
    for tool in [report.appstream_validation, report.desktop_validation, report.rpm_header_validation]:
        if tool is None:
            continue
        if tool.available:
            print(f"  {tool.name}: {'ok' if tool.ok else f'exit {tool.returncode}'}")
        else:
            print(f"  {tool.name}: skipped (not installed)")
    for error in report.errors:
        print(f"  error: {error}", file=sys.stderr)


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Validate Linux .deb/.rpm AppStream, desktop-file, and license metadata.")
    parser.add_argument("package", nargs="+", help=".deb/.rpm artifact path or glob")
    parser.add_argument("--expected-component-id", default=DEFAULT_COMPONENT_ID, help="expected AppStream id")
    parser.add_argument("--expected-license", default=DEFAULT_LICENSE, help="expected project license")
    parser.add_argument("--expected-binary", default=DEFAULT_BINARY, help="expected main binary name")
    parser.add_argument("--expected-desktop-id", default=DEFAULT_DESKTOP_ID, help="expected desktop file basename")
    parser.add_argument("--expected-rpm-package-name", default=DEFAULT_RPM_PACKAGE_NAME, help="expected RPM package name")
    parser.add_argument("--expected-rpm-arch", default=DEFAULT_RPM_ARCH, help="expected RPM architecture")
    parser.add_argument("--json-report", help="optional path to write a JSON report")
    tools = parser.add_mutually_exclusive_group()
    tools.add_argument("--skip-external-tools", action="store_true", help="skip appstreamcli, desktop-file-validate, and rpm")
    tools.add_argument(
        "--require-external-tools",
        action="store_true",
        help="fail if appstreamcli, desktop-file-validate, or rpm are missing",
    )
    return parser


def main() -> int:
    parser = build_arg_parser()
    args = parser.parse_args()
    package_paths = expand_package_patterns(args.package)
    if not package_paths:
        print("error: no .deb/.rpm artifacts matched the supplied path/glob", file=sys.stderr)
        return 2

    run_external_tools = not args.skip_external_tools
    require_external_tools = args.require_external_tools
    reports = [
        validate_package(
            package_path=package_path,
            expected_component_id=args.expected_component_id,
            expected_license=args.expected_license,
            expected_binary=args.expected_binary,
            expected_desktop_id=args.expected_desktop_id,
            expected_rpm_package_name=args.expected_rpm_package_name,
            expected_rpm_arch=args.expected_rpm_arch,
            run_external_tools=run_external_tools,
            require_external_tools=require_external_tools,
        )
        for package_path in package_paths
    ]

    for report in reports:
        print_summary(report)

    payload = {
        "ok": all(report.ok for report in reports),
        "external_validation": {
            "enabled": run_external_tools,
            "required": require_external_tools,
        },
        "expected": {
            "component_id": args.expected_component_id,
            "license": args.expected_license,
            "binary": args.expected_binary,
            "desktop_id": args.expected_desktop_id,
            "rpm_package_name": args.expected_rpm_package_name,
            "rpm_arch": args.expected_rpm_arch,
        },
        "packages": [report.to_json() for report in reports],
    }
    if args.json_report:
        report_path = pathlib.Path(args.json_report)
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"Wrote JSON report: {report_path}")

    return 0 if payload["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
