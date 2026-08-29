#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import json
import tempfile
from pathlib import Path

from whisper_identity_v3 import (
    ADMISSION_GATE_FIELDS,
    canonical_json_bytes,
    execution_artifact_id,
    inference_contract_id,
    local_environment_key,
    verify_acceleration_set,
)
from whisper_release_common import (
    package_inventory,
    prefixed_package_inventory,
    read_json_strict,
    sha256_bytes,
    verify_contained_symlinks,
    verify_v3_execution_files,
)

SCHEMA_VERSION = 1
READINESS = "local-selection-proof-only-until-pr16.4"
MAX_EVIDENCE_LIFETIME_SECS = 30 * 24 * 60 * 60
MANIFESTS = {
    "portable-selection.v1.json",
    "legacy-exact-index.v1.json",
    "portable-selection-binding.v1.json",
}


def fail(message: str) -> None:
    raise ValueError(message)


def canonical_digest(value: object) -> str:
    return sha256_bytes(canonical_json_bytes(value))


def project_acceleration_set(
    acceleration_set: dict[str, object],
) -> tuple[dict[str, object], dict[str, object]]:
    identities = verify_acceleration_set(acceleration_set)
    portable = {
        "schemaVersion": SCHEMA_VERSION,
        "executionArtifact": copy.deepcopy(acceleration_set["executionArtifact"]),
        "inferenceContracts": copy.deepcopy(acceleration_set["inferenceContracts"]),
    }
    environments = {
        record["key"]: record["value"]
        for record in acceleration_set["localEnvironments"]
    }
    legacy = {
        "schemaVersion": SCHEMA_VERSION,
        "executionArtifactId": identities["executionArtifactId"],
        "records": [
            {
                "performanceEvidenceId": record["id"],
                "inferenceContractId": record["value"]["inferenceContractId"],
                "localEnvironmentKey": record["value"]["localEnvironmentKey"],
                "localEnvironment": copy.deepcopy(
                    environments[record["value"]["localEnvironmentKey"]]
                ),
                "acceptedAt": record["value"]["acceptedAt"],
                "expiresAt": record["value"]["expiresAt"],
            }
            for record in acceleration_set["performanceEvidence"]
        ],
    }
    verify_portable_selection(portable)
    verify_legacy_exact_index(legacy, portable)
    return portable, legacy


def verify_portable_selection(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != {
        "schemaVersion",
        "executionArtifact",
        "inferenceContracts",
    }:
        fail("portable selection fields differ")
    if value["schemaVersion"] != SCHEMA_VERSION:
        fail("portable selection schema differs")
    execution = value["executionArtifact"]
    if not isinstance(execution, dict) or set(execution) != {"id", "value"}:
        fail("portable execution artifact record differs")
    if execution["id"] != execution_artifact_id(execution["value"]):
        fail("portable execution artifact ID differs")
    runtime = execution["value"]
    if not runtime["runtimeRelativePath"].startswith("runtime/") or not runtime[
        "probeRelativePath"
    ].startswith("runtime/"):
        fail("portable runtime paths are outside runtime/")
    contracts = value["inferenceContracts"]
    if not isinstance(contracts, list) or not contracts:
        fail("portable inference contracts are empty")
    contract_ids = []
    for record in contracts:
        if not isinstance(record, dict) or set(record) != {"id", "value"}:
            fail("portable inference contract record differs")
        if record["id"] != inference_contract_id(record["value"]):
            fail("portable inference contract ID differs")
        contract_ids.append(record["id"])
    if contract_ids != sorted(set(contract_ids)):
        fail("portable inference contracts are not sorted and unique")
    return {
        "executionArtifactId": execution["id"],
        "inferenceContractIds": contract_ids,
    }


def verify_legacy_exact_index(value: object, portable: dict[str, object]) -> list[str]:
    portable_ids = verify_portable_selection(portable)
    if not isinstance(value, dict) or set(value) != {
        "schemaVersion",
        "executionArtifactId",
        "records",
    }:
        fail("legacy exact index fields differ")
    if (
        value["schemaVersion"] != SCHEMA_VERSION
        or value["executionArtifactId"] != portable_ids["executionArtifactId"]
    ):
        fail("legacy exact index execution artifact differs")
    records = value["records"]
    if not isinstance(records, list) or not records:
        fail("legacy exact index records are empty")
    evidence_ids = []
    for record in records:
        if not isinstance(record, dict) or set(record) != {
            "acceptedAt",
            "expiresAt",
            "inferenceContractId",
            "localEnvironment",
            "localEnvironmentKey",
            "performanceEvidenceId",
        }:
            fail("legacy exact record fields differ")
        if record["inferenceContractId"] not in portable_ids["inferenceContractIds"]:
            fail("legacy exact record references an unknown inference contract")
        if record["localEnvironmentKey"] != local_environment_key(
            record["localEnvironment"]
        ):
            fail("legacy exact local environment key differs")
        if (
            type(record["acceptedAt"]) is not int
            or type(record["expiresAt"]) is not int
        ):
            fail("legacy exact evidence lifetime differs")
        lifetime = record["expiresAt"] - record["acceptedAt"]
        if (
            record["acceptedAt"] <= 0
            or lifetime <= 0
            or lifetime > MAX_EVIDENCE_LIFETIME_SECS
        ):
            fail("legacy exact evidence lifetime differs")
        evidence_ids.append(record["performanceEvidenceId"])
    if evidence_ids != sorted(set(evidence_ids)):
        fail("legacy exact records are not sorted and unique")
    return evidence_ids


def build_binding(
    *,
    portable: dict[str, object],
    legacy: dict[str, object],
    source_release_binding_id: str,
    package_type: str,
    version: str,
    echo_commit: str,
    echo_binary_sha256: str,
) -> dict[str, object]:
    identities = verify_portable_selection(portable)
    verify_legacy_exact_index(legacy, portable)
    binding = {
        "schemaVersion": SCHEMA_VERSION,
        "packageType": package_type,
        "version": version,
        "echoCommit": echo_commit,
        "echoBinarySha256": echo_binary_sha256,
        "portableSelectionSha256": canonical_digest(portable),
        "legacyExactIndexSha256": canonical_digest(legacy),
        "executionArtifactId": identities["executionArtifactId"],
        "allowedInferenceContractIds": identities["inferenceContractIds"],
        "sourceReleaseBindingId": source_release_binding_id,
        "productionReadiness": READINESS,
    }
    verify_binding(binding, portable, legacy)
    return binding


def verify_binding(
    value: object,
    portable: dict[str, object],
    legacy: dict[str, object],
) -> None:
    identities = verify_portable_selection(portable)
    verify_legacy_exact_index(legacy, portable)
    if not isinstance(value, dict) or set(value) != {
        "allowedInferenceContractIds",
        "echoBinarySha256",
        "echoCommit",
        "executionArtifactId",
        "legacyExactIndexSha256",
        "packageType",
        "portableSelectionSha256",
        "productionReadiness",
        "schemaVersion",
        "sourceReleaseBindingId",
        "version",
    }:
        fail("portable selection binding fields differ")
    if (
        value["schemaVersion"] != SCHEMA_VERSION
        or value["packageType"] not in {"deb", "rpm"}
        or not isinstance(value["version"], str)
        or not value["version"]
        or value["productionReadiness"] != READINESS
        or value["portableSelectionSha256"] != canonical_digest(portable)
        or value["legacyExactIndexSha256"] != canonical_digest(legacy)
        or value["executionArtifactId"] != identities["executionArtifactId"]
        or value["allowedInferenceContractIds"] != identities["inferenceContractIds"]
    ):
        fail("portable selection binding differs")
    for name in (
        "echoBinarySha256",
        "portableSelectionSha256",
        "legacyExactIndexSha256",
        "sourceReleaseBindingId",
    ):
        if (
            not isinstance(value[name], str)
            or len(value[name]) != 64
            or any(character not in "0123456789abcdef" for character in value[name])
        ):
            fail(f"portable selection binding {name} is not a digest")
    if (
        not isinstance(value["echoCommit"], str)
        or len(value["echoCommit"]) != 40
        or any(character not in "0123456789abcdef" for character in value["echoCommit"])
    ):
        fail("portable selection binding Echo commit is not a commit")


def write_projection(
    root: Path, acceleration_set: dict[str, object]
) -> tuple[dict, dict]:
    portable, legacy = project_acceleration_set(acceleration_set)
    for name, value in (
        ("portable-selection.v1.json", portable),
        ("legacy-exact-index.v1.json", legacy),
    ):
        (root / name).write_text(
            json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    return portable, legacy


def verify_portable_filesystem(root: Path) -> tuple[dict, dict, dict]:
    for name in MANIFESTS:
        path = root / name
        if not path.is_file() or path.is_symlink():
            fail(f"portable selection manifest is not a regular file: {name}")
    portable = read_json_strict(
        root / "portable-selection.v1.json", "portable selection"
    )
    legacy = read_json_strict(root / "legacy-exact-index.v1.json", "legacy exact index")
    binding = read_json_strict(
        root / "portable-selection-binding.v1.json", "portable selection binding"
    )
    verify_binding(binding, portable, legacy)
    verify_v3_execution_files(
        root, {"executionArtifact": portable["executionArtifact"]}
    )
    runtime_root = Path(
        portable["executionArtifact"]["value"]["runtimeRelativePath"]
    ).parent
    expected = prefixed_package_inventory(root / runtime_root, str(runtime_root))
    actual = [
        entry for entry in package_inventory(root) if entry["path"] not in MANIFESTS
    ]
    if sorted(actual, key=lambda entry: entry["path"]) != sorted(
        expected, key=lambda entry: entry["path"]
    ):
        fail("portable selection package contains non-runtime files")
    if any(
        entry["path"].startswith("cache-seeds/") or "shader" in entry["path"].lower()
        for entry in package_inventory(root)
    ):
        fail("portable selection package contains shader cache material")
    verify_contained_symlinks(root)
    return portable, legacy, binding


def self_test() -> None:
    fixture = read_json_strict(
        Path(__file__).resolve().parent.parent
        / "crates/echo/tests/fixtures/whisper-v3-identities.json",
        "Whisper v3 identity fixture",
    )["cases"]
    acceleration_set = {
        "schemaVersion": 3,
        "executionArtifact": {
            "id": fixture["executionArtifact"]["id"],
            "value": fixture["executionArtifact"]["input"],
        },
        "inferenceContracts": [
            {
                "id": fixture["inferenceContract"]["id"],
                "value": fixture["inferenceContract"]["input"],
            }
        ],
        "localEnvironments": [
            {
                "key": fixture["localEnvironment"]["id"],
                "launch": {
                    "icdManifestPath": "/host/intel_icd.json",
                    "icdLibraryPath": "/host/libvulkan_intel.so",
                },
                "value": fixture["localEnvironment"]["input"],
            }
        ],
        "performanceEvidence": [
            {
                "cacheSeed": {
                    "relativePath": f"cache-seeds/{fixture['performanceEvidence']['id']}",
                    "sha256": "9" * 64,
                },
                "gates": {name: True for name in ADMISSION_GATE_FIELDS},
                "id": fixture["performanceEvidence"]["id"],
                "value": fixture["performanceEvidence"]["input"],
                "verdict": "PASSED",
            }
        ],
        "reusableInventorySha256": "3" * 64,
    }
    portable, legacy = project_acceleration_set(acceleration_set)
    encoded = canonical_json_bytes([portable, legacy]).decode()
    assert "/host/" not in encoded
    assert "cache-seeds" not in encoded
    binding = build_binding(
        portable=portable,
        legacy=legacy,
        source_release_binding_id=fixture["releaseBinding"]["id"],
        package_type="deb",
        version="0.12.5",
        echo_commit="a" * 40,
        echo_binary_sha256="b" * 64,
    )
    verify_binding(binding, portable, legacy)
    changed = copy.deepcopy(portable)
    changed["inferenceContracts"][0]["value"]["tuning"]["threads"] += 1
    try:
        verify_binding(binding, changed, legacy)
    except ValueError:
        pass
    else:
        raise AssertionError("binding accepted a changed portable selection")
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "portable-selection.v1.json").write_text("{}", encoding="utf-8")
        try:
            read_json_strict(root / "portable-selection.v1.json", "portable selection")
        except ValueError as error:
            raise AssertionError("strict JSON rejected an ordinary object") from error


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if not args.self_test:
        parser.error("--self-test is required")
    self_test()


if __name__ == "__main__":
    main()
