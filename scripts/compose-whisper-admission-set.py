#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

from whisper_release_common import (
    package_inventory,
    read_json_strict,
    runtime_identity,
    sha256_file,
    tree_sha256,
    verify_admission_set,
)


def source(path: Path) -> tuple[dict[str, object], dict[str, object], Path]:
    promotion = read_json_strict(path / "promotion.json", "promotion")
    package = path / "whisper-acceleration"
    admission = verify_admission_set(package)
    if len(admission["records"]) != 1:
        raise ValueError("each source promotion must contain exactly one record")
    if promotion.get("schemaVersion") != 2:
        raise ValueError("source promotion has the wrong schema")
    if promotion.get("admissionSetSha256") != sha256_file(
        package / "admission-set.json"
    ):
        raise ValueError("source promotion admission-set digest changed")
    record = admission["records"][0]
    expected = {
        "echoCommit": record["identity"]["echoCommit"],
        "echoBinarySha256": record["identity"]["echoBinarySha256"],
        "admissionIdentityKeys": [record["identityKey"]],
        "runtimeIdentitySha256": record["identity"]["runtimeIdentitySha256"],
        "cacheSeedSha256ByIdentityKey": {
            record["identityKey"]: record["cacheSeed"]["sha256"]
        },
    }
    if any(promotion.get(field) != value for field, value in expected.items()):
        raise ValueError("source promotion metadata differs from its admission set")
    if promotion.get("packageType") not in ("deb", "rpm"):
        raise ValueError("source promotion has an invalid package type")
    return promotion, admission, package


def compose(promotions: list[Path], output: Path) -> None:
    output = output.resolve()
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    if not promotions:
        raise ValueError("at least one promotion is required")
    inputs = [source(path.resolve()) for path in promotions]
    first_promotion, first_set, first_package = inputs[0]
    first_record = first_set["records"][0]
    compatibility = (
        first_promotion["echoCommit"],
        first_promotion["echoBinarySha256"],
        first_promotion["packageType"],
        first_record["identity"]["runtimeIdentitySha256"],
        first_record["identity"]["vadSha256"],
        first_record["identity"]["protocol"],
        first_record["identity"]["languagePolicy"],
        first_record["identity"]["promptPolicy"],
        first_record["identity"]["launchContractSchema"],
        first_set["shared"],
    )
    records: list[dict[str, object]] = []
    keys: set[str] = set()
    identities: set[str] = set()
    cache_paths: set[str] = set()
    for promotion, admission, _ in inputs:
        record = admission["records"][0]
        current = (
            promotion["echoCommit"],
            promotion["echoBinarySha256"],
            promotion["packageType"],
            record["identity"]["runtimeIdentitySha256"],
            record["identity"]["vadSha256"],
            record["identity"]["protocol"],
            record["identity"]["languagePolicy"],
            record["identity"]["promptPolicy"],
            record["identity"]["launchContractSchema"],
            admission["shared"],
        )
        key = record["identityKey"]
        encoded = json.dumps(record["identity"], sort_keys=True, separators=(",", ":"))
        cache_path = record["cacheSeed"]["relativePath"]
        if current != compatibility:
            raise ValueError(
                "source promotions have incompatible binary or runtime contracts"
            )
        if key in keys or encoded in identities or cache_path in cache_paths:
            raise ValueError("source promotions contain a duplicate admission")
        keys.add(key)
        identities.add(encoded)
        cache_paths.add(cache_path)
        records.append(record)
    package = output / "whisper-acceleration"
    package.mkdir(parents=True)
    runtime_root = Path(first_set["shared"]["runtimeRelativePath"]).parent
    shutil.copytree(first_package / runtime_root, package / runtime_root)
    for _, admission, source_package in inputs:
        record = admission["records"][0]
        relative = Path(record["cacheSeed"]["relativePath"])
        shutil.copytree(source_package / relative, package / relative)
    records.sort(key=lambda record: record["identityKey"])
    result = {
        "schemaVersion": 2,
        "shared": first_set["shared"],
        "records": records,
        "inventory": package_inventory(package),
    }
    (package / "admission-set.json").write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    verify_admission_set(package)
    config = {
        "bundle": {"resources": {str(package.resolve()) + "/": "whisper-acceleration/"}}
    }
    (output / "tauri-acceleration.conf.json").write_text(
        json.dumps(config, indent=2) + "\n", encoding="utf-8"
    )
    cache_digests = {
        record["identityKey"]: record["cacheSeed"]["sha256"] for record in records
    }
    metadata = {
        "schemaVersion": 2,
        "echoCommit": compatibility[0],
        "echoBinarySha256": compatibility[1],
        "packageType": compatibility[2],
        "admissionIdentityKeys": sorted(keys),
        "admissionSetSha256": sha256_file(package / "admission-set.json"),
        "runtimeIdentitySha256": compatibility[3],
        "cacheSeedSha256ByIdentityKey": cache_digests,
    }
    (output / "promotion.json").write_text(
        json.dumps(metadata, indent=2) + "\n", encoding="utf-8"
    )


def fixture(
    root: Path,
    key_seed: str,
    model_seed: str,
    binary_seed: str = "a",
    runtime_bytes: bytes = b"cli",
) -> Path:
    package = root / "whisper-acceleration"
    runtime = package / "runtime"
    cache = package / "cache-seeds" / key_seed
    runtime.mkdir(parents=True)
    cache.mkdir(parents=True)
    (runtime / "whisper-cli").write_bytes(runtime_bytes)
    (runtime / "libwhisper.so").write_bytes(b"lib")
    (runtime / "echo-whisper-runtime-probe").write_bytes(b"probe")
    (cache / "seed").write_text(key_seed, encoding="utf-8")
    identity = {
        "schemaVersion": 1,
        "echoCommit": "4" * 40,
        "echoBinarySha256": binary_seed * 64,
        "runtimeIdentitySha256": runtime_identity(runtime / "whisper-cli"),
        "modelSha256": model_seed * 64,
        "vadSha256": "d" * 64,
        "protocol": "oneShotCli",
        "tuning": {"threads": 4, "beamSize": 1, "bestOf": 2, "noFallback": True},
        "languagePolicy": "pinned",
        "promptPolicy": "empty",
        "device": {
            "backend": "vulkan",
            "selectedIndex": 0,
            "vendorId": 32902,
            "deviceId": 18086,
            "apiVersion": 1,
            "driverVersion": 1,
            "deviceUUID": "1" * 32,
            "driverUUID": "2" * 32,
            "pipelineCacheUUID": "3" * 32,
        },
        "drmDriver": "i915",
        "icdManifestSha256": "e" * 64,
        "icdLibrarySha256": "f" * 64,
        "launchContractSchema": 1,
    }
    actual_key = hashlib.sha256(
        b"echo-whisper-admission-identity-v1\0"
        + json.dumps(identity, ensure_ascii=False, separators=(",", ":")).encode()
    ).hexdigest()
    target = package / "cache-seeds" / actual_key
    cache.rename(target)
    record = {
        "identity": identity,
        "identityKey": actual_key,
        "evidenceSha256": "a" * 64,
        "icdManifestPath": "/icd.json",
        "icdLibraryPath": "/icd.so",
        "cacheSeed": {
            "relativePath": f"cache-seeds/{actual_key}",
            "sha256": tree_sha256(target),
        },
        "gates": {
            name: True
            for name in (
                "completePairs",
                "pairIntegrity",
                "sampleSize",
                "backendTruth",
                "identityMatch",
                "hardwareDevice",
                "medianReduction",
                "medianSpeedup",
                "p95Improved",
                "perLanguageQuality",
                "noNewHallucinations",
                "receiptConsistency",
                "coverageComplete",
                "cacheEvidence",
                "resetEvidence",
                "driverIcdIdentity",
                "cleanChildEnvironment",
                "exactRuntime",
                "stabilitySuccess",
                "memoryEvidence",
                "memoryFloor",
                "swapStable",
            )
        },
        "verdict": "PASSED",
        "acceptedAt": 1,
        "expiresAt": 2,
    }
    shared = {
        "runtimeRelativePath": "runtime/whisper-cli",
        "runtimeLibraryBindings": {
            "libwhisper.so": sha256_file(runtime / "libwhisper.so")
        },
        "probeRelativePath": "runtime/echo-whisper-runtime-probe",
        "probeSha256": sha256_file(runtime / "echo-whisper-runtime-probe"),
    }
    admission = {
        "schemaVersion": 2,
        "shared": shared,
        "records": [record],
        "inventory": package_inventory(package),
    }
    (package / "admission-set.json").write_text(
        json.dumps(admission, indent=2) + "\n", encoding="utf-8"
    )
    promotion = {
        "schemaVersion": 2,
        "echoCommit": "4" * 40,
        "echoBinarySha256": binary_seed * 64,
        "packageType": "deb",
        "admissionIdentityKeys": [actual_key],
        "admissionSetSha256": sha256_file(package / "admission-set.json"),
        "runtimeIdentitySha256": identity["runtimeIdentitySha256"],
        "cacheSeedSha256ByIdentityKey": {actual_key: record["cacheSeed"]["sha256"]},
    }
    (root / "promotion.json").write_text(
        json.dumps(promotion, indent=2) + "\n", encoding="utf-8"
    )
    return root


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        small = fixture(root / "small", "small", "c")
        large = fixture(root / "large", "large", "9")
        output = root / "out"
        compose([large, small], output)
        result = verify_admission_set(output / "whisper-acceleration")
        assert len(result["records"]) == 2
        assert [r["identityKey"] for r in result["records"]] == sorted(
            r["identityKey"] for r in result["records"]
        )
        second = root / "second"
        compose([small, large], second)
        assert (output / "whisper-acceleration/admission-set.json").read_bytes() == (
            second / "whisper-acceleration/admission-set.json"
        ).read_bytes()
        assert (output / "promotion.json").read_bytes() == (
            second / "promotion.json"
        ).read_bytes()
        duplicate = root / "duplicate"
        shutil.copytree(small, duplicate)
        try:
            compose([small, duplicate], root / "duplicate-output")
        except ValueError:
            pass
        else:
            raise AssertionError("duplicate admission passed composition")
        incompatible = fixture(root / "incompatible", "other", "8", "b")
        try:
            compose([small, incompatible], root / "incompatible-output")
        except ValueError:
            pass
        else:
            raise AssertionError("incompatible binary passed composition")
        incompatible_runtime = fixture(
            root / "incompatible-runtime", "runtime", "7", runtime_bytes=b"other"
        )
        try:
            compose([small, incompatible_runtime], root / "runtime-output")
        except ValueError:
            pass
        else:
            raise AssertionError("incompatible runtime passed composition")
        metadata_drift = root / "metadata-drift"
        shutil.copytree(small, metadata_drift)
        metadata_path = metadata_drift / "promotion.json"
        metadata = read_json_strict(metadata_path, "metadata drift")
        metadata["runtimeIdentitySha256"] = "0" * 64
        metadata_path.write_text(
            json.dumps(metadata, indent=2) + "\n", encoding="utf-8"
        )
        try:
            compose([metadata_drift], root / "metadata-output")
        except ValueError:
            pass
        else:
            raise AssertionError("source promotion metadata drift passed composition")
        source_digest = sha256_file(small / "whisper-acceleration/admission-set.json")
        assert source_digest == sha256_file(
            small / "whisper-acceleration/admission-set.json"
        )
        package = output / "whisper-acceleration"
        for name, mutate in (
            ("extra", lambda candidate: (candidate / "extra").write_bytes(b"x")),
            (
                "missing",
                lambda candidate: next(
                    (candidate / "cache-seeds").rglob("seed")
                ).unlink(),
            ),
            (
                "type",
                lambda candidate: (
                    (candidate / "runtime/whisper-cli").unlink(),
                    (candidate / "runtime/whisper-cli").mkdir(),
                ),
            ),
            (
                "seed-drift",
                lambda candidate: next(
                    (candidate / "cache-seeds").rglob("seed")
                ).write_bytes(b"drift"),
            ),
        ):
            candidate = root / f"drift-{name}"
            shutil.copytree(package, candidate)
            mutate(candidate)
            try:
                verify_admission_set(candidate)
            except (IsADirectoryError, ValueError):
                pass
            else:
                raise AssertionError(f"{name} drift passed verification")
        link = root / "drift-link"
        shutil.copytree(package, link)
        probe = link / "runtime/echo-whisper-runtime-probe"
        probe.unlink()
        probe.symlink_to("whisper-cli")
        try:
            verify_admission_set(link)
        except ValueError:
            pass
        else:
            raise AssertionError("link drift passed verification")
        path_drift = root / "drift-path"
        shutil.copytree(package, path_drift)
        manifest_path = path_drift / "admission-set.json"
        manifest = read_json_strict(manifest_path, "path drift")
        manifest["inventory"][0]["path"] = "../escape"
        manifest_path.write_text(
            json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
        )
        try:
            verify_admission_set(path_drift)
        except ValueError:
            pass
        else:
            raise AssertionError("path drift passed verification")
        invalid_identity = root / "invalid-identity"
        shutil.copytree(package, invalid_identity)
        invalid_manifest_path = invalid_identity / "admission-set.json"
        invalid_manifest = read_json_strict(invalid_manifest_path, "invalid identity")
        invalid_manifest["records"][0]["identity"]["protocol"] = "other"
        invalid_manifest_path.write_text(
            json.dumps(invalid_manifest, indent=2) + "\n", encoding="utf-8"
        )
        try:
            verify_admission_set(invalid_identity)
        except ValueError:
            pass
        else:
            raise AssertionError("Rust-incompatible identity passed verification")
    print("compose-whisper-admission-set: self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Compose exact Whisper admission promotions"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--promotion", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.output is None:
            parser.error("composition requires --output")
        else:
            compose(args.promotion, args.output)
    except (KeyError, OSError, TypeError, ValueError) as error:
        print(f"compose-whisper-admission-set: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
