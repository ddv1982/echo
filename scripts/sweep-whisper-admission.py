#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import uuid
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


REPO_ROOT = Path(__file__).resolve().parent.parent
SUPPORTED_REVISIONS = frozenset({"v1.9.2", "v1.9.3"})
PRE_RELEASE_REVISIONS = frozenset({"v1.9.3"})
RECEIPT_PREFIX = "echo_whisper_runtime_receipt: "
ENVIRONMENT_RESET_PREFIXES = (
    "LD_",
    "VK_",
    "MESA_",
    "DRI_",
    "LIBGL_",
    "GALLIUM_",
    "INTEL_",
    "AMD_",
    "RADV_",
    "NVIDIA_",
    "__GL",
    "CUDA_",
    "ROCR_",
    "HIP_",
    "HSA_",
    "ONEAPI_",
    "SYCL_",
    "ZES_",
    "ZE_",
    "OPENCL_",
    "OCL_",
    "RUSTICL_",
    "GGML_",
    "OMP_",
    "OPENBLAS_",
    "LIBVA_",
)
RESEARCH_GATE_NAMES = (
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
SCREEN_GATE_NAMES = tuple(name for name in RESEARCH_GATE_NAMES if name != "sampleSize")
BINDING_GATE_NAMES = (
    "coverageComplete",
    "cacheEvidence",
    "resetEvidence",
    "driverIcdIdentity",
    "cleanChildEnvironment",
    "exactRuntime",
)
LABEL_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}")


@dataclass(frozen=True)
class Runtime:
    label: str
    revision: str
    root: Path
    cli: Path
    sha256: str


@dataclass(frozen=True)
class Cache:
    label: str
    cycle_root: Path
    cache_root: Path
    cycle: dict[str, object]
    host: dict[str, object]


@dataclass(frozen=True)
class Cell:
    label: str
    runtime_label: str
    cache_label: str
    beam_size: int
    best_of: int
    no_fallback: bool


def now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_echo_boundary(
    binary: Path,
    expected_commit: str,
    expected_binary_sha256: str,
    *,
    repo_root: Path = REPO_ROOT,
    include_untracked: bool = True,
) -> dict[str, str]:
    if re.fullmatch(r"[0-9a-f]{40}", expected_commit) is None:
        raise ValueError("expected Echo commit must be a full hexadecimal commit")
    if SHA256_PATTERN.fullmatch(expected_binary_sha256) is None:
        raise ValueError("expected Echo binary SHA-256 must be hexadecimal")
    actual_commit = subprocess.run(
        ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    status_command = ["git", "-C", str(repo_root), "status", "--porcelain"]
    if not include_untracked:
        status_command.append("--untracked-files=no")
    dirty = subprocess.run(
        status_command,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if dirty:
        raise ValueError("dirty Echo checkout cannot satisfy admission")
    if actual_commit != expected_commit:
        raise ValueError("current Echo commit does not match --expected-echo-commit")
    actual_binary_sha256 = sha256(binary)
    if actual_binary_sha256 != expected_binary_sha256:
        raise ValueError("Echo binary does not match --expected-echo-binary-sha256")
    return {
        "echoCommit": actual_commit,
        "echoBinarySha256": actual_binary_sha256,
    }


def product_runtime_identity(cli: Path) -> str:
    libraries: set[Path] = set()
    for path in cli.parent.iterdir():
        if ".so" not in path.name:
            continue
        try:
            resolved = path.resolve(strict=True)
        except OSError:
            continue
        if resolved.is_file():
            libraries.add(resolved)
    digest = hashlib.sha256(b"echo-whisper-runtime-v1\0")
    for path in [cli, *sorted(libraries)]:
        name = path.name.encode()
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(path.stat().st_size.to_bytes(8, "little"))
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def canonical_json(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def write_atomic(path: Path, value: str) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{uuid.uuid4().hex}.tmp")
    try:
        temporary.write_text(value, encoding="utf-8")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_json(path: Path, value: object) -> None:
    write_atomic(path, json.dumps(value, indent=2, sort_keys=True) + "\n")


def read_json(path: Path, label: str) -> dict[str, object]:
    def reject_duplicates(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"{label} has duplicate key {key!r}")
            result[key] = value
        return result

    try:
        value = json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"could not read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be a JSON object")
    return value


def recorded_path(value: object, label: str) -> Path:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label} must be a non-empty path")
    if value == "$REPO" or value.startswith("$REPO/"):
        return REPO_ROOT / value.removeprefix("$REPO/")
    if value == "$HOME" or value.startswith("$HOME/"):
        return Path.home() / value.removeprefix("$HOME/")
    return Path(value)


def artifact_path(root: Path, reference: object, label: str) -> Path:
    if not isinstance(reference, dict):
        raise ValueError(f"{label} must be an artifact object")
    raw_path = reference.get("path")
    digest = reference.get("sha256")
    if (
        not isinstance(raw_path, str)
        or not isinstance(digest, str)
        or not SHA256_PATTERN.fullmatch(digest)
    ):
        raise ValueError(f"{label} is missing a safe artifact identity")
    relative = PurePosixPath(raw_path)
    if (
        relative.is_absolute()
        or raw_path != relative.as_posix()
        or not relative.parts
        or any(part in {"", ".", ".."} for part in relative.parts)
    ):
        raise ValueError(f"{label} has an unsafe artifact path")
    try:
        path = root.joinpath(*relative.parts).resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{label} is missing") from error
    if (
        not path.is_file()
        or root.resolve() not in path.parents
        or sha256(path) != digest
    ):
        raise ValueError(f"{label} digest does not match")
    return path


def verify_external_artifact(reference: object, label: str) -> Path:
    if not isinstance(reference, dict):
        raise ValueError(f"{label} must be an artifact object")
    path = recorded_path(reference.get("path"), f"{label}.path").resolve()
    size = reference.get("bytes")
    digest = reference.get("sha256")
    if (
        not path.is_file()
        or isinstance(size, bool)
        or not isinstance(size, int)
        or size < 0
        or path.stat().st_size != size
        or not isinstance(digest, str)
        or SHA256_PATTERN.fullmatch(digest) is None
        or sha256(path) != digest
    ):
        raise ValueError(f"{label} does not match the current host artifact")
    return path


def require_label(value: str, label: str) -> str:
    if LABEL_PATTERN.fullmatch(value) is None:
        raise ValueError(f"{label} must use letters, digits, '.', '_' or '-'")
    return value


def parse_assignment(raw: str, option: str) -> tuple[str, str]:
    label, separator, value = raw.partition("=")
    if not separator or not value:
        raise ValueError(f"{option} must be LABEL=VALUE")
    return require_label(label, f"{option} label"), value


def parse_cell(raw: str) -> Cell:
    label, separator, definition = raw.partition("=")
    if not separator or not definition:
        raise ValueError(
            "--cell must be LABEL=runtime=R,cache=C,beam=N,best-of=N,fallback=allow|none"
        )
    values: dict[str, str] = {}
    for entry in definition.split(","):
        key, field_separator, value = entry.partition("=")
        if not field_separator or key in values:
            raise ValueError(f"invalid --cell definition: {raw}")
        values[key] = value
    expected = {"runtime", "cache", "beam", "best-of", "fallback"}
    if set(values) != expected:
        raise ValueError(
            f"--cell {label} must set exactly: {', '.join(sorted(expected))}"
        )
    try:
        beam_size = int(values["beam"])
        best_of = int(values["best-of"])
    except ValueError as error:
        raise ValueError(f"--cell {label} beam and best-of must be integers") from error
    if beam_size < 1 or best_of < 1:
        raise ValueError(f"--cell {label} beam and best-of must be positive")
    fallback = values["fallback"]
    if fallback not in {"allow", "none"}:
        raise ValueError(f"--cell {label} fallback must be allow or none")
    return Cell(
        label=require_label(label, "--cell label"),
        runtime_label=require_label(values["runtime"], f"--cell {label} runtime"),
        cache_label=require_label(values["cache"], f"--cell {label} cache"),
        beam_size=beam_size,
        best_of=best_of,
        no_fallback=fallback == "none",
    )


def cache_snapshot(root: Path) -> dict[str, object]:
    if not root.is_dir():
        raise ValueError(f"Mesa cache root is not a directory: {root}")
    files: list[dict[str, object]] = []
    for directory, directories, names in os.walk(root, followlinks=False):
        current = Path(directory)
        directories.sort()
        for child in directories:
            if (current / child).is_symlink():
                raise ValueError(
                    f"Mesa cache has a symlinked directory: {current / child}"
                )
        for name in sorted(names):
            path = current / name
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"Mesa cache has a non-regular file: {path}")
            files.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "bytes": path.stat().st_size,
                    "sha256": sha256(path),
                }
            )
    files.sort(key=lambda item: str(item["path"]))
    return {
        "schemaVersion": 1,
        "capturedAt": now(),
        "root": str(root.resolve()),
        "files": files,
    }


def child_environment(
    runtime: Runtime,
    model_dir: Path,
    cache_root: Path,
    vk_driver_files: Path,
    home: Path,
) -> dict[str, str]:
    # Deliberately build a small environment instead of inheriting loader or cache state.
    environment = {
        "ECHO_MODEL_DIR": str(model_dir),
        "HOME": str(home),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "PATH": str(runtime.root) + os.pathsep + os.environ.get("PATH", os.defpath),
        "TZ": "UTC",
    }
    cache_root.resolve(strict=True)
    vk_driver_files.resolve(strict=True)
    return environment


def candidate_label(model_name: str, threads: int, cell: Cell, force_cpu: bool) -> str:
    values = [f"threads={threads}", f"beam={cell.beam_size}", f"best-of={cell.best_of}"]
    if cell.no_fallback:
        values.append("no-fallback")
    if force_cpu:
        values.append("cpu-only")
    return f"whisper:{model_name}@{','.join(values)}"


def run_command(
    command: list[str], environment: dict[str, str], output_dir: Path, name: str
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        command, check=False, capture_output=True, text=True, env=environment
    )
    write_atomic(
        output_dir / f"{name}.command.json", json.dumps(command, indent=2) + "\n"
    )
    write_atomic(output_dir / f"{name}.stdout.txt", completed.stdout)
    write_atomic(output_dir / f"{name}.stderr.txt", completed.stderr)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"{name} failed with exit {completed.returncode}: {detail}")
    return completed


def read_fixture_audio(manifest_path: Path) -> Path:
    manifest = read_json(manifest_path, "fixture manifest")
    utterances = manifest.get("utterances")
    if (
        not isinstance(utterances, list)
        or not utterances
        or not isinstance(utterances[0], dict)
    ):
        raise ValueError("fixture manifest must contain at least one utterance")
    raw = utterances[0].get("file")
    if not isinstance(raw, str) or not raw:
        raise ValueError("first fixture has no file")
    path = (manifest_path.parent / raw).resolve()
    if not path.is_file():
        raise ValueError(f"first fixture audio is missing: {path}")
    return path


def create_model_dir(cell_root: Path, model_path: Path, vad_path: Path) -> Path:
    model_dir = cell_root / "model-cache"
    model_dir.mkdir()
    (model_dir / model_path.name).symlink_to(model_path)
    (model_dir / vad_path.name).symlink_to(vad_path)
    return model_dir


def exact_runtime_gate(
    bundle_root: Path,
    expected_runtime: Runtime,
    expected_model: Path,
    expected_vad: Path,
    expected_environment: dict[str, str],
    expected_driver: Path,
    expected_cache: Path,
    cpu_label: str,
    accelerated_label: str,
    cell: Cell,
    threads: int,
    repeats: int,
    fixture_count: int,
) -> tuple[bool, str | None]:
    try:
        expected_model_sha256 = sha256(expected_model)
        expected_vad_sha256 = sha256(expected_vad)
        status = read_json(bundle_root / "status.json", "benchmark status")
        manifest = read_json(bundle_root / "run-manifest.json", "benchmark manifest")
        if status.get("state") != "complete" or status.get("runId") != manifest.get(
            "runId"
        ):
            return False, "benchmark bundle is not complete"
        candidates = manifest.get("candidates")
        if not isinstance(candidates, list) or len(candidates) != 2:
            return False, "benchmark must contain exactly two candidates"
        by_label = {
            candidate.get("label"): candidate
            for candidate in candidates
            if isinstance(candidate, dict)
        }
        if set(by_label) != {cpu_label, accelerated_label}:
            return False, "benchmark candidates do not match this cell"
        for label, force_cpu in ((cpu_label, True), (accelerated_label, False)):
            candidate = by_label[label]
            if (
                candidate.get("threads") != threads
                or candidate.get("beamSize") != cell.beam_size
                or candidate.get("bestOf") != cell.best_of
                or candidate.get("noFallback") is not cell.no_fallback
                or candidate.get("forceCpu") is not force_cpu
            ):
                return False, "CPU and accelerated decoding settings differ"
        rows = [
            json.loads(line)
            for line in (bundle_root / "runs.jsonl")
            .read_text(encoding="utf-8")
            .splitlines()
        ]
        if len(rows) != 2 * repeats * fixture_count:
            return False, "benchmark does not have exact measurement cardinality"
        for row in rows:
            if not isinstance(row, dict) or row.get("candidate") not in {
                cpu_label,
                accelerated_label,
            }:
                return False, "benchmark contains an unexpected candidate"
            runtime = row.get("runtimeArtifact")
            model = row.get("modelArtifact")
            vad = row.get("vadArtifact")
            if (
                not isinstance(runtime, dict)
                or runtime.get("sha256") != expected_runtime.sha256
            ):
                return False, "child runtime hash differs from selected receipt runtime"
            if (
                recorded_path(runtime.get("path"), "child runtime path").resolve()
                != expected_runtime.cli.resolve()
            ):
                return False, "child runtime path differs from selected receipt runtime"
            if (
                not isinstance(model, dict)
                or model.get("sha256") != expected_model_sha256
            ):
                return False, "child model hash differs from selected model"
            if (
                recorded_path(model.get("path"), "child model path").resolve()
                != expected_model.resolve()
            ):
                return False, "child model path differs from selected model"
            if not isinstance(vad, dict) or vad.get("sha256") != expected_vad_sha256:
                return False, "child VAD hash differs from selected VAD"
            if (
                recorded_path(vad.get("path"), "child VAD path").resolve()
                != expected_vad.resolve()
            ):
                return False, "child VAD path differs from selected VAD"
            engine = row.get("engine")
            if not isinstance(engine, dict) or engine.get("vad") is not True:
                return False, "measurement did not keep VAD active"
            whisper = row.get("whisper")
            telemetry = whisper.get("runtime") if isinstance(whisper, dict) else None
            if not isinstance(telemetry, dict):
                return False, "measurement has no effective Whisper launch telemetry"
            if telemetry.get("identitySha256") != product_runtime_identity(
                expected_runtime.cli
            ):
                return (
                    False,
                    "effective child runtime identity differs from selected runtime",
                )
            launch_paths = {
                "libraryPath": expected_runtime.root,
                "vulkanDriverFiles": expected_driver,
                "mesaShaderCacheDir": expected_cache,
            }
            for name, expected in launch_paths.items():
                if (
                    recorded_path(telemetry.get(name), name).resolve()
                    != expected.resolve()
                ):
                    return (
                        False,
                        f"effective child {name} differs from the launch contract",
                    )
            artifact = row.get("observationArtifact")
            if not isinstance(artifact, dict):
                return False, "measurement row has no raw observation artifact"
            environment_path = artifact_path(
                bundle_root, artifact.get("environment"), "observation environment"
            )
            environment = read_json(environment_path, "observation environment")
            for name in (
                "PATH",
                "ECHO_MODEL_DIR",
            ):
                if environment.get(name) != expected_environment[name]:
                    return (
                        False,
                        f"benchmark parent environment has a stale or ambient {name}",
                    )
            if any(name.startswith(ENVIRONMENT_RESET_PREFIXES) for name in environment):
                return (
                    False,
                    "benchmark parent contains an inherited loader or cache variable",
                )
    except (OSError, ValueError, TypeError, json.JSONDecodeError) as error:
        return False, str(error)
    return True, None


def probe_receipt_consistency(
    probe_root: Path, expected_receipt: object
) -> tuple[bool, str | None, dict[str, object] | None]:
    try:
        rows = [
            json.loads(line)
            for line in (probe_root / "runs.jsonl")
            .read_text(encoding="utf-8")
            .splitlines()
        ]
        if len(rows) != 2:
            return False, "receipt probe did not run one CPU and Vulkan pair", None
        by_candidate = {
            row.get("candidate"): row for row in rows if isinstance(row, dict)
        }
        if set(by_candidate) != {"cpu", "accelerated"}:
            return False, "receipt probe candidates are incomplete", None
        cpu = by_candidate["cpu"]
        accelerated = by_candidate["accelerated"]
        if (
            cpu.get("resolvedBackend") != "cpu"
            or cpu.get("runtimeReceipt") is not None
            or cpu.get("runtimeReceiptError") is not None
        ):
            return False, "CPU receipt control is not receipt-free CPU", None
        receipt = accelerated.get("runtimeReceipt")
        stderr = accelerated.get("rawStderr")
        if (
            accelerated.get("resolvedBackend") != "vulkan"
            or accelerated.get("runtimeReceiptError") is not None
            or not isinstance(receipt, dict)
            or not isinstance(stderr, str)
            or sum(line.startswith(RECEIPT_PREFIX) for line in stderr.splitlines()) != 1
        ):
            return False, "accelerated probe did not retain one Vulkan receipt", None
        if receipt != expected_receipt:
            return False, "receipt differs from populated-cache evidence", receipt
    except (OSError, ValueError, TypeError, json.JSONDecodeError) as error:
        return False, str(error), None
    return True, None, receipt


def cache_binding(
    runtime: Runtime,
    cache: Cache,
    model_path: Path,
    vad_path: Path,
    vk_driver_files: Path,
) -> tuple[bool, bool, bool, str | None, object]:
    try:
        identity = cache.cycle.get("identity")
        identity_value = identity.get("value") if isinstance(identity, dict) else None
        if not isinstance(identity_value, dict):
            return False, False, False, "cache cycle has no identity", None
        cycle_runtime = identity_value.get("runtime")
        cycle_model = identity_value.get("model")
        cycle_vad = identity_value.get("vad")
        host = identity_value.get("host")
        if (
            not isinstance(cycle_runtime, dict)
            or not isinstance(cycle_model, dict)
            or not isinstance(cycle_vad, dict)
            or not isinstance(host, dict)
        ):
            return False, False, False, "cache cycle identity is incomplete", None
        selected_manifest = host.get("selectedIcdManifest")
        if not isinstance(selected_manifest, dict):
            return False, False, False, "cache cycle has no selected ICD identity", None
        matches = (
            cycle_runtime.get("sha256") == runtime.sha256
            and cycle_runtime.get("identitySha256")
            == product_runtime_identity(runtime.cli)
            and cycle_model.get("sha256") == sha256(model_path)
            and cycle_vad.get("sha256") == sha256(vad_path)
            and selected_manifest.get("sha256") == sha256(vk_driver_files)
        )
        snapshots = cache.cycle.get("cacheSnapshots")
        probes = cache.cycle.get("probes")
        if not isinstance(snapshots, dict) or not isinstance(probes, dict):
            return False, False, False, "cache cycle evidence is incomplete", None
        after = artifact_path(
            cache.cycle_root,
            snapshots.get("afterPopulated"),
            "populated cache snapshot",
        )
        snapshot = read_json(after, "populated cache snapshot")
        files = snapshot.get("files")
        receipt = (
            probes.get("populated", {}).get("receipt")
            if isinstance(probes.get("populated"), dict)
            else None
        )
        cache_nonempty = (
            isinstance(files, list) and bool(files) and cache.cache_root.is_dir()
        )
        driver_icd = (
            matches
            and cache.host.get("loader", {})
            .get("selectedIcd", {})
            .get("environmentVariable")
            == "VK_DRIVER_FILES"
            if isinstance(cache.host.get("loader"), dict)
            else False
        )
        return (
            matches and cache_nonempty,
            bool(driver_icd),
            cache.cycle.get("resetEvidence", {}).get("state") == "COMPLETE"
            if isinstance(cache.cycle.get("resetEvidence"), dict)
            else False,
            None,
            receipt,
        )
    except (OSError, ValueError, TypeError) as error:
        return False, False, False, str(error), None


def admission_decision(
    revision: str, research_gates: dict[str, bool], binding_gates: dict[str, bool]
) -> str:
    if revision in PRE_RELEASE_REVISIONS:
        return "STOP"
    if any(not research_gates[name] for name in SCREEN_GATE_NAMES):
        return "STOP"
    if not binding_gates["exactRuntime"] or not binding_gates["cleanChildEnvironment"]:
        return "STOP"
    if any(
        not binding_gates[name]
        for name in (
            "coverageComplete",
            "cacheEvidence",
            "resetEvidence",
            "driverIcdIdentity",
        )
    ):
        return "INCOMPLETE"
    if not research_gates["sampleSize"]:
        return "INCOMPLETE"
    if all(research_gates.values()) and all(binding_gates.values()):
        return "PROCEED"
    return "STOP"


def run_cell(
    args: argparse.Namespace,
    cell: Cell,
    runtime: Runtime,
    cache: Cache,
    fixture_audio: Path,
    fixture_count: int,
    output_root: Path,
) -> dict[str, object]:
    cell_root = output_root / "cells" / cell.label
    cell_root.mkdir(parents=True)
    write_json(
        cell_root / "status.json",
        {
            "schemaVersion": 1,
            "state": "running",
            "startedAt": now(),
            "cell": cell.label,
        },
    )
    try:
        model_dir = create_model_dir(cell_root, args.model_path, args.vad_path)
        home = cell_root / "home"
        home.mkdir()
        environment = child_environment(
            runtime, model_dir, cache.cache_root, args.vk_driver_files, home
        )
        accelerated_label = candidate_label(args.model_name, args.threads, cell, False)
        cpu_label = candidate_label(args.model_name, args.threads, cell, True)
        cache_ok, driver_ok, reset_ok, cache_error, expected_receipt = cache_binding(
            runtime, cache, args.model_path, args.vad_path, args.vk_driver_files
        )
        before = cache_snapshot(cache.cache_root)
        write_json(cell_root / "cache-before.json", before)
        bundle_root = cell_root / "bundle"
        benchmark_command = [
            sys.executable,
            str(REPO_ROOT / "scripts/benchmark-stt.py"),
            "--binary",
            str(args.echo_binary),
            "--manifest",
            str(args.fixture_manifest),
            "--candidate",
            accelerated_label,
            "--candidate",
            cpu_label,
            "--repeats",
            str(args.repeats),
            "--warmups",
            str(args.warmups),
            "--seed",
            str(args.seed),
            "--cache-state",
            "populated",
            "--reset-cycle",
            f"cache-{cache.label}-{cache.cycle.get('runId', 'unknown')}",
            "--driver-identity",
            sha256(args.vk_driver_files),
            "--icd-identity",
            sha256(args.vk_driver_files),
            "--output-dir",
            str(bundle_root),
            "--expected-echo-commit",
            args.expected_echo_commit,
            "--expected-echo-binary-sha256",
            args.expected_echo_binary_sha256,
            "--whisper-vulkan-driver-files",
            str(args.vk_driver_files),
            "--whisper-mesa-shader-cache-dir",
            str(cache.cache_root),
        ]
        run_command(benchmark_command, environment, cell_root, "benchmark")
        analyzer_root = cell_root / "analysis"
        analyzer_command = [
            sys.executable,
            str(REPO_ROOT / "scripts/analyze-stt-host-matrix.py"),
            "--runs",
            str(bundle_root / "runs.jsonl"),
            "--corpus-manifest",
            str(args.coverage_manifest),
            "--cpu-candidate",
            cpu_label,
            "--accelerated-candidate",
            accelerated_label,
            "--expected-backend",
            "vulkan",
            "--output-dir",
            str(analyzer_root),
        ]
        run_command(analyzer_command, environment, cell_root, "analyzer")
        analysis = read_json(
            analyzer_root / "decision.json", "recomputed Phase 2 decision"
        )
        phase2_gates = analysis.get("gates")
        if not isinstance(phase2_gates, dict):
            raise ValueError("Phase 2 decision has no gates")
        probe_root = cell_root / "receipt-probe"
        probe_command = [
            sys.executable,
            str(REPO_ROOT / "scripts/probe-whisper-acceleration.py"),
            "--binary",
            str(runtime.cli),
            "--model",
            str(args.model_path),
            "--audio",
            str(fixture_audio),
            "--backend",
            "vulkan",
            "--language",
            "en",
            "--threads",
            str(args.threads),
            "--beam-size",
            str(cell.beam_size),
            "--best-of",
            str(cell.best_of),
            "--warmups",
            "0",
            "--repeats",
            "1",
            "--seed",
            str(args.seed),
            "--timeout",
            str(args.timeout),
            "--mesa-shader-cache-dir",
            str(cache.cache_root),
            "--vk-driver-files",
            str(args.vk_driver_files),
            "--reuse-mesa-shader-cache",
            "--output-dir",
            str(probe_root),
        ]
        if cell.no_fallback:
            probe_command.append("--no-fallback")
        run_command(probe_command, environment, cell_root, "receipt-probe")
        receipt_ok, receipt_error, receipt = probe_receipt_consistency(
            probe_root, expected_receipt
        )
        exact_runtime, runtime_error = exact_runtime_gate(
            bundle_root,
            runtime,
            args.model_path,
            args.vad_path,
            environment,
            args.vk_driver_files,
            cache.cache_root,
            cpu_label,
            accelerated_label,
            cell,
            args.threads,
            args.repeats,
            fixture_count,
        )
        after = cache_snapshot(cache.cache_root)
        write_json(cell_root / "cache-after.json", after)
        cache_documented = bool(before["files"]) and bool(after["files"])
        research_gates = {
            name: bool(phase2_gates.get(name))
            for name in RESEARCH_GATE_NAMES
            if name != "receiptConsistency"
        }
        research_gates["backendTruth"] = research_gates["backendTruth"] and receipt_ok
        research_gates["hardwareDevice"] = (
            research_gates["hardwareDevice"] and receipt_ok
        )
        research_gates["receiptConsistency"] = receipt_ok
        binding_gates = {
            "coverageComplete": bool(phase2_gates.get("coverageComplete")),
            "cacheEvidence": cache_ok and cache_documented,
            "resetEvidence": reset_ok,
            "driverIcdIdentity": driver_ok,
            "cleanChildEnvironment": exact_runtime,
            "exactRuntime": exact_runtime,
        }
        decision = admission_decision(runtime.revision, research_gates, binding_gates)
        result = {
            "schemaVersion": 1,
            "cell": {
                "label": cell.label,
                "runtime": runtime.label,
                "revision": runtime.revision,
                "cache": cache.label,
                "threads": args.threads,
                "beamSize": cell.beam_size,
                "bestOf": cell.best_of,
                "noFallback": cell.no_fallback,
            },
            "identity": {
                "echo": {
                    "commit": args.expected_echo_commit,
                    "binary": {
                        "path": str(args.echo_binary),
                        "sha256": args.expected_echo_binary_sha256,
                    },
                },
                "runtime": {"path": str(runtime.cli), "sha256": runtime.sha256},
                "model": {
                    "path": str(args.model_path),
                    "sha256": sha256(args.model_path),
                },
                "vkDriverFiles": {
                    "path": str(args.vk_driver_files),
                    "sha256": sha256(args.vk_driver_files),
                },
            },
            "candidates": {"cpu": cpu_label, "accelerated": accelerated_label},
            "phase2": analysis,
            "receipt": receipt,
            "researchGates": research_gates,
            "bindingGates": binding_gates,
            "researchPass": all(research_gates[name] for name in RESEARCH_GATE_NAMES),
            "screenPass": all(research_gates[name] for name in SCREEN_GATE_NAMES),
            "decision": decision,
            "preReleaseInvestigativeOnly": runtime.revision in PRE_RELEASE_REVISIONS,
            "evidence": {
                "bundle": str(bundle_root),
                "analysis": str(analyzer_root),
                "receiptProbe": str(probe_root),
                "cacheBefore": str(cell_root / "cache-before.json"),
                "cacheAfter": str(cell_root / "cache-after.json"),
                "cacheCycle": str(cache.cycle_root),
                "cacheError": cache_error,
                "receiptError": receipt_error,
                "runtimeError": runtime_error,
            },
        }
        write_json(cell_root / "decision.json", result)
        write_json(
            cell_root / "status.json",
            {
                "schemaVersion": 1,
                "state": "complete",
                "completedAt": now(),
                "decision": decision,
            },
        )
        return result
    except Exception as error:
        write_json(
            cell_root / "status.json",
            {
                "schemaVersion": 1,
                "state": "failed",
                "failedAt": now(),
                "errorType": type(error).__name__,
                "error": str(error),
            },
        )
        raise


def render_summary(cells: list[dict[str, object]]) -> str:
    lines = [
        "# Whisper admission sweep",
        "",
        "| Cell | Revision | Decoding | Screen | Confirmed | Decision |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for result in cells:
        cell = result["cell"]
        assert isinstance(cell, dict)
        decoding = f"t{cell['threads']} / b{cell['beamSize']} / bo{cell['bestOf']} / {'no fallback' if cell['noFallback'] else 'fallback'}"
        lines.append(
            f"| {cell['label']} | {cell['revision']} | {decoding} | "
            f"{'PASS' if result['screenPass'] else 'FAIL'} | "
            f"{'PASS' if result['researchPass'] else 'FAIL'} | {result['decision']} |"
        )
    lines.extend(
        [
            "",
            "A screen pass excludes only the sample-size gate. A confirmed pass includes it.",
            "`PROCEED` is research admission only; this sweep does not select a production runtime.",
            "v1.9.3 is pre-release investigative evidence and is always `STOP` for shipping.",
            "",
        ]
    )
    return "\n".join(lines)


def parse_runtimes(raw_values: list[str], vk_driver_files: Path) -> dict[str, Runtime]:
    runtimes: dict[str, Runtime] = {}
    for raw in raw_values:
        label, raw_path = parse_assignment(raw, "--receipt-runtime")
        if label not in SUPPORTED_REVISIONS:
            raise ValueError("--receipt-runtime label must be v1.9.2 or v1.9.3")
        if label in runtimes:
            raise ValueError(f"duplicate receipt runtime label: {label}")
        root = Path(raw_path).resolve()
        cli = root / "whisper-cli"
        if not cli.is_file() or not os.access(cli, os.X_OK):
            raise ValueError(f"receipt runtime has no executable whisper-cli: {root}")
        environment = {
            "PATH": str(root),
            "LD_LIBRARY_PATH": str(root),
            "VK_DRIVER_FILES": str(vk_driver_files),
            "MESA_SHADER_CACHE_DISABLE": "true",
        }
        completed = subprocess.run(
            [str(cli), "--version"],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
        )
        version_output = completed.stdout + completed.stderr
        if (
            completed.returncode != 0
            or f"version: {label.removeprefix('v')}" not in version_output
        ):
            raise ValueError(f"receipt runtime {root} does not report pinned {label}")
        runtimes[label] = Runtime(label, label, root, cli.resolve(), sha256(cli))
    if not runtimes:
        raise ValueError("at least one --receipt-runtime is required")
    return runtimes


def parse_caches(raw_values: list[str]) -> dict[str, Cache]:
    caches: dict[str, Cache] = {}
    for raw in raw_values:
        label, raw_path = parse_assignment(raw, "--populated-mesa-cache")
        if label in caches:
            raise ValueError(f"duplicate populated cache label: {label}")
        cycle_root = Path(raw_path).resolve()
        completed = subprocess.run(
            [
                sys.executable,
                str(REPO_ROOT / "scripts/run-whisper-cache-cycle.py"),
                "--validate-cycle",
                str(cycle_root),
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            raise ValueError(
                f"Phase 4 cache cycle is invalid: {completed.stderr.strip()}"
            )
        cycle = read_json(cycle_root / "cache-cycle.json", "cache cycle")
        snapshots = cycle.get("cacheSnapshots")
        if not isinstance(snapshots, dict):
            raise ValueError("cache cycle has no snapshots")
        snapshot_path = artifact_path(
            cycle_root, snapshots.get("afterPopulated"), "populated cache snapshot"
        )
        snapshot = read_json(snapshot_path, "populated cache snapshot")
        files = snapshot.get("files")
        cache_root = recorded_path(
            snapshot.get("root"), "populated cache root"
        ).resolve()
        host_path = artifact_path(
            cycle_root, cycle.get("hostEvidence"), "cache host evidence"
        )
        if not isinstance(files, list) or not files or not cache_root.is_dir():
            raise ValueError("Phase 4 cache is not populated and available")
        current_cache = cache_snapshot(cache_root)
        if current_cache["files"] != files:
            raise ValueError(
                "populated Mesa cache no longer matches its Phase 4 snapshot"
            )
        host = read_json(host_path, "cache host evidence")
        loader = host.get("loader")
        selected = loader.get("selectedIcd") if isinstance(loader, dict) else None
        if not isinstance(selected, dict):
            raise ValueError("cache host evidence has no selected ICD")
        verify_external_artifact(selected.get("manifest"), "selected ICD manifest")
        verify_external_artifact(selected.get("library"), "selected ICD library")
        caches[label] = Cache(label, cycle_root, cache_root, cycle, host)
    if not caches:
        raise ValueError("at least one --populated-mesa-cache is required")
    return caches


def copy_caches(caches: dict[str, Cache], output: Path) -> dict[str, Cache]:
    copied: dict[str, Cache] = {}
    cache_output = output / "working-caches"
    cache_output.mkdir()
    for label, cache in caches.items():
        destination = cache_output / label
        shutil.copytree(cache.cache_root, destination)
        source_files = cache_snapshot(cache.cache_root)["files"]
        copied_files = cache_snapshot(destination)["files"]
        if copied_files != source_files:
            raise ValueError(f"copied Mesa cache differs from source evidence: {label}")
        copied[label] = Cache(
            label,
            cache.cycle_root,
            destination.resolve(),
            cache.cycle,
            cache.host,
        )
    return copied


def prepare_output(output: Path) -> None:
    if output.exists():
        raise ValueError(f"sweep output must not already exist: {output}")
    output.mkdir(parents=True)
    write_json(
        output / "status.json",
        {"schemaVersion": 1, "state": "running", "startedAt": now()},
    )


def run_sweep(args: argparse.Namespace) -> int:
    for label, path in (
        ("Echo binary", args.echo_binary),
        ("fixture manifest", args.fixture_manifest),
        ("coverage manifest", args.coverage_manifest),
        ("model", args.model_path),
        ("VAD", args.vad_path),
        ("VK_DRIVER_FILES", args.vk_driver_files),
    ):
        if not path.is_file():
            raise ValueError(f"{label} is missing: {path}")
    validate_echo_boundary(
        args.echo_binary,
        args.expected_echo_commit,
        args.expected_echo_binary_sha256,
    )
    if args.threads < 1 or args.repeats < 1 or args.warmups < 0 or args.timeout < 1:
        raise ValueError(
            "threads, repeats, timeout must be positive; warmups must be non-negative"
        )
    model_stem = (
        args.model_path.name.removesuffix(".bin")
        .removesuffix(".gguf")
        .removeprefix("ggml-")
    )
    if model_stem != args.model_name:
        raise ValueError("--model-name must match the ggml model filename exactly")
    runtimes = parse_runtimes(args.receipt_runtime, args.vk_driver_files)
    caches = parse_caches(args.populated_mesa_cache)
    cells = [parse_cell(value) for value in args.cell]
    if not cells or len({cell.label for cell in cells}) != len(cells):
        raise ValueError("at least one uniquely labelled --cell is required")
    for cell in cells:
        if cell.runtime_label not in runtimes or cell.cache_label not in caches:
            raise ValueError(
                f"cell {cell.label} references an unknown runtime or cache"
            )
    fixture_manifest = read_json(args.fixture_manifest, "fixture manifest")
    utterances = fixture_manifest.get("utterances")
    if not isinstance(utterances, list) or not utterances:
        raise ValueError("fixture manifest must contain utterances")
    fixture_audio = read_fixture_audio(args.fixture_manifest)
    prepare_output(args.output)
    try:
        caches = copy_caches(caches, args.output)
        results = [
            run_cell(
                args,
                cell,
                runtimes[cell.runtime_label],
                caches[cell.cache_label],
                fixture_audio,
                len(utterances),
                args.output,
            )
            for cell in cells
        ]
        summary = {
            "schemaVersion": 1,
            "completedAt": now(),
            "cells": results,
            "screenPass": any(bool(result["screenPass"]) for result in results),
            "researchPass": any(bool(result["researchPass"]) for result in results),
            "productionSelection": "not-attempted",
        }
        write_json(args.output / "sweep.json", summary)
        write_atomic(args.output / "summary.md", render_summary(results))
        write_json(
            args.output / "status.json",
            {
                "schemaVersion": 1,
                "state": "complete",
                "completedAt": now(),
                "summary": "sweep.json",
            },
        )
    except Exception as error:
        write_json(
            args.output / "status.json",
            {
                "schemaVersion": 1,
                "state": "failed",
                "failedAt": now(),
                "errorType": type(error).__name__,
                "error": str(error),
            },
        )
        raise
    return 0


def self_test() -> None:
    cell = parse_cell(
        "identity=runtime=v1.9.2,cache=mesa,beam=2,best-of=5,fallback=none"
    )
    assert cell.beam_size == 2 and cell.best_of == 5 and cell.no_fallback
    cpu = candidate_label("base-q5_1", 4, cell, True)
    gpu = candidate_label("base-q5_1", 4, cell, False)
    assert cpu != gpu and "cpu-only" in cpu and "cpu-only" not in gpu
    baseline_research = {name: True for name in RESEARCH_GATE_NAMES}
    baseline_bindings = {name: True for name in BINDING_GATE_NAMES}
    assert (
        admission_decision("v1.9.2", baseline_research, baseline_bindings) == "PROCEED"
    )
    asymmetric = dict(baseline_research)
    asymmetric["identityMatch"] = False
    assert admission_decision("v1.9.2", asymmetric, baseline_bindings) == "STOP"
    hypothesis = dict(baseline_research)
    hypothesis["sampleSize"] = False
    assert admission_decision("v1.9.2", hypothesis, baseline_bindings) == "INCOMPLETE"
    changed_receipt = dict(baseline_research)
    changed_receipt["receiptConsistency"] = False
    assert admission_decision("v1.9.2", changed_receipt, baseline_bindings) == "STOP"
    changed_runtime = dict(baseline_bindings)
    changed_runtime["exactRuntime"] = False
    assert admission_decision("v1.9.2", baseline_research, changed_runtime) == "STOP"
    changed_cache = dict(baseline_bindings)
    changed_cache["cacheEvidence"] = False
    assert (
        admission_decision("v1.9.2", baseline_research, changed_cache) == "INCOMPLETE"
    )
    with tempfile.TemporaryDirectory(prefix="echo-sweep-exact-") as temporary:
        root = Path(temporary)
        runtime_root = root / "runtime"
        runtime_root.mkdir()
        cli = runtime_root / "whisper-cli"
        cli.write_bytes(b"cli")
        (runtime_root / "libwhisper.so").write_bytes(b"library")
        model = root / "model.bin"
        model.write_bytes(b"model")
        vad = root / "ggml-silero-v6.2.0.bin"
        vad.write_bytes(b"vad")
        driver = root / "driver.json"
        driver.write_text("{}", encoding="utf-8")
        cache = root / "cache"
        cache.mkdir()
        model_dir = root / "models"
        model_dir.mkdir()
        home = root / "home"
        home.mkdir()
        runtime = Runtime("runtime", "v1.9.2", runtime_root, cli, sha256(cli))
        environment = child_environment(runtime, model_dir, cache, driver, home)
        assert not any(
            name.startswith(ENVIRONMENT_RESET_PREFIXES) for name in environment
        )
        bundle = root / "bundle"
        artifact_dir = bundle / "artifacts"
        artifact_dir.mkdir(parents=True)
        environment_path = artifact_dir / "environment.json"
        write_json(environment_path, environment)
        environment_ref = {
            "path": "artifacts/environment.json",
            "sha256": sha256(environment_path),
        }
        write_json(
            bundle / "status.json",
            {"state": "complete", "runId": "self-test"},
        )
        write_json(
            bundle / "run-manifest.json",
            {
                "runId": "self-test",
                "candidates": [
                    {
                        "label": label,
                        "threads": 4,
                        "beamSize": cell.beam_size,
                        "bestOf": cell.best_of,
                        "noFallback": cell.no_fallback,
                        "forceCpu": force_cpu,
                    }
                    for label, force_cpu in ((cpu, True), (gpu, False))
                ],
            },
        )
        rows = []
        for label in (cpu, gpu):
            rows.append(
                {
                    "candidate": label,
                    "runtimeArtifact": {"path": str(cli), "sha256": sha256(cli)},
                    "modelArtifact": {"path": str(model), "sha256": sha256(model)},
                    "vadArtifact": {"path": str(vad), "sha256": sha256(vad)},
                    "engine": {"vad": True},
                    "whisper": {
                        "runtime": {
                            "identitySha256": product_runtime_identity(cli),
                            "libraryPath": str(runtime_root),
                            "vulkanDriverFiles": str(driver),
                            "mesaShaderCacheDir": str(cache),
                        }
                    },
                    "observationArtifact": {"environment": environment_ref},
                }
            )
        write_atomic(
            bundle / "runs.jsonl",
            "".join(json.dumps(row) + "\n" for row in rows),
        )
        exact, error = exact_runtime_gate(
            bundle,
            runtime,
            model,
            vad,
            environment,
            driver,
            cache,
            cpu,
            gpu,
            cell,
            4,
            1,
            1,
        )
        assert exact, error
        environment["LD_PRELOAD"] = "/poison.so"
        write_json(environment_path, environment)
        environment_ref["sha256"] = sha256(environment_path)
        write_atomic(
            bundle / "runs.jsonl",
            "".join(json.dumps(row) + "\n" for row in rows),
        )
        exact, _ = exact_runtime_gate(
            bundle,
            runtime,
            model,
            vad,
            environment,
            driver,
            cache,
            cpu,
            gpu,
            cell,
            4,
            1,
            1,
        )
        assert not exact
    assert admission_decision("v1.9.3", baseline_research, baseline_bindings) == "STOP"
    with tempfile.TemporaryDirectory(prefix="echo-sweep-boundary-") as temporary:
        root = Path(temporary)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(
            ["git", "-C", str(root), "config", "user.email", "test@example.com"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(root), "config", "user.name", "Echo Test"],
            check=True,
        )
        binary = root / "echo"
        binary.write_bytes(b"echo binary")
        subprocess.run(["git", "-C", str(root), "add", "echo"], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "fixture"], check=True)
        commit = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        identity = validate_echo_boundary(
            binary, commit, sha256(binary), repo_root=root
        )
        assert identity["echoCommit"] == commit
        for wrong_commit, wrong_digest in (
            ("0" * 40, sha256(binary)),
            (commit, "0" * 64),
        ):
            try:
                validate_echo_boundary(
                    binary, wrong_commit, wrong_digest, repo_root=root
                )
            except ValueError:
                pass
            else:
                raise AssertionError("wrong Echo admission identity was accepted")
        (root / "dirty.txt").write_text("dirty", encoding="utf-8")
        try:
            validate_echo_boundary(binary, commit, sha256(binary), repo_root=root)
        except ValueError as error:
            assert "dirty Echo checkout" in str(error)
        else:
            raise AssertionError("dirty Echo checkout was accepted")
    print("sweep-whisper-admission: self-test ok")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--self-test", action="store_true")
    result.add_argument("--echo-binary", type=Path)
    result.add_argument("--expected-echo-commit")
    result.add_argument("--expected-echo-binary-sha256")
    result.add_argument("--fixture-manifest", type=Path)
    result.add_argument("--coverage-manifest", type=Path)
    result.add_argument("--model-name")
    result.add_argument("--model-path", type=Path)
    result.add_argument("--vad-path", type=Path)
    result.add_argument("--vk-driver-files", type=Path)
    result.add_argument(
        "--receipt-runtime", action="append", default=[], metavar="REVISION=DIR"
    )
    result.add_argument(
        "--populated-mesa-cache",
        action="append",
        default=[],
        metavar="LABEL=PHASE4_CYCLE_DIR",
    )
    result.add_argument(
        "--cell",
        action="append",
        default=[],
        metavar="LABEL=runtime=R,cache=C,beam=N,best-of=N,fallback=allow|none",
    )
    result.add_argument("--threads", type=int, default=4)
    result.add_argument("--repeats", type=int, default=1)
    result.add_argument("--warmups", type=int, default=1)
    result.add_argument("--seed", type=int, default=20260825)
    result.add_argument("--timeout", type=int, default=600)
    result.add_argument("--output", type=Path)
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    required = (
        "echo_binary",
        "expected_echo_commit",
        "expected_echo_binary_sha256",
        "fixture_manifest",
        "coverage_manifest",
        "model_name",
        "model_path",
        "vad_path",
        "vk_driver_files",
        "output",
    )
    missing = [
        name.replace("_", "-") for name in required if getattr(args, name) is None
    ]
    if missing:
        raise ValueError("missing required arguments: " + ", ".join(missing))
    args.echo_binary = args.echo_binary.resolve()
    args.fixture_manifest = args.fixture_manifest.resolve()
    args.coverage_manifest = args.coverage_manifest.resolve()
    args.model_path = args.model_path.resolve()
    args.vk_driver_files = args.vk_driver_files.resolve()
    args.output = args.output.resolve()
    return run_sweep(args)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (
        OSError,
        RuntimeError,
        ValueError,
        subprocess.SubprocessError,
        json.JSONDecodeError,
    ) as error:
        print(f"sweep-whisper-admission: {error}", file=sys.stderr)
        raise SystemExit(2) from error
