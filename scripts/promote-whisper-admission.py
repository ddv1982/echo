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

from whisper_release_common import (
    runtime_identity,
    runtime_library_bindings,
    sha256_file,
    tree_sha256,
    verify_contained_symlinks,
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
RECEIPT_PREFIX = "echo_whisper_runtime_receipt: "


def read_json(path: Path, label: str) -> dict[str, object]:
    def reject_duplicates(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"{label} has duplicate key {key!r}")
            result[key] = value
        return result

    value = json.loads(
        path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates
    )
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    return value


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
    require_green(cell.get("researchGates"), RESEARCH_GATES, "research gates")
    require_green(cell.get("bindingGates"), BINDING_GATES, "binding gates")
    return cell


def replay_analysis(cell: dict[str, object], corpus: Path, scratch: Path) -> None:
    evidence = cell.get("evidence")
    candidates = cell.get("candidates")
    if not isinstance(evidence, dict) or not isinstance(candidates, dict):
        raise ValueError("cell evidence and candidates are required")
    bundle = require_path(evidence.get("bundle"), "cell bundle")
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
    ):
        if replay.get(field) != phase2.get(field):
            raise ValueError(f"replayed analysis changed {field}")


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
    verify_runtime_probe(
        runtime_probe,
        runtime_dir,
        icd_manifest,
        cycle_path / "mesa-cache",
        receipt,
    )
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
    shutil.copytree(runtime_dir, package / "runtime", symlinks=True)
    verify_runtime_alias_bindings(
        qualified_runtime_bindings, package / "runtime/whisper-cli"
    )
    shutil.copy2(runtime_probe, package / "runtime/echo-whisper-runtime-probe")
    cache_source = cycle_path / "mesa-cache"
    shutil.copytree(cache_source, package / "cache-seed")
    verify_contained_symlinks(package)
    cache_sha = tree_sha256(package / "cache-seed")
    tuning = {
        "threads": int(cell_config["threads"]),
        "beamSize": int(cell_config["beamSize"]),
        "bestOf": int(cell_config["bestOf"]),
        "noFallback": bool(cell_config["noFallback"]),
    }
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
    gates = {name: True for name in (*RESEARCH_GATES, *BINDING_GATES)}
    accepted_at = parse_timestamp(sweep["completedAt"])
    record = {
        "schemaVersion": 1,
        "identity": identity,
        "artifacts": {
            "runtimeRelativePath": "runtime/whisper-cli",
            "runtimeLibraryBindings": runtime_library_bindings(
                package / "runtime/whisper-cli"
            ),
            "probeRelativePath": "runtime/echo-whisper-runtime-probe",
            "probeSha256": sha256_file(runtime_probe),
            "icdManifestPath": str(icd_manifest),
            "icdLibraryPath": str(icd_library),
            "cacheSeedRelativePath": "cache-seed",
            "cacheSeedSha256": cache_sha,
        },
        "identityKey": identity_key(identity),
        "evidenceSha256": sha256_file(sweep_path / "sweep.json"),
        "gates": gates,
        "verdict": "PASSED",
        "acceptedAt": accepted_at,
        "expiresAt": accepted_at + args.expires_days * 24 * 60 * 60,
    }
    (package / "admission.json").write_text(
        json.dumps(record, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    config = {
        "bundle": {"resources": {str(package.resolve()) + "/": "whisper-acceleration/"}}
    }
    (output / "tauri-acceleration.conf.json").write_text(
        json.dumps(config, indent=2) + "\n", encoding="utf-8"
    )
    promotion = {
        "schemaVersion": 1,
        "echoCommit": actual_commit,
        "echoBinarySha256": echo_sha,
        "admissionIdentityKey": record["identityKey"],
        "admissionSha256": sha256_file(package / "admission.json"),
        "runtimeIdentitySha256": runtime_sha,
        "cacheSeedSha256": cache_sha,
    }
    (output / "promotion.json").write_text(
        json.dumps(promotion, indent=2) + "\n", encoding="utf-8"
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
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
