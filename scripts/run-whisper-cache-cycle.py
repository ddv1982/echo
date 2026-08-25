#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import uuid
from pathlib import Path, PurePosixPath


REPO_ROOT = Path(__file__).resolve().parent.parent
REPORT_NAMES = ("status.json", "summary.json", "runs.jsonl")
RECEIPT_PREFIX = "echo_whisper_runtime_receipt: "
RECEIPT_KEYS = frozenset(
    {
        "schemaVersion",
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
)
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
UUID_PATTERN = re.compile(r"^[0-9a-f]{32}$")
VULKAN_INDEX_PATTERN = re.compile(r"using Vulkan(\d+) backend")
CHILD_ENVIRONMENT_RESET_NAMES = (
    "VK_DRIVER_FILES",
    "VK_ICD_FILENAMES",
    "VK_ADD_DRIVER_FILES",
    "VK_LOADER_DRIVERS_SELECT",
    "VK_LOADER_DRIVERS_DISABLE",
    "VK_INSTANCE_LAYERS",
    "VK_LAYER_PATH",
    "VK_ADD_LAYER_PATH",
    "VK_IMPLICIT_LAYER_PATH",
    "VK_ADD_IMPLICIT_LAYER_PATH",
    "VK_LOADER_DEBUG",
    "MESA_SHADER_CACHE_DIR",
    "MESA_SHADER_CACHE_DISABLE",
    "MESA_SHADER_CACHE_MAX_SIZE",
    "MESA_DISK_CACHE_DATABASE",
    "MESA_DISK_CACHE_SINGLE_FILE",
    "MESA_DISK_CACHE_COMBINE_RW_WITH_RO_FOZ",
    "MESA_DISK_CACHE_READ_ONLY_FOZ_DBS",
)
INFERENCE_ENVIRONMENT_PREFIXES = (
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


def now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def portable_path(path: Path) -> str:
    resolved = path.resolve()
    for label, root in (("$REPO", REPO_ROOT), ("$HOME", Path.home())):
        try:
            return str(Path(label) / resolved.relative_to(root.resolve()))
        except ValueError:
            continue
    return str(resolved)


def recorded_path(value: object) -> Path:
    if not isinstance(value, str) or not value:
        raise ValueError("recorded path must be a non-empty string")
    if value == "$REPO" or value.startswith("$REPO/"):
        return REPO_ROOT / value.removeprefix("$REPO/")
    if value == "$HOME" or value.startswith("$HOME/"):
        return Path.home() / value.removeprefix("$HOME/")
    return Path(value)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact(path: Path, root: Path | None = None) -> dict[str, object]:
    resolved = path.resolve(strict=True)
    if not resolved.is_file():
        raise ValueError(f"artifact is not a regular file: {path}")
    if root is None:
        recorded = portable_path(resolved)
    else:
        try:
            recorded = resolved.relative_to(root.resolve()).as_posix()
        except ValueError as error:
            raise ValueError(f"artifact is outside its run bundle: {path}") from error
    return {
        "path": recorded,
        "bytes": resolved.stat().st_size,
        "sha256": sha256(resolved),
    }


def write_atomic(path: Path, value: str) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        temporary.write_text(value, encoding="utf-8")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_json(path: Path, value: object) -> None:
    write_atomic(path, json.dumps(value, indent=2, sort_keys=True) + "\n")


def read_json(path: Path, label: str) -> dict[str, object]:
    def reject_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
        value: dict[str, object] = {}
        for key, item in pairs:
            if key in value:
                raise ValueError(f"{label} has duplicate key {key!r}")
            value[key] = item
        return value

    try:
        value = json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_pairs
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"could not read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be a JSON object")
    return value


def require_artifact(root: Path, value: object, label: str) -> Path:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an artifact object")
    raw = value.get("path")
    if not isinstance(raw, str):
        raise ValueError(f"{label}.path must be a string")
    relative = PurePosixPath(raw)
    if (
        relative.is_absolute()
        or raw != relative.as_posix()
        or not relative.parts
        or any(part in {"", ".", ".."} for part in relative.parts)
    ):
        raise ValueError(f"{label}.path must be a safe bundle-relative path")
    candidate = root.joinpath(*relative.parts)
    try:
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{label} is missing") from error
    if not resolved.is_file() or root.resolve() not in resolved.parents:
        raise ValueError(f"{label} escapes the run bundle")
    size = value.get("bytes")
    digest = value.get("sha256")
    if isinstance(size, bool) or not isinstance(size, int) or size < 0:
        raise ValueError(f"{label}.bytes must be a non-negative integer")
    if not isinstance(digest, str) or SHA256_PATTERN.fullmatch(digest) is None:
        raise ValueError(f"{label}.sha256 must be a SHA-256")
    if resolved.stat().st_size != size or sha256(resolved) != digest:
        raise ValueError(f"{label} digest does not match its contents")
    return resolved


def write_status(
    output_dir: Path, state: str, run_id: str, started_at: str, **detail: object
) -> None:
    write_json(
        output_dir / "status.json",
        {
            "schemaVersion": 1,
            "state": state,
            "runId": run_id,
            "startedAt": started_at,
            "updatedAt": now(),
            **detail,
        },
    )


def prepare_output(output_dir: Path) -> tuple[str, str]:
    if output_dir.exists():
        raise ValueError(f"cache-cycle output must not exist: {output_dir}")
    output_dir.mkdir(parents=True)
    run_id = str(uuid.uuid4())
    started_at = now()
    write_status(output_dir, "running", run_id, started_at)
    return run_id, started_at


def cache_snapshot(cache_root: Path) -> dict[str, object]:
    if not cache_root.is_dir():
        raise ValueError(f"Mesa cache root is not a directory: {cache_root}")
    files: list[dict[str, object]] = []
    for directory, directories, names in os.walk(cache_root, followlinks=False):
        current = Path(directory)
        for child in directories:
            if (current / child).is_symlink():
                raise ValueError(
                    f"Mesa cache contains a symlinked directory: {current / child}"
                )
        directories.sort()
        for name in sorted(names):
            path = current / name
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"Mesa cache contains a non-regular file: {path}")
            files.append(
                {
                    "path": path.relative_to(cache_root).as_posix(),
                    "bytes": path.stat().st_size,
                    "sha256": sha256(path),
                }
            )
    files.sort(key=lambda item: str(item["path"]))
    return {
        "schemaVersion": 1,
        "capturedAt": now(),
        "root": portable_path(cache_root),
        "rootOwnership": "created-by-run-whisper-cache-cycle",
        "files": files,
    }


def validate_snapshot(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict) or value.get("schemaVersion") != 1:
        raise ValueError(f"{label} has an unsupported schema")
    if value.get("rootOwnership") != "created-by-run-whisper-cache-cycle":
        raise ValueError(f"{label} does not prove tool-owned cache state")
    if not isinstance(value.get("root"), str) or not isinstance(
        value.get("capturedAt"), str
    ):
        raise ValueError(f"{label} is missing cache root or capture time")
    files = value.get("files")
    if not isinstance(files, list):
        raise ValueError(f"{label}.files must be an array")
    paths: list[str] = []
    for item in files:
        if not isinstance(item, dict):
            raise ValueError(f"{label}.files has an invalid item")
        path = item.get("path")
        if (
            not isinstance(path, str)
            or not path
            or path.startswith("/")
            or ".." in path.split("/")
        ):
            raise ValueError(f"{label}.files has an unsafe path")
        if (
            isinstance(item.get("bytes"), bool)
            or not isinstance(item.get("bytes"), int)
            or int(item["bytes"]) < 0
        ):
            raise ValueError(f"{label}.files has an invalid byte count")
        digest = item.get("sha256")
        if not isinstance(digest, str) or SHA256_PATTERN.fullmatch(digest) is None:
            raise ValueError(f"{label}.files has an invalid SHA-256")
        paths.append(path)
    if paths != sorted(paths) or len(paths) != len(set(paths)):
        raise ValueError(f"{label}.files is not a deterministic unique tree")
    return value


def write_snapshot(output_dir: Path, name: str, cache_root: Path) -> dict[str, object]:
    path = output_dir / name
    write_json(path, cache_snapshot(cache_root))
    return artifact(path, output_dir)


def child_environment(vk_driver_files: Path) -> dict[str, str]:
    environment = {
        name: value
        for name, value in os.environ.items()
        if not name.upper().startswith(INFERENCE_ENVIRONMENT_PREFIXES)
    }
    for name in CHILD_ENVIRONMENT_RESET_NAMES:
        environment.pop(name, None)
    if not vk_driver_files.resolve(strict=True).is_file():
        raise ValueError("Vulkan driver manifest is not a file")
    return environment


def child_command(
    args: argparse.Namespace, output_dir: Path, cache_root: Path, reuse: bool
) -> list[str]:
    command = [
        sys.executable,
        str(
            getattr(
                args,
                "probe_script",
                REPO_ROOT / "scripts/probe-whisper-acceleration.py",
            )
        ),
        "--binary",
        str(args.binary),
        "--model",
        str(args.model),
        "--audio",
        str(args.audio),
        "--backend",
        "vulkan",
        "--language",
        args.language,
        "--prompt",
        args.prompt,
        "--threads",
        str(args.threads),
        "--beam-size",
        str(args.beam_size),
        "--best-of",
        str(args.best_of),
        "--warmups",
        "0",
        "--repeats",
        "1",
        "--seed",
        str(args.seed),
        "--timeout",
        str(args.timeout),
        "--mesa-shader-cache-dir",
        str(cache_root),
        "--vk-driver-files",
        str(args.vk_driver_files),
        "--output-dir",
        str(output_dir),
    ]
    if args.no_fallback:
        command.append("--no-fallback")
    if args.vad is not None:
        command.extend(("--vad", str(args.vad)))
    if reuse:
        command.append("--reuse-mesa-shader-cache")
    return command


def read_runs(path: Path, label: str) -> list[dict[str, object]]:
    rows = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError) as error:
        raise ValueError(f"could not read {label}: {error}") from error
    for index, line in enumerate(lines):
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"{label} line {index + 1} is invalid JSON") from error
        if not isinstance(row, dict):
            raise ValueError(f"{label} line {index + 1} is not an object")
        rows.append(row)
    return rows


def parse_accelerated_receipt(row: dict[str, object]) -> dict[str, object]:
    raw_stderr = row.get("rawStderr")
    receipt = row.get("runtimeReceipt")
    if not isinstance(raw_stderr, str) or not isinstance(receipt, dict):
        raise ValueError("accelerated probe did not retain raw receipt evidence")
    if row.get("runtimeReceiptError") is not None:
        raise ValueError("accelerated probe reported a runtime receipt error")
    lines = [
        line.removeprefix(RECEIPT_PREFIX)
        for line in raw_stderr.splitlines()
        if line.startswith(RECEIPT_PREFIX)
    ]
    if len(lines) != 1:
        raise ValueError("accelerated probe must contain exactly one raw receipt")
    try:
        parsed = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise ValueError("accelerated raw receipt is invalid JSON") from error
    if parsed != receipt or frozenset(receipt) != RECEIPT_KEYS:
        raise ValueError("accelerated raw receipt does not match the recorded receipt")
    if receipt.get("schemaVersion") != 1 or receipt.get("backend") != "vulkan":
        raise ValueError("accelerated receipt has an unsupported schema or backend")
    for name in (
        "selectedIndex",
        "vendorId",
        "deviceId",
        "apiVersion",
        "driverVersion",
    ):
        value = receipt.get(name)
        if (
            isinstance(value, bool)
            or not isinstance(value, int)
            or not 0 <= value < 2**32
        ):
            raise ValueError(f"accelerated receipt has an invalid {name}")
    for name in ("deviceUUID", "driverUUID", "pipelineCacheUUID"):
        value = receipt.get(name)
        if (
            not isinstance(value, str)
            or UUID_PATTERN.fullmatch(value) is None
            or value == "0" * 32
        ):
            raise ValueError(f"accelerated receipt has an invalid {name}")
    selected = VULKAN_INDEX_PATTERN.findall(raw_stderr)
    if len(selected) != 1 or int(selected[0]) != receipt["selectedIndex"]:
        raise ValueError(
            "accelerated raw receipt does not bind to one selected backend"
        )
    return receipt


def verify_child_probe(
    root: Path, artifacts: object, expected_cache_root: Path, reused: bool
) -> dict[str, object]:
    if not isinstance(artifacts, dict) or set(artifacts) != set(REPORT_NAMES):
        raise ValueError("probe artifacts must reference status, summary, and runs")
    paths = {
        name: require_artifact(root, artifacts[name], f"probe.{name}")
        for name in REPORT_NAMES
    }
    status = read_json(paths["status.json"], "probe status")
    if status.get("schemaVersion") != 1 or status.get("state") != "complete":
        raise ValueError("child probe status is not complete")
    summary = read_json(paths["summary.json"], "probe summary")
    gates = summary.get("gates")
    required_gates = (
        "backendTruth",
        "hardwareDevice",
        "runtimeReceipt",
        "pairedCompleteness",
        "transcriptParity",
    )
    if not isinstance(gates, dict):
        raise ValueError("child probe summary has no gate evidence")
    for gate in required_gates:
        if gates.get(gate) is not True:
            raise ValueError(f"child probe failed required gate: {gate}")
    rows = read_runs(paths["runs.jsonl"], "probe runs")
    if len(rows) != 2:
        raise ValueError(
            "cache cycle child probe must contain exactly one CPU/GPU pair"
        )
    by_candidate = {row.get("candidate"): row for row in rows}
    if set(by_candidate) != {"cpu", "accelerated"}:
        raise ValueError(
            "cache cycle child probe must contain one CPU and one accelerated row"
        )
    if len(by_candidate) != len(rows):
        raise ValueError("cache cycle child probe has duplicate candidate rows")
    for candidate, expected_backend in (("cpu", "cpu"), ("accelerated", "vulkan")):
        row = by_candidate[candidate]
        if (
            row.get("schemaVersion") != 1
            or row.get("resolvedBackend") != expected_backend
        ):
            raise ValueError(f"{candidate} child row has the wrong backend")
        elapsed = row.get("outerMs")
        if (
            isinstance(elapsed, bool)
            or not isinstance(elapsed, (int, float))
            or not math.isfinite(float(elapsed))
            or float(elapsed) < 0
        ):
            raise ValueError(f"{candidate} child row has an invalid outer latency")
        environment = row.get("environment")
        if not isinstance(environment, dict):
            raise ValueError(f"{candidate} child row has no environment")
        cached = environment.get("mesaShaderCacheDir")
        try:
            same_cache_root = (
                recorded_path(cached).resolve() == expected_cache_root.resolve()
            )
        except (OSError, ValueError) as error:
            raise ValueError(
                f"{candidate} child row has an invalid cache root"
            ) from error
        if not same_cache_root:
            raise ValueError(
                f"{candidate} child row did not reuse the cycle cache root"
            )
        if environment.get("mesaShaderCacheReuse") is not reused:
            raise ValueError(f"{candidate} child row has the wrong cache reuse mode")
        if not isinstance(row.get("rawStdout"), str) or not isinstance(
            row.get("rawStderr"), str
        ):
            raise ValueError(f"{candidate} child row did not retain raw output")
    cpu = by_candidate["cpu"]
    if (
        cpu.get("runtimeReceipt") is not None
        or cpu.get("runtimeReceiptError") is not None
    ):
        raise ValueError("CPU control must not retain a Vulkan receipt")
    if not isinstance(cpu.get("text"), str) or not isinstance(
        by_candidate["accelerated"].get("text"), str
    ):
        raise ValueError("child probe did not retain CPU and accelerated transcripts")
    if cpu["text"] != by_candidate["accelerated"]["text"]:
        raise ValueError("child probe CPU and accelerated transcripts differ")
    receipt = parse_accelerated_receipt(by_candidate["accelerated"])
    return {
        "artifacts": artifacts,
        "receipt": receipt,
        "transcript": cpu["text"],
        "latency": {
            "cpuOuterMs": float(cpu["outerMs"]),
            "acceleratedOuterMs": float(by_candidate["accelerated"]["outerMs"]),
        },
    }


def run_child_probe(
    args: argparse.Namespace, root: Path, cache_root: Path, name: str, reuse: bool
) -> dict[str, object]:
    output_dir = root / "probes" / name
    command = child_command(args, output_dir, cache_root, reuse)
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        env=child_environment(args.vk_driver_files),
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"{name} child probe failed with exit {completed.returncode}: "
            f"{completed.stderr.strip()}"
        )
    artifacts = {name: artifact(output_dir / name, root) for name in REPORT_NAMES}
    return verify_child_probe(root, artifacts, cache_root, reuse)


def validate_host_evidence(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or value.get("schemaVersion") != 1:
        raise ValueError("host evidence has an unsupported schema")
    if not isinstance(value.get("capturedAt"), str) or not isinstance(
        value.get("bootId"), str
    ):
        raise ValueError("host evidence is missing capture time or boot ID")
    if not isinstance(value.get("kernel"), dict) or not isinstance(
        value.get("drmDevices"), list
    ):
        raise ValueError("host evidence is missing kernel or DRM identities")
    if not isinstance(value.get("power"), dict) or not isinstance(
        value.get("memory"), dict
    ):
        raise ValueError("host evidence is missing power or memory evidence")
    loader = value.get("loader")
    if not isinstance(loader, dict) or not isinstance(
        loader.get("defaultIcdEnumeration"), dict
    ):
        raise ValueError("host evidence is missing default ICD enumeration")
    selected = loader.get("selectedIcd")
    if (
        not isinstance(selected, dict)
        or selected.get("environmentVariable") != "VK_DRIVER_FILES"
    ):
        raise ValueError(
            "host evidence does not prove an explicit VK_DRIVER_FILES selection"
        )
    if not isinstance(selected.get("manifest"), dict) or not isinstance(
        selected.get("library"), dict
    ):
        raise ValueError("host evidence is missing selected ICD artifacts")
    return value


def identity_artifact(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an artifact")
    digest = value.get("sha256")
    size = value.get("bytes")
    if not isinstance(digest, str) or SHA256_PATTERN.fullmatch(digest) is None:
        raise ValueError(f"{label} has no SHA-256")
    if isinstance(size, bool) or not isinstance(size, int) or size < 0:
        raise ValueError(f"{label} has no valid byte count")
    return {"bytes": size, "sha256": digest}


def runtime_identity(binary: Path) -> dict[str, object]:
    libraries: set[Path] = set()
    for candidate in binary.parent.iterdir():
        if ".so" not in candidate.name:
            continue
        try:
            resolved = candidate.resolve(strict=True)
        except OSError:
            continue
        if resolved.is_file():
            libraries.add(resolved)
    digest = hashlib.sha256(b"echo-whisper-runtime-v1\0")
    artifacts = []
    binary_artifact: dict[str, object] | None = None
    for path in [binary, *sorted(libraries)]:
        name = path.name.encode()
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        size = path.stat().st_size
        digest.update(size.to_bytes(8, "little"))
        file_digest = hashlib.sha256()
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
                file_digest.update(chunk)
        identity = {"bytes": size, "sha256": file_digest.hexdigest()}
        if path == binary:
            binary_artifact = identity
        else:
            artifacts.append(
                {
                    "name": path.name,
                    **identity,
                }
            )
    assert binary_artifact is not None
    return {
        **binary_artifact,
        "identitySha256": digest.hexdigest(),
        "adjacentLibraries": artifacts,
    }


def canonical_json(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def cycle_identity(
    args: argparse.Namespace, host: dict[str, object]
) -> dict[str, object]:
    selected = host["loader"]["selectedIcd"]
    assert isinstance(selected, dict)
    value = {
        "schemaVersion": 1,
        "runtime": runtime_identity(args.binary),
        "model": identity_artifact(artifact(args.model), "model"),
        "audio": identity_artifact(artifact(args.audio), "audio"),
        "vad": identity_artifact(artifact(args.vad), "VAD")
        if args.vad is not None
        else None,
        "probe": {
            "backend": "vulkan",
            "language": args.language,
            "prompt": args.prompt,
            "threads": args.threads,
            "beamSize": args.beam_size,
            "bestOf": args.best_of,
            "noFallback": args.no_fallback,
        },
        "host": {
            "kernel": host["kernel"],
            "drmDevices": host["drmDevices"],
            "selectedIcdManifest": identity_artifact(
                selected["manifest"], "ICD manifest"
            ),
            "selectedIcdLibrary": identity_artifact(selected["library"], "ICD library"),
        },
    }
    return {
        "schemaVersion": 1,
        "value": value,
        "sha256": hashlib.sha256(canonical_json(value).encode()).hexdigest(),
    }


def validate_identity(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or value.get("schemaVersion") != 1:
        raise ValueError("cycle identity has an unsupported schema")
    identity = value.get("value")
    digest = value.get("sha256")
    if not isinstance(identity, dict) or not isinstance(digest, str):
        raise ValueError("cycle identity is incomplete")
    actual = hashlib.sha256(canonical_json(identity).encode()).hexdigest()
    if actual != digest:
        raise ValueError("cycle identity digest does not match its value")
    return value


def read_artifact_json(root: Path, reference: object, label: str) -> dict[str, object]:
    return read_json(require_artifact(root, reference, label), label)


def validate_complete_cycle(
    root: Path, visited: set[Path] | None = None
) -> dict[str, object]:
    root = root.resolve()
    if visited is None:
        visited = set()
    if root in visited:
        raise ValueError("cache-cycle reset evidence contains a loop")
    visited.add(root)
    try:
        status = read_json(root / "status.json", "cache-cycle status")
        if status.get("schemaVersion") != 1 or status.get("state") != "complete":
            raise ValueError("cache-cycle status is not complete")
        cycle_path = require_artifact(root, status.get("cycle"), "status.cycle")
        cycle = read_json(cycle_path, "cache-cycle evidence")
        if cycle.get("schemaVersion") != 1 or cycle.get("runId") != status.get("runId"):
            raise ValueError("cache-cycle evidence does not match its status")
        host = validate_host_evidence(
            read_artifact_json(root, cycle.get("hostEvidence"), "host evidence")
        )
        if cycle.get("bootId") != host["bootId"]:
            raise ValueError("cache-cycle boot ID is not bound to host evidence")
        identity = validate_identity(cycle.get("identity"))
        snapshots = cycle.get("cacheSnapshots")
        if not isinstance(snapshots, dict) or set(snapshots) != {
            "beforeFresh",
            "afterFresh",
            "afterPopulated",
        }:
            raise ValueError("cache-cycle cache snapshots are incomplete")
        before = validate_snapshot(
            read_artifact_json(root, snapshots["beforeFresh"], "before-fresh snapshot"),
            "before-fresh snapshot",
        )
        after_fresh = validate_snapshot(
            read_artifact_json(root, snapshots["afterFresh"], "after-fresh snapshot"),
            "after-fresh snapshot",
        )
        after_populated = validate_snapshot(
            read_artifact_json(
                root, snapshots["afterPopulated"], "after-populated snapshot"
            ),
            "after-populated snapshot",
        )
        roots = {before["root"], after_fresh["root"], after_populated["root"]}
        if (
            len(roots) != 1
            or before["files"]
            or not after_fresh["files"]
            or not after_populated["files"]
        ):
            raise ValueError(
                "cache-cycle snapshots do not prove fresh then populated state"
            )
        probes = cycle.get("probes")
        if not isinstance(probes, dict) or set(probes) != {"fresh", "populated"}:
            raise ValueError("cache-cycle probe evidence is incomplete")
        expected_root = recorded_path(before["root"])
        fresh = (
            verify_child_probe(
                root, probes["fresh"].get("artifacts"), expected_root, False
            )
            if isinstance(probes["fresh"], dict)
            else None
        )
        populated = (
            verify_child_probe(
                root, probes["populated"].get("artifacts"), expected_root, True
            )
            if isinstance(probes["populated"], dict)
            else None
        )
        if (
            fresh is None
            or populated is None
            or fresh["receipt"] != populated["receipt"]
        ):
            raise ValueError(
                "cache-cycle populated probe does not preserve the fresh receipt"
            )
        if fresh["transcript"] != populated["transcript"]:
            raise ValueError("cache-cycle fresh and populated transcripts differ")
        reset = cycle.get("resetEvidence")
        if not isinstance(reset, dict) or reset.get("state") not in {
            "COMPLETE",
            "INCOMPLETE",
        }:
            raise ValueError("cache-cycle reset evidence has an invalid state")
        if reset["state"] == "COMPLETE":
            prior_value = reset.get("priorCycle")
            try:
                prior_root = recorded_path(prior_value).resolve()
            except (OSError, ValueError) as error:
                raise ValueError(
                    "complete reset evidence has no valid prior cycle"
                ) from error
            prior = validate_complete_cycle(prior_root, visited)
            if prior["identity"] != identity or prior["bootId"] == host["bootId"]:
                raise ValueError(
                    "complete reset evidence does not bind distinct matching boots"
                )
        return {"bootId": host["bootId"], "identity": identity, "cycle": cycle}
    finally:
        visited.remove(root)


def reset_evidence(
    prior_cycle: Path | None, identity: dict[str, object], boot_id: str
) -> dict[str, object]:
    if prior_cycle is None:
        return {"state": "INCOMPLETE", "reason": "noPriorCycle", "bootId": boot_id}
    prior = validate_complete_cycle(prior_cycle)
    evidence: dict[str, object] = {
        "state": "INCOMPLETE",
        "priorCycle": portable_path(prior_cycle),
        "bootId": boot_id,
    }
    if prior["identity"] != identity:
        evidence["reason"] = "identityMismatch"
    elif prior["bootId"] == boot_id:
        evidence["reason"] = "sameBootId"
    else:
        evidence["state"] = "COMPLETE"
        evidence["reason"] = "distinctBootId"
    return evidence


def validate_inputs(args: argparse.Namespace) -> None:
    for label, path in (
        ("binary", args.binary),
        ("model", args.model),
        ("audio", args.audio),
    ):
        if not path.is_file():
            raise ValueError(f"{label} is missing: {path}")
    if args.vad is not None and not args.vad.is_file():
        raise ValueError(f"VAD is missing: {args.vad}")
    if not args.vk_driver_files.is_file():
        raise ValueError(f"VK_DRIVER_FILES manifest is missing: {args.vk_driver_files}")
    if args.threads < 1 or args.beam_size < 1 or args.best_of < 1 or args.timeout < 1:
        raise ValueError("threads, beam size, best-of, and timeout must be positive")


def create_cache_root(output_dir: Path, cache_root: Path | None) -> Path:
    root = output_dir / "mesa-cache" if cache_root is None else cache_root
    if root.exists():
        raise ValueError(f"Mesa cache root must not exist before the cycle: {root}")
    root.parent.mkdir(parents=True, exist_ok=True)
    root.mkdir(mode=0o700)
    return root.resolve()


def run_host_collector(args: argparse.Namespace, output_dir: Path) -> dict[str, object]:
    host_path = output_dir / "host-evidence.json"
    command = [
        sys.executable,
        str(
            getattr(
                args,
                "host_collector",
                REPO_ROOT / "scripts/collect-whisper-host-evidence.py",
            )
        ),
        "--output",
        str(host_path),
        "--vk-driver-files",
        str(args.vk_driver_files),
    ]
    completed = subprocess.run(command, check=False, capture_output=True, text=True)
    if completed.returncode != 0:
        raise RuntimeError(f"host collector failed: {completed.stderr.strip()}")
    return validate_host_evidence(read_json(host_path, "host evidence"))


def run_cache_cycle(args: argparse.Namespace) -> int:
    validate_inputs(args)
    output_dir = args.output_dir.resolve()
    run_id, started_at = prepare_output(output_dir)
    try:
        cache_root = create_cache_root(output_dir, args.cache_root)
        host = run_host_collector(args, output_dir)
        identity = cycle_identity(args, host)
        snapshots = {
            "beforeFresh": write_snapshot(
                output_dir, "cache-before-fresh.json", cache_root
            )
        }
        before = read_artifact_json(
            output_dir, snapshots["beforeFresh"], "before-fresh snapshot"
        )
        if before["files"]:
            raise ValueError("new tool-owned Mesa cache root was not empty")
        fresh = run_child_probe(args, output_dir, cache_root, "fresh", False)
        snapshots["afterFresh"] = write_snapshot(
            output_dir, "cache-after-fresh.json", cache_root
        )
        if not read_artifact_json(
            output_dir, snapshots["afterFresh"], "after-fresh snapshot"
        )["files"]:
            raise ValueError("fresh child probe did not populate the Mesa cache")
        populated = run_child_probe(args, output_dir, cache_root, "populated", True)
        snapshots["afterPopulated"] = write_snapshot(
            output_dir, "cache-after-populated.json", cache_root
        )
        if fresh["receipt"] != populated["receipt"]:
            raise ValueError("populated child probe did not preserve the fresh receipt")
        if fresh["transcript"] != populated["transcript"]:
            raise ValueError(
                "populated child probe did not preserve the fresh transcript"
            )
        evidence = {
            "schemaVersion": 1,
            "runId": run_id,
            "startedAt": started_at,
            "completedAt": now(),
            "bootId": host["bootId"],
            "hostEvidence": artifact(output_dir / "host-evidence.json", output_dir),
            "identity": identity,
            "cacheSnapshots": snapshots,
            "probes": {"fresh": fresh, "populated": populated},
            "latency": {"fresh": fresh["latency"], "populated": populated["latency"]},
            "resetEvidence": reset_evidence(
                args.prior_cycle, identity, str(host["bootId"])
            ),
        }
        evidence_path = output_dir / "cache-cycle.json"
        write_json(evidence_path, evidence)
        write_status(
            output_dir,
            "complete",
            run_id,
            started_at,
            completedAt=now(),
            cycle=artifact(evidence_path, output_dir),
        )
        print(json.dumps(evidence["latency"], indent=2, sort_keys=True))
        return 0
    except Exception as error:
        write_status(
            output_dir,
            "failed",
            run_id,
            started_at,
            errorType=type(error).__name__,
            error=str(error),
        )
        raise


def fake_scripts(root: Path) -> tuple[Path, Path]:
    collector = root / "fake-collector.py"
    probe = root / "fake-probe.py"
    collector.write_text(
        textwrap.dedent(
            """\
            import argparse, json, os
            from pathlib import Path
            parser = argparse.ArgumentParser()
            parser.add_argument('--output', type=Path, required=True)
            parser.add_argument('--vk-driver-files', required=True)
            args = parser.parse_args()
            digest = 'a' * 64
            value = {
              'schemaVersion': 1, 'capturedAt': 'now',
              'bootId': os.environ.get('ECHO_FAKE_BOOT_ID', 'fake-boot'),
              'kernel': {'release': 'fake'}, 'drmDevices': [{'device': 'fake'}],
              'power': {'cpuGovernors': []}, 'memory': {'MemAvailable': '1 kB'},
              'loader': {'defaultIcdEnumeration': {'manifests': []},
                'selectedIcd': {'environmentVariable': 'VK_DRIVER_FILES',
                  'manifest': {'bytes': 1, 'sha256': digest},
                  'library': {'bytes': 1, 'sha256': 'b' * 64}}}}
            args.output.write_text(json.dumps(value), encoding='utf-8')
            """
        ),
        encoding="utf-8",
    )
    probe.write_text(
        textwrap.dedent(
            """\
            import argparse, json, os
            from pathlib import Path
            parser = argparse.ArgumentParser()
            parser.add_argument('--output-dir', type=Path, required=True)
            parser.add_argument('--mesa-shader-cache-dir', type=Path, required=True)
            parser.add_argument('--vk-driver-files', type=Path, required=True)
            parser.add_argument('--reuse-mesa-shader-cache', action='store_true')
            parser.add_argument('--binary'); parser.add_argument('--model'); parser.add_argument('--audio')
            parser.add_argument('--backend'); parser.add_argument('--language'); parser.add_argument('--prompt')
            parser.add_argument('--threads'); parser.add_argument('--beam-size'); parser.add_argument('--best-of')
            parser.add_argument('--warmups'); parser.add_argument('--repeats'); parser.add_argument('--seed')
            parser.add_argument('--timeout'); parser.add_argument('--no-fallback', action='store_true')
            parser.add_argument('--vad')
            args = parser.parse_args()
            if not args.reuse_mesa_shader_cache:
                (args.mesa_shader_cache_dir / 'shader').write_text('cache', encoding='utf-8')
            args.output_dir.mkdir(parents=True)
            receipt = {'schemaVersion': 1, 'backend': 'vulkan', 'selectedIndex': 0,
              'vendorId': 1, 'deviceId': 2, 'apiVersion': 3, 'driverVersion': 4,
              'deviceUUID': '0123456789abcdef0123456789abcdef',
              'driverUUID': 'fedcba9876543210fedcba9876543210',
              'pipelineCacheUUID': '00112233445566778899aabbccddeeff'}
            text = 'changed' if args.reuse_mesa_shader_cache and os.environ.get('ECHO_FAKE_POPULATED_TEXT_MISMATCH') else 'same'
            cpu = {'schemaVersion': 1, 'candidate': 'cpu', 'resolvedBackend': 'cpu',
              'outerMs': 20.0, 'runtimeReceipt': None, 'runtimeReceiptError': None,
              'rawStdout': '', 'rawStderr': '', 'text': text,
              'environment': {'mesaShaderCacheDir': str(args.mesa_shader_cache_dir),
                'mesaShaderCacheReuse': args.reuse_mesa_shader_cache}}
            gpu_stderr = 'whisper_backend_init_gpu: using Vulkan0 backend\\n' + 'echo_whisper_runtime_receipt: ' + json.dumps(receipt) + '\\n'
            gpu = {'schemaVersion': 1, 'candidate': 'accelerated', 'resolvedBackend': 'vulkan',
              'outerMs': 10.0, 'runtimeReceipt': receipt, 'runtimeReceiptError': None,
              'rawStdout': '', 'rawStderr': gpu_stderr, 'text': text,
              'environment': {'mesaShaderCacheDir': str(args.mesa_shader_cache_dir),
                'mesaShaderCacheReuse': args.reuse_mesa_shader_cache}}
            (args.output_dir / 'status.json').write_text(json.dumps({'schemaVersion': 1, 'state': 'complete'}), encoding='utf-8')
            gates = {name: True for name in ('backendTruth', 'hardwareDevice', 'runtimeReceipt', 'pairedCompleteness', 'transcriptParity')}
            bad_gate = os.environ.get('ECHO_FAKE_BAD_GATE')
            if bad_gate:
                gates[bad_gate] = False
            (args.output_dir / 'summary.json').write_text(json.dumps({'gates': gates}), encoding='utf-8')
            (args.output_dir / 'runs.jsonl').write_text(json.dumps(cpu) + '\\n' + json.dumps(gpu) + '\\n', encoding='utf-8')
            """
        ),
        encoding="utf-8",
    )
    return collector, probe


def test_args(
    root: Path,
    collector: Path,
    probe: Path,
    output: Path,
    prior: Path | None = None,
    cache_root: Path | None = None,
) -> argparse.Namespace:
    return argparse.Namespace(
        output_dir=output,
        binary=root / "whisper-cli",
        model=root / "model.bin",
        audio=root / "fixture.wav",
        vad=None,
        vk_driver_files=root / "intel_icd.json",
        language="en",
        prompt="",
        threads=4,
        beam_size=1,
        best_of=1,
        no_fallback=True,
        seed=1,
        timeout=30,
        prior_cycle=prior,
        cache_root=cache_root,
        host_collector=collector,
        probe_script=probe,
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-cache-cycle-") as temporary:
        root = Path(temporary)
        existing_output = root / "existing-output"
        existing_output.mkdir()
        try:
            prepare_output(existing_output)
        except ValueError as error:
            assert "must not exist" in str(error)
        else:
            raise AssertionError(
                "cache cycle should reject an existing output directory"
            )
        for name in ("whisper-cli", "model.bin", "fixture.wav", "intel_icd.json"):
            (root / name).write_text(name, encoding="utf-8")
        library = root / "libwhisper.so"
        library.write_text("library-v1", encoding="utf-8")
        first_runtime_identity = runtime_identity(root / "whisper-cli")
        library.write_text("library-v2", encoding="utf-8")
        second_runtime_identity = runtime_identity(root / "whisper-cli")
        assert (
            first_runtime_identity["identitySha256"]
            != second_runtime_identity["identitySha256"]
        )
        cache_tree = root / "deterministic-cache"
        (cache_tree / "0a").mkdir(parents=True)
        (cache_tree / "index").write_text("index", encoding="utf-8")
        (cache_tree / "0a" / "entry").write_text("entry", encoding="utf-8")
        snapshot_paths = [item["path"] for item in cache_snapshot(cache_tree)["files"]]
        assert snapshot_paths == sorted(snapshot_paths)
        collector, probe = fake_scripts(root)
        temporary_names = (
            *CHILD_ENVIRONMENT_RESET_NAMES,
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "MESA_VK_DEVICE_SELECT",
            "DRI_PRIME",
            "ECHO_FAKE_BOOT_ID",
            "ECHO_FAKE_BAD_GATE",
            "ECHO_FAKE_POPULATED_TEXT_MISMATCH",
        )
        previous_environment = {name: os.environ.get(name) for name in temporary_names}
        try:
            for name in (
                *CHILD_ENVIRONMENT_RESET_NAMES,
                "LD_LIBRARY_PATH",
                "LD_PRELOAD",
                "MESA_VK_DEVICE_SELECT",
                "DRI_PRIME",
            ):
                os.environ[name] = "poison"
            isolated = child_environment(root / "intel_icd.json")
            assert all(
                name not in isolated
                for name in (
                    *CHILD_ENVIRONMENT_RESET_NAMES,
                    "LD_LIBRARY_PATH",
                    "LD_PRELOAD",
                    "MESA_VK_DEVICE_SELECT",
                    "DRI_PRIME",
                )
            )
            os.environ["ECHO_FAKE_BOOT_ID"] = "boot-a"
            prior_root = root / "prior"
            assert run_cache_cycle(test_args(root, collector, probe, prior_root)) == 0
            assert validate_complete_cycle(prior_root)["bootId"] == "boot-a"
            same_boot_root = root / "same-boot"
            run_cache_cycle(
                test_args(root, collector, probe, same_boot_root, prior_root)
            )
            same = read_json(same_boot_root / "cache-cycle.json", "same boot cycle")
            assert same["resetEvidence"]["state"] == "INCOMPLETE"
            assert same["resetEvidence"]["reason"] == "sameBootId"
            os.environ["ECHO_FAKE_BOOT_ID"] = "boot-b"
            reset_root = root / "distinct-boot"
            run_cache_cycle(test_args(root, collector, probe, reset_root, prior_root))
            reset = validate_complete_cycle(reset_root)["cycle"]
            assert reset["resetEvidence"]["state"] == "COMPLETE"
            assert reset["cacheSnapshots"]["beforeFresh"]["sha256"]
            forged = read_json(reset_root / "cache-cycle.json", "forged cycle")
            forged["resetEvidence"] = {
                "state": "COMPLETE",
                "priorCycle": str(reset_root),
            }
            write_json(reset_root / "cache-cycle.json", forged)
            try:
                validate_complete_cycle(reset_root)
            except ValueError as error:
                assert "digest" in str(error)
            else:
                raise AssertionError("forged reset label should be rejected")
            cache_tamper_root = root / "cache-tamper"
            run_cache_cycle(test_args(root, collector, probe, cache_tamper_root))
            (cache_tamper_root / "cache-after-fresh.json").write_text(
                "{}", encoding="utf-8"
            )
            try:
                validate_complete_cycle(cache_tamper_root)
            except ValueError as error:
                assert "digest" in str(error)
            else:
                raise AssertionError("mutated cache snapshot should be rejected")
            probe_tamper_root = root / "probe-tamper"
            run_cache_cycle(test_args(root, collector, probe, probe_tamper_root))
            (probe_tamper_root / "probes" / "fresh" / "runs.jsonl").write_text(
                "{}\n", encoding="utf-8"
            )
            try:
                validate_complete_cycle(probe_tamper_root)
            except ValueError as error:
                assert "digest" in str(error)
            else:
                raise AssertionError("mutated probe artifact should be rejected")
            nonempty = root / "nonempty-cache"
            nonempty.mkdir()
            (nonempty / "user-file").write_text("do not delete", encoding="utf-8")
            rejected_root = root / "rejected"
            try:
                run_cache_cycle(
                    test_args(
                        root, collector, probe, rejected_root, cache_root=nonempty
                    )
                )
            except ValueError as error:
                assert "must not exist" in str(error)
            else:
                raise AssertionError(
                    "nonempty caller-owned cache root should be rejected"
                )
            rejected_status = read_json(
                rejected_root / "status.json", "rejected status"
            )
            assert rejected_status["state"] == "failed"
            assert (nonempty / "user-file").read_text(
                encoding="utf-8"
            ) == "do not delete"
            os.environ["ECHO_FAKE_BAD_GATE"] = "backendTruth"
            bad_gate_root = root / "bad-gate"
            try:
                run_cache_cycle(test_args(root, collector, probe, bad_gate_root))
            except ValueError as error:
                assert "failed required gate: backendTruth" in str(error)
            else:
                raise AssertionError("failed child gate should reject the cache cycle")
            assert (
                read_json(bad_gate_root / "status.json", "bad gate status")["state"]
                == "failed"
            )
            os.environ.pop("ECHO_FAKE_BAD_GATE")
            os.environ["ECHO_FAKE_POPULATED_TEXT_MISMATCH"] = "1"
            text_mismatch_root = root / "text-mismatch"
            try:
                run_cache_cycle(test_args(root, collector, probe, text_mismatch_root))
            except ValueError as error:
                assert "fresh transcript" in str(error)
            else:
                raise AssertionError(
                    "cross-cache transcript mismatch should be rejected"
                )
            assert (
                read_json(text_mismatch_root / "status.json", "text mismatch status")[
                    "state"
                ]
                == "failed"
            )
        finally:
            for name, value in previous_environment.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value
    print("whisper cache-cycle self-test passed")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--self-test", action="store_true")
    result.add_argument("--validate-cycle", type=Path)
    result.add_argument("--output-dir", type=Path)
    result.add_argument("--binary", type=Path)
    result.add_argument("--model", type=Path)
    result.add_argument("--audio", type=Path)
    result.add_argument("--vad", type=Path)
    result.add_argument("--vk-driver-files", type=Path)
    result.add_argument("--language", default="auto")
    result.add_argument("--prompt", default="")
    result.add_argument("--threads", type=int, default=4)
    result.add_argument("--beam-size", type=int, default=1)
    result.add_argument("--best-of", type=int, default=1)
    result.add_argument("--no-fallback", action="store_true")
    result.add_argument("--seed", type=int, default=20260824)
    result.add_argument("--timeout", type=int, default=600)
    result.add_argument("--cache-root", type=Path)
    result.add_argument("--prior-cycle", type=Path)
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.validate_cycle is not None:
        validated = validate_complete_cycle(args.validate_cycle)
        print(
            json.dumps(
                {
                    "bootId": validated["bootId"],
                    "identitySha256": validated["identity"]["sha256"],
                    "resetState": validated["cycle"]["resetEvidence"]["state"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    missing = [
        name
        for name in ("output_dir", "binary", "model", "audio", "vk_driver_files")
        if getattr(args, name) is None
    ]
    if missing:
        raise ValueError("missing required arguments: " + ", ".join(missing))
    return run_cache_cycle(args)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"run-whisper-cache-cycle: {error}", file=sys.stderr)
        raise SystemExit(2) from error
