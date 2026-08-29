from __future__ import annotations

import hashlib
import json
import os
import tempfile
import time
from pathlib import Path

from whisper_identity_v3 import (
    ADMISSION_GATE_FIELDS,
    canonical_json_bytes,
    verify_acceleration_set,
)

BUNDLE_MARKER = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
BUNDLE_TOKENS = {
    "deb": b"__TAURI_BUNDLE_TYPE_VAR_DEB",
    "rpm": b"__TAURI_BUNDLE_TYPE_VAR_RPM",
    "appimage": b"__TAURI_BUNDLE_TYPE_VAR_APP",
}
MAX_ADMISSION_SET_BYTES = 1024 * 1024
MAX_ADMISSION_RECORDS = 128
MAX_PACKAGE_ENTRIES = 4096
MAX_PACKAGE_ENTRY_BYTES = 1024 * 1024 * 1024
MAX_PACKAGE_BYTES = 4 * 1024 * 1024 * 1024
IDENTITY_FIELDS = {
    "schemaVersion",
    "echoCommit",
    "echoBinarySha256",
    "runtimeIdentitySha256",
    "modelSha256",
    "vadSha256",
    "protocol",
    "tuning",
    "languagePolicy",
    "promptPolicy",
    "device",
    "drmDriver",
    "icdManifestSha256",
    "icdLibrarySha256",
    "launchContractSchema",
}
TUNING_FIELDS = {"threads", "beamSize", "bestOf", "noFallback"}
DEVICE_FIELDS = {
    "backend",
    "selectedIndex",
    "vendorId",
    "deviceId",
    "apiVersion",
    "driverVersion",
    "deviceUUID",
    "driverUUID",
    "pipelineCacheUUID",
}
GATE_FIELDS = ADMISSION_GATE_FIELDS


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_sha256(root: Path) -> str:
    files = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError("cache seed must not contain symlinks")
        if path.is_file():
            files.append((path.relative_to(root).as_posix(), path))
        elif not path.is_dir():
            raise ValueError(f"unsupported cache entry: {path}")
    if not files:
        raise ValueError("cache seed must contain files")
    digest = hashlib.sha256(b"echo-whisper-tree-v1\0")
    for relative, path in sorted(files):
        name = relative.encode()
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(path.stat().st_size.to_bytes(8, "little"))
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def runtime_libraries(cli: Path) -> list[Path]:
    by_content: dict[tuple[int, str], Path] = {}
    for candidate in cli.parent.iterdir():
        if ".so" not in candidate.name:
            continue
        try:
            path = candidate.resolve(strict=True)
        except OSError:
            continue
        if not path.is_file():
            continue
        key = (path.stat().st_size, sha256_file(path))
        selected = by_content.get(key)
        if selected is None or (len(path.name), path.name) > (
            len(selected.name),
            selected.name,
        ):
            by_content[key] = path
    return sorted(by_content.values())


def runtime_library_bindings(cli: Path) -> dict[str, str]:
    bindings = {}
    for candidate in cli.parent.iterdir():
        if ".so" not in candidate.name:
            continue
        try:
            path = candidate.resolve(strict=True)
        except OSError:
            continue
        if path.is_file():
            bindings[candidate.name] = sha256_file(path)
    return dict(sorted(bindings.items()))


def runtime_identity(cli: Path) -> str:
    digest = hashlib.sha256(b"echo-whisper-runtime-v1\0")
    for path in [cli.resolve(), *runtime_libraries(cli)]:
        name = path.name.encode()
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(path.stat().st_size.to_bytes(8, "little"))
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def bundle_variant(canonical: bytes, bundle_type: str) -> bytes:
    if canonical.count(BUNDLE_MARKER) != 1:
        raise ValueError(
            "canonical binary must contain one unknown Tauri bundle marker"
        )
    return canonical.replace(BUNDLE_MARKER, BUNDLE_TOKENS[bundle_type], 1)


def verify_contained_symlinks(root: Path) -> None:
    resolved_root = root.resolve()
    for path in root.rglob("*"):
        if path.is_symlink() and not path.resolve(strict=True).is_relative_to(
            resolved_root
        ):
            raise ValueError(f"symlink escapes package root: {path}")


def read_json_strict(path: Path, label: str) -> dict[str, object]:
    def unique(pairs: list[tuple[str, object]]) -> dict[str, object]:
        value: dict[str, object] = {}
        for key, item in pairs:
            if key in value:
                raise ValueError(f"{label} has duplicate key {key!r}")
            value[key] = item
        return value

    value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique)
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    return value


def safe_relative(value: object) -> bool:
    if not isinstance(value, str) or not value or value.startswith("/"):
        return False
    return all(part not in ("", ".", "..") for part in value.split("/"))


def sha256_string(value: object) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def lower_hex(value: object, length: int) -> bool:
    return (
        isinstance(value, str)
        and len(value) == length
        and all(character in "0123456789abcdef" for character in value)
    )


def bounded_integer(value: object, minimum: int, maximum: int) -> bool:
    return (
        isinstance(value, int)
        and not isinstance(value, bool)
        and minimum <= value <= maximum
    )


def package_inventory(root: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    total = 0
    for path in sorted(
        root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()
    ):
        relative = path.relative_to(root).as_posix()
        if relative == "admission-set.json" or path.is_dir():
            continue
        metadata = path.lstat()
        if metadata.st_size > MAX_PACKAGE_ENTRY_BYTES:
            raise ValueError(f"package entry exceeds 1 GiB: {relative}")
        total += metadata.st_size
        if total > MAX_PACKAGE_BYTES:
            raise ValueError("package inventory exceeds 4 GiB")
        if path.is_symlink():
            target = os.readlink(path)
            if not safe_relative(target):
                raise ValueError(f"invalid package symlink target: {relative}")
            resolved = (path.parent / target).resolve(strict=True)
            if not resolved.is_relative_to(root.resolve()):
                raise ValueError(f"symlink escapes package root: {path}")
            entries.append(
                {
                    "path": relative,
                    "kind": "symlink",
                    "bytes": metadata.st_size,
                    "sha256": None,
                    "linkTarget": target,
                }
            )
        elif path.is_file():
            entries.append(
                {
                    "path": relative,
                    "kind": "file",
                    "bytes": metadata.st_size,
                    "sha256": sha256_file(path),
                    "linkTarget": None,
                }
            )
        else:
            raise ValueError(f"unsupported package entry: {relative}")
    if not entries or len(entries) > MAX_PACKAGE_ENTRIES:
        raise ValueError("package inventory is empty or exceeds 4096 entries")
    return entries


def prefixed_package_inventory(root: Path, prefix: str) -> list[dict[str, object]]:
    entries = []
    for entry in package_inventory(root):
        copied = dict(entry)
        copied["path"] = f"{prefix}/{entry['path']}"
        entries.append(copied)
    return entries


def v3_declared_inventory(
    root: Path, acceleration_set: dict[str, object]
) -> list[dict[str, object]]:
    verify_acceleration_set(acceleration_set)
    runtime_root = Path(
        acceleration_set["executionArtifact"]["value"]["runtimeRelativePath"]
    ).parent
    expected = prefixed_package_inventory(root / runtime_root, str(runtime_root))
    for record in acceleration_set["performanceEvidence"]:
        relative = Path(record["cacheSeed"]["relativePath"])
        expected.extend(prefixed_package_inventory(root / relative, str(relative)))

    return expected


def verify_v3_execution_files(root: Path, acceleration_set: dict[str, object]) -> None:
    expected = acceleration_set["executionArtifact"]["value"]
    runtime_relative = Path(expected["runtimeRelativePath"])
    probe_relative = Path(expected["probeRelativePath"])
    runtime = root / runtime_relative
    probe = root / probe_relative
    receipt_path = runtime.parent / "build-receipt.json"
    if not runtime.is_file() or not probe.is_file() or not receipt_path.is_file():
        raise ValueError("v3 execution artifact files are missing")
    receipt = read_json_strict(receipt_path, "runtime build receipt")
    runtime_inventory = prefixed_package_inventory(
        runtime.parent, runtime_relative.parent.as_posix()
    )
    actual = {
        "runtimeArtifactId": receipt.get("artifactId"),
        "runtimeIdentitySha256": runtime_identity(runtime),
        "runtimeSha256": sha256_file(runtime),
        "runtimeLibraryBindings": runtime_library_bindings(runtime),
        "probeSha256": sha256_file(probe),
        "buildReceiptSha256": sha256_file(receipt_path),
        "reusableInventorySha256": sha256_bytes(
            canonical_json_bytes(runtime_inventory)
        ),
    }
    if any(actual[field] != expected[field] for field in actual):
        raise ValueError("v3 runtime files differ from the execution artifact")


def verify_v3_reusable_subset(
    root: Path, acceleration_set: dict[str, object]
) -> list[dict[str, object]]:
    inventory = v3_declared_inventory(root, acceleration_set)
    verify_v3_execution_files(root, acceleration_set)
    if sha256_bytes(canonical_json_bytes(inventory)) != acceleration_set.get(
        "reusableInventorySha256"
    ):
        raise ValueError("v3 reusable filesystem digest differs")
    for record in acceleration_set["performanceEvidence"]:
        relative = Path(record["cacheSeed"]["relativePath"])
        if tree_sha256(root / relative) != record["cacheSeed"]["sha256"]:
            raise ValueError("v3 cache seed differs from its evidence record")
    return inventory


def v3_reusable_inventory(
    root: Path, acceleration_set: dict[str, object]
) -> list[dict[str, object]]:
    expected = verify_v3_reusable_subset(root, acceleration_set)
    manifests = {"acceleration-set.v3.json", "release-binding.v3.json"}
    actual = [
        entry for entry in package_inventory(root) if entry["path"] not in manifests
    ]

    def by_path(entry: dict[str, object]) -> object:
        return entry["path"]

    if sorted(actual, key=by_path) != sorted(expected, key=by_path):
        raise ValueError("v3 reusable filesystem differs from its declared inventory")
    return expected


def verify_v3_reusable_filesystem(
    root: Path, acceleration_set: dict[str, object]
) -> None:
    v3_reusable_inventory(root, acceleration_set)


def verify_admission_set(root: Path) -> dict[str, object]:
    manifest_path = root / "admission-set.json"
    if manifest_path.stat().st_size > MAX_ADMISSION_SET_BYTES:
        raise ValueError("admission set exceeds 1 MiB")
    value = read_json_strict(manifest_path, "admission set")
    if (
        set(value) != {"schemaVersion", "shared", "records", "inventory"}
        or value["schemaVersion"] != 2
    ):
        raise ValueError("invalid admission set fields or schema")
    shared = value["shared"]
    records = value["records"]
    inventory = value["inventory"]
    if not isinstance(shared, dict) or set(shared) != {
        "runtimeRelativePath",
        "runtimeLibraryBindings",
        "probeRelativePath",
        "probeSha256",
    }:
        raise ValueError("invalid shared runtime fields")
    bindings = shared["runtimeLibraryBindings"]
    if (
        not safe_relative(shared["runtimeRelativePath"])
        or not safe_relative(shared["probeRelativePath"])
        or not sha256_string(shared["probeSha256"])
        or not isinstance(bindings, dict)
        or not bindings
        or any(
            not isinstance(name, str)
            or ".so" not in name
            or "/" in name
            or not sha256_string(digest)
            for name, digest in bindings.items()
        )
    ):
        raise ValueError("invalid shared runtime values")
    if not isinstance(records, list) or not 1 <= len(records) <= MAX_ADMISSION_RECORDS:
        raise ValueError("invalid admission record count")
    first_record = records[0]
    if not isinstance(first_record, dict):
        raise ValueError("invalid admission record")
    first_identity = first_record.get("identity")
    if not isinstance(first_identity, dict) or set(first_identity) != IDENTITY_FIELDS:
        raise ValueError("invalid admission identity fields")
    if inventory != package_inventory(root):
        raise ValueError("package inventory differs from filesystem")
    runtime = root / str(shared["runtimeRelativePath"])
    probe = root / str(shared["probeRelativePath"])
    if runtime_identity(runtime) != records[0]["identity"]["runtimeIdentitySha256"]:
        raise ValueError("runtime identity changed")
    if runtime_library_bindings(runtime) != shared["runtimeLibraryBindings"]:
        raise ValueError("runtime library bindings changed")
    if sha256_file(probe) != shared["probeSha256"]:
        raise ValueError("runtime probe changed")
    keys: set[str] = set()
    identities: set[bytes] = set()
    cache_paths: set[str] = set()
    contract_fields = (
        "echoCommit",
        "echoBinarySha256",
        "runtimeIdentitySha256",
        "vadSha256",
        "protocol",
        "languagePolicy",
        "promptPolicy",
        "launchContractSchema",
    )
    shared_contract = tuple(first_identity[field] for field in contract_fields)
    current_time = int(time.time())
    for record in records:
        if not isinstance(record, dict) or set(record) != {
            "identity",
            "identityKey",
            "evidenceSha256",
            "icdManifestPath",
            "icdLibraryPath",
            "cacheSeed",
            "gates",
            "verdict",
            "acceptedAt",
            "expiresAt",
        }:
            raise ValueError("invalid admission record fields")
        identity = record["identity"]
        key = record["identityKey"]
        expected_key = hashlib.sha256(
            b"echo-whisper-admission-identity-v1\0"
            + json.dumps(identity, ensure_ascii=False, separators=(",", ":")).encode()
        ).hexdigest()
        cache = record["cacheSeed"]
        encoded_identity = json.dumps(
            identity, sort_keys=True, separators=(",", ":")
        ).encode()
        if (
            not isinstance(identity, dict)
            or set(identity) != IDENTITY_FIELDS
            or not isinstance(identity.get("tuning"), dict)
            or set(identity["tuning"]) != TUNING_FIELDS
            or not isinstance(identity.get("device"), dict)
            or set(identity["device"]) != DEVICE_FIELDS
        ):
            raise ValueError("invalid admission identity fields")
        tuning = identity["tuning"]
        device = identity["device"]
        if (
            not bounded_integer(identity["schemaVersion"], 1, 1)
            or not lower_hex(identity["echoCommit"], 40)
            or identity["protocol"] != "oneShotCli"
            or identity["languagePolicy"] != "pinned"
            or identity["promptPolicy"] != "empty"
            or not bounded_integer(identity["launchContractSchema"], 1, 2**32 - 1)
            or not isinstance(identity["drmDriver"], str)
            or not identity["drmDriver"]
        ):
            raise ValueError("invalid admission identity contract")
        if (
            not bounded_integer(tuning["threads"], 1, 65535)
            or not bounded_integer(tuning["beamSize"], 1, 255)
            or not bounded_integer(tuning["bestOf"], 1, 255)
            or not isinstance(tuning["noFallback"], bool)
        ):
            raise ValueError("invalid admission tuning")
        if (
            device["backend"] != "vulkan"
            or not bounded_integer(device["selectedIndex"], 0, 2**32 - 1)
            or not bounded_integer(device["vendorId"], 1, 2**32 - 1)
            or not bounded_integer(device["deviceId"], 1, 2**32 - 1)
            or not bounded_integer(device["apiVersion"], 0, 2**32 - 1)
            or not bounded_integer(device["driverVersion"], 0, 2**32 - 1)
        ):
            raise ValueError("invalid admission device")
        if any(
            not lower_hex(device[name], 32) or device[name] == "0" * 32
            for name in ("deviceUUID", "driverUUID", "pipelineCacheUUID")
        ):
            raise ValueError("invalid admission device UUID")
        digests = [
            identity["echoBinarySha256"],
            identity["runtimeIdentitySha256"],
            identity["modelSha256"],
            identity["icdManifestSha256"],
            identity["icdLibrarySha256"],
        ]
        if identity["vadSha256"] is not None:
            digests.append(identity["vadSha256"])
        gates = record["gates"]
        contract = tuple(identity[field] for field in contract_fields)
        if (
            any(not sha256_string(digest) for digest in digests)
            or contract != shared_contract
            or not isinstance(gates, dict)
            or set(gates) != GATE_FIELDS
            or any(value is not True for value in gates.values())
            or record["verdict"] != "PASSED"
        ):
            raise ValueError("invalid admission identity or gates")
        accepted_at = record["acceptedAt"]
        expires_at = record["expiresAt"]
        if (
            not bounded_integer(accepted_at, 0, 2**64 - 1)
            or not bounded_integer(expires_at, 0, 2**64 - 1)
            or not accepted_at <= current_time < expires_at
            or expires_at - accepted_at > 30 * 24 * 60 * 60
        ):
            raise ValueError("admission interval is not current or bounded")
        if (
            not isinstance(record["icdManifestPath"], str)
            or not record["icdManifestPath"].startswith("/")
            or not isinstance(record["icdLibraryPath"], str)
            or not record["icdLibraryPath"].startswith("/")
            or not sha256_string(record["evidenceSha256"])
        ):
            raise ValueError("invalid admission evidence or ICD paths")
        if (
            key != expected_key
            or key in keys
            or encoded_identity in identities
            or not isinstance(cache, dict)
            or set(cache) != {"relativePath", "sha256"}
            or cache.get("relativePath") != f"cache-seeds/{key}"
            or cache.get("relativePath") in cache_paths
            or not sha256_string(cache.get("sha256"))
        ):
            raise ValueError("invalid or duplicate admission identity")
        if tree_sha256(root / cache["relativePath"]) != cache.get("sha256"):
            raise ValueError("cache seed changed")
        keys.add(key)
        identities.add(encoded_identity)
        cache_paths.add(cache["relativePath"])
    return value


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "runtime").mkdir()
        (root / "runtime/file").write_bytes(b"file")
        inventory = package_inventory(root)
        assert inventory == [
            {
                "path": "runtime/file",
                "kind": "file",
                "bytes": 4,
                "sha256": sha256_file(root / "runtime/file"),
                "linkTarget": None,
            }
        ]
        (root / "extra").write_bytes(b"extra")
        assert package_inventory(root) != inventory
    print("whisper_release_common: self-test passed")


if __name__ == "__main__":
    self_test()
