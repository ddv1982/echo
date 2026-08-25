#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from whisper_release_common import (
    BUNDLE_MARKER,
    BUNDLE_TOKENS,
    bundle_variant,
    runtime_identity,
    runtime_library_bindings,
    sha256_bytes,
    sha256_file,
    tree_sha256,
    verify_contained_symlinks,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
CANONICAL_BINARY = REPO_ROOT / "target/release/echo-desktop"


def read_json(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def variant_bytes(canonical: bytes, bundle_type: str) -> bytes:
    return bundle_variant(canonical, bundle_type)


def preserve_old_bundle(label: str) -> None:
    bundle = REPO_ROOT / "target/release/bundle"
    if bundle.exists():
        backup = bundle.with_name(f"bundle.pre-qualified-{os.getpid()}-{label}")
        if backup.exists():
            raise ValueError(f"bundle backup already exists: {backup}")
        bundle.rename(backup)


def extract_package(
    package: Path,
    bundle_type: str,
    destination: Path,
    *,
    force_7z: bool = False,
) -> None:
    if bundle_type == "deb":
        subprocess.run(["dpkg-deb", "-x", str(package), str(destination)], check=True)
        return
    destination.mkdir(parents=True, exist_ok=True)
    if not force_7z and shutil.which("rpm2cpio") and shutil.which("cpio"):
        converted = subprocess.run(
            ["rpm2cpio", str(package)], check=False, capture_output=True
        )
        if converted.returncode == 0:
            subprocess.run(
                ["cpio", "-idmu", "--quiet"],
                cwd=destination,
                input=converted.stdout,
                check=True,
            )
            return
    if not shutil.which("7z"):
        raise ValueError("RPM extraction requires working rpm2cpio/cpio, or 7z")
    with tempfile.TemporaryDirectory(prefix="echo-rpm-") as temporary:
        stage = Path(temporary)
        subprocess.run(
            ["7z", "x", "-y", f"-o{stage}", str(package)],
            check=True,
            capture_output=True,
        )
        archives = [path for path in stage.iterdir() if path.is_file()]
        if len(archives) != 1:
            raise ValueError("RPM did not contain exactly one cpio archive")
        subprocess.run(
            ["7z", "x", "-y", f"-o{destination}", str(archives[0])],
            check=True,
            capture_output=True,
        )


def verify_rpm_extraction(package: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="echo-rpm-smoke-") as temporary:
        destination = Path(temporary) / "payload"
        extract_package(package.resolve(), "rpm", destination, force_7z=True)
        if not (destination / "usr/bin/echo-desktop").is_file():
            raise ValueError("RPM fallback extraction has no Echo executable")


def verify_extracted(
    extracted: Path,
    expected_binary: bytes,
    promotion_root: Path | None,
) -> dict[str, object]:
    binary = extracted / "usr/bin/echo-desktop"
    resource = extracted / "usr/lib/io.github.ddv1982.echo/whisper-acceleration"
    if binary.read_bytes() != expected_binary:
        raise ValueError("packaged executable differs outside the Tauri bundle marker")
    packaged_admission = resource / "admission.json"
    if promotion_root is not None:
        source_admission = promotion_root / "whisper-acceleration/admission.json"
        if packaged_admission.read_bytes() != source_admission.read_bytes():
            raise ValueError("packaged admission record changed")
    admission = read_json(packaged_admission)
    verify_contained_symlinks(resource)
    if admission["identity"]["echoBinarySha256"] != sha256_file(binary):
        raise ValueError("admission record does not bind the packaged executable")
    runtime = resource / admission["artifacts"]["runtimeRelativePath"]
    if runtime_identity(runtime) != admission["identity"]["runtimeIdentitySha256"]:
        raise ValueError("packaged runtime identity changed")
    packaged_library_bindings = runtime_library_bindings(runtime)
    if (
        packaged_library_bindings
        != admission["artifacts"]["runtimeLibraryBindings"]
    ):
        raise ValueError("packaged runtime library alias admission changed")
    if promotion_root is not None:
        source_runtime = (
            promotion_root
            / "whisper-acceleration"
            / admission["artifacts"]["runtimeRelativePath"]
        )
        if packaged_library_bindings != runtime_library_bindings(source_runtime):
            raise ValueError("packaged runtime library alias bindings changed")
    probe = resource / admission["artifacts"]["probeRelativePath"]
    if sha256_file(probe) != admission["artifacts"]["probeSha256"]:
        raise ValueError("packaged runtime probe identity changed")
    cache_seed = resource / admission["artifacts"]["cacheSeedRelativePath"]
    if tree_sha256(cache_seed) != admission["artifacts"]["cacheSeedSha256"]:
        raise ValueError("packaged cache seed identity changed")
    return {
        "echoCommit": admission["identity"]["echoCommit"],
        "binarySha256": sha256_file(binary),
        "admissionSha256": sha256_file(packaged_admission),
        "runtimeIdentitySha256": admission["identity"]["runtimeIdentitySha256"],
        "runtimeLibraryBindings": packaged_library_bindings,
        "cacheSeedSha256": admission["artifacts"]["cacheSeedSha256"],
        "admissionIdentityKey": admission["identityKey"],
    }


def bundle_one(
    bundle_type: str,
    promotion_root: Path,
    output: Path,
    canonical: bytes,
    expected_commit: str,
) -> dict[str, object]:
    promotion = read_json(promotion_root / "promotion.json")
    if promotion.get("echoCommit") != expected_commit:
        raise ValueError(f"{bundle_type} promotion belongs to another Echo commit")
    expected_binary = variant_bytes(canonical, bundle_type)
    if promotion["echoBinarySha256"] != sha256_bytes(expected_binary):
        raise ValueError(f"{bundle_type} promotion does not bind its exact ELF variant")
    preserve_old_bundle(bundle_type)
    subprocess.run(
        [
            "cargo",
            "tauri",
            "bundle",
            "--ci",
            "--bundles",
            bundle_type,
            "--config",
            str(promotion_root / "tauri-acceleration.conf.json"),
        ],
        cwd=REPO_ROOT,
        check=True,
    )
    packages = list(
        (REPO_ROOT / f"target/release/bundle/{bundle_type}").glob(f"*.{bundle_type}")
    )
    if len(packages) != 1:
        raise ValueError(f"expected one {bundle_type} package")
    destination = output / packages[0].name
    shutil.copy2(packages[0], destination)
    with tempfile.TemporaryDirectory(
        prefix=f"echo-{bundle_type}-extract-"
    ) as temporary:
        extract_package(destination, bundle_type, Path(temporary))
        details = verify_extracted(Path(temporary), expected_binary, promotion_root)
    return {
        "file": destination.name,
        "sha256": sha256_file(destination),
        **details,
    }


def stage(args: argparse.Namespace) -> None:
    output = args.output.resolve()
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    canonical_path = args.canonical_binary.resolve()
    if canonical_path != CANONICAL_BINARY.resolve():
        raise ValueError(f"canonical binary must be {CANONICAL_BINARY}")
    canonical = canonical_path.read_bytes()
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    dirty = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if dirty or head != args.commit:
        raise ValueError("staging requires the exact clean qualified Echo commit")
    output.mkdir(parents=True)
    subprocess.run(
        ["npm", "run", "build", "--prefix", "frontend"], cwd=REPO_ROOT, check=True
    )
    assets = {
        "deb": bundle_one(
            "deb", args.deb_promotion.resolve(), output, canonical, args.commit
        ),
        "rpm": bundle_one(
            "rpm", args.rpm_promotion.resolve(), output, canonical, args.commit
        ),
    }
    raw = output / "echo-desktop"
    shutil.copy2(canonical_path, raw)
    assets["binary"] = {"file": raw.name, "sha256": sha256_file(raw)}
    manifest = {
        "schemaVersion": 1,
        "version": args.version,
        "echoCommit": args.commit,
        "assets": assets,
    }
    (output / "qualified-release.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )


def verify_asset_inventory(root: Path, expected_files: set[str]) -> None:
    entries = list(root.iterdir())
    if (
        any(not path.is_file() or path.is_symlink() for path in entries)
        or {path.name for path in entries} != expected_files
    ):
        raise ValueError("qualified release has an unexpected directory inventory")


def verify_manifest(root: Path, expected_version: str, expected_commit: str) -> None:
    manifest = read_json(root / "qualified-release.json")
    if (
        manifest.get("schemaVersion") != 1
        or manifest.get("version") != expected_version
        or manifest.get("echoCommit") != expected_commit
    ):
        raise ValueError("qualified release identity does not match the tag")
    assets = manifest.get("assets")
    if not isinstance(assets, dict) or set(assets) != {"deb", "rpm", "binary"}:
        raise ValueError("qualified release has the wrong asset set")
    for label, value in assets.items():
        if not isinstance(value, dict):
            raise ValueError(f"qualified {label} asset is invalid")
        path = root / str(value.get("file"))
        if (
            path.name != value.get("file")
            or not path.is_file()
            or sha256_file(path) != value.get("sha256")
        ):
            raise ValueError(f"qualified {label} asset digest changed")
    expected_files = {
        "qualified-release.json",
        *(str(value["file"]) for value in assets.values()),
    }
    verify_asset_inventory(root, expected_files)
    canonical = (root / assets["binary"]["file"]).read_bytes()
    for bundle_type in ("deb", "rpm"):
        package = root / assets[bundle_type]["file"]
        if bundle_type == "deb":
            packaged_version = subprocess.run(
                ["dpkg-deb", "-f", str(package), "Version"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
        else:
            packaged_version = subprocess.run(
                ["rpm", "-qp", "--queryformat", "%{VERSION}", str(package)],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
        if packaged_version != expected_version:
            raise ValueError(f"qualified {bundle_type} package version changed")
        with tempfile.TemporaryDirectory(
            prefix=f"echo-verify-{bundle_type}-"
        ) as temporary:
            extracted = Path(temporary)
            extract_package(package, bundle_type, extracted)
            details = verify_extracted(
                extracted,
                variant_bytes(canonical, bundle_type),
                None,
            )
        for field in (
            "echoCommit",
            "binarySha256",
            "admissionSha256",
            "runtimeIdentitySha256",
            "runtimeLibraryBindings",
            "cacheSeedSha256",
            "admissionIdentityKey",
        ):
            if details[field] != assets[bundle_type].get(field):
                raise ValueError(f"qualified {bundle_type} {field} changed")
        if details["echoCommit"] != expected_commit:
            raise ValueError(f"qualified {bundle_type} belongs to another commit")


def self_test() -> None:
    from unittest.mock import patch

    canonical = b"before" + BUNDLE_MARKER + b"after"
    for bundle_type, token in BUNDLE_TOKENS.items():
        assert variant_bytes(canonical, bundle_type) == b"before" + token + b"after"
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        rpm = root / "fixture.rpm"
        rpm.write_bytes(b"rpm")
        existing = root / "existing"
        existing.mkdir()
        with (
            patch.object(shutil, "which", return_value="/fake/tool"),
            patch.object(subprocess, "run") as run,
        ):
            run.return_value = subprocess.CompletedProcess([], 0, stdout=b"")
            extract_package(rpm, "rpm", existing)
            assert run.call_count == 2
            assert run.call_args_list[0].args[0][0] == "rpm2cpio"
            assert run.call_args_list[1].args[0][0] == "cpio"

        fallback = root / "fallback"
        fallback.mkdir()

        def run_7z(command: list[str], **_: object) -> subprocess.CompletedProcess:
            output = next(value for value in command if value.startswith("-o"))[2:]
            if command[-1] == str(rpm):
                (Path(output) / "payload.cpio").write_bytes(b"cpio")
            return subprocess.CompletedProcess(command, 0, stdout=b"")

        with (
            patch.object(
                shutil,
                "which",
                side_effect=lambda command: "/fake/7z" if command == "7z" else None,
            ),
            patch.object(subprocess, "run", side_effect=run_7z) as run,
        ):
            extract_package(rpm, "rpm", fallback)
            assert run.call_count == 2
            assert all(call.args[0][0] == "7z" for call in run.call_args_list)

        nonzero = root / "nonzero"
        nonzero.mkdir()

        def run_after_nonzero(
            command: list[str], **kwargs: object
        ) -> subprocess.CompletedProcess:
            if command[0] == "rpm2cpio":
                return subprocess.CompletedProcess(command, 1, stdout=b"cpio")
            return run_7z(command, **kwargs)

        with (
            patch.object(shutil, "which", return_value="/fake/tool"),
            patch.object(subprocess, "run", side_effect=run_after_nonzero) as run,
        ):
            extract_package(rpm, "rpm", nonzero)
            assert [call.args[0][0] for call in run.call_args_list] == [
                "rpm2cpio",
                "7z",
                "7z",
            ]
            assert run.call_args_list[0].kwargs["check"] is False
            assert run.call_args_list[0].kwargs["capture_output"] is True
        inventory = root / "inventory"
        inventory.mkdir()
        (inventory / "qualified-release.json").write_bytes(b"manifest")
        (inventory / "asset").write_bytes(b"asset")
        expected_inventory = {
            "qualified-release.json",
            "asset",
        }
        verify_asset_inventory(inventory, expected_inventory)
        (inventory / "unexpected.rpm").write_bytes(b"extra")
        try:
            verify_asset_inventory(inventory, expected_inventory)
        except ValueError:
            pass
        else:
            raise AssertionError("unexpected release asset passed inventory verification")
        package = root / "package"
        package.mkdir()
        outside = root / "outside"
        outside.write_bytes(b"outside")
        (package / "escape").symlink_to(outside)
        try:
            verify_contained_symlinks(package)
        except ValueError:
            pass
        else:
            raise AssertionError("escaping package symlink was accepted")
        runtime = root / "runtime"
        runtime.mkdir()
        cli = runtime / "whisper-cli"
        versioned = runtime / "libwhisper.so.1.9.2"
        ggml = runtime / "libggml.so.0.18.1"
        cli.write_bytes(b"cli")
        versioned.write_bytes(b"library")
        ggml.write_bytes(b"other library")
        (runtime / "libwhisper.so").write_bytes(b"library")
        (runtime / "libwhisper.so.1").write_bytes(b"library")
        original = runtime_identity(cli)
        original_bindings = runtime_library_bindings(cli)
        (runtime / "libwhisper.so.1").write_bytes(b"other library")
        assert runtime_identity(cli) == original
        assert runtime_library_bindings(cli) != original_bindings
    print("stage-qualified-whisper-release: self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Bundle and verify exact qualified Whisper release assets"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify", type=Path)
    parser.add_argument("--verify-rpm-extraction", type=Path)
    parser.add_argument("--expected-version")
    parser.add_argument("--expected-commit")
    parser.add_argument("--canonical-binary", type=Path)
    parser.add_argument("--deb-promotion", type=Path)
    parser.add_argument("--rpm-promotion", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--version")
    parser.add_argument("--commit")
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.verify_rpm_extraction is not None:
            verify_rpm_extraction(args.verify_rpm_extraction)
        elif args.verify is not None:
            if args.expected_version is None or args.expected_commit is None:
                parser.error(
                    "--verify requires --expected-version and --expected-commit"
                )
            verify_manifest(
                args.verify.resolve(), args.expected_version, args.expected_commit
            )
        elif any(
            value is None
            for value in (
                args.canonical_binary,
                args.deb_promotion,
                args.rpm_promotion,
                args.output,
                args.version,
                args.commit,
            )
        ):
            parser.error(
                "staging requires every binary, promotion, output, version, and commit argument"
            )
        else:
            stage(args)
    except (
        KeyError,
        OSError,
        TypeError,
        ValueError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"stage-qualified-whisper-release: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
