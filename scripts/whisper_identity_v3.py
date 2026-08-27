#!/usr/bin/env python3
import argparse
import copy
import hashlib
import json
import re
from pathlib import Path


DOMAIN_PREFIXES = {
    "executionArtifact": b"echo-whisper-execution-artifact-v3\0",
    "inferenceContract": b"echo-whisper-inference-contract-v3\0",
    "localEnvironment": b"echo-whisper-local-environment-v3\0",
    "performanceEvidence": b"echo-whisper-performance-evidence-v3\0",
    "releaseBinding": b"echo-whisper-release-binding-v3\0",
}
SHA256 = re.compile(r"[0-9a-f]{64}")
COMMIT = re.compile(r"[0-9a-f]{40}")
SAFE_PATH = re.compile(r"[A-Za-z0-9._+/-]+")
LOWER_HEX_32 = re.compile(r"[0-9a-f]{32}")
ADMISSION_GATE_FIELDS = frozenset(
    {
        "backendTruth",
        "cacheEvidence",
        "cleanChildEnvironment",
        "completePairs",
        "coverageComplete",
        "driverIcdIdentity",
        "exactRuntime",
        "hardwareDevice",
        "identityMatch",
        "memoryEvidence",
        "memoryFloor",
        "medianReduction",
        "medianSpeedup",
        "noNewHallucinations",
        "p95Improved",
        "pairIntegrity",
        "perLanguageQuality",
        "receiptConsistency",
        "resetEvidence",
        "sampleSize",
        "stabilitySuccess",
        "swapStable",
    }
)


class IdentityError(ValueError):
    pass


def fail(message):
    raise IdentityError(message)


def strict_json_loads(raw):
    def unique(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                fail(f"duplicate JSON key: {key}")
            value[key] = item
        return value

    return json.loads(raw, object_pairs_hook=unique)


def strict_json_file(path):
    try:
        return strict_json_loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"could not read {path}: {error}")


def validate_canonical_value(value, context="value"):
    if value is None or isinstance(value, (bool, str)):
        return
    if type(value) is int:
        return
    if isinstance(value, list):
        for index, item in enumerate(value):
            validate_canonical_value(item, f"{context}[{index}]")
        return
    if isinstance(value, dict):
        for key, item in value.items():
            if not isinstance(key, str):
                fail(f"{context} has a non-string key")
            validate_canonical_value(item, f"{context}.{key}")
        return
    fail(f"{context} contains an unsupported JSON value")


def canonical_json_bytes(value):
    validate_canonical_value(value)
    return json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def require_object(value, keys, context):
    if not isinstance(value, dict) or set(value) != set(keys):
        fail(f"{context} keys differ")


def require_schema(value, context):
    if type(value) is not int or value != 3:
        fail(f"{context} schemaVersion is not 3")


def require_digest(value, context):
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        fail(f"{context} is not a lowercase SHA-256 digest")


def require_string(value, context, allowed=None):
    if not isinstance(value, str) or not value:
        fail(f"{context} is not a non-empty string")
    if allowed is not None and value not in allowed:
        fail(f"{context} is unsupported")


def require_uint(value, maximum, context, positive=False):
    if type(value) is not int or not 0 <= value <= maximum or (positive and value == 0):
        fail(f"{context} is out of range")


def require_relative_path(value, context):
    require_string(value, context)
    path = Path(value)
    if (
        SAFE_PATH.fullmatch(value) is None
        or path.is_absolute()
        or value != path.as_posix()
        or any(part in {"", ".", ".."} for part in path.parts)
    ):
        fail(f"{context} is not a safe relative path")


def require_sorted_ids(value, context):
    if not isinstance(value, list) or not value:
        fail(f"{context} is not a non-empty array")
    for item in value:
        require_digest(item, context)
    if value != sorted(set(value)):
        fail(f"{context} is not sorted and unique")


def validate_execution_artifact(value):
    require_object(
        value,
        {
            "buildReceiptSha256",
            "probeRelativePath",
            "probeSha256",
            "reusableInventorySha256",
            "runtimeArtifactId",
            "runtimeIdentitySha256",
            "runtimeLibraryBindings",
            "runtimeRelativePath",
            "runtimeSha256",
            "schemaVersion",
        },
        "execution artifact",
    )
    require_schema(value["schemaVersion"], "execution artifact")
    for key in [
        "buildReceiptSha256",
        "probeSha256",
        "reusableInventorySha256",
        "runtimeArtifactId",
        "runtimeIdentitySha256",
        "runtimeSha256",
    ]:
        require_digest(value[key], f"execution artifact {key}")
    require_relative_path(value["runtimeRelativePath"], "runtime path")
    require_relative_path(value["probeRelativePath"], "probe path")
    bindings = value["runtimeLibraryBindings"]
    if not isinstance(bindings, dict) or not bindings:
        fail("runtimeLibraryBindings is not a non-empty object")
    for name, digest in bindings.items():
        if "/" in name:
            fail("runtime library alias contains a path separator")
        require_string(name, "runtime library alias")
        require_digest(digest, f"runtime library {name}")


def validate_inference_contract(value):
    require_object(
        value,
        {
            "behavior",
            "claimScope",
            "modelSha256",
            "protocol",
            "requestPolicy",
            "schemaVersion",
            "tuning",
            "vadSha256",
        },
        "inference contract",
    )
    require_schema(value["schemaVersion"], "inference contract")
    require_digest(value["modelSha256"], "modelSha256")
    if value["vadSha256"] is not None:
        require_digest(value["vadSha256"], "vadSha256")
    require_string(value["protocol"], "protocol", {"oneShotCli"})
    require_string(value["claimScope"], "claimScope")
    require_object(
        value["tuning"], {"beamSize", "bestOf", "noFallback", "threads"}, "tuning"
    )
    for key in ["beamSize", "bestOf", "threads"]:
        require_uint(value["tuning"][key], 65535, f"tuning {key}", positive=True)
    if type(value["tuning"]["noFallback"]) is not bool:
        fail("tuning noFallback is not a boolean")
    require_object(
        value["requestPolicy"], {"hints", "language", "prompt"}, "requestPolicy"
    )
    require_string(value["requestPolicy"]["language"], "language policy", {"pinned"})
    require_string(value["requestPolicy"]["prompt"], "prompt policy", {"empty"})
    require_string(value["requestPolicy"]["hints"], "hints policy", {"qualifiedOnly"})
    require_object(
        value["behavior"],
        {
            "launchSchema",
            "projectionSha256",
            "receiptSchema",
            "recoverySchema",
            "telemetrySchema",
        },
        "behavior",
    )
    for key in ["launchSchema", "receiptSchema", "recoverySchema", "telemetrySchema"]:
        require_uint(
            value["behavior"][key], 2**32 - 1, f"behavior {key}", positive=True
        )
    require_digest(value["behavior"]["projectionSha256"], "behavior projection")


def validate_local_environment(value):
    require_object(
        value,
        {
            "apiVersion",
            "architecture",
            "backend",
            "deviceId",
            "deviceUUID",
            "drmDriver",
            "driverUUID",
            "driverVersion",
            "icdLibrarySha256",
            "icdManifestSha256",
            "pipelineCacheUUID",
            "schemaVersion",
            "vendorId",
        },
        "local environment",
    )
    require_schema(value["schemaVersion"], "local environment")
    require_string(value["architecture"], "architecture", {"x86_64"})
    require_string(value["backend"], "backend", {"vulkan"})
    require_string(value["drmDriver"], "DRM driver")
    for key in ["apiVersion", "deviceId", "driverVersion", "vendorId"]:
        require_uint(value[key], 2**32 - 1, key, positive=key != "driverVersion")
    for key in ["deviceUUID", "driverUUID", "pipelineCacheUUID"]:
        if (
            not isinstance(value[key], str)
            or LOWER_HEX_32.fullmatch(value[key]) is None
        ):
            fail(f"{key} is not 32 lowercase hexadecimal characters")
    require_digest(value["icdManifestSha256"], "ICD manifest digest")
    require_digest(value["icdLibrarySha256"], "ICD library digest")


def validate_performance_evidence(value):
    require_object(
        value,
        {
            "acceptedAt",
            "cacheCycleSha256",
            "corpusManifestSha256",
            "coverageManifestSha256",
            "executionArtifactId",
            "expiresAt",
            "gatePolicySha256",
            "inferenceContractId",
            "localEnvironmentKey",
            "measurementProtocol",
            "observationBundleSha256",
            "schemaVersion",
        },
        "performance evidence",
    )
    require_schema(value["schemaVersion"], "performance evidence")
    for key in [
        "cacheCycleSha256",
        "corpusManifestSha256",
        "coverageManifestSha256",
        "executionArtifactId",
        "gatePolicySha256",
        "inferenceContractId",
        "localEnvironmentKey",
        "observationBundleSha256",
    ]:
        require_digest(value[key], f"performance evidence {key}")
    require_string(
        value["measurementProtocol"],
        "measurement protocol",
        {"paired-product-sweep-v2"},
    )
    require_uint(value["acceptedAt"], 2**64 - 1, "acceptedAt", positive=True)
    require_uint(value["expiresAt"], 2**64 - 1, "expiresAt", positive=True)
    if value["expiresAt"] <= value["acceptedAt"]:
        fail("performance evidence expiry does not follow acceptance")


def validate_release_binding(value):
    require_object(
        value,
        {
            "accelerationSetSha256",
            "allowedInferenceContractIds",
            "allowedPerformanceEvidenceIds",
            "bundleMarker",
            "echoBinarySha256",
            "echoCommit",
            "executionArtifactId",
            "packageType",
            "reusableInventorySha256",
            "schemaVersion",
            "version",
        },
        "release binding",
    )
    require_schema(value["schemaVersion"], "release binding")
    require_string(value["packageType"], "package type", {"deb", "rpm"})
    require_string(value["bundleMarker"], "bundle marker", {"deb", "rpm"})
    if value["bundleMarker"] != value["packageType"]:
        fail("bundle marker differs from package type")
    require_string(value["version"], "version")
    if (
        not isinstance(value["echoCommit"], str)
        or COMMIT.fullmatch(value["echoCommit"]) is None
    ):
        fail("Echo commit is not 40 lowercase hexadecimal characters")
    for key in [
        "accelerationSetSha256",
        "echoBinarySha256",
        "executionArtifactId",
        "reusableInventorySha256",
    ]:
        require_digest(value[key], f"release binding {key}")
    require_sorted_ids(
        value["allowedInferenceContractIds"], "allowed inference contract IDs"
    )
    require_sorted_ids(
        value["allowedPerformanceEvidenceIds"], "allowed performance evidence IDs"
    )


def content_id(prefix, value):
    return hashlib.sha256(prefix + canonical_json_bytes(value)).hexdigest()


def execution_artifact_id(value):
    validate_execution_artifact(value)
    return content_id(DOMAIN_PREFIXES["executionArtifact"], value)


def inference_contract_id(value):
    validate_inference_contract(value)
    return content_id(DOMAIN_PREFIXES["inferenceContract"], value)


def local_environment_key(value):
    validate_local_environment(value)
    return content_id(DOMAIN_PREFIXES["localEnvironment"], value)


def performance_evidence_id(value):
    validate_performance_evidence(value)
    return content_id(DOMAIN_PREFIXES["performanceEvidence"], value)


def release_binding_id(value):
    validate_release_binding(value)
    return content_id(DOMAIN_PREFIXES["releaseBinding"], value)


ID_FUNCTIONS = {
    "executionArtifact": execution_artifact_id,
    "inferenceContract": inference_contract_id,
    "localEnvironment": local_environment_key,
    "performanceEvidence": performance_evidence_id,
    "releaseBinding": release_binding_id,
}


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def build_record(name, value):
    if name not in ID_FUNCTIONS:
        fail(f"unsupported content record: {name}")
    return {"id": ID_FUNCTIONS[name](value), "value": value}


def verify_record(record, name):
    if not isinstance(record, dict) or set(record) != {"id", "value"}:
        fail(f"{name} record keys differ")
    expected = ID_FUNCTIONS[name](record["value"])
    if record["id"] != expected:
        fail(f"{name} record ID differs")
    return expected


def require_absolute_path(value, context):
    require_string(value, context)
    if not Path(value).is_absolute() or "\x00" in value:
        fail(f"{context} is not an absolute path")


def acceleration_set_sha256(value):
    return sha256_bytes(canonical_json_bytes(value))


def verify_acceleration_set(value):
    require_object(
        value,
        {
            "executionArtifact",
            "inferenceContracts",
            "localEnvironments",
            "performanceEvidence",
            "reusableInventorySha256",
            "schemaVersion",
        },
        "acceleration set",
    )
    require_schema(value["schemaVersion"], "acceleration set")
    require_digest(value["reusableInventorySha256"], "reusable inventory")
    execution_id = verify_record(value["executionArtifact"], "executionArtifact")

    contracts = value["inferenceContracts"]
    if not isinstance(contracts, list) or not contracts:
        fail("inferenceContracts is not a non-empty array")
    contract_ids = [verify_record(record, "inferenceContract") for record in contracts]
    if contract_ids != sorted(set(contract_ids)):
        fail("inferenceContracts is not sorted and unique")

    environments = value["localEnvironments"]
    if not isinstance(environments, list) or not environments:
        fail("localEnvironments is not a non-empty array")
    environment_keys = []
    for record in environments:
        if not isinstance(record, dict) or set(record) != {"key", "launch", "value"}:
            fail("local environment record keys differ")
        key = local_environment_key(record["value"])
        if record["key"] != key:
            fail("local environment record key differs")
        require_object(
            record["launch"],
            {"icdLibraryPath", "icdManifestPath"},
            "environment launch",
        )
        require_absolute_path(record["launch"]["icdManifestPath"], "ICD manifest path")
        require_absolute_path(record["launch"]["icdLibraryPath"], "ICD library path")
        environment_keys.append(key)
    if environment_keys != sorted(set(environment_keys)):
        fail("localEnvironments is not sorted and unique")

    evidence = value["performanceEvidence"]
    if not isinstance(evidence, list) or not evidence:
        fail("performanceEvidence is not a non-empty array")
    evidence_ids = []
    for record in evidence:
        require_object(
            record,
            {"cacheSeed", "gates", "id", "value", "verdict"},
            "performance evidence record",
        )
        evidence_id = performance_evidence_id(record["value"])
        if record["id"] != evidence_id:
            fail("performance evidence record ID differs")
        if record["value"]["executionArtifactId"] != execution_id:
            fail("performance evidence execution artifact differs")
        if record["value"]["inferenceContractId"] not in contract_ids:
            fail("performance evidence inference contract is missing")
        if record["value"]["localEnvironmentKey"] not in environment_keys:
            fail("performance evidence local environment is missing")
        require_object(record["cacheSeed"], {"relativePath", "sha256"}, "cache seed")
        require_relative_path(record["cacheSeed"]["relativePath"], "cache seed path")
        if record["cacheSeed"]["relativePath"] != f"cache-seeds/{evidence_id}":
            fail("cache seed path does not match performance evidence ID")
        require_digest(record["cacheSeed"]["sha256"], "cache seed digest")
        gates = record["gates"]
        if (
            not isinstance(gates, dict)
            or set(gates) != ADMISSION_GATE_FIELDS
            or any(value is not True for value in gates.values())
        ):
            fail("performance evidence gates are not all true")
        if record["verdict"] != "PASSED":
            fail("performance evidence verdict is not PASSED")
        evidence_ids.append(evidence_id)
    if evidence_ids != sorted(set(evidence_ids)):
        fail("performanceEvidence is not sorted and unique")
    return {
        "executionArtifactId": execution_id,
        "inferenceContractIds": contract_ids,
        "localEnvironmentKeys": environment_keys,
        "performanceEvidenceIds": evidence_ids,
    }


def verify_release_binding_record(record, acceleration_set):
    binding_id = verify_record(record, "releaseBinding")
    identities = verify_acceleration_set(acceleration_set)
    value = record["value"]
    if value["accelerationSetSha256"] != acceleration_set_sha256(acceleration_set):
        fail("release binding acceleration set digest differs")
    if value["reusableInventorySha256"] != acceleration_set["reusableInventorySha256"]:
        fail("release binding reusable inventory differs")
    if value["executionArtifactId"] != identities["executionArtifactId"]:
        fail("release binding execution artifact differs")
    if not set(value["allowedInferenceContractIds"]).issubset(
        identities["inferenceContractIds"]
    ):
        fail("release binding inference contract is missing")
    if not set(value["allowedPerformanceEvidenceIds"]).issubset(
        identities["performanceEvidenceIds"]
    ):
        fail("release binding performance evidence is missing")
    return binding_id


def v3_promotion_metadata(acceleration_set):
    identities = verify_acceleration_set(acceleration_set)
    return {
        "schemaVersion": 3,
        "accelerationSetSha256": acceleration_set_sha256(acceleration_set),
        "executionArtifactId": identities["executionArtifactId"],
        "inferenceContractIds": identities["inferenceContractIds"],
        "localEnvironmentKeys": identities["localEnvironmentKeys"],
        "performanceEvidenceIds": identities["performanceEvidenceIds"],
        "reusableInventorySha256": acceleration_set["reusableInventorySha256"],
    }


def verify_v3_promotion_metadata(promotion, acceleration_set):
    if promotion != v3_promotion_metadata(acceleration_set):
        fail("v3 promotion metadata differs from acceleration set")
    return promotion


def reverse_objects(value):
    if isinstance(value, dict):
        return {key: reverse_objects(value[key]) for key in reversed(value)}
    if isinstance(value, list):
        return [reverse_objects(item) for item in value]
    return value


def verify_fixture(path):
    fixture = strict_json_file(path)
    if not isinstance(fixture, dict) or set(fixture) != {"cases", "schemaVersion"}:
        fail("identity fixture keys differ")
    if fixture["schemaVersion"] != 1:
        fail("unsupported identity fixture schema")
    cases = fixture["cases"]
    if not isinstance(cases, dict) or set(cases) != set(DOMAIN_PREFIXES):
        fail("identity fixture cases differ")
    for name, prefix in DOMAIN_PREFIXES.items():
        case = cases[name]
        if not isinstance(case, dict) or set(case) != {
            "canonical",
            "id",
            "input",
            "prefix",
        }:
            fail(f"{name} fixture keys differ")
        expected_prefix = prefix.decode("ascii")
        if case["prefix"] != expected_prefix:
            fail(f"{name} domain prefix differs")
        canonical = canonical_json_bytes(case["input"])
        if canonical.decode("utf-8") != case["canonical"]:
            fail(f"{name} canonical JSON differs")
        derived = ID_FUNCTIONS[name](case["input"])
        if derived != case["id"]:
            fail(f"{name} content ID differs")
        if ID_FUNCTIONS[name](reverse_objects(case["input"])) != derived:
            fail(f"{name} content ID depends on object insertion order")
        mutation = copy.deepcopy(case["input"])
        if name == "executionArtifact":
            mutation["runtimeSha256"] = "0" * 64
        elif name == "inferenceContract":
            mutation["modelSha256"] = "0" * 64
        elif name == "localEnvironment":
            mutation["driverVersion"] += 1
        elif name == "performanceEvidence":
            mutation["observationBundleSha256"] = "0" * 64
        else:
            mutation["echoBinarySha256"] = "0" * 64
        if ID_FUNCTIONS[name](mutation) == derived:
            fail(f"{name} content ID ignored a changed field")
    return fixture


def self_test():
    fixture = (
        Path(__file__).resolve().parent.parent
        / "crates/echo/tests/fixtures/whisper-v3-identities.json"
    )
    verify_fixture(fixture)
    try:
        canonical_json_bytes({"latency": 1.5})
    except IdentityError:
        pass
    else:
        fail("canonical JSON accepted a float")
    try:
        strict_json_loads('{"schemaVersion":1,"schemaVersion":2}')
    except IdentityError:
        pass
    else:
        fail("strict JSON accepted a duplicate key")
    cases = strict_json_file(fixture)["cases"]
    invalid = copy.deepcopy(cases["executionArtifact"]["input"])
    invalid["echoCommit"] = "a" * 40
    try:
        execution_artifact_id(invalid)
    except IdentityError:
        pass
    else:
        fail("execution artifact accepted app identity")
    invalid = copy.deepcopy(cases["localEnvironment"]["input"])
    invalid["executionArtifactId"] = cases["executionArtifact"]["id"]
    try:
        local_environment_key(invalid)
    except IdentityError:
        pass
    else:
        fail("local environment accepted runtime identity")
    invalid = copy.deepcopy(cases["releaseBinding"]["input"])
    invalid["allowedInferenceContractIds"] *= 2
    try:
        release_binding_id(invalid)
    except IdentityError:
        pass
    else:
        fail("release binding accepted duplicate contract IDs")

    execution = build_record("executionArtifact", cases["executionArtifact"]["input"])
    contract = build_record("inferenceContract", cases["inferenceContract"]["input"])
    environment = {
        "key": cases["localEnvironment"]["id"],
        "launch": {
            "icdLibraryPath": "/usr/lib/libvulkan_intel.so",
            "icdManifestPath": "/usr/share/vulkan/icd.d/intel_icd.json",
        },
        "value": cases["localEnvironment"]["input"],
    }
    evidence = {
        "cacheSeed": {
            "relativePath": f"cache-seeds/{cases['performanceEvidence']['id']}",
            "sha256": "4" * 64,
        },
        "gates": {name: True for name in sorted(ADMISSION_GATE_FIELDS)},
        "id": cases["performanceEvidence"]["id"],
        "value": cases["performanceEvidence"]["input"],
        "verdict": "PASSED",
    }
    acceleration_set = {
        "executionArtifact": execution,
        "inferenceContracts": [contract],
        "localEnvironments": [environment],
        "performanceEvidence": [evidence],
        "reusableInventorySha256": "3" * 64,
        "schemaVersion": 3,
    }
    identities = verify_acceleration_set(acceleration_set)
    if identities["performanceEvidenceIds"] != [cases["performanceEvidence"]["id"]]:
        fail("acceleration set returned the wrong evidence IDs")

    binding_input = copy.deepcopy(cases["releaseBinding"]["input"])
    binding_input["accelerationSetSha256"] = acceleration_set_sha256(acceleration_set)
    binding_record = build_record("releaseBinding", binding_input)
    verify_release_binding_record(binding_record, acceleration_set)
    changed_version = copy.deepcopy(binding_input)
    changed_version["version"] = "0.12.6"
    if release_binding_id(changed_version) == binding_record["id"]:
        fail("app version did not change the release binding")
    changed_gates = copy.deepcopy(acceleration_set)
    changed_gates["performanceEvidence"][0]["gates"]["backendTruth"] = False
    try:
        verify_acceleration_set(changed_gates)
    except IdentityError:
        pass
    else:
        fail("acceleration set accepted a false gate")
    broken_reference = copy.deepcopy(acceleration_set)
    broken_value = broken_reference["performanceEvidence"][0]["value"]
    broken_value["inferenceContractId"] = "0" * 64
    broken_id = performance_evidence_id(broken_value)
    broken_reference["performanceEvidence"][0]["id"] = broken_id
    broken_reference["performanceEvidence"][0]["cacheSeed"]["relativePath"] = (
        f"cache-seeds/{broken_id}"
    )
    try:
        verify_acceleration_set(broken_reference)
    except IdentityError:
        pass
    else:
        fail("acceleration set accepted a missing contract reference")
    print("whisper_identity_v3: self-test passed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-fixture", type=Path)
    args = parser.parse_args()
    if args.self_test and args.verify_fixture is None:
        self_test()
    elif args.verify_fixture is not None and not args.self_test:
        verify_fixture(args.verify_fixture)
        print("whisper_identity_v3: fixture verified")
    else:
        parser.error("use --self-test or --verify-fixture PATH")


if __name__ == "__main__":
    try:
        main()
    except IdentityError as error:
        print(f"whisper_identity_v3: {error}")
        raise SystemExit(2)
