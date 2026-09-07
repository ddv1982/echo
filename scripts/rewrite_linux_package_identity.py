#!/usr/bin/env python3
"""Rewrite Tauri Linux package names to echo without touching payload paths.

Tauri names the .deb/.rpm from productName, which must stay equal to the
portal identifier so the desktop file remains io.github.ddv1982.echo.desktop.
After cargo tauri build, this rewrites Debian Package / RPM Name to echo,
injects cutover relations for the old name, and renames the artifacts.
The desktop file and /usr/bin/echo-desktop are left in place.
"""
from __future__ import annotations

import argparse
import io
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path


OLD_PACKAGE = "io.github.ddv1982.echo"
NEW_PACKAGE = "echo"
DEB_RELATIONS = ("Replaces", "Conflicts", "Provides")
RPM_RELATIONS = ("Obsoletes", "Conflicts", "Provides")


def parse_control(text: str) -> list[tuple[str, str]]:
    fields: list[tuple[str, str]] = []
    key: str | None = None
    parts: list[str] = []
    for line in text.splitlines():
        if key is not None and (line.startswith(" ") or line.startswith("\t")):
            parts.append(line.strip())
            continue
        if key is not None:
            fields.append((key, "\n".join(parts)))
        if not line.strip():
            key = None
            parts = []
            continue
        key, _, rest = line.partition(":")
        key = key.strip()
        parts = [rest.strip()]
    if key is not None:
        fields.append((key, "\n".join(parts)))
    return fields


def format_control(fields: list[tuple[str, str]]) -> str:
    lines: list[str] = []
    for key, value in fields:
        first, *rest = value.split("\n")
        lines.append(f"{key}: {first}")
        lines.extend(f" {chunk}" for chunk in rest)
    return "\n".join(lines) + "\n"


def control_get(fields: list[tuple[str, str]], key: str) -> str | None:
    wanted = key.lower()
    for name, value in fields:
        if name.lower() == wanted:
            return value
    return None


def control_set(fields: list[tuple[str, str]], key: str, value: str) -> None:
    wanted = key.lower()
    for index, (name, _) in enumerate(fields):
        if name.lower() == wanted:
            fields[index] = (name, value)
            return
    fields.append((key, value))


def ensure_control_relation(fields: list[tuple[str, str]], key: str, package: str) -> None:
    existing = control_get(fields, key)
    if existing is None:
        control_set(fields, key, package)
        return
    parts = [part.strip() for part in existing.split(",") if part.strip()]
    names = [part.split()[0] for part in parts]
    if package not in names:
        parts.append(package)
        control_set(fields, key, ", ".join(parts))


def debian_version_for_filename(version: str) -> str:
    if ":" in version:
        return version.split(":", 1)[1]
    return version


def run(command: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=True, text=True, **kwargs)


def rewrite_deb(package: Path) -> Path:
    package = package.resolve()
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary) / "root"
        run(["dpkg-deb", "-R", str(package), str(root)])
        control_path = root / "DEBIAN" / "control"
        fields = parse_control(control_path.read_text(encoding="utf-8"))
        control_set(fields, "Package", NEW_PACKAGE)
        for key in DEB_RELATIONS:
            ensure_control_relation(fields, key, OLD_PACKAGE)
        control_path.write_text(format_control(fields), encoding="utf-8")
        version = control_get(fields, "Version")
        architecture = control_get(fields, "Architecture")
        if not version or not architecture:
            raise ValueError(f"{package} control is missing Version or Architecture")
        destination = package.with_name(
            f"{NEW_PACKAGE}_{debian_version_for_filename(version)}_{architecture}.deb"
        )
        built = Path(temporary) / destination.name
        run(["dpkg-deb", "--root-owner-group", "-b", str(root), str(built)])
        shutil.copy2(built, destination)
    if package != destination:
        package.unlink()
    return destination


def deb_data_members(package: Path) -> list[str]:
    archive = subprocess.run(
        ["dpkg-deb", "--fsys-tarfile", str(package)],
        check=True,
        stdout=subprocess.PIPE,
    )
    with tarfile.open(fileobj=io.BytesIO(archive.stdout), mode="r:*") as tar:
        return [member.lstrip("./") for member in tar.getnames()]


def which_all(*names: str) -> list[str] | None:
    found = [shutil.which(name) for name in names]
    if any(path is None for path in found):
        return None
    return [path for path in found if path is not None]


def rpm_tools_available() -> bool:
    return bool(
        shutil.which("rpmrebuild")
        or which_all("rpm", "rpmbuild", "rpm2cpio", "cpio")
    )


def rpm_query(package: Path, fmt: str) -> str:
    rpm = shutil.which("rpm")
    if rpm is None:
        raise ValueError("rpm is required to query package metadata")
    return subprocess.check_output(
        [rpm, "-qp", "--nosignature", "--queryformat", fmt, str(package)],
        text=True,
    )


def rpm_tag_has_package(value: str, package: str) -> bool:
    for token in value.replace(",", " ").split():
        name = token.split("=")[0].split("<")[0].split(">")[0]
        if name == package:
            return True
    return False


def rewrite_rpm_spec(spec: str) -> str:
    lines = spec.splitlines(keepends=True)
    result: list[str] = []
    in_preamble = True
    seen: set[str] = set()

    def missing_tags() -> list[str]:
        extra: list[str] = []
        for tag in RPM_RELATIONS:
            if tag.lower() not in seen:
                extra.append(f"{tag}: {OLD_PACKAGE}\n")
        return extra

    for line in lines:
        if in_preamble and line.startswith("%"):
            result.extend(missing_tags())
            in_preamble = False
            result.append(line)
            continue
        if in_preamble:
            lowered = line.lower()
            if lowered.startswith("name:"):
                result.append(f"Name: {NEW_PACKAGE}\n")
                seen.add("name")
                continue
            handled = False
            for tag in RPM_RELATIONS:
                if lowered.startswith(tag.lower() + ":"):
                    value = line.split(":", 1)[1].strip()
                    if not rpm_tag_has_package(value, OLD_PACKAGE):
                        suffix = f" {OLD_PACKAGE}" if value else OLD_PACKAGE
                        line = f"{tag}: {value}{suffix}\n" if value else f"{tag}: {OLD_PACKAGE}\n"
                    seen.add(tag.lower())
                    handled = True
                    break
            if not handled:
                pass
        result.append(line)
    if in_preamble:
        result.extend(missing_tags())
    return "".join(result)


def install_rewritten_rpm(built: Path, original: Path) -> Path:
    name = rpm_query(built, "%{NAME}")
    version = rpm_query(built, "%{VERSION}")
    release = rpm_query(built, "%{RELEASE}")
    arch = rpm_query(built, "%{ARCH}")
    if name != NEW_PACKAGE:
        raise ValueError(f"rewritten rpm Name is {name!r}, expected {NEW_PACKAGE!r}")
    destination = original.with_name(f"{NEW_PACKAGE}-{version}-{release}.{arch}.rpm")
    shutil.copy2(built, destination)
    if original.resolve() != destination.resolve():
        original.unlink()
    return destination


def rewrite_rpm_with_rpmrebuild(package: Path) -> Path:
    rpmrebuild = shutil.which("rpmrebuild")
    if rpmrebuild is None:
        raise ValueError("rpmrebuild is not available")
    with tempfile.TemporaryDirectory() as temporary:
        tmp = Path(temporary)
        outdir = tmp / "out"
        outdir.mkdir()
        filter_path = tmp / "filter-spec"
        filter_path.write_text(
            "#!{python}\n"
            "import sys\n"
            "sys.path.insert(0, {scripts!r})\n"
            "import rewrite_linux_package_identity as rewrite\n"
            "sys.stdout.write(rewrite.rewrite_rpm_spec(sys.stdin.read()))\n".format(
                python=sys.executable,
                scripts=str(Path(__file__).resolve().parent),
            ),
            encoding="utf-8",
        )
        filter_path.chmod(0o755)
        run(
            [
                rpmrebuild,
                "--package",
                "--batch",
                "--notest-install",
                "--directory",
                str(outdir),
                "--change-spec-whole",
                str(filter_path),
                str(package),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        built = list(outdir.rglob("*.rpm"))
        if len(built) != 1:
            raise ValueError(f"rpmrebuild produced {len(built)} rpm files")
        return install_rewritten_rpm(built[0], package)


def rpm_has_relation(package: Path, tag: str, name: str) -> bool:
    flag = {
        "Provides": "--provides",
        "Obsoletes": "--obsoletes",
        "Conflicts": "--conflicts",
    }[tag]
    rpm = shutil.which("rpm")
    if rpm is None:
        raise ValueError("rpm is required to query package relations")
    output = subprocess.check_output(
        [rpm, "-qp", "--nosignature", flag, str(package)],
        text=True,
    )
    return rpm_tag_has_package(output, name)


def rewrite_rpm_with_rpmbuild(package: Path) -> Path:
    tools = which_all("rpm", "rpmbuild", "rpm2cpio", "cpio")
    if tools is None:
        raise ValueError("rpm, rpmbuild, rpm2cpio, and cpio are required")
    rpm, rpmbuild, rpm2cpio, cpio = tools
    version = rpm_query(package, "%{VERSION}")
    release = rpm_query(package, "%{RELEASE}")
    arch = rpm_query(package, "%{ARCH}")
    summary = rpm_query(package, "%{SUMMARY}")
    license_field = rpm_query(package, "%{LICENSE}")
    url = rpm_query(package, "%{URL}")
    description = rpm_query(package, "%{DESCRIPTION}")
    requires = [
        item
        for item in rpm_query(package, "[%{REQUIRENAME}\n]").splitlines()
        if item and not item.startswith("rpmlib(") and item != OLD_PACKAGE
    ]
    with tempfile.TemporaryDirectory() as temporary:
        tmp = Path(temporary)
        top = tmp / "rpmbuild"
        for name in ("BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"):
            (top / name).mkdir()
        payload = top / "SOURCES" / "payload"
        payload.mkdir()
        extractor = subprocess.Popen([rpm2cpio, str(package)], stdout=subprocess.PIPE)
        assert extractor.stdout is not None
        subprocess.run(
            [cpio, "-idmu", "--quiet"],
            stdin=extractor.stdout,
            cwd=payload,
            check=True,
        )
        if extractor.wait() != 0:
            raise ValueError(f"rpm2cpio failed on {package}")
        files = sorted(
            "/" + str(path.relative_to(payload))
            for path in payload.rglob("*")
            if path.is_file() or path.is_symlink()
        )
        spec_lines = [
            "%global __os_install_post %{nil}",
            "%global debug_package %{nil}",
            "AutoReq: no",
            "AutoProv: no",
            f"Name: {NEW_PACKAGE}",
            f"Version: {version}",
            f"Release: {release}",
            f"Summary: {summary if summary and summary != '(none)' else 'Echo'}",
            f"License: {license_field if license_field and license_field != '(none)' else 'MIT'}",
            f"BuildArch: {arch}",
        ]
        if url and url != "(none)":
            spec_lines.append(f"URL: {url}")
        for requirement in requires:
            spec_lines.append(f"Requires: {requirement}")
        for tag in RPM_RELATIONS:
            spec_lines.append(f"{tag}: {OLD_PACKAGE}")
        spec_lines.extend(
            [
                "",
                "%description",
                description if description and description != "(none)" else "Echo",
                "",
                "%install",
                "rm -rf %{buildroot}",
                "mkdir -p %{buildroot}",
                f"cp -a {payload}/. %{{buildroot}}/",
                "",
                "%files",
            ]
        )
        spec_lines.extend(files)
        spec_lines.append("")
        spec = top / "SPECS" / "echo.spec"
        spec.write_text("\n".join(spec_lines) + "\n", encoding="utf-8")
        run(
            [
                rpmbuild,
                "-bb",
                "--define",
                f"_topdir {top}",
                str(spec),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        built = list((top / "RPMS").rglob("*.rpm"))
        if len(built) != 1:
            raise ValueError(f"rpmbuild produced {len(built)} rpm files")
        return install_rewritten_rpm(built[0], package)


def rewrite_rpm(package: Path) -> Path:
    package = package.resolve()
    if shutil.which("rpmrebuild"):
        return rewrite_rpm_with_rpmrebuild(package)
    return rewrite_rpm_with_rpmbuild(package)


def rewrite_package(package: Path) -> Path:
    if not package.is_file():
        raise ValueError(f"no such package: {package}")
    suffix = package.suffix.lower()
    if suffix == ".deb":
        return rewrite_deb(package)
    if suffix == ".rpm":
        return rewrite_rpm(package)
    raise ValueError(f"unsupported package type: {package}")


def build_synthetic_deb(root: Path) -> Path:
    pkg = root / "pkg"
    (pkg / "DEBIAN").mkdir(parents=True)
    (pkg / "usr" / "bin").mkdir(parents=True)
    (pkg / "usr" / "share" / "applications").mkdir(parents=True)
    (pkg / "DEBIAN" / "control").write_text(
        "Package: io.github.ddv1982.echo\n"
        "Version: 1.0.0\n"
        "Architecture: amd64\n"
        "Maintainer: Douwe de Vries <douwe.de.vries.82@gmail.com>\n"
        "Description: synthetic identity rewrite fixture\n",
        encoding="utf-8",
    )
    binary = pkg / "usr" / "bin" / "echo-desktop"
    binary.write_text("#!/bin/sh\n", encoding="utf-8")
    binary.chmod(0o755)
    desktop = pkg / "usr" / "share" / "applications" / "io.github.ddv1982.echo.desktop"
    desktop.write_text(
        "[Desktop Entry]\nName=Echo\nExec=/usr/bin/echo-desktop\nType=Application\n",
        encoding="utf-8",
    )
    deb = root / "io.github.ddv1982.echo_1.0.0_amd64.deb"
    run(["dpkg-deb", "--root-owner-group", "-b", str(pkg), str(deb)])
    return deb


def build_synthetic_rpm(root: Path) -> Path:
    tools = which_all("rpmbuild", "rpm")
    if tools is None:
        raise ValueError("rpmbuild is required to build a synthetic rpm")
    rpmbuild, _rpm = tools
    top = root / "rpmbuild"
    for name in ("BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"):
        (top / name).mkdir(parents=True)
    payload = root / "rpm-payload"
    (payload / "usr" / "bin").mkdir(parents=True)
    (payload / "usr" / "share" / "applications").mkdir(parents=True)
    binary = payload / "usr" / "bin" / "echo-desktop"
    binary.write_text("#!/bin/sh\n", encoding="utf-8")
    binary.chmod(0o755)
    desktop = payload / "usr" / "share" / "applications" / "io.github.ddv1982.echo.desktop"
    desktop.write_text(
        "[Desktop Entry]\nName=Echo\nExec=/usr/bin/echo-desktop\nType=Application\n",
        encoding="utf-8",
    )
    spec = top / "SPECS" / "old.spec"
    spec.write_text(
        "\n".join(
            [
                "%global debug_package %{nil}",
                "Name: io.github.ddv1982.echo",
                "Version: 1.0.0",
                "Release: 1",
                "Summary: synthetic identity rewrite fixture",
                "License: MIT",
                "BuildArch: noarch",
                "",
                "%description",
                "synthetic identity rewrite fixture",
                "",
                "%install",
                "mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/share/applications",
                f"cp {binary} %{{buildroot}}/usr/bin/echo-desktop",
                f"cp {desktop} %{{buildroot}}/usr/share/applications/io.github.ddv1982.echo.desktop",
                "",
                "%files",
                "/usr/bin/echo-desktop",
                "/usr/share/applications/io.github.ddv1982.echo.desktop",
                "",
            ]
        ),
        encoding="utf-8",
    )
    run(
        [rpmbuild, "-bb", "--define", f"_topdir {top}", str(spec)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    built = list((top / "RPMS").rglob("*.rpm"))
    if len(built) != 1:
        raise ValueError(f"synthetic rpmbuild produced {len(built)} rpm files")
    destination = root / built[0].name
    shutil.copy2(built[0], destination)
    return destination


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        original = build_synthetic_deb(root)
        rewritten = rewrite_deb(original)
        if rewritten.name != "echo_1.0.0_amd64.deb":
            raise ValueError(f"deb renamed to {rewritten.name}")
        package = subprocess.check_output(
            ["dpkg-deb", "-f", str(rewritten), "Package"], text=True
        ).strip()
        if package != NEW_PACKAGE:
            raise ValueError(f"Package is {package!r}")
        for key in DEB_RELATIONS:
            value = subprocess.check_output(
                ["dpkg-deb", "-f", str(rewritten), key], text=True
            ).strip()
            if OLD_PACKAGE not in value:
                raise ValueError(f"{key} is {value!r}")
        members = deb_data_members(rewritten)
        desktop = "usr/share/applications/io.github.ddv1982.echo.desktop"
        if desktop not in members:
            raise ValueError(f"desktop path missing from data.tar: {members}")
        if "usr/share/applications/echo.desktop" in members:
            raise ValueError("desktop file was renamed")
        if "usr/bin/echo-desktop" not in members:
            raise ValueError("binary path missing from data.tar")
        if rpm_tools_available():
            rpm_original = build_synthetic_rpm(root)
            rpm_rewritten = rewrite_rpm(rpm_original)
            name = rpm_query(rpm_rewritten, "%{NAME}")
            if name != NEW_PACKAGE:
                raise ValueError(f"rpm Name is {name!r}")
            release = rpm_query(rpm_rewritten, "%{RELEASE}")
            version = rpm_query(rpm_rewritten, "%{VERSION}")
            arch = rpm_query(rpm_rewritten, "%{ARCH}")
            expected = f"{NEW_PACKAGE}-{version}-{release}.{arch}.rpm"
            if rpm_rewritten.name != expected:
                raise ValueError(f"rpm renamed to {rpm_rewritten.name}, expected {expected}")
            for tag in RPM_RELATIONS:
                if not rpm_has_relation(rpm_rewritten, tag, OLD_PACKAGE):
                    raise ValueError(f"rpm missing {tag} {OLD_PACKAGE}")
        else:
            print("rewrite_linux_package_identity: rpm tools missing; skipped rpm self-test")
    print("rewrite_linux_package_identity: self-test passed")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Rewrite Debian Package / RPM Name to echo after a Tauri bundle"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--filter-rpm-spec",
        action="store_true",
        help=argparse.SUPPRESS,
    )
    parser.add_argument("packages", nargs="*", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.filter_rpm_spec:
            sys.stdout.write(rewrite_rpm_spec(sys.stdin.read()))
            return 0
        if args.self_test:
            self_test()
            return 0
        if not args.packages:
            parser.error("pass .deb/.rpm paths, or --self-test")
        for package in args.packages:
            rewritten = rewrite_package(package)
            print(rewritten)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"rewrite_linux_package_identity: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
