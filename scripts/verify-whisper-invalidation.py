#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path

from whisper_identity_v3 import (
    IdentityError,
    execution_artifact_id,
    inference_contract_id,
    local_environment_key,
    performance_evidence_id,
    release_binding_id,
    strict_json_file,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_PATH = REPO_ROOT / "scripts/fixtures/whisper-v3-identities.json"
ID_FIELDS = (
    "executionArtifactId",
    "inferenceContractId",
    "localEnvironmentKey",
    "performanceEvidenceId",
    "releaseBindingId",
)


def inputs() -> dict[str, dict[str, object]]:
    cases = strict_json_file(FIXTURE_PATH)["cases"]
    return {
        name: copy.deepcopy(cases[name]["input"])
        for name in (
            "executionArtifact",
            "inferenceContract",
            "localEnvironment",
            "performanceEvidence",
            "releaseBinding",
        )
    }


def derive(values: dict[str, dict[str, object]]) -> dict[str, str]:
    execution = execution_artifact_id(values["executionArtifact"])
    inference = inference_contract_id(values["inferenceContract"])
    environment = local_environment_key(values["localEnvironment"])
    performance_input = copy.deepcopy(values["performanceEvidence"])
    performance_input.update(
        {
            "executionArtifactId": execution,
            "inferenceContractId": inference,
            "localEnvironmentKey": environment,
        }
    )
    performance = performance_evidence_id(performance_input)
    binding_input = copy.deepcopy(values["releaseBinding"])
    binding_input.update(
        {
            "executionArtifactId": execution,
            "allowedInferenceContractIds": [inference],
            "allowedPerformanceEvidenceIds": [performance],
        }
    )
    return {
        "executionArtifactId": execution,
        "inferenceContractId": inference,
        "localEnvironmentKey": environment,
        "performanceEvidenceId": performance,
        "releaseBindingId": release_binding_id(binding_input),
    }


def set_digest(value: dict[str, object], field: str, digit: str) -> None:
    value[field] = digit * 64


def cases():
    return (
        (
            "version-only",
            lambda values: values["releaseBinding"].update(version="0.12.6"),
            {"releaseBindingId"},
            False,
        ),
        (
            "debian-marker-only",
            lambda values: set_digest(
                values["releaseBinding"], "echoBinarySha256", "1"
            ),
            {"releaseBindingId"},
            False,
        ),
        (
            "rpm-marker-only",
            lambda values: values["releaseBinding"].update(
                packageType="rpm", bundleMarker="rpm", echoBinarySha256="2" * 64
            ),
            {"releaseBindingId"},
            False,
        ),
        (
            "runtime-library",
            lambda values: set_digest(
                values["executionArtifact"], "runtimeSha256", "4"
            ),
            {"executionArtifactId", "performanceEvidenceId", "releaseBindingId"},
            True,
        ),
        (
            "model",
            lambda values: set_digest(values["inferenceContract"], "modelSha256", "4"),
            {"inferenceContractId", "performanceEvidenceId", "releaseBindingId"},
            True,
        ),
        (
            "vad",
            lambda values: set_digest(values["inferenceContract"], "vadSha256", "4"),
            {"inferenceContractId", "performanceEvidenceId", "releaseBindingId"},
            True,
        ),
        (
            "tuning",
            lambda values: values["inferenceContract"]["tuning"].update(beamSize=2),
            {"inferenceContractId", "performanceEvidenceId", "releaseBindingId"},
            True,
        ),
        (
            "behavior-projection",
            lambda values: set_digest(
                values["inferenceContract"]["behavior"], "projectionSha256", "4"
            ),
            {"inferenceContractId", "performanceEvidenceId", "releaseBindingId"},
            True,
        ),
        (
            "hardware-driver",
            lambda values: values["localEnvironment"].update(driverVersion=104865801),
            {"localEnvironmentKey", "performanceEvidenceId", "releaseBindingId"},
            True,
        ),
        (
            "measurement",
            lambda values: set_digest(
                values["performanceEvidence"], "observationBundleSha256", "4"
            ),
            {"performanceEvidenceId", "releaseBindingId"},
            True,
        ),
    )


def verify_matrix() -> dict[str, object]:
    baseline_values = inputs()
    baseline = derive(baseline_values)
    rows = []
    for name, mutate, expected, physical_sweep in cases():
        candidate_values = copy.deepcopy(baseline_values)
        mutate(candidate_values)
        candidate = derive(candidate_values)
        changed = {field for field in ID_FIELDS if candidate[field] != baseline[field]}
        if changed != expected:
            raise ValueError(
                f"{name} changed {sorted(changed)}, expected {sorted(expected)}"
            )
        rows.append(
            {
                "change": name,
                "changedIds": sorted(changed),
                "physicalRequalificationRequired": physical_sweep,
                "reuseRefused": False,
            }
        )
    unsupported_policy = copy.deepcopy(baseline_values)
    unsupported_policy["inferenceContract"]["requestPolicy"]["language"] = "auto"
    try:
        derive(unsupported_policy)
    except IdentityError:
        rows.append(
            {
                "change": "request-policy",
                "changedIds": [],
                "physicalRequalificationRequired": True,
                "reuseRefused": True,
            }
        )
    else:
        raise ValueError("unsupported request policy reused qualified evidence")
    unsupported_scope = copy.deepcopy(baseline_values)
    unsupported_scope["inferenceContract"]["claimScope"] = "product-stt-corpus-v2"
    try:
        derive(unsupported_scope)
    except IdentityError:
        rows.append(
            {
                "change": "claim-scope",
                "changedIds": [],
                "physicalRequalificationRequired": True,
                "reuseRefused": True,
            }
        )
    else:
        raise ValueError("unsupported claim scope reused qualified evidence")
    return {"schemaVersion": 1, "baseline": baseline, "cases": rows}


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify Whisper release identity invalidation boundaries"
    )
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    result = verify_matrix()
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    if args.self_test or args.output is None:
        print("verify-whisper-invalidation: 12 cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
