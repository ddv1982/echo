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
    build_record,
    verify_acceleration_set,
)
from whisper_release_common import (
    package_inventory,
    prefixed_package_inventory,
    read_json_strict,
    runtime_identity,
    runtime_library_bindings,
    sha256_bytes,
    sha256_file,
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
REPO_ROOT = Path(__file__).resolve().parent.parent
CALIBRATION_FIXTURE_SOURCE = REPO_ROOT / "crates/echo/tests/fixtures/claude_code.wav"
CALIBRATION_FIXTURE_RELATIVE = "calibration/english-canary.wav"


def fail(message: str) -> None:
    raise ValueError(message)


def canonical_digest(value: object) -> str:
    return sha256_bytes(canonical_json_bytes(value))


def portable_runtime_inventory(root: Path) -> list[dict[str, object]]:
    inventory = []
    for entry in package_inventory(root):
        copied = dict(entry)
        copied["path"] = f"runtime/{entry['path']}"
        inventory.append(copied)
    return inventory


def portable_execution_record(runtime: Path) -> dict[str, object]:
    cli = runtime / "whisper-cli"
    probe = runtime / "echo-whisper-runtime-probe"
    receipt_path = runtime / "build-receipt.json"
    receipt = read_json_strict(receipt_path, "runtime build receipt")
    return build_record(
        "executionArtifact",
        {
            "schemaVersion": 3,
            "runtimeArtifactId": receipt["artifactId"],
            "runtimeIdentitySha256": runtime_identity(cli),
            "runtimeRelativePath": "runtime/whisper-cli",
            "runtimeSha256": sha256_file(cli),
            "runtimeLibraryBindings": runtime_library_bindings(cli),
            "probeRelativePath": "runtime/echo-whisper-runtime-probe",
            "probeSha256": sha256_file(probe),
            "buildReceiptSha256": sha256_file(receipt_path),
            "reusableInventorySha256": sha256_bytes(
                canonical_json_bytes(portable_runtime_inventory(runtime))
            ),
        },
    )


def project_acceleration_set(
    acceleration_set: dict[str, object],
    calibration_fixture_sha256: str,
) -> tuple[dict[str, object], dict[str, object]]:
    identities = verify_acceleration_set(acceleration_set)
    portable = {
        "schemaVersion": SCHEMA_VERSION,
        "executionArtifact": copy.deepcopy(acceleration_set["executionArtifact"]),
        "inferenceContracts": copy.deepcopy(acceleration_set["inferenceContracts"]),
        "calibrationFixture": {
            "relativePath": CALIBRATION_FIXTURE_RELATIVE,
            "sha256": calibration_fixture_sha256,
        },
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
        "calibrationFixture",
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
    fixture = value["calibrationFixture"]
    if (
        not isinstance(fixture, dict)
        or set(fixture) != {"relativePath", "sha256"}
        or fixture["relativePath"] != CALIBRATION_FIXTURE_RELATIVE
        or not isinstance(fixture["sha256"], str)
        or len(fixture["sha256"]) != 64
        or any(character not in "0123456789abcdef" for character in fixture["sha256"])
    ):
        fail("portable calibration fixture differs")
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
    if not isinstance(records, list):
        fail("legacy exact index records are not an array")
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
    source_acceleration_set_sha256: str,
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
        "sourceAccelerationSetSha256": source_acceleration_set_sha256,
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
        "sourceAccelerationSetSha256",
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
        "sourceAccelerationSetSha256",
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
    root: Path,
    acceleration_set: dict[str, object],
    *,
    rebind_runtime: bool = False,
) -> tuple[dict, dict]:
    fixture = root / CALIBRATION_FIXTURE_RELATIVE
    fixture.parent.mkdir(parents=True)
    fixture.write_bytes(CALIBRATION_FIXTURE_SOURCE.read_bytes())
    if rebind_runtime:
        portable = {
            "schemaVersion": SCHEMA_VERSION,
            "executionArtifact": portable_execution_record(root / "runtime"),
            "inferenceContracts": copy.deepcopy(acceleration_set["inferenceContracts"]),
            "calibrationFixture": {
                "relativePath": CALIBRATION_FIXTURE_RELATIVE,
                "sha256": sha256_bytes(fixture.read_bytes()),
            },
        }
        legacy = {
            "schemaVersion": SCHEMA_VERSION,
            "executionArtifactId": portable["executionArtifact"]["id"],
            "records": [],
        }
        verify_portable_selection(portable)
        verify_legacy_exact_index(legacy, portable)
    else:
        portable, legacy = project_acceleration_set(
            acceleration_set, sha256_bytes(fixture.read_bytes())
        )
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
    expected.extend(prefixed_package_inventory(root / "calibration", "calibration"))
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
    fixture = root / portable["calibrationFixture"]["relativePath"]
    if sha256_bytes(fixture.read_bytes()) != portable["calibrationFixture"]["sha256"]:
        fail("portable calibration fixture digest differs")
    verify_contained_symlinks(root)
    return portable, legacy, binding


def self_test() -> None:
    fixture = read_json_strict(
        Path(__file__).resolve().parent.parent
        / "scripts/fixtures/whisper-v3-identities.json",
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
    portable, legacy = project_acceleration_set(acceleration_set, "8" * 64)
    encoded = canonical_json_bytes([portable, legacy]).decode()
    assert "/host/" not in encoded
    assert "cache-seeds" not in encoded
    binding = build_binding(
        portable=portable,
        legacy=legacy,
        source_acceleration_set_sha256="9" * 64,
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
