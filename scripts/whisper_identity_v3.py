#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path


DOMAIN_PREFIXES = {
    "executionArtifact": b"echo-whisper-execution-artifact-v3\0",
    "inferenceContract": b"echo-whisper-inference-contract-v3\0",
    "localEnvironment": b"echo-whisper-local-environment-v3\0",
    "performanceEvidence": b"echo-whisper-performance-evidence-v3\0",
    "releaseBinding": b"echo-whisper-release-binding-v3\0",
}


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


def content_id(prefix, value):
    return hashlib.sha256(prefix + canonical_json_bytes(value)).hexdigest()


def execution_artifact_id(value):
    return content_id(DOMAIN_PREFIXES["executionArtifact"], value)


def inference_contract_id(value):
    return content_id(DOMAIN_PREFIXES["inferenceContract"], value)


def local_environment_key(value):
    return content_id(DOMAIN_PREFIXES["localEnvironment"], value)


def performance_evidence_id(value):
    return content_id(DOMAIN_PREFIXES["performanceEvidence"], value)


def release_binding_id(value):
    return content_id(DOMAIN_PREFIXES["releaseBinding"], value)


ID_FUNCTIONS = {
    "executionArtifact": execution_artifact_id,
    "inferenceContract": inference_contract_id,
    "localEnvironment": local_environment_key,
    "performanceEvidence": performance_evidence_id,
    "releaseBinding": release_binding_id,
}


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
        mutation = dict(case["input"])
        mutation["schemaVersion"] = 4
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
