#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from whisper_identity_v3 import (
    ADMISSION_GATE_FIELDS,
    COMMIT,
    build_record,
    canonical_json_bytes,
    verify_acceleration_set,
    verify_release_binding_record,
    verify_v3_promotion_metadata,
    v3_promotion_metadata,
)
from whisper_v3_contract import verify_reusable_evidence_for_commit
from whisper_release_common import (
    BUNDLE_MARKER,
    BUNDLE_TOKENS,
    bundle_variant,
    package_inventory,
    runtime_identity,
    runtime_library_bindings,
    read_json_strict,
    sha256_bytes,
    sha256_file,
    tree_sha256,
    verify_contained_symlinks,
    verify_admission_set,
    verify_v3_reusable_filesystem,
    v3_declared_inventory,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
CANONICAL_BINARY = REPO_ROOT / "target/release/echo-desktop"
BUILD_COMMIT_PREFIX = b"\0echo-build-commit-v1\0"


def read_json(path: Path) -> dict[str, object]:
    return read_json_strict(path, str(path))


def variant_bytes(canonical: bytes, bundle_type: str) -> bytes:
    return bundle_variant(canonical, bundle_type)


def compiled_build_commit(binary: bytes) -> str:
    starts = []
    offset = 0
    while True:
        found = binary.find(BUILD_COMMIT_PREFIX, offset)
        if found < 0:
            break
        starts.append(found)
        offset = found + 1
    if len(starts) != 1:
        raise ValueError("Echo binary does not contain exactly one build commit marker")
    value_start = starts[0] + len(BUILD_COMMIT_PREFIX)
    value_end = value_start + 40
    if value_end >= len(binary) or binary[value_end] != 0:
        raise ValueError("Echo binary build commit marker is malformed")
    try:
        commit = binary[value_start:value_end].decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError("Echo binary build commit marker is not ASCII") from error
    if COMMIT.fullmatch(commit) is None:
        raise ValueError("Echo binary build commit marker is not a commit")
    return commit


def verify_embedded_commit(binary: bytes, commit: str) -> None:
    if compiled_build_commit(binary) != commit:
        raise ValueError("Echo binary build commit differs from its release binding")


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
    packaged_admission = resource / "admission-set.json"
    if promotion_root is not None:
        source_resource = promotion_root / "whisper-acceleration"
        verify_admission_set(source_resource)
        source_admission = source_resource / "admission-set.json"
        if packaged_admission.read_bytes() != source_admission.read_bytes():
            raise ValueError("packaged admission set changed")
        source_inventory = {
            path.relative_to(source_resource).as_posix(): path.read_bytes()
            for path in source_resource.rglob("*")
            if path.is_file() and not path.is_symlink()
        }
        packaged_inventory = {
            path.relative_to(resource).as_posix(): path.read_bytes()
            for path in resource.rglob("*")
            if path.is_file() and not path.is_symlink()
        }
        if packaged_inventory != source_inventory:
            raise ValueError("packaged acceleration tree changed")
    admission = verify_admission_set(resource)
    verify_contained_symlinks(resource)
    records = admission["records"]
    if any(
        record["identity"]["echoBinarySha256"] != sha256_file(binary)
        for record in records
    ):
        raise ValueError("admission set does not bind the packaged executable")
    runtime = resource / admission["shared"]["runtimeRelativePath"]
    runtime_digest = records[0]["identity"]["runtimeIdentitySha256"]
    if runtime_identity(runtime) != runtime_digest:
        raise ValueError("packaged runtime identity changed")
    packaged_library_bindings = runtime_library_bindings(runtime)
    if packaged_library_bindings != admission["shared"]["runtimeLibraryBindings"]:
        raise ValueError("packaged runtime library alias admission changed")
    if promotion_root is not None:
        source_runtime = (
            promotion_root
            / "whisper-acceleration"
            / admission["shared"]["runtimeRelativePath"]
        )
        if packaged_library_bindings != runtime_library_bindings(source_runtime):
            raise ValueError("packaged runtime library alias bindings changed")
    probe = resource / admission["shared"]["probeRelativePath"]
    if sha256_file(probe) != admission["shared"]["probeSha256"]:
        raise ValueError("packaged runtime probe identity changed")
    cache_digests = {}
    for record in records:
        cache_seed = resource / record["cacheSeed"]["relativePath"]
        if tree_sha256(cache_seed) != record["cacheSeed"]["sha256"]:
            raise ValueError("packaged cache seed identity changed")
        cache_digests[record["identityKey"]] = record["cacheSeed"]["sha256"]
    identity_keys = sorted(record["identityKey"] for record in records)
    return {
        "echoCommit": records[0]["identity"]["echoCommit"],
        "binarySha256": sha256_file(binary),
        "admissionSetSha256": sha256_file(packaged_admission),
        "runtimeIdentitySha256": runtime_digest,
        "runtimeLibraryBindings": packaged_library_bindings,
        "cacheSeedSha256ByIdentityKey": cache_digests,
        "admissionIdentityKeys": identity_keys,
    }


def copy_v3_evidence(source_root: Path, destination_root: Path) -> dict[str, object]:
    source = source_root / "whisper-acceleration"
    acceleration_set = read_json(source / "acceleration-set.v3.json")
    verify_acceleration_set(acceleration_set)
    destination_root.mkdir(parents=True)
    runtime_relative = Path(
        acceleration_set["executionArtifact"]["value"]["runtimeRelativePath"]
    ).parent
    shutil.copytree(
        source / runtime_relative, destination_root / runtime_relative, symlinks=True
    )
    for record in acceleration_set["performanceEvidence"]:
        relative = Path(record["cacheSeed"]["relativePath"])
        destination = destination_root / relative
        if not destination.exists():
            shutil.copytree(source / relative, destination, symlinks=True)
    shutil.copy2(
        source / "acceleration-set.v3.json",
        destination_root / "acceleration-set.v3.json",
    )
    verify_contained_symlinks(destination_root)
    verify_v3_reusable_filesystem(destination_root, acceleration_set)
    return acceleration_set


def v3_release_binding_input(
    *,
    acceleration_set: dict[str, object],
    promotion: dict[str, object],
    package_type: str,
    version: str,
    commit: str,
    binary: bytes,
) -> dict[str, object]:
    verify_v3_promotion_metadata(promotion, acceleration_set)
    verify_embedded_commit(binary, commit)
    return {
        "schemaVersion": 3,
        "packageType": package_type,
        "version": version,
        "echoCommit": commit,
        "echoBinarySha256": sha256_bytes(binary),
        "bundleMarker": package_type,
        "productionReadiness": "proof-only-until-pr16.3",
        "accelerationSetSha256": promotion["accelerationSetSha256"],
        "executionArtifactId": promotion["executionArtifactId"],
        "allowedInferenceContractIds": promotion["inferenceContractIds"],
        "allowedPerformanceEvidenceIds": promotion["performanceEvidenceIds"],
        "reusableInventorySha256": promotion["reusableInventorySha256"],
    }


def write_v3_release_binding(
    resource: Path,
    acceleration_set: dict[str, object],
    binding_input: dict[str, object],
) -> dict[str, object]:
    record = build_record("releaseBinding", binding_input)
    verify_release_binding_record(record, acceleration_set)
    (resource / "release-binding.v3.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return record


def verify_extracted_v3(
    extracted: Path,
    expected_binary: bytes,
    source_resource: Path | None,
) -> dict[str, object]:
    binary = extracted / "usr/bin/echo-desktop"
    resource = extracted / "usr/lib/io.github.ddv1982.echo/whisper-acceleration"
    if binary.read_bytes() != expected_binary:
        raise ValueError(
            "packaged v3 executable differs outside the Tauri bundle marker"
        )
    acceleration_set = read_json(resource / "acceleration-set.v3.json")
    binding = read_json(resource / "release-binding.v3.json")
    identities = verify_acceleration_set(acceleration_set)
    verify_v3_reusable_filesystem(resource, acceleration_set)
    binding_id = verify_release_binding_record(binding, acceleration_set)
    verify_embedded_commit(binary.read_bytes(), binding["value"]["echoCommit"])
    if binding["value"]["echoBinarySha256"] != sha256_file(binary):
        raise ValueError("v3 release binding does not match the packaged executable")
    if source_resource is not None:
        if (resource / "acceleration-set.v3.json").read_bytes() != (
            source_resource / "acceleration-set.v3.json"
        ).read_bytes():
            raise ValueError("packaged v3 acceleration set changed")
        source_inventory = {
            path.relative_to(source_resource).as_posix(): path.read_bytes()
            for path in source_resource.rglob("*")
            if path.is_file()
            and not path.is_symlink()
            and path.name != "release-binding.v3.json"
        }
        packaged_inventory = {
            path.relative_to(resource).as_posix(): path.read_bytes()
            for path in resource.rglob("*")
            if path.is_file()
            and not path.is_symlink()
            and path.name != "release-binding.v3.json"
        }
        if packaged_inventory != source_inventory:
            raise ValueError("packaged v3 reusable evidence changed")
    verify_contained_symlinks(resource)
    return {
        "binarySha256": sha256_file(binary),
        "echoCommit": binding["value"]["echoCommit"],
        "version": binding["value"]["version"],
        "packageType": binding["value"]["packageType"],
        "productionReadiness": binding["value"]["productionReadiness"],
        "productionReady": False,
        "releaseBindingId": binding_id,
        "executionArtifactId": identities["executionArtifactId"],
        "inferenceContractIds": identities["inferenceContractIds"],
        "performanceEvidenceIds": identities["performanceEvidenceIds"],
        "physicalRequalificationRequired": False,
        "reusedInferenceEvidence": True,
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
    if promotion.get("packageType") != bundle_type:
        raise ValueError(f"{bundle_type} promotion has the wrong package type")
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
    for field in (
        "admissionSetSha256",
        "runtimeIdentitySha256",
        "cacheSeedSha256ByIdentityKey",
        "admissionIdentityKeys",
    ):
        if details[field] != promotion.get(field):
            raise ValueError(f"{bundle_type} promotion {field} changed")
    return {
        "file": destination.name,
        "sha256": sha256_file(destination),
        **details,
    }


def bundle_v3_one(
    bundle_type: str,
    reusable_root: Path,
    output: Path,
    canonical: bytes,
    version: str,
    commit: str,
) -> dict[str, object]:
    promotion = read_json(reusable_root / "promotion-v3.json")
    expected_binary = variant_bytes(canonical, bundle_type)
    with tempfile.TemporaryDirectory(prefix=f"echo-v3-{bundle_type}-") as temporary:
        temporary_root = Path(temporary)
        resource = temporary_root / "whisper-acceleration"
        acceleration_set = copy_v3_evidence(reusable_root, resource)
        binding_input = v3_release_binding_input(
            acceleration_set=acceleration_set,
            promotion=promotion,
            package_type=bundle_type,
            version=version,
            commit=commit,
            binary=expected_binary,
        )
        write_v3_release_binding(resource, acceleration_set, binding_input)
        config = {
            "bundle": {
                "resources": {str(resource.resolve()) + "/": "whisper-acceleration/"}
            }
        }
        config_path = temporary_root / "tauri-acceleration-v3.conf.json"
        config_path.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
        preserve_old_bundle(f"v3-{bundle_type}")
        subprocess.run(
            [
                "cargo",
                "tauri",
                "bundle",
                "--ci",
                "--bundles",
                bundle_type,
                "--config",
                str(config_path),
            ],
            cwd=REPO_ROOT,
            check=True,
        )
        packages = list(
            (REPO_ROOT / f"target/release/bundle/{bundle_type}").glob(
                f"*.{bundle_type}"
            )
        )
        if len(packages) != 1:
            raise ValueError(f"expected one {bundle_type} package")
        destination = output / packages[0].name
        shutil.copy2(packages[0], destination)
        with tempfile.TemporaryDirectory(
            prefix=f"echo-v3-{bundle_type}-extract-"
        ) as extracted_temporary:
            extracted = Path(extracted_temporary)
            extract_package(destination, bundle_type, extracted)
            details = verify_extracted_v3(extracted, expected_binary, resource)
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
    if args.reusable_evidence is not None:
        verify_embedded_commit(canonical, args.commit)
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
    if args.reusable_evidence is not None:
        reusable = args.reusable_evidence.resolve()
        reusable_set = read_json(
            reusable / "whisper-acceleration/acceleration-set.v3.json"
        )
        verify_reusable_evidence_for_commit(
            repo_root=REPO_ROOT,
            commit=args.commit,
            acceleration_set=reusable_set,
            now=int(time.time()),
        )
        assets = {
            bundle_type: bundle_v3_one(
                bundle_type,
                reusable,
                output,
                canonical,
                args.version,
                args.commit,
            )
            for bundle_type in ("deb", "rpm")
        }
    else:
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
        "schemaVersion": 3 if args.reusable_evidence is not None else 2,
        "version": args.version,
        "echoCommit": args.commit,
        "assets": assets,
    }
    if args.reusable_evidence is not None:
        manifest["physicalRequalificationRequired"] = False
        manifest["reusedInferenceEvidence"] = True
        manifest["productionReadiness"] = "proof-only-until-pr16.3"
        manifest["productionReady"] = False
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


def verify_manifest(
    root: Path,
    expected_version: str,
    expected_commit: str,
    *,
    require_production_ready: bool = False,
) -> None:
    manifest = read_json(root / "qualified-release.json")
    schema_version = manifest.get("schemaVersion")
    if (
        schema_version not in {2, 3}
        or manifest.get("version") != expected_version
        or manifest.get("echoCommit") != expected_commit
    ):
        raise ValueError("qualified release identity does not match the tag")
    if schema_version == 3 and (
        manifest.get("physicalRequalificationRequired") is not False
        or manifest.get("reusedInferenceEvidence") is not True
        or manifest.get("productionReadiness") != "proof-only-until-pr16.3"
        or manifest.get("productionReady") is not False
    ):
        raise ValueError("v3 qualified release did not reuse inference evidence")
    if (
        require_production_ready
        and schema_version == 3
        and manifest.get("productionReady") is not True
    ):
        raise ValueError("qualified release is proof-only and not production-ready")
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
            if schema_version == 3:
                details = verify_extracted_v3(
                    extracted,
                    variant_bytes(canonical, bundle_type),
                    None,
                )
                fields = (
                    "binarySha256",
                    "releaseBindingId",
                    "executionArtifactId",
                    "inferenceContractIds",
                    "performanceEvidenceIds",
                    "physicalRequalificationRequired",
                    "reusedInferenceEvidence",
                    "productionReadiness",
                    "productionReady",
                )
            else:
                details = verify_extracted(
                    extracted,
                    variant_bytes(canonical, bundle_type),
                    None,
                )
                fields = (
                    "echoCommit",
                    "binarySha256",
                    "admissionSetSha256",
                    "runtimeIdentitySha256",
                    "runtimeLibraryBindings",
                    "cacheSeedSha256ByIdentityKey",
                    "admissionIdentityKeys",
                )
        for field in fields:
            if details[field] != assets[bundle_type].get(field):
                raise ValueError(f"qualified {bundle_type} {field} changed")
        if details["echoCommit"] != expected_commit:
            raise ValueError(f"qualified {bundle_type} belongs to another commit")
        if schema_version == 3 and (
            details["version"] != expected_version
            or details["packageType"] != bundle_type
        ):
            raise ValueError(f"qualified {bundle_type} release binding changed")


def self_test() -> None:
    from unittest.mock import patch

    canonical = b"before" + BUNDLE_MARKER + b"after"
    marker_commit = "a" * 40
    marker = BUILD_COMMIT_PREFIX + marker_commit.encode() + b"\0"
    assert compiled_build_commit(canonical + marker) == marker_commit
    for invalid_marker in (
        marker_commit.encode(),
        marker + marker,
        BUILD_COMMIT_PREFIX + marker_commit.encode(),
        BUILD_COMMIT_PREFIX + b"unbound" + b"_" * 33 + b"\0",
    ):
        try:
            compiled_build_commit(canonical + invalid_marker)
        except ValueError:
            pass
        else:
            raise AssertionError("malformed build commit marker was accepted")
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
            "echo-desktop",
            "echo.deb",
            "echo.rpm",
        }
        (inventory / "echo-desktop").write_bytes(b"binary")
        (inventory / "echo.deb").write_bytes(b"deb")
        (inventory / "echo.rpm").write_bytes(b"rpm")
        (inventory / "asset").unlink()
        verify_asset_inventory(inventory, expected_inventory)
        (inventory / "unexpected.rpm").write_bytes(b"extra")
        try:
            verify_asset_inventory(inventory, expected_inventory)
        except ValueError:
            pass
        else:
            raise AssertionError(
                "unexpected release asset passed inventory verification"
            )
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
        fixture_path = (
            REPO_ROOT / "crates/echo/tests/fixtures/whisper-v3-identities.json"
        )
        cases = read_json(fixture_path)["cases"]
        reusable = root / "reusable"
        reusable_resource = reusable / "whisper-acceleration"
        reusable_runtime = reusable_resource / "runtime"
        reusable_runtime.mkdir(parents=True)
        reusable_cli = reusable_runtime / "whisper-cli"
        reusable_probe = reusable_runtime / "echo-whisper-runtime-probe"
        reusable_receipt = reusable_runtime / "build-receipt.json"
        reusable_cli.write_bytes(b"runtime")
        reusable_probe.write_bytes(b"probe")
        (reusable_runtime / "libwhisper.so").write_bytes(b"library")
        reusable_receipt.write_text(
            json.dumps({"artifactId": "4" * 64}), encoding="utf-8"
        )
        runtime_inventory = []
        for entry in package_inventory(reusable_runtime):
            copied = dict(entry)
            copied["path"] = f"runtime/{entry['path']}"
            runtime_inventory.append(copied)
        execution_input = {
            "schemaVersion": 3,
            "runtimeArtifactId": "4" * 64,
            "runtimeIdentitySha256": runtime_identity(reusable_cli),
            "runtimeRelativePath": "runtime/whisper-cli",
            "runtimeSha256": sha256_file(reusable_cli),
            "runtimeLibraryBindings": runtime_library_bindings(reusable_cli),
            "probeRelativePath": "runtime/echo-whisper-runtime-probe",
            "probeSha256": sha256_file(reusable_probe),
            "buildReceiptSha256": sha256_file(reusable_receipt),
            "reusableInventorySha256": sha256_bytes(
                canonical_json_bytes(runtime_inventory)
            ),
        }
        execution = build_record("executionArtifact", execution_input)
        evidence_input = json.loads(json.dumps(cases["performanceEvidence"]["input"]))
        evidence_input["executionArtifactId"] = execution["id"]
        evidence = build_record("performanceEvidence", evidence_input)
        evidence_id = evidence["id"]
        cache_relative = f"cache-seeds/{evidence_id}"
        reusable_cache = reusable_resource / cache_relative
        reusable_cache.mkdir(parents=True)
        (reusable_cache / "seed").write_bytes(b"seed")
        acceleration_set = {
            "schemaVersion": 3,
            "executionArtifact": execution,
            "inferenceContracts": [
                build_record("inferenceContract", cases["inferenceContract"]["input"])
            ],
            "localEnvironments": [
                {
                    "key": cases["localEnvironment"]["id"],
                    "launch": {
                        "icdManifestPath": "/usr/share/vulkan/icd.d/intel_icd.json",
                        "icdLibraryPath": "/usr/lib/libvulkan_intel.so",
                    },
                    "value": cases["localEnvironment"]["input"],
                }
            ],
            "performanceEvidence": [
                {
                    "cacheSeed": {
                        "relativePath": cache_relative,
                        "sha256": tree_sha256(reusable_cache),
                    },
                    "gates": {name: True for name in ADMISSION_GATE_FIELDS},
                    "id": evidence_id,
                    "value": evidence_input,
                    "verdict": "PASSED",
                }
            ],
            "reusableInventorySha256": "3" * 64,
        }
        acceleration_set["reusableInventorySha256"] = sha256_bytes(
            canonical_json_bytes(
                v3_declared_inventory(reusable_resource, acceleration_set)
            )
        )
        verify_acceleration_set(acceleration_set)
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        verify_reusable_evidence_for_commit(
            repo_root=REPO_ROOT,
            commit=head,
            acceleration_set=acceleration_set,
            now=evidence_input["acceptedAt"] + 1,
        )
        try:
            verify_reusable_evidence_for_commit(
                repo_root=REPO_ROOT,
                commit=head,
                acceleration_set=acceleration_set,
                now=evidence_input["expiresAt"] + 1,
            )
        except ValueError:
            pass
        else:
            raise AssertionError("expired reusable evidence was accepted")
        (reusable_resource / "acceleration-set.v3.json").write_text(
            json.dumps(acceleration_set, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        promotion = v3_promotion_metadata(acceleration_set)
        (reusable / "promotion-v3.json").write_text(
            json.dumps(promotion, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        forged = root / "forged-runtime"
        shutil.copytree(reusable, forged)
        forged_resource = forged / "whisper-acceleration"
        (forged_resource / "runtime/whisper-cli").write_bytes(b"changed runtime")
        forged_set = read_json(forged_resource / "acceleration-set.v3.json")
        forged_set["reusableInventorySha256"] = sha256_bytes(
            canonical_json_bytes(v3_declared_inventory(forged_resource, forged_set))
        )
        (forged_resource / "acceleration-set.v3.json").write_text(
            json.dumps(forged_set, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        try:
            copy_v3_evidence(forged, root / "forged-staged-resource")
        except ValueError:
            pass
        else:
            raise AssertionError("forged runtime retained the old execution artifact")
        staged_resource = root / "staged-resource"
        copied_set = copy_v3_evidence(reusable, staged_resource)
        measured_commit = "a" * 40
        marker = BUILD_COMMIT_PREFIX + measured_commit.encode() + b"\0"
        deb_binary = variant_bytes(canonical, "deb") + marker
        try:
            v3_release_binding_input(
                acceleration_set=copied_set,
                promotion=promotion,
                package_type="deb",
                version="0.12.5",
                commit=measured_commit,
                binary=variant_bytes(canonical, "deb") + measured_commit.encode(),
            )
        except ValueError:
            pass
        else:
            raise AssertionError("release binding accepted an uncommitted Echo binary")
        deb_input = v3_release_binding_input(
            acceleration_set=copied_set,
            promotion=promotion,
            package_type="deb",
            version="0.12.5",
            commit=measured_commit,
            binary=deb_binary,
        )
        deb_record = write_v3_release_binding(staged_resource, copied_set, deb_input)
        extracted = root / "v3-extracted"
        (extracted / "usr/bin").mkdir(parents=True)
        (extracted / "usr/bin/echo-desktop").write_bytes(deb_binary)
        extracted_resource = (
            extracted / "usr/lib/io.github.ddv1982.echo/whisper-acceleration"
        )
        shutil.copytree(staged_resource, extracted_resource, symlinks=True)
        details = verify_extracted_v3(extracted, deb_binary, staged_resource)
        assert details["releaseBindingId"] == deb_record["id"]
        assert details["physicalRequalificationRequired"] is False
        assert details["productionReadiness"] == "proof-only-until-pr16.3"
        assert details["productionReady"] is False

        def expect_filesystem_rejection(label, mutate):
            candidate = root / f"v3-invalid-{label}"
            shutil.copytree(extracted, candidate, symlinks=True)
            candidate_resource = (
                candidate / "usr/lib/io.github.ddv1982.echo/whisper-acceleration"
            )
            mutate(candidate_resource)
            try:
                verify_extracted_v3(candidate, deb_binary, None)
            except ValueError:
                return
            raise AssertionError(f"v3 {label} filesystem mutation was accepted")

        expect_filesystem_rejection(
            "missing-cli", lambda resource: (resource / "runtime/whisper-cli").unlink()
        )
        expect_filesystem_rejection(
            "missing-probe",
            lambda resource: (resource / "runtime/echo-whisper-runtime-probe").unlink(),
        )
        expect_filesystem_rejection(
            "changed-runtime",
            lambda resource: (resource / "runtime/whisper-cli").write_bytes(b"changed"),
        )
        expect_filesystem_rejection(
            "changed-cache",
            lambda resource: (resource / cache_relative / "seed").write_bytes(
                b"changed"
            ),
        )
        expect_filesystem_rejection(
            "extra-file",
            lambda resource: (resource / "unexpected").write_bytes(b"extra"),
        )
        changed_version = dict(deb_input)
        changed_version["version"] = "0.12.6"
        changed_record = build_record("releaseBinding", changed_version)
        assert changed_record["id"] != deb_record["id"]
        assert (
            changed_version["executionArtifactId"] == deb_input["executionArtifactId"]
        )
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
    parser.add_argument("--require-production-ready", action="store_true")
    parser.add_argument("--canonical-binary", type=Path)
    parser.add_argument("--deb-promotion", type=Path)
    parser.add_argument("--rpm-promotion", type=Path)
    parser.add_argument("--reusable-evidence", type=Path)
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
                args.verify.resolve(),
                args.expected_version,
                args.expected_commit,
                require_production_ready=args.require_production_ready,
            )
        elif any(
            value is None
            for value in (
                args.canonical_binary,
                args.output,
                args.version,
                args.commit,
            )
        ) or (
            args.reusable_evidence is None
            and (args.deb_promotion is None or args.rpm_promotion is None)
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
