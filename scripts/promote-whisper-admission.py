#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from whisper_identity_v3 import (
    ADMISSION_GATE_FIELDS,
    build_record,
    canonical_json_bytes,
    inference_contract_id,
    sha256_bytes,
    verify_acceleration_set,
    v3_promotion_metadata,
)
from whisper_release_common import (
    package_inventory,
    read_json_strict,
    runtime_identity,
    runtime_library_bindings,
    sha256_file,
    tree_sha256,
    verify_contained_symlinks,
    verify_admission_set,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
RESEARCH_GATES = (
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
)
BINDING_GATES = (
    "coverageComplete",
    "cacheEvidence",
    "resetEvidence",
    "driverIcdIdentity",
    "cleanChildEnvironment",
    "exactRuntime",
)
RESOURCE_GATES = (
    "stabilitySuccess",
    "memoryEvidence",
    "memoryFloor",
    "swapStable",
)
RECEIPT_PREFIX = "echo_whisper_runtime_receipt: "


def prefixed_inventory(root: Path, prefix: str) -> list[dict[str, object]]:
    entries = []
    for entry in package_inventory(root):
        copied = dict(entry)
        copied["path"] = f"{prefix}/{entry['path']}"
        entries.append(copied)
    return entries


def write_v3_promotion(
    *,
    output: Path,
    package: Path,
    runtime_cli: Path,
    runtime_probe: Path,
    model: Path,
    vad: Path,
    tuning: dict[str, object],
    receipt: dict[str, object],
    drm_driver: str,
    icd_manifest: Path,
    icd_library: Path,
    inference_contract_path: Path,
    cache_source: Path,
    cache_sha256: str,
    sweep_path: Path,
    corpus: Path,
    cycle_path: Path,
    phase2: dict[str, object],
    accepted_at: int,
    expires_at: int,
) -> dict[str, object]:
    contract_input = read_json_strict(inference_contract_path, "v3 inference contract")
    contract_id = inference_contract_id(contract_input)
    if (
        contract_input["modelSha256"] != sha256_file(model)
        or contract_input["vadSha256"] != sha256_file(vad)
        or contract_input["tuning"] != tuning
    ):
        raise ValueError("v3 inference contract differs from promotion inputs")

    build_receipt_path = runtime_cli.parent / "build-receipt.json"
    build_receipt = read_json_strict(build_receipt_path, "runtime build receipt")
    runtime_bindings = runtime_library_bindings(runtime_cli)
    runtime_inventory = prefixed_inventory(package / "runtime", "runtime")
    execution_input = {
        "schemaVersion": 3,
        "runtimeArtifactId": build_receipt.get("artifactId"),
        "runtimeIdentitySha256": runtime_identity(runtime_cli),
        "runtimeRelativePath": "runtime/whisper-cli",
        "runtimeSha256": sha256_file(runtime_cli),
        "runtimeLibraryBindings": runtime_bindings,
        "probeRelativePath": "runtime/echo-whisper-runtime-probe",
        "probeSha256": sha256_file(runtime_probe),
        "buildReceiptSha256": sha256_file(build_receipt_path),
        "reusableInventorySha256": sha256_bytes(
            canonical_json_bytes(runtime_inventory)
        ),
    }
    execution = build_record("executionArtifact", execution_input)
    environment_input = {
        "schemaVersion": 3,
        "architecture": "x86_64",
        "backend": receipt["backend"],
        "vendorId": receipt["vendorId"],
        "deviceId": receipt["deviceId"],
        "apiVersion": receipt["apiVersion"],
        "driverVersion": receipt["driverVersion"],
        "deviceUUID": receipt["deviceUUID"],
        "driverUUID": receipt["driverUUID"],
        "pipelineCacheUUID": receipt["pipelineCacheUUID"],
        "drmDriver": drm_driver,
        "icdManifestSha256": sha256_file(icd_manifest),
        "icdLibrarySha256": sha256_file(icd_library),
    }
    environment = {
        "key": build_record("localEnvironment", environment_input)["id"],
        "launch": {
            "icdManifestPath": str(icd_manifest),
            "icdLibraryPath": str(icd_library),
        },
        "value": environment_input,
    }
    gate_policy = {name: True for name in sorted(ADMISSION_GATE_FIELDS)}
    coverage = {
        "claimBoundary": phase2.get("claimBoundary"),
        "languages": phase2.get("languages"),
    }
    evidence_input = {
        "schemaVersion": 3,
        "executionArtifactId": execution["id"],
        "inferenceContractId": contract_id,
        "localEnvironmentKey": environment["key"],
        "measurementProtocol": "paired-product-sweep-v2",
        "corpusManifestSha256": sha256_file(corpus),
        "coverageManifestSha256": sha256_bytes(canonical_json_bytes(coverage)),
        "observationBundleSha256": sha256_file(sweep_path / "sweep.json"),
        "cacheCycleSha256": sha256_file(cycle_path / "cache-cycle.json"),
        "gatePolicySha256": sha256_bytes(canonical_json_bytes(gate_policy)),
        "acceptedAt": accepted_at,
        "expiresAt": expires_at,
    }
    evidence = build_record("performanceEvidence", evidence_input)
    cache_relative = f"cache-seeds/{evidence['id']}"
    shutil.copytree(cache_source, package / cache_relative)
    reusable_inventory = [
        *runtime_inventory,
        *prefixed_inventory(package / cache_relative, cache_relative),
    ]
    acceleration_set = {
        "schemaVersion": 3,
        "executionArtifact": execution,
        "inferenceContracts": [build_record("inferenceContract", contract_input)],
        "localEnvironments": [environment],
        "performanceEvidence": [
            {
                "cacheSeed": {
                    "relativePath": cache_relative,
                    "sha256": cache_sha256,
                },
                "gates": gate_policy,
                "id": evidence["id"],
                "value": evidence_input,
                "verdict": "PASSED",
            }
        ],
        "reusableInventorySha256": sha256_bytes(
            canonical_json_bytes(reusable_inventory)
        ),
    }
    verify_acceleration_set(acceleration_set)
    set_path = package / "acceleration-set.v3.json"
    set_path.write_text(
        json.dumps(acceleration_set, indent=2, ensure_ascii=False, sort_keys=True)
        + "\n",
        encoding="utf-8",
    )
    promotion = v3_promotion_metadata(acceleration_set)
    (output / "promotion-v3.json").write_text(
        json.dumps(promotion, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return promotion


def read_json(path: Path, label: str) -> dict[str, object]:
    return read_json_strict(path, label)


def verify_runtime_alias_bindings(recorded: object, runtime_cli: Path) -> None:
    if not isinstance(recorded, dict) or recorded != runtime_library_bindings(
        runtime_cli
    ):
        raise ValueError("runtime library aliases changed after qualification")


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()


def identity_key(identity: dict[str, object]) -> str:
    return hashlib.sha256(
        b"echo-whisper-admission-identity-v1\0" + canonical(identity)
    ).hexdigest()


def require_green(gates: object, names: tuple[str, ...], label: str) -> dict[str, bool]:
    if not isinstance(gates, dict) or set(gates) != set(names):
        raise ValueError(f"{label} has the wrong gate set")
    if any(gates[name] is not True for name in names):
        raise ValueError(f"{label} contains a failed gate")
    return {name: True for name in names}


def require_path(value: object, label: str) -> Path:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label} must be a path")
    if value == "$REPO" or value.startswith("$REPO/"):
        return REPO_ROOT / value.removeprefix("$REPO/")
    if value == "$HOME" or value.startswith("$HOME/"):
        return Path.home() / value.removeprefix("$HOME/")
    return Path(value)


def selected_cell(sweep: dict[str, object], label: str) -> dict[str, object]:
    cells = sweep.get("cells")
    if not isinstance(cells, list):
        raise ValueError("sweep cells must be an array")
    matches = [
        cell
        for cell in cells
        if isinstance(cell, dict) and cell.get("cell", {}).get("label") == label
    ]
    if len(matches) != 1:
        raise ValueError(f"expected one sweep cell named {label}")
    cell = matches[0]
    if cell.get("decision") != "PROCEED" or cell.get("researchPass") is not True:
        raise ValueError("sweep cell is not a confirmed research pass")
    require_green(
        cell.get("researchGates"),
        (*RESEARCH_GATES, *RESOURCE_GATES),
        "research and resource gates",
    )
    require_green(cell.get("bindingGates"), BINDING_GATES, "binding gates")
    return cell


def strict_nonnegative_integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{label} must be a non-negative integer")
    return value


def persisted_resource_thresholds(cell: dict[str, object]) -> tuple[int, int]:
    recorded = cell.get("resourceEvidence")
    if not isinstance(recorded, dict) or recorded.get("verdict") != "VERIFIED":
        raise ValueError("promotion requires VERIFIED resource evidence")
    return (
        strict_nonnegative_integer(
            recorded.get("minimumAvailableMemoryBytes"),
            "minimum available memory threshold",
        ),
        strict_nonnegative_integer(
            recorded.get("maximumSustainedSwapGrowthBytes"),
            "maximum sustained swap threshold",
        ),
    )


def replay_analysis(cell: dict[str, object], corpus: Path, scratch: Path) -> None:
    evidence = cell.get("evidence")
    candidates = cell.get("candidates")
    if not isinstance(evidence, dict) or not isinstance(candidates, dict):
        raise ValueError("cell evidence and candidates are required")
    bundle = require_path(evidence.get("bundle"), "cell bundle")
    minimum_memory, maximum_swap = persisted_resource_thresholds(cell)
    command = [
        sys.executable,
        str(REPO_ROOT / "scripts/analyze-stt-host-matrix.py"),
        "--runs",
        str(bundle / "runs.jsonl"),
        "--corpus-manifest",
        str(corpus),
        "--cpu-candidate",
        str(candidates["cpu"]),
        "--accelerated-candidate",
        str(candidates["accelerated"]),
        "--expected-backend",
        "vulkan",
        "--output-dir",
        str(scratch / "analysis"),
        "--minimum-available-memory-bytes",
        str(minimum_memory),
        "--maximum-sustained-swap-growth-bytes",
        str(maximum_swap),
        "--require-resource-evidence",
    ]
    subprocess.run(command, cwd=REPO_ROOT, check=True, capture_output=True, text=True)
    replay = read_json(scratch / "analysis/decision.json", "replayed analysis")
    phase2 = cell.get("phase2")
    if not isinstance(phase2, dict):
        raise ValueError("cell phase2 analysis is missing")
    for field in (
        "decision",
        "gates",
        "expectedBackend",
        "cpuCandidate",
        "acceleratedCandidate",
        "runsPerCandidate",
        "cpuMedianOuterMs",
        "acceleratedMedianOuterMs",
        "medianReductionMs",
        "medianSpeedupPercent",
        "cpuP95OuterMs",
        "acceleratedP95OuterMs",
        "newHallucinations",
        "languages",
        "resourceEvidence",
    ):
        if replay.get(field) != phase2.get(field):
            raise ValueError(f"replayed analysis changed {field}")


def admitted_tuning(cell: dict[str, object]) -> dict[str, object]:
    config = cell.get("cell")
    evidence = cell.get("evidence")
    candidates = cell.get("candidates")
    if (
        not isinstance(config, dict)
        or not isinstance(evidence, dict)
        or not isinstance(candidates, dict)
    ):
        raise ValueError("cell tuning evidence is incomplete")
    values = {
        "threads": config.get("threads"),
        "beamSize": config.get("beamSize"),
        "bestOf": config.get("bestOf"),
        "noFallback": config.get("noFallback"),
    }
    if any(
        isinstance(values[name], bool)
        or not isinstance(values[name], int)
        or values[name] < 1
        for name in ("threads", "beamSize", "bestOf")
    ) or not isinstance(values["noFallback"], bool):
        raise ValueError("cell tuning has invalid types or bounds")
    manifest = read_json(
        require_path(evidence.get("bundle"), "cell bundle") / "run-manifest.json",
        "benchmark run manifest",
    )
    records = manifest.get("candidates")
    labels = {candidates.get("cpu"), candidates.get("accelerated")}
    if not isinstance(records, list) or len(records) != 2 or None in labels:
        raise ValueError("benchmark tuning candidates are incomplete")
    by_label = {
        record.get("label"): record for record in records if isinstance(record, dict)
    }
    if set(by_label) != labels:
        raise ValueError("benchmark tuning candidates differ from the selected cell")
    for label in labels:
        record = by_label[label]
        if any(
            record.get(name) != value or type(record.get(name)) is not type(value)
            for name, value in values.items()
        ):
            raise ValueError(
                "benchmark candidate tuning differs from the selected cell"
            )
    return values


def admitted_cache_probe(tuning: dict[str, object]) -> dict[str, object]:
    return {
        "backend": "vulkan",
        "language": "en",
        "prompt": "",
        **tuning,
    }


def populated_cache_snapshot(
    cycle_root: Path, cycle: dict[str, object]
) -> tuple[Path, list[dict[str, object]]]:
    snapshots = cycle.get("cacheSnapshots")
    if not isinstance(snapshots, dict) or not isinstance(
        snapshots.get("afterPopulated"), dict
    ):
        raise ValueError("cache cycle has no populated snapshot")
    reference = snapshots["afterPopulated"]
    raw = reference.get("path")
    if not isinstance(raw, str):
        raise ValueError("populated snapshot path is invalid")
    snapshot = read_json(cycle_root / raw, "populated cache snapshot")
    files = snapshot.get("files")
    root = require_path(snapshot.get("root"), "populated cache root").resolve()
    if (
        root != (cycle_root / "mesa-cache").resolve()
        or not isinstance(files, list)
        or not files
        or not root.is_dir()
    ):
        raise ValueError("populated cache snapshot has no files")
    return root, files


def verify_cache_snapshot(root: Path, expected: list[dict[str, object]]) -> None:
    actual = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or (not path.is_file() and not path.is_dir()):
            raise ValueError("cache seed contains an unsupported entry")
        if path.is_file():
            actual.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "bytes": path.stat().st_size,
                    "sha256": sha256_file(path),
                }
            )
    if actual != expected:
        raise ValueError("cache seed differs from the recorded populated snapshot")


def verify_sweep_vad(cell: dict[str, object], expected_vad: Path) -> None:
    evidence = cell.get("evidence")
    if not isinstance(evidence, dict):
        raise ValueError("cell evidence is required")
    bundle = require_path(evidence.get("bundle"), "cell bundle")
    expected_digest = sha256_file(expected_vad)
    rows = [
        json.loads(line)
        for line in (bundle / "runs.jsonl").read_text(encoding="utf-8").splitlines()
    ]
    if not rows:
        raise ValueError("sweep has no measurement rows")
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("vadArtifact"), dict):
            raise ValueError("sweep measurement has no VAD identity")
        vad = row["vadArtifact"]
        if (
            vad.get("sha256") != expected_digest
            or require_path(vad.get("path"), "measurement VAD").resolve()
            != expected_vad.resolve()
            or row.get("engine", {}).get("vad") is not True
        ):
            raise ValueError("sweep measurement used a different or inactive VAD")


def parse_timestamp(value: object) -> int:
    if not isinstance(value, str):
        raise ValueError("sweep completedAt is missing")
    return int(datetime.datetime.fromisoformat(value).timestamp())


def verify_runtime_probe(
    probe: Path,
    runtime_dir: Path,
    icd_manifest: Path,
    cache: Path,
    expected_receipt: dict[str, object],
) -> None:
    completed = subprocess.run(
        [str(probe)],
        check=False,
        capture_output=True,
        text=True,
        timeout=15,
        env={
            "HOME": str(Path.home()),
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            "LD_LIBRARY_PATH": str(runtime_dir),
            "MESA_SHADER_CACHE_DIR": str(cache),
            "PATH": os.defpath,
            "VK_DRIVER_FILES": str(icd_manifest),
        },
    )
    lines = [
        line.removeprefix(RECEIPT_PREFIX)
        for line in completed.stderr.splitlines()
        if line.startswith(RECEIPT_PREFIX)
    ]
    if (
        completed.returncode != 0
        or len(lines) != 1
        or json.loads(lines[0]) != expected_receipt
    ):
        raise ValueError("runtime probe did not reproduce the admitted receipt")


def promote(args: argparse.Namespace) -> None:
    output = args.output.resolve()
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    sweep_path = args.sweep.resolve()
    status = read_json(sweep_path / "status.json", "sweep status")
    sweep = read_json(sweep_path / "sweep.json", "sweep")
    if status.get("state") != "complete" or sweep.get("researchPass") is not True:
        raise ValueError("sweep is not complete and green")
    cell = selected_cell(sweep, args.cell)
    with tempfile.TemporaryDirectory(prefix="echo-whisper-promotion-") as temporary:
        replay_analysis(cell, args.corpus.resolve(), Path(temporary))

    cycle_path = args.cache_cycle.resolve()
    cell_evidence = cell.get("evidence")
    if (
        not isinstance(cell_evidence, dict)
        or require_path(cell_evidence.get("cacheCycle"), "cell cache cycle").resolve()
        != cycle_path
    ):
        raise ValueError("promotion cache cycle differs from the selected sweep cell")
    subprocess.run(
        [
            sys.executable,
            str(REPO_ROOT / "scripts/run-whisper-cache-cycle.py"),
            "--validate-cycle",
            str(cycle_path),
        ],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    cycle = read_json(cycle_path / "cache-cycle.json", "cache cycle")
    host = read_json(cycle_path / "host-evidence.json", "host evidence")
    if cycle.get("resetEvidence", {}).get("state") != "COMPLETE":
        raise ValueError("cache cycle lacks complete reset evidence")

    identity_block = cell.get("identity")
    receipt = cell.get("receipt")
    cell_config = cell.get("cell")
    if not all(
        isinstance(value, dict) for value in (identity_block, receipt, cell_config)
    ):
        raise ValueError("cell identity, receipt, and configuration are required")
    echo_binary = args.echo_binary.resolve()
    expected_commit = identity_block["echo"]["commit"]
    actual_commit = subprocess.run(
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
    if dirty or actual_commit != expected_commit:
        raise ValueError("promotion requires the exact clean measured Echo commit")
    echo_sha = sha256_file(echo_binary)
    if echo_sha != identity_block["echo"]["binary"]["sha256"]:
        raise ValueError("Echo binary changed after qualification")

    runtime_dir = args.runtime_dir.resolve()
    runtime_cli = runtime_dir / "whisper-cli"
    runtime_probe = args.runtime_probe.resolve()
    if not runtime_probe.is_file():
        raise ValueError("receipt runtime has no echo-whisper-runtime-probe")
    model = args.model.resolve()
    vad = args.vad.resolve()
    verify_sweep_vad(cell, vad)
    cycle_identity = cycle["identity"]["value"]
    runtime_sha = runtime_identity(runtime_cli)
    qualified_runtime_bindings = identity_block["runtime"].get("libraryBindings")
    verify_runtime_alias_bindings(qualified_runtime_bindings, runtime_cli)
    for actual, expected, label in (
        (runtime_sha, cycle_identity["runtime"]["identitySha256"], "runtime"),
        (sha256_file(model), cycle_identity["model"]["sha256"], "model"),
        (sha256_file(vad), cycle_identity["vad"]["sha256"], "VAD"),
    ):
        if actual != expected:
            raise ValueError(f"{label} changed after qualification")
    if (
        identity_block["runtime"]["sha256"] != sha256_file(runtime_cli)
        or identity_block["runtimeProbe"]["sha256"] != sha256_file(runtime_probe)
        or identity_block["model"]["sha256"] != sha256_file(model)
    ):
        raise ValueError("sweep artifacts differ from promotion inputs")
    if receipt != cycle["probes"]["populated"]["receipt"]:
        raise ValueError("sweep and cache-cycle receipts differ")

    selected_icd = host["loader"]["selectedIcd"]
    icd_manifest = require_path(
        selected_icd["manifest"]["path"], "ICD manifest"
    ).resolve()
    icd_library = require_path(selected_icd["library"]["path"], "ICD library").resolve()
    if sha256_file(icd_manifest) != selected_icd["manifest"]["sha256"]:
        raise ValueError("ICD manifest changed after qualification")
    if identity_block["vkDriverFiles"]["sha256"] != sha256_file(icd_manifest):
        raise ValueError("sweep and cache-cycle ICD manifests differ")
    if sha256_file(icd_library) != selected_icd["library"]["sha256"]:
        raise ValueError("ICD library changed after qualification")
    drm = [
        device
        for device in host["drmDevices"]
        if int(device["vendor"], 16) == receipt["vendorId"]
        and int(device["device"], 16) == receipt["deviceId"]
    ]
    if len(drm) != 1:
        raise ValueError("receipt does not bind exactly one DRM device")

    package = output / "whisper-acceleration"
    package.mkdir(parents=True)
    shutil.copytree(runtime_dir, package / "runtime", symlinks=False)
    verify_runtime_alias_bindings(
        qualified_runtime_bindings, package / "runtime/whisper-cli"
    )
    shutil.copy2(runtime_probe, package / "runtime/echo-whisper-runtime-probe")
    cache_source, expected_cache_files = populated_cache_snapshot(cycle_path, cycle)
    tuning = admitted_tuning(cell)
    cycle_probe = cycle_identity.get("probe")
    expected_probe = admitted_cache_probe(tuning)
    if not isinstance(cycle_probe, dict) or cycle_probe != expected_probe:
        raise ValueError("cache-cycle probe settings differ from admitted tuning")
    admission_device = {
        "backend": receipt["backend"],
        "selectedIndex": receipt["selectedIndex"],
        "vendorId": receipt["vendorId"],
        "deviceId": receipt["deviceId"],
        "apiVersion": receipt["apiVersion"],
        "driverVersion": receipt["driverVersion"],
        "deviceUUID": receipt["deviceUUID"],
        "driverUUID": receipt["driverUUID"],
        "pipelineCacheUUID": receipt["pipelineCacheUUID"],
    }
    identity = {
        "schemaVersion": 1,
        "echoCommit": actual_commit,
        "echoBinarySha256": echo_sha,
        "runtimeIdentitySha256": runtime_sha,
        "modelSha256": sha256_file(model),
        "vadSha256": sha256_file(vad),
        "protocol": "oneShotCli",
        "tuning": tuning,
        "languagePolicy": "pinned",
        "promptPolicy": "empty",
        "device": admission_device,
        "drmDriver": drm[0]["driver"],
        "icdManifestSha256": sha256_file(icd_manifest),
        "icdLibrarySha256": sha256_file(icd_library),
        "launchContractSchema": 1,
    }
    key = identity_key(identity)
    cache_relative = f"cache-seeds/{key}"
    shutil.copytree(cache_source, package / cache_relative)
    verify_cache_snapshot(package / cache_relative, expected_cache_files)
    with tempfile.TemporaryDirectory(
        prefix="echo-whisper-promotion-probe-"
    ) as probe_cache:
        probe_cache_path = Path(probe_cache) / "mesa-cache"
        shutil.copytree(package / cache_relative, probe_cache_path)
        verify_runtime_probe(
            runtime_probe,
            runtime_dir,
            icd_manifest,
            probe_cache_path,
            receipt,
        )
    verify_cache_snapshot(package / cache_relative, expected_cache_files)
    verify_contained_symlinks(package)
    cache_sha = tree_sha256(package / cache_relative)
    gates = {name: True for name in (*RESEARCH_GATES, *BINDING_GATES, *RESOURCE_GATES)}
    accepted_at = parse_timestamp(sweep["completedAt"])
    model_record = {
        "identity": identity,
        "identityKey": key,
        "evidenceSha256": sha256_file(sweep_path / "sweep.json"),
        "icdManifestPath": str(icd_manifest),
        "icdLibraryPath": str(icd_library),
        "cacheSeed": {"relativePath": cache_relative, "sha256": cache_sha},
        "gates": gates,
        "verdict": "PASSED",
        "acceptedAt": accepted_at,
        "expiresAt": accepted_at + args.expires_days * 24 * 60 * 60,
    }
    if args.inference_contract_v3 is not None:
        phase2 = cell.get("phase2")
        if not isinstance(phase2, dict):
            raise ValueError("selected cell has no phase2 evidence")
        write_v3_promotion(
            output=output,
            package=package,
            runtime_cli=package / "runtime/whisper-cli",
            runtime_probe=package / "runtime/echo-whisper-runtime-probe",
            model=model,
            vad=vad,
            tuning=tuning,
            receipt=receipt,
            drm_driver=drm[0]["driver"],
            icd_manifest=icd_manifest,
            icd_library=icd_library,
            inference_contract_path=args.inference_contract_v3.resolve(),
            cache_source=cache_source,
            cache_sha256=cache_sha,
            sweep_path=sweep_path,
            corpus=args.corpus.resolve(),
            cycle_path=cycle_path,
            phase2=phase2,
            accepted_at=accepted_at,
            expires_at=model_record["expiresAt"],
        )
    admission_set = {
        "schemaVersion": 2,
        "shared": {
            "runtimeRelativePath": "runtime/whisper-cli",
            "runtimeLibraryBindings": runtime_library_bindings(
                package / "runtime/whisper-cli"
            ),
            "probeRelativePath": "runtime/echo-whisper-runtime-probe",
            "probeSha256": sha256_file(package / "runtime/echo-whisper-runtime-probe"),
        },
        "records": [model_record],
        "inventory": package_inventory(package),
    }
    (package / "admission-set.json").write_text(
        json.dumps(admission_set, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    verify_admission_set(package)
    config = {
        "bundle": {"resources": {str(package.resolve()) + "/": "whisper-acceleration/"}}
    }
    (output / "tauri-acceleration.conf.json").write_text(
        json.dumps(config, indent=2) + "\n", encoding="utf-8"
    )
    promotion = {
        "schemaVersion": 2,
        "echoCommit": actual_commit,
        "echoBinarySha256": echo_sha,
        "packageType": args.package_type,
        "admissionIdentityKeys": [key],
        "admissionSetSha256": sha256_file(package / "admission-set.json"),
        "runtimeIdentitySha256": runtime_sha,
        "cacheSeedSha256ByIdentityKey": {key: cache_sha},
    }
    (output / "promotion.json").write_text(
        json.dumps(promotion, indent=2) + "\n", encoding="utf-8"
    )


def self_test() -> None:
    resource_cell = {
        "resourceEvidence": {
            "verdict": "VERIFIED",
            "minimumAvailableMemoryBytes": 4096,
            "maximumSustainedSwapGrowthBytes": 64,
        }
    }
    assert persisted_resource_thresholds(resource_cell) == (4096, 64)
    for field, value in (
        ("verdict", "INCONCLUSIVE"),
        ("minimumAvailableMemoryBytes", True),
    ):
        changed = json.loads(json.dumps(resource_cell))
        changed["resourceEvidence"][field] = value
        try:
            persisted_resource_thresholds(changed)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid persisted resource evidence passed promotion")
    green_cell = {
        "cell": {"label": "qualified"},
        "decision": "PROCEED",
        "researchPass": True,
        "researchGates": {name: True for name in (*RESEARCH_GATES, *RESOURCE_GATES)},
        "bindingGates": {name: True for name in BINDING_GATES},
    }
    assert selected_cell({"cells": [green_cell]}, "qualified") is green_cell
    failed_resource = json.loads(json.dumps(green_cell))
    failed_resource["researchGates"]["memoryFloor"] = False
    try:
        selected_cell({"cells": [failed_resource]}, "qualified")
    except ValueError:
        pass
    else:
        raise AssertionError("failed resource gate passed promotion")
    with tempfile.TemporaryDirectory() as temporary:
        from unittest.mock import patch

        root = Path(temporary)
        bundle = root / "bundle"
        bundle.mkdir()
        corpus = root / "corpus.json"
        corpus.write_text("{}", encoding="utf-8")
        phase2 = {
            field: None
            for field in (
                "decision",
                "gates",
                "expectedBackend",
                "cpuCandidate",
                "acceleratedCandidate",
                "runsPerCandidate",
                "cpuMedianOuterMs",
                "acceleratedMedianOuterMs",
                "medianReductionMs",
                "medianSpeedupPercent",
                "cpuP95OuterMs",
                "acceleratedP95OuterMs",
                "newHallucinations",
                "languages",
            )
        }
        phase2["resourceEvidence"] = resource_cell["resourceEvidence"]
        replay_cell = {
            **resource_cell,
            "evidence": {"bundle": str(bundle)},
            "candidates": {"cpu": "cpu", "accelerated": "gpu"},
            "phase2": phase2,
        }

        def replay_run(command: list[str], **_: object) -> subprocess.CompletedProcess:
            output = Path(command[command.index("--output-dir") + 1])
            output.mkdir(parents=True)
            (output / "decision.json").write_text(json.dumps(phase2), encoding="utf-8")
            return subprocess.CompletedProcess(command, 0)

        with patch.object(subprocess, "run", side_effect=replay_run) as replayed:
            replay_analysis(replay_cell, corpus, root / "replay")
        replay_command = replayed.call_args.args[0]
        assert (
            replay_command[replay_command.index("--minimum-available-memory-bytes") + 1]
            == "4096"
        )
        assert (
            replay_command[
                replay_command.index("--maximum-sustained-swap-growth-bytes") + 1
            ]
            == "64"
        )
        assert "--require-resource-evidence" in replay_command
        tuning_cell = {
            "cell": {"threads": 4, "beamSize": 1, "bestOf": 2, "noFallback": True},
            "candidates": {"cpu": "cpu", "accelerated": "gpu"},
            "evidence": {"bundle": str(bundle)},
        }
        (bundle / "run-manifest.json").write_text(
            json.dumps(
                {
                    "candidates": [
                        {
                            "label": "cpu",
                            "threads": 4,
                            "beamSize": 1,
                            "bestOf": 2,
                            "noFallback": True,
                        },
                        {
                            "label": "gpu",
                            "threads": 4,
                            "beamSize": 1,
                            "bestOf": 2,
                            "noFallback": True,
                        },
                    ]
                }
            ),
            encoding="utf-8",
        )
        assert admitted_tuning(tuning_cell)["noFallback"] is True
        candidate_drift = json.loads(json.dumps(tuning_cell))
        manifest = json.loads(
            (bundle / "run-manifest.json").read_text(encoding="utf-8")
        )
        manifest["candidates"][1]["threads"] = 5
        (bundle / "run-manifest.json").write_text(
            json.dumps(manifest), encoding="utf-8"
        )
        try:
            admitted_tuning(candidate_drift)
        except ValueError:
            pass
        else:
            raise AssertionError("candidate tuning drift passed promotion")
        manifest["candidates"][1]["threads"] = 4
        (bundle / "run-manifest.json").write_text(
            json.dumps(manifest), encoding="utf-8"
        )
        invalid_tuning = json.loads(json.dumps(tuning_cell))
        invalid_tuning["cell"]["noFallback"] = 1
        try:
            admitted_tuning(invalid_tuning)
        except ValueError:
            pass
        else:
            raise AssertionError("non-boolean fallback passed admitted tuning")
        expected_probe = admitted_cache_probe(admitted_tuning(tuning_cell))
        changed_probe = dict(expected_probe)
        changed_probe["bestOf"] = 3
        assert changed_probe != expected_probe
        cache = root / "cache"
        cache.mkdir()
        (cache / "seed").write_bytes(b"seed")
        expected_cache = [
            {"path": "seed", "bytes": 4, "sha256": sha256_file(cache / "seed")}
        ]
        verify_cache_snapshot(cache, expected_cache)
        (cache / "seed").write_bytes(b"changed")
        try:
            verify_cache_snapshot(cache, expected_cache)
        except ValueError:
            pass
        else:
            raise AssertionError("cache drift passed populated snapshot binding")
        (root / "b").mkdir()
        (root / "b/two").write_bytes(b"two")
        (root / "one").write_bytes(b"one")
        first = tree_sha256(root)
        assert first == tree_sha256(root)
        (root / "one").write_bytes(b"changed")
        assert first != tree_sha256(root)
        runtime = root / "runtime"
        runtime.mkdir()
        cli = runtime / "whisper-cli"
        cli.write_bytes(b"cli")
        (runtime / "libwhisper.so.1.9.2").write_bytes(b"whisper")
        alias = runtime / "libwhisper.so.1"
        alias.write_bytes(b"whisper")
        (runtime / "libggml.so.0.18.1").write_bytes(b"ggml")
        bindings = runtime_library_bindings(cli)
        verify_runtime_alias_bindings(bindings, cli)
        alias.write_bytes(b"ggml")
        try:
            verify_runtime_alias_bindings(bindings, cli)
        except ValueError:
            pass
        else:
            raise AssertionError("changed runtime alias passed qualification binding")
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        output = root / "promotion"
        package = output / "whisper-acceleration"
        runtime = package / "runtime"
        runtime.mkdir(parents=True)
        cli = runtime / "whisper-cli"
        cli.write_bytes(b"cli")
        (runtime / "libwhisper.so").write_bytes(b"library")
        probe = runtime / "echo-whisper-runtime-probe"
        probe.write_bytes(b"probe")
        (runtime / "build-receipt.json").write_text(
            json.dumps({"artifactId": "4" * 64}), encoding="utf-8"
        )
        model = root / "model.bin"
        model.write_bytes(b"model")
        vad = root / "vad.bin"
        vad.write_bytes(b"vad")
        tuning = {"threads": 4, "beamSize": 1, "bestOf": 2, "noFallback": True}
        contract = {
            "schemaVersion": 3,
            "protocol": "oneShotCli",
            "modelSha256": sha256_file(model),
            "vadSha256": sha256_file(vad),
            "tuning": tuning,
            "requestPolicy": {
                "language": "pinned",
                "prompt": "empty",
                "hints": "qualifiedOnly",
            },
            "behavior": {
                "launchSchema": 1,
                "receiptSchema": 1,
                "telemetrySchema": 1,
                "recoverySchema": 1,
                "projectionSha256": "9" * 64,
            },
            "claimScope": "product-stt-corpus-v1",
        }
        contract_path = root / "inference-contract.v3.json"
        contract_path.write_text(json.dumps(contract), encoding="utf-8")
        cache = root / "cache"
        cache.mkdir()
        (cache / "seed").write_bytes(b"seed")
        sweep = root / "sweep"
        sweep.mkdir()
        (sweep / "sweep.json").write_text("{}", encoding="utf-8")
        corpus = root / "corpus.json"
        corpus.write_text("{}", encoding="utf-8")
        cycle = root / "cycle"
        cycle.mkdir()
        (cycle / "cache-cycle.json").write_text("{}", encoding="utf-8")
        icd_manifest = root / "intel_icd.json"
        icd_manifest.write_text("{}", encoding="utf-8")
        icd_library = root / "libvulkan_intel.so"
        icd_library.write_bytes(b"driver")
        receipt = {
            "backend": "vulkan",
            "vendorId": 32902,
            "deviceId": 18086,
            "apiVersion": 4211006,
            "driverVersion": 104865800,
            "deviceUUID": "8680a6460c0000000002000000000000",
            "driverUUID": "ee99561e45e1e718c6121d36d8345582",
            "pipelineCacheUUID": "35e9eb9761bf7afc9291ffc449ddf849",
        }
        result = write_v3_promotion(
            output=output,
            package=package,
            runtime_cli=cli,
            runtime_probe=probe,
            model=model,
            vad=vad,
            tuning=tuning,
            receipt=receipt,
            drm_driver="i915",
            icd_manifest=icd_manifest,
            icd_library=icd_library,
            inference_contract_path=contract_path,
            cache_source=cache,
            cache_sha256=tree_sha256(cache),
            sweep_path=sweep,
            corpus=corpus,
            cycle_path=cycle,
            phase2={"claimBoundary": "small", "languages": ["en"]},
            accepted_at=1000,
            expires_at=2000,
        )
        assert result["executionArtifactId"]
        assert result["inferenceContractIds"] == [inference_contract_id(contract)]
        assert (package / "acceleration-set.v3.json").is_file()
        assert (output / "promotion-v3.json").is_file()
    sample = {
        "schemaVersion": 1,
        "echoCommit": "4" * 40,
        "echoBinarySha256": "a" * 64,
        "runtimeIdentitySha256": "b" * 64,
        "modelSha256": "c" * 64,
        "vadSha256": "d" * 64,
        "protocol": "oneShotCli",
        "tuning": {"threads": 4, "beamSize": 3, "bestOf": 5, "noFallback": False},
        "languagePolicy": "pinned",
        "promptPolicy": "empty",
        "device": {
            "backend": "vulkan",
            "selectedIndex": 0,
            "vendorId": 32902,
            "deviceId": 18086,
            "apiVersion": 4211006,
            "driverVersion": 104865800,
            "deviceUUID": "8680a6460c0000000002000000000000",
            "driverUUID": "ee99561e45e1e718c6121d36d8345582",
            "pipelineCacheUUID": "35e9eb9761bf7afc9291ffc449ddf849",
        },
        "drmDriver": "i915",
        "icdManifestSha256": "e" * 64,
        "icdLibrarySha256": "f" * 64,
        "launchContractSchema": 1,
    }
    assert (
        identity_key(sample)
        == "1aafa0c27dc5c344c14f2c43685ed182b4650469ffed13d6bbfbc7663fffd360"
    )
    print("promote-whisper-admission: self-test passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Promote a confirmed Whisper sweep into a package admission"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--sweep", type=Path)
    parser.add_argument("--cell")
    parser.add_argument("--cache-cycle", type=Path)
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--runtime-dir", type=Path)
    parser.add_argument("--runtime-probe", type=Path)
    parser.add_argument("--echo-binary", type=Path)
    parser.add_argument("--model", type=Path)
    parser.add_argument("--vad", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--expires-days", type=int, default=30)
    parser.add_argument("--package-type", choices=("deb", "rpm"))
    parser.add_argument("--inference-contract-v3", type=Path)
    args = parser.parse_args()
    if not args.self_test and any(
        getattr(args, name) is None
        for name in (
            "sweep",
            "cell",
            "cache_cycle",
            "corpus",
            "runtime_dir",
            "runtime_probe",
            "echo_binary",
            "model",
            "vad",
            "output",
            "package_type",
        )
    ):
        parser.error("promotion requires every path and --cell")
    if not 1 <= args.expires_days <= 30:
        parser.error("--expires-days must be between 1 and 30")
    return args


def main() -> int:
    args = parse_args()
    try:
        if args.self_test:
            self_test()
        else:
            promote(args)
    except (
        KeyError,
        OSError,
        TypeError,
        ValueError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"promote-whisper-admission: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
