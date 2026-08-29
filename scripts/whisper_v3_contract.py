from __future__ import annotations

import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

from whisper_identity_v3 import (
    INFERENCE_CLAIM_SCOPE,
    canonical_json_bytes,
    inference_contract_id,
    sha256_bytes,
    strict_json_file,
    strict_json_loads,
    verify_acceleration_set,
    verify_fixture,
)

BEHAVIOR_PATH = Path("crates/echo/tests/fixtures/whisper-behavior-v3.json")
IDENTITIES_PATH = Path("crates/echo/tests/fixtures/whisper-v3-identities.json")
BEHAVIOR_FIELDS = {"projection", "projectionSha256", "schemaVersion", "watchedPaths"}
COMMIT = re.compile(r"[0-9a-f]{40}")
REQUEST_POLICY = {
    "language": "pinned",
    "prompt": "empty",
    "hints": "qualifiedOnly",
}


@dataclass(frozen=True)
class BehaviorAuthority:
    projection_sha256: str
    inference_behavior: dict[str, object]
    watched_paths: tuple[str, ...]


@dataclass(frozen=True)
class VerifiedInferenceContract:
    id: str
    value: dict[str, object]
    measured_commit: str


def validate_behavior(
    value: object, *, repo_root: Path | None = None
) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != BEHAVIOR_FIELDS:
        raise ValueError("behavior contract has unknown or missing fields")
    if value["schemaVersion"] != 3:
        raise ValueError("behavior contract has the wrong schema version")
    projection = value["projection"]
    if not isinstance(projection, dict) or set(projection) != {
        "decode",
        "launch",
        "receipt",
        "recovery",
        "telemetry",
    }:
        raise ValueError("behavior projection has unknown or missing sections")
    expected = sha256_bytes(canonical_json_bytes(projection))
    if value["projectionSha256"] != expected:
        raise ValueError("behavior projection digest differs from its canonical values")
    watched = value["watchedPaths"]
    if (
        not isinstance(watched, list)
        or not watched
        or watched != sorted(set(watched))
        or not all(isinstance(path, str) and path.endswith(".rs") for path in watched)
    ):
        raise ValueError("behavior watched paths are not sorted, unique Rust paths")
    if repo_root is not None:
        missing = [path for path in watched if not (repo_root / path).is_file()]
        if missing:
            raise ValueError(f"behavior watched paths do not exist: {missing}")
    return value


def authority(value: object, *, repo_root: Path | None = None) -> BehaviorAuthority:
    behavior = validate_behavior(value, repo_root=repo_root)
    projection = behavior["projection"]
    return BehaviorAuthority(
        projection_sha256=behavior["projectionSha256"],
        inference_behavior={
            "launchSchema": 1,
            "receiptSchema": projection["receipt"]["schema"],
            "telemetrySchema": projection["telemetry"]["schema"],
            "recoverySchema": projection["recovery"]["schema"],
            "projectionSha256": behavior["projectionSha256"],
        },
        watched_paths=tuple(behavior["watchedPaths"]),
    )


def behavior_at_commit(repo_root: Path, commit: str) -> BehaviorAuthority:
    if COMMIT.fullmatch(commit) is None:
        raise ValueError("behavior commit is not a full lowercase commit")
    completed = subprocess.run(
        ["git", "show", f"{commit}:{BEHAVIOR_PATH.as_posix()}"],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return authority(strict_json_loads(completed.stdout))


def validate_current(repo_root: Path) -> dict[str, object]:
    behavior = validate_behavior(
        strict_json_file(repo_root / BEHAVIOR_PATH), repo_root=repo_root
    )
    identities = verify_fixture(repo_root / IDENTITIES_PATH)
    contract_digest = identities["cases"]["inferenceContract"]["input"]["behavior"][
        "projectionSha256"
    ]
    if contract_digest != behavior["projectionSha256"]:
        raise ValueError("inference contract does not bind the behavior projection")
    return behavior


def verify_measured_inference_contract(
    *,
    repo_root: Path,
    measured_commit: str,
    contract_path: Path,
    model_sha256: str,
    vad_sha256: str | None,
    tuning: dict[str, object],
) -> VerifiedInferenceContract:
    measured = behavior_at_commit(repo_root, measured_commit)
    candidate = strict_json_file(contract_path)
    expected = {
        "schemaVersion": 3,
        "protocol": "oneShotCli",
        "modelSha256": model_sha256,
        "vadSha256": vad_sha256,
        "tuning": tuning,
        "requestPolicy": REQUEST_POLICY,
        "behavior": measured.inference_behavior,
        "claimScope": INFERENCE_CLAIM_SCOPE,
    }
    inference_contract_id(candidate)
    if candidate != expected:
        raise ValueError("v3 inference contract differs from measured inference inputs")
    return VerifiedInferenceContract(
        id=inference_contract_id(candidate),
        value=candidate,
        measured_commit=measured_commit,
    )


def verify_reusable_evidence_for_commit(
    *, repo_root: Path, commit: str, acceleration_set: dict[str, object]
) -> dict[str, object]:
    current = behavior_at_commit(repo_root, commit)
    identities = verify_acceleration_set(acceleration_set)
    for record in acceleration_set["inferenceContracts"]:
        contract = record["value"]
        if (
            contract["claimScope"] != INFERENCE_CLAIM_SCOPE
            or contract["behavior"] != current.inference_behavior
        ):
            raise ValueError(
                "reusable v3 evidence differs from current inference behavior"
            )
    return identities
