#!/usr/bin/env python3
"""Build a static signed APT repository from Echo .deb artifacts."""

from __future__ import annotations

import argparse
import datetime as dt
import glob
import gzip
import hashlib
import html
import io
import pathlib
import shutil
import subprocess
import sys
import tarfile
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from email.parser import Parser
from email.utils import format_datetime

from deb_common import (
    child_text,
    decompress_deb_tar_member,
    descendant_text,
    first_release,
    launchable_desktop_id,
    parse_desktop_file,
    read_ar_entries,
    strip_ns,
)


DEFAULT_SUITE = "stable"
DEFAULT_COMPONENT = "main"
DEFAULT_PACKAGE = "echo"
DEFAULT_COMPONENT_ID = "io.github.ddv1982.echo"
DEFAULT_ORIGIN = "echo-stable-main"
DEFAULT_LABEL = "Echo"
DEFAULT_DESCRIPTION = "Echo APT repository"
DEFAULT_REPOSITORY_URL = "https://ddv1982.github.io/echo/apt/"
DEFAULT_SETUP_PACKAGE_NAME = "echo-repository-setup"
DEFAULT_SETUP_PACKAGE_VERSION = "1.0"
DEFAULT_SETUP_MAINTAINER = "Douwe de Vries <douwe.de.vries.82@gmail.com>"
DEFAULT_SETUP_KEYRING_PATH = "/usr/share/keyrings/echo-archive-keyring.pgp"
DEFAULT_SETUP_SOURCES_PATH = "/etc/apt/sources.list.d/echo.sources"
APPLICATION_POOL_DIR = pathlib.PurePosixPath("pool/main/e/echo")


@dataclass(frozen=True)
class DebPackage:
    pool_path: pathlib.PurePosixPath
    package: str
    architecture: str
    fields: dict[str, str]
    size: int
    md5: str
    sha1: str
    sha256: str
    metainfo_xml: bytes
    desktop_fields: dict[str, str]


def extract_tar_member(archive_name: str, data: bytes, predicate) -> dict[str, bytes]:
    data, mode = decompress_deb_tar_member(archive_name, data)
    found: dict[str, bytes] = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode=mode) as tar:
        for member in tar.getmembers():
            normalized = member.name.removeprefix("./")
            if not member.isfile() or not predicate(normalized):
                continue
            extracted = tar.extractfile(member)
            if extracted is not None:
                found[normalized] = extracted.read()
    return found


def parse_control(control_bytes: bytes) -> dict[str, str]:
    message = Parser().parsestr(control_bytes.decode("utf-8", errors="replace"))
    return {key: value for key, value in message.items()}


def deb_control_metainfo_and_desktop(deb_path: pathlib.Path) -> tuple[dict[str, str], bytes, dict[str, str]]:
    entries = read_ar_entries(deb_path)
    control_name = next((name for name in entries if name.startswith("control.tar")), None)
    data_name = next((name for name in entries if name.startswith("data.tar")), None)
    if not control_name or not data_name:
        raise ValueError(f"{deb_path} must contain control.tar.* and data.tar.*")

    control_files = extract_tar_member(control_name, entries[control_name], lambda name: name == "control")
    if "control" not in control_files:
        raise ValueError(f"{deb_path} control archive does not contain control")

    data_files = extract_tar_member(
        data_name,
        entries[data_name],
        lambda name: (
            name == f"usr/share/metainfo/{DEFAULT_COMPONENT_ID}.metainfo.xml"
            or (name.startswith("usr/share/applications/") and name.endswith(".desktop"))
        ),
    )
    metainfo_name = f"usr/share/metainfo/{DEFAULT_COMPONENT_ID}.metainfo.xml"
    if metainfo_name not in data_files:
        raise ValueError(f"{deb_path} does not contain /{metainfo_name}")

    desktop = next((data for name, data in data_files.items() if name.endswith(".desktop")), None)
    if desktop is None:
        raise ValueError(f"{deb_path} does not contain a desktop file")

    return parse_control(control_files["control"]), data_files[metainfo_name], parse_desktop_file(desktop)


def format_deb822_field(key: str, value: str) -> str:
    value = value.rstrip("\n")
    if "\n" not in value:
        return f"{key}: {value}"
    first, *rest = value.splitlines()
    return "\n".join([f"{key}: {first}", *(f" {line}" for line in rest)])


def package_stanza(package: DebPackage) -> str:
    fields = dict(package.fields)
    fields["Filename"] = str(package.pool_path)
    fields["Size"] = str(package.size)
    fields["MD5sum"] = package.md5
    fields["SHA1"] = package.sha1
    fields["SHA256"] = package.sha256
    preferred = [
        "Package",
        "Version",
        "Architecture",
        "Maintainer",
        "Installed-Size",
        "Depends",
        "Recommends",
        "Section",
        "Priority",
        "Homepage",
        "Description",
        "Filename",
        "Size",
        "MD5sum",
        "SHA1",
        "SHA256",
    ]
    emitted: set[str] = set()
    lines: list[str] = []
    for key in preferred:
        if key in fields:
            lines.append(format_deb822_field(key, fields[key]))
            emitted.add(key)
    for key in sorted(fields):
        if key not in emitted:
            lines.append(format_deb822_field(key, fields[key]))
    return "\n".join(lines) + "\n"


def homepage_url(element: ET.Element) -> str:
    for child in element.iter():
        if strip_ns(child.tag) == "url" and child.attrib.get("type") == "homepage":
            return (child.text or "").strip()
    return ""


def description_markup(element: ET.Element) -> str:
    for child in element:
        if strip_ns(child.tag) != "description":
            continue
        paragraphs = [" ".join(part.strip() for part in p.itertext() if part.strip()) for p in child]
        paragraphs = [p for p in paragraphs if p]
        if paragraphs:
            return "".join(f"<p>{html.escape(paragraph)}</p>" for paragraph in paragraphs)
    return ""


def yaml_quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def dep11_yaml_header(origin: str, architecture: str) -> str:
    return "\n".join(
        [
            "---",
            "File: DEP-11",
            "Version: '1.0'",
            f"Origin: {yaml_quote(origin)}",
            f"Architecture: {yaml_quote(architecture)}",
        ]
    ) + "\n"


def dep11_yaml_component(package: DebPackage) -> str:
    root = ET.fromstring(package.metainfo_xml)
    component_id = child_text(root, "id")
    project_license = child_text(root, "project_license")
    metadata_license = child_text(root, "metadata_license")
    name = child_text(root, "name")
    summary = child_text(root, "summary")
    launchable = launchable_desktop_id(root)
    binary = descendant_text(root, "binary") or package.fields.get("Package", "")
    homepage = homepage_url(root)
    release_version, release_date = first_release(root)
    description = description_markup(root)
    categories = [category for category in package.desktop_fields.get("Categories", "").split(";") if category]
    icon = package.desktop_fields.get("Icon", "")

    if not component_id or not name or not summary or not launchable:
        raise ValueError("AppStream metainfo must contain id, name, summary, and launchable")
    if not project_license:
        raise ValueError("AppStream metainfo must contain project_license")

    lines = [
        "---",
        "Type: desktop-application",
        f"ID: {yaml_quote(component_id)}",
        f"Package: {yaml_quote(package.package)}",
        f"ProjectLicense: {yaml_quote(project_license)}",
    ]
    if metadata_license:
        lines.append(f"MetadataLicense: {yaml_quote(metadata_license)}")
    lines.extend(["Name:", f"  C: {yaml_quote(name)}"])
    lines.extend(["Summary:", f"  C: {yaml_quote(summary)}"])
    if description:
        lines.extend(["Description:", "  C: |", *(f"    {line}" for line in description.splitlines())])
    if homepage:
        lines.extend(["Url:", f"  homepage: {yaml_quote(homepage)}"])
    if icon:
        lines.extend(["Icon:", f"  stock: {yaml_quote(icon)}"])
    if categories:
        lines.append("Categories:")
        lines.extend(f"  - {yaml_quote(category)}" for category in categories)
    lines.extend(["Launchable:", "  desktop-id:", f"    - {yaml_quote(launchable)}"])
    if binary:
        lines.extend(["Provides:", "  binaries:", f"    - {yaml_quote(binary)}"])
    if release_version:
        lines.extend(["Releases:", f"  - version: {yaml_quote(release_version)}"])
        if release_date:
            lines.append(f"    date: {yaml_quote(release_date)}")
    return "\n".join(lines) + "\n"


def gzip_write(path: pathlib.Path, data: bytes) -> None:
    with path.open("wb") as handle:
        with gzip.GzipFile(filename="", mode="wb", fileobj=handle, mtime=0) as gz:
            gz.write(data)


def gzip_bytes(data: bytes) -> bytes:
    buffer = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=buffer, mtime=0) as gz:
        gz.write(data)
    return buffer.getvalue()


def file_hashes(path: pathlib.Path) -> tuple[str, str, str, int]:
    data = path.read_bytes()
    return (
        hashlib.md5(data, usedforsecurity=False).hexdigest(),
        hashlib.sha1(data).hexdigest(),
        hashlib.sha256(data).hexdigest(),
        len(data),
    )


def release_file(repo_root: pathlib.Path, args: argparse.Namespace, architectures: list[str]) -> str:
    dists_root = repo_root / "dists" / args.suite
    targets = sorted(
        path
        for path in dists_root.rglob("*")
        if path.is_file() and path.name not in {"Release", "InRelease", "Release.gpg"}
    )
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0)
    lines = [
        f"Origin: {args.origin}",
        f"Label: {args.label}",
        f"Suite: {args.suite}",
        f"Codename: {args.suite}",
        f"Date: {format_datetime(now, usegmt=True)}",
        f"Architectures: {' '.join(architectures)}",
        f"Components: {args.component}",
        f"Description: {args.description}",
    ]
    hash_rows = [(path.relative_to(dists_root).as_posix(), *file_hashes(path)) for path in targets]
    for section, index in [("MD5Sum", 1), ("SHA1", 2), ("SHA256", 3)]:
        lines.append(f"{section}:")
        for row in hash_rows:
            lines.append(f" {row[index]} {row[4]:16d} {row[0]}")
    return "\n".join(lines) + "\n"


def gpg_base(args: argparse.Namespace) -> list[str]:
    gpg = shutil.which("gpg")
    if not gpg:
        raise RuntimeError("gpg is required for signed repository generation")
    base = [gpg, "--batch", "--yes", "--pinentry-mode", "loopback"]
    if args.gpg_homedir:
        base.extend(["--homedir", args.gpg_homedir])
    if args.gpg_passphrase_file:
        base.extend(["--passphrase-file", args.gpg_passphrase_file])
    if args.gpg_key:
        base.extend(["--local-user", args.gpg_key])
    return base


def sign_release(args: argparse.Namespace, release_path: pathlib.Path) -> None:
    base = gpg_base(args)
    subprocess.run([*base, "--clearsign", "--output", str(release_path.with_name("InRelease")), str(release_path)], check=True)
    subprocess.run(
        [*base, "--detach-sign", "--armor", "--output", str(release_path.with_name("Release.gpg")), str(release_path)],
        check=True,
    )


def export_public_key(args: argparse.Namespace) -> None:
    if not args.public_key_out:
        return
    output = pathlib.Path(args.public_key_out)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as handle:
        subprocess.run([*gpg_base(args), "--export", args.gpg_key], check=True, stdout=handle)


def normalize_deb_data_path(path: str) -> str:
    if not path.startswith("/"):
        raise ValueError(f"Debian package install path must be absolute: {path!r}")
    parts = pathlib.PurePosixPath(path).parts[1:]
    if not parts or any(part in {".", ".."} or any(ord(char) < 32 for char in part) for part in parts):
        raise ValueError(f"unsafe Debian package path: {path!r}")
    return "/".join(parts)


def tar_gz_bytes(files: dict[str, tuple[bytes, int]]) -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as tar:
        for name, (data, mode) in sorted(files.items()):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            info.mtime = 0
            info.uid = 0
            info.gid = 0
            info.uname = "root"
            info.gname = "root"
            tar.addfile(info, io.BytesIO(data))
    return gzip_bytes(buffer.getvalue())


def ar_member(name: str, data: bytes) -> bytes:
    encoded = name.encode("ascii")
    if len(encoded) > 16:
        raise ValueError(f"ar member name is too long: {name}")
    header = (
        encoded.ljust(16, b" ")
        + b"0".ljust(12, b" ")
        + b"0".ljust(6, b" ")
        + b"0".ljust(6, b" ")
        + b"100644".ljust(8, b" ")
        + str(len(data)).encode("ascii").ljust(10, b" ")
        + b"`\n"
    )
    return header + data + (b"\n" if len(data) % 2 else b"")


def write_deb_archive(output: pathlib.Path, control_files: dict[str, tuple[bytes, int]], data_files: dict[str, tuple[bytes, int]]) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(
        b"!<arch>\n"
        + ar_member("debian-binary", b"2.0\n")
        + ar_member("control.tar.gz", tar_gz_bytes(control_files))
        + ar_member("data.tar.gz", tar_gz_bytes(data_files))
    )


def setup_sources_text(args: argparse.Namespace, architectures: list[str], signed_by_path: str) -> str:
    lines = [
        "Types: deb",
        f"URIs: {args.repository_url}",
        f"Suites: {args.suite}",
        f"Components: {args.component}",
    ]
    if architectures:
        lines.append(f"Architectures: {' '.join(architectures)}")
    lines.append(f"Signed-By: {signed_by_path}")
    return "\n".join(lines) + "\n"


def setup_keyring_bytes(args: argparse.Namespace) -> bytes:
    key_path = args.setup_public_key or args.public_key_out
    if not key_path:
        raise ValueError("--setup-package-out requires --setup-public-key or signed generation with --public-key-out")
    keyring = pathlib.Path(key_path)
    if not keyring.is_file() or keyring.stat().st_size == 0:
        raise ValueError(f"repository setup keyring source is missing or empty: {keyring}")
    return keyring.read_bytes()


def build_setup_package(args: argparse.Namespace, architectures: list[str]) -> None:
    if not args.setup_package_out:
        return

    keyring_path = normalize_deb_data_path(args.setup_keyring_path)
    sources_path = normalize_deb_data_path(args.setup_sources_path)
    keyring_install_path = f"/{keyring_path}"
    sources_install_path = f"/{sources_path}"
    keyring = setup_keyring_bytes(args)
    sources = setup_sources_text(args, architectures, keyring_install_path).encode("utf-8")
    data_files = {
        f"./{keyring_path}": (keyring, 0o644),
        f"./{sources_path}": (sources, 0o644),
    }
    md5sums = "".join(
        f"{hashlib.md5(data, usedforsecurity=False).hexdigest()}  {name.removeprefix('./')}\n"
        for name, (data, _mode) in sorted(data_files.items())
    ).encode("utf-8")
    control = (
        f"Package: {args.setup_package_name}\n"
        f"Version: {args.setup_package_version}\n"
        "Architecture: all\n"
        "Section: admin\n"
        "Priority: optional\n"
        f"Maintainer: {args.setup_maintainer}\n"
        "Depends: apt, ca-certificates\n"
        "Description: Echo APT repository setup package\n"
        " Installs the Echo archive keyring and Deb822 source configuration.\n"
    ).encode("utf-8")
    write_deb_archive(
        pathlib.Path(args.setup_package_out),
        {
            "./control": (control, 0o644),
            "./conffiles": ((sources_install_path + "\n").encode("utf-8"), 0o644),
            "./md5sums": (md5sums, 0o644),
        },
        data_files,
    )


def normalized_pool_filename(fields: dict[str, str]) -> str:
    values = {key: fields.get(key, "") for key in ["Package", "Version", "Architecture"]}
    if not all(values.values()):
        raise ValueError("Debian control file must declare Package, Version, and Architecture")
    for key, value in values.items():
        if any(char in value for char in ("/", "\x00", "\n", "\r")) or value in {".", ".."}:
            raise ValueError(f"unsafe {key} value for pool filename: {value!r}")
    return f"{values['Package']}_{values['Version']}_{values['Architecture']}.deb"


def collect_package(deb_path: pathlib.Path, pool_path: pathlib.PurePosixPath) -> DebPackage:
    fields, metainfo, desktop_fields = deb_control_metainfo_and_desktop(deb_path)
    package = fields.get("Package")
    arch = fields.get("Architecture")
    if not package or not arch:
        raise ValueError(f"{deb_path} control file must declare Package and Architecture")
    data = deb_path.read_bytes()
    return DebPackage(
        pool_path=pool_path,
        package=package,
        architecture=arch,
        fields=fields,
        size=len(data),
        md5=hashlib.md5(data, usedforsecurity=False).hexdigest(),
        sha1=hashlib.sha1(data).hexdigest(),
        sha256=hashlib.sha256(data).hexdigest(),
        metainfo_xml=metainfo,
        desktop_fields=desktop_fields,
    )


def expand_deb_inputs(patterns: list[str]) -> list[pathlib.Path]:
    paths: list[pathlib.Path] = []
    for pattern in patterns:
        matches = sorted(glob.glob(pattern, recursive=True))
        if matches:
            paths.extend(pathlib.Path(match) for match in matches)
        else:
            candidate = pathlib.Path(pattern)
            if candidate.exists():
                paths.append(candidate)
    unique = sorted({path.resolve() for path in paths})
    if not unique:
        raise ValueError("no .deb artifacts matched the supplied paths/globs")
    return unique


def is_application_deb_filename(path: pathlib.Path) -> bool:
    name = path.name
    if name.startswith(f"{DEFAULT_SETUP_PACKAGE_NAME}_") and name.endswith(".deb"):
        return False
    return name.startswith(f"{DEFAULT_PACKAGE}_") and name.endswith(".deb")


def select_application_debs(paths: list[pathlib.Path]) -> list[pathlib.Path]:
    selected = [path for path in paths if is_application_deb_filename(path)]
    if not selected:
        raise ValueError("no echo_*.deb artifacts matched the supplied paths/globs")
    return selected


def build_repository(args: argparse.Namespace) -> None:
    repo_root = pathlib.Path(args.output).resolve()
    if args.clean and repo_root.exists():
        shutil.rmtree(repo_root)
    repo_root.mkdir(parents=True, exist_ok=True)

    packages: list[DebPackage] = []
    for source in select_application_debs(expand_deb_inputs(args.deb)):
        fields, _metainfo, _desktop_fields = deb_control_metainfo_and_desktop(source)
        if fields.get("Package") != DEFAULT_PACKAGE:
            raise ValueError(f"unexpected package {fields.get('Package')!r}; expected {DEFAULT_PACKAGE!r}")
        pool_rel = APPLICATION_POOL_DIR / normalized_pool_filename(fields)
        destination = repo_root / pool_rel
        if destination.exists():
            raise ValueError(f"duplicate normalized package output path: {pool_rel}")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        packages.append(collect_package(destination, pool_rel))

    by_arch: dict[str, list[DebPackage]] = {}
    for package in packages:
        by_arch.setdefault(package.architecture, []).append(package)

    for arch, arch_packages in sorted(by_arch.items()):
        binary_dir = repo_root / "dists" / args.suite / args.component / f"binary-{arch}"
        binary_dir.mkdir(parents=True, exist_ok=True)
        packages_bytes = "\n".join(package_stanza(package) for package in arch_packages).encode("utf-8")
        (binary_dir / "Packages").write_bytes(packages_bytes)
        gzip_write(binary_dir / "Packages.gz", packages_bytes)

        dep11_dir = repo_root / "dists" / args.suite / args.component / "dep11"
        dep11_dir.mkdir(parents=True, exist_ok=True)
        dep11_bytes = (
            dep11_yaml_header(args.origin, arch)
            + "".join(dep11_yaml_component(package) for package in arch_packages)
        ).encode("utf-8")
        dep11_path = dep11_dir / f"Components-{arch}.yml"
        dep11_path.write_bytes(dep11_bytes)
        gzip_write(dep11_path.with_suffix(dep11_path.suffix + ".gz"), dep11_bytes)

    release_path = repo_root / "dists" / args.suite / "Release"
    architectures = sorted(by_arch)
    release_path.write_text(release_file(repo_root, args, architectures), encoding="utf-8")

    if args.unsigned:
        build_setup_package(args, architectures)
        return
    sign_release(args, release_path)
    export_public_key(args)
    build_setup_package(args, architectures)


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Build a signed Echo APT repository.")
    parser.add_argument("deb", nargs="+", help="built .deb artifact path(s) or glob(s) to publish")
    parser.add_argument("--output", required=True, help="repository output directory")
    parser.add_argument("--suite", default=DEFAULT_SUITE, help="APT suite/codename")
    parser.add_argument("--component", default=DEFAULT_COMPONENT, help="APT component")
    parser.add_argument("--origin", default=DEFAULT_ORIGIN, help="Release/DEP-11 origin")
    parser.add_argument("--label", default=DEFAULT_LABEL, help="Release label")
    parser.add_argument("--description", default=DEFAULT_DESCRIPTION, help="Release description")
    parser.add_argument("--repository-url", default=DEFAULT_REPOSITORY_URL, help="public APT repository base URL")
    parser.add_argument("--gpg-key", help="GPG key id/fingerprint used for signing")
    parser.add_argument("--gpg-homedir", help="GPG home directory containing the signing key")
    parser.add_argument("--gpg-passphrase-file", help="optional passphrase file for loopback signing")
    parser.add_argument("--public-key-out", help="optional path for binary gpg --export output")
    parser.add_argument("--setup-package-out", help="optional repository setup .deb output path")
    parser.add_argument("--setup-public-key", help="public keyring bytes for unsigned setup-package tests")
    parser.add_argument("--setup-package-name", default=DEFAULT_SETUP_PACKAGE_NAME, help="repository setup package name")
    parser.add_argument("--setup-package-version", default=DEFAULT_SETUP_PACKAGE_VERSION, help="repository setup package version")
    parser.add_argument("--setup-maintainer", default=DEFAULT_SETUP_MAINTAINER, help="repository setup package maintainer")
    parser.add_argument("--setup-keyring-path", default=DEFAULT_SETUP_KEYRING_PATH, help="absolute keyring install path")
    parser.add_argument("--setup-sources-path", default=DEFAULT_SETUP_SOURCES_PATH, help="absolute Deb822 source install path")
    parser.add_argument("--clean", action="store_true", help="remove output directory before generating")
    parser.add_argument("--unsigned", action="store_true", help="generate unsigned metadata for local tests only")
    return parser


def main() -> int:
    parser = build_arg_parser()
    args = parser.parse_args()
    if not args.unsigned and not args.gpg_key:
        parser.error("signed generation is the default; pass --gpg-key or --unsigned for local tests")
    if args.unsigned and args.public_key_out:
        parser.error("--public-key-out requires signed generation")
    if args.setup_package_out and args.unsigned and not args.setup_public_key:
        parser.error("--setup-package-out with --unsigned requires --setup-public-key")
    try:
        build_repository(args)
    except Exception as error:  # noqa: BLE001 - CLI should surface concise failures
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
