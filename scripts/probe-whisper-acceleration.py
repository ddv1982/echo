#!/usr/bin/env python3
"""Compare one whisper.cpp GPU backend with its CPU negative control."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import math
import os
import platform
import random
import re
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib
from collections import defaultdict
from pathlib import Path
from unittest import mock


TIMING_PATTERN = re.compile(
    r"whisper_print_timings:\s+"
    r"(load|mel|sample|encode|decode|batchd|prompt|total) time\s*=\s*([0-9.]+) ms"
)
VULKAN_DEVICE_PATTERN = re.compile(r"ggml_vulkan:\s*(\d+)\s*=\s*(.+?)(?:\s*\||$)")
VULKAN_BACKEND_PATTERN = re.compile(r"using Vulkan(\d+) backend")
RUNTIME_RECEIPT_PREFIX = "echo_whisper_runtime_receipt: "
UUID_PATTERN = re.compile(r"^[0-9a-f]{32}$")
VULKAN_RECEIPT_KEYS = frozenset(
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
UINT32_MAX = (1 << 32) - 1
REPO_ROOT = Path(__file__).resolve().parent.parent
REPORT_FILES = ("warmups.jsonl", "runs.jsonl", "summary.json", "summary.md")
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


def portable_path(path: Path) -> str:
    resolved = path.resolve()
    for label, root in (("$REPO", REPO_ROOT), ("$HOME", Path.home())):
        try:
            relative = resolved.relative_to(root.resolve())
        except ValueError:
            continue
        return str(Path(label) / relative)
    return str(resolved)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact(path: Path) -> dict[str, object]:
    return {
        "path": portable_path(path),
        "bytes": path.stat().st_size,
        "sha256": sha256(path),
    }


def repo_metadata() -> dict[str, object]:
    commit = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    dirty = bool(
        subprocess.run(
            ["git", "-C", str(REPO_ROOT), "status", "--porcelain"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    )
    with (REPO_ROOT / "Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["workspace"]["package"]["version"]
    return {"commit": commit, "dirty": dirty, "version": version}


def write_atomic(path: Path, value: str) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        temporary.write_text(value, encoding="utf-8")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_status(output_dir: Path, state: str, **detail: object) -> None:
    payload = {
        "schemaVersion": 1,
        "state": state,
        "updatedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        **detail,
    }
    write_atomic(
        output_dir / "status.json", json.dumps(payload, indent=2, sort_keys=True) + "\n"
    )


def prepare_output(output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    for name in REPORT_FILES:
        path = output_dir / name
        if path.is_dir():
            raise ValueError(f"report path is a directory: {path}")
        path.unlink(missing_ok=True)
    write_status(output_dir, "running")


def read_optional(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8").strip() or None
    except OSError:
        return None


def drm_devices() -> list[dict[str, object]]:
    devices: list[dict[str, object]] = []
    for render in sorted(Path("/sys/class/drm").glob("renderD*")):
        device = render / "device"
        driver_link = device / "driver"
        driver = None
        try:
            driver = driver_link.resolve().name
        except OSError:
            pass
        devices.append(
            {
                "renderNode": f"/dev/dri/{render.name}",
                "vendor": read_optional(device / "vendor"),
                "device": read_optional(device / "device"),
                "revision": read_optional(device / "revision"),
                "driver": driver,
                "driverModuleVersion": read_optional(driver_link / "module/version"),
            }
        )
    return devices


def host_metadata() -> dict[str, object]:
    uname = platform.uname()
    meminfo = {}
    if Path("/proc/meminfo").is_file():
        for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
            name, _, value = line.partition(":")
            if name in {"MemTotal", "MemAvailable", "SwapTotal", "SwapFree"}:
                meminfo[name] = value.strip()
    governors = sorted(
        {
            value
            for path in Path("/sys/devices/system/cpu").glob(
                "cpu*/cpufreq/scaling_governor"
            )
            if (value := read_optional(path)) is not None
        }
    )
    power_sources = {
        path.parent.name: value
        for path in Path("/sys/class/power_supply").glob("*/online")
        if (value := read_optional(path)) is not None
    }
    return {
        "system": uname.system,
        "release": uname.release,
        "machine": uname.machine,
        "processor": uname.processor,
        "cpuCount": os.cpu_count(),
        "drmDevices": drm_devices(),
        "powerProfile": read_optional(Path("/sys/firmware/acpi/platform_profile")),
        "cpuGovernors": governors,
        "powerSources": power_sources,
        "memory": meminfo,
    }


def adjacent_libraries(binary: Path) -> list[dict[str, object]]:
    libraries = []
    seen = set()
    for path in sorted(binary.parent.glob("*.so*")):
        try:
            resolved = path.resolve(strict=True)
        except OSError:
            continue
        if not resolved.is_file() or resolved in seen:
            continue
        seen.add(resolved)
        libraries.append(artifact(resolved))
    return libraries


def vulkan_icds() -> list[dict[str, object]]:
    paths = []
    for root in (Path("/usr/share/vulkan/icd.d"), Path("/etc/vulkan/icd.d")):
        paths.extend(root.glob("*.json"))
    return [artifact(path) for path in sorted(set(paths)) if path.is_file()]


def runtime_environment(
    binary: Path,
    mesa_shader_cache_dir: Path | None = None,
    vk_driver_files: Path | None = None,
) -> dict[str, str]:
    environment = {
        name: value
        for name, value in os.environ.items()
        if not name.upper().startswith(INFERENCE_ENVIRONMENT_PREFIXES)
    }
    environment["LD_LIBRARY_PATH"] = str(binary.parent.resolve())
    if mesa_shader_cache_dir is not None:
        environment["MESA_SHADER_CACHE_DIR"] = str(
            mesa_shader_cache_dir.resolve(strict=True)
        )
    if vk_driver_files is not None:
        environment["VK_DRIVER_FILES"] = str(vk_driver_files.resolve(strict=True))
    return environment


def prepare_mesa_shader_cache(path: Path, reuse: bool) -> None:
    if reuse:
        if not path.is_dir():
            raise ValueError(
                f"populated Mesa shader cache directory must already exist: {path}"
            )
        if not any(candidate.is_file() for candidate in path.rglob("*")):
            raise ValueError(
                f"populated Mesa shader cache directory must contain files: {path}"
            )
        return
    if path.exists() and (not path.is_dir() or any(path.iterdir())):
        raise ValueError(f"Mesa shader cache directory must be empty: {path}")
    path.mkdir(parents=True, exist_ok=True)


def runtime_version(
    binary: Path, timeout: int, vk_driver_files: Path | None = None
) -> str:
    completed = subprocess.run(
        [str(binary), "--version"],
        capture_output=True,
        text=True,
        timeout=timeout,
        env=runtime_environment(binary, vk_driver_files=vk_driver_files),
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"could not read runtime version: {completed.stderr.strip()}"
        )
    lines = [
        line.strip()
        for line in (completed.stdout + "\n" + completed.stderr).splitlines()
        if "version" in line.lower()
    ]
    if not lines:
        raise RuntimeError("runtime version output did not contain a version line")
    return lines[-1]


def parse_transcript(raw: str) -> tuple[str, str | None]:
    payload = json.loads(raw)
    transcription = payload.get("transcription", [])
    if not isinstance(transcription, list):
        raise ValueError("whisper JSON transcription is not a list")
    text = "".join(
        segment.get("text", "")
        for segment in transcription
        if isinstance(segment, dict) and isinstance(segment.get("text", ""), str)
    ).strip()
    result = payload.get("result")
    language = result.get("language") if isinstance(result, dict) else None
    return text, language if isinstance(language, str) else None


def parse_timings(stderr: str) -> dict[str, float]:
    return {f"{name}Ms": float(value) for name, value in TIMING_PATTERN.findall(stderr)}


def strict_json_object(value: str) -> dict[str, object]:
    def reject_constant(constant: str) -> object:
        raise ValueError(f"non-finite JSON value: {constant}")

    def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, item in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = item
        return result

    parsed = json.loads(
        value,
        object_pairs_hook=reject_duplicate_keys,
        parse_constant=reject_constant,
    )
    if not isinstance(parsed, dict):
        raise ValueError("receipt must be a JSON object")
    return parsed


def receipt_lines(stderr: str) -> list[str]:
    return [
        line.removeprefix(RUNTIME_RECEIPT_PREFIX)
        for line in stderr.splitlines()
        if line.startswith(RUNTIME_RECEIPT_PREFIX)
    ]


def parse_vulkan_runtime_receipt(stderr: str) -> dict[str, object]:
    lines = receipt_lines(stderr)
    if len(lines) != 1:
        raise ValueError(
            f"expected exactly one Vulkan runtime receipt, found {len(lines)}"
        )
    try:
        receipt = strict_json_object(lines[0])
    except (TypeError, ValueError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid Vulkan runtime receipt: {error}") from error
    if frozenset(receipt) != VULKAN_RECEIPT_KEYS:
        raise ValueError("Vulkan runtime receipt has an unexpected schema")
    if receipt["schemaVersion"] != 1 or isinstance(receipt["schemaVersion"], bool):
        raise ValueError("Vulkan runtime receipt has an unsupported schemaVersion")
    if receipt["backend"] != "vulkan":
        raise ValueError("Vulkan runtime receipt backend is not vulkan")
    for field in (
        "selectedIndex",
        "vendorId",
        "deviceId",
        "apiVersion",
        "driverVersion",
    ):
        value = receipt[field]
        if (
            not isinstance(value, int)
            or isinstance(value, bool)
            or not 0 <= value <= UINT32_MAX
        ):
            raise ValueError(
                f"Vulkan runtime receipt {field} is not an unsigned 32-bit integer"
            )
    for field in ("deviceUUID", "driverUUID", "pipelineCacheUUID"):
        value = receipt[field]
        if not isinstance(value, str) or UUID_PATTERN.fullmatch(value) is None:
            raise ValueError(f"Vulkan runtime receipt {field} is not lowercase 32-hex")
        if value == "0" * 32:
            raise ValueError(f"Vulkan runtime receipt {field} must not be all zero")
    return receipt


def canonical_runtime_receipt(receipt: dict[str, object]) -> str:
    return json.dumps(receipt, sort_keys=True, separators=(",", ":"))


def observed_vulkan_index(stderr: str) -> int | None:
    selected = [int(value) for value in VULKAN_BACKEND_PATTERN.findall(stderr)]
    return selected[0] if len(selected) == 1 else None


def runtime_receipt_observation(
    stderr: str, control: bool
) -> tuple[dict[str, object] | None, str | None]:
    lines = receipt_lines(stderr)
    if control:
        if lines:
            return None, "CPU control emitted a Vulkan runtime receipt"
        return None, None
    try:
        receipt = parse_vulkan_runtime_receipt(stderr)
    except ValueError as error:
        return None, str(error)
    selected_index = observed_vulkan_index(stderr)
    if selected_index is None:
        return None, "loader logs did not report exactly one selected Vulkan backend"
    if receipt["selectedIndex"] != selected_index:
        return None, (
            "Vulkan runtime receipt selectedIndex does not match the selected backend "
            f"({receipt['selectedIndex']} != {selected_index})"
        )
    return receipt, None


def detect_backend(stderr: str, control: bool) -> tuple[str, str | None]:
    if control:
        if (
            "whisper_backend_init_gpu: no GPU found" in stderr
            and " total size =" in stderr
        ):
            return "cpu", None
        return "unknown", None
    selected_index = observed_vulkan_index(stderr)
    if selected_index is not None:
        devices = {
            index: device.strip()
            for index, device in VULKAN_DEVICE_PATTERN.findall(stderr)
        }
        return "vulkan", devices.get(str(selected_index))
    return "unknown", None


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def invoke(
    args: argparse.Namespace,
    audio: Path,
    candidate: str,
    repeat: int,
    order: int,
) -> dict[str, object]:
    control = candidate == "cpu"
    command = [
        str(args.binary),
        "-m",
        str(args.model),
        "-f",
        str(audio),
        "-nt",
        "-oj",
        "-of",
        "-",
        "-l",
        args.language,
        "-t",
        str(args.threads),
        "-bs",
        str(args.beam_size),
        "-bo",
        str(args.best_of),
    ]
    if control:
        command.append("--no-gpu")
    if args.no_fallback:
        command.append("-nf")
    if args.prompt:
        command.extend(["--prompt", args.prompt])
    if args.vad is not None:
        command.extend(["--vad", "-vm", str(args.vad)])
    started = time.perf_counter_ns()
    completed = subprocess.run(
        command,
        capture_output=True,
        text=True,
        timeout=args.timeout,
        env=runtime_environment(
            args.binary, args.mesa_shader_cache_dir, args.vk_driver_files
        ),
    )
    outer_ms = (time.perf_counter_ns() - started) / 1_000_000
    if completed.returncode != 0:
        raise RuntimeError(
            f"{candidate} transcription failed for {audio}: {completed.stderr.strip()}"
        )
    text, language = parse_transcript(completed.stdout)
    backend, device = detect_backend(completed.stderr, control)
    receipt, receipt_error = runtime_receipt_observation(completed.stderr, control)
    return {
        "schemaVersion": 1,
        "candidate": candidate,
        "expectedBackend": "cpu" if control else args.backend,
        "resolvedBackend": backend,
        "device": device,
        "runtimeReceipt": receipt,
        "runtimeReceiptError": receipt_error,
        "audio": portable_path(audio),
        "audioSha256": sha256(audio),
        "repeat": repeat,
        "candidateOrder": order,
        "outerMs": round(outer_ms, 3),
        "text": text,
        "language": language,
        "timings": parse_timings(completed.stderr),
        "rawStdout": completed.stdout,
        "rawStderr": completed.stderr,
        "evidence": [
            line.strip()
            for line in completed.stderr.splitlines()
            if any(
                marker in line
                for marker in (
                    "ggml_vulkan:",
                    "whisper_backend_init_gpu:",
                    "system_info:",
                    "whisper_print_timings:",
                )
            )
        ],
    }


def summarize(
    rows: list[dict[str, object]],
    expected_backend: str,
    min_speedup_percent: float,
    min_speedup_ms: float,
) -> dict[str, object]:
    by_candidate: dict[str, list[dict[str, object]]] = defaultdict(list)
    for row in rows:
        by_candidate[str(row["candidate"])].append(row)
    candidate_summary: dict[str, dict[str, object]] = {}
    for candidate, values in sorted(by_candidate.items()):
        outer = [float(row["outerMs"]) for row in values]
        candidate_summary[candidate] = {
            "runs": len(values),
            "medianOuterMs": round(statistics.median(outer), 3),
            "p95OuterMs": round(percentile(outer, 0.95), 3),
            "resolvedBackends": sorted({str(row["resolvedBackend"]) for row in values}),
            "devices": sorted({str(row["device"]) for row in values if row["device"]}),
            "runtimeReceiptErrors": sorted(
                {
                    str(row["runtimeReceiptError"])
                    for row in values
                    if row.get("runtimeReceiptError") is not None
                }
            ),
        }

    def row_key(row: dict[str, object]) -> tuple[str, str, int]:
        return str(row["audio"]), str(row["audioSha256"]), int(row["repeat"])

    cpu_rows = {row_key(row): row for row in by_candidate["cpu"]}
    accelerated_rows = {row_key(row): row for row in by_candidate["accelerated"]}
    unique_pairs = len(cpu_rows) == len(by_candidate["cpu"]) and len(
        accelerated_rows
    ) == len(by_candidate["accelerated"])
    complete_pairs = unique_pairs and cpu_rows.keys() == accelerated_rows.keys()
    paired_rows = [
        (cpu_rows[key], accelerated_rows[key]) for key in sorted(cpu_rows.keys())
    ]
    reductions = [
        float(cpu["outerMs"]) - float(gpu["outerMs"]) for cpu, gpu in paired_rows
    ]
    speedups = [
        100 * (float(cpu["outerMs"]) - float(gpu["outerMs"])) / float(cpu["outerMs"])
        for cpu, gpu in paired_rows
    ]
    reduction_ms = statistics.median(reductions)
    speedup_percent = statistics.median(speedups)
    transcript_parity = complete_pairs and all(
        str(cpu["text"]) == str(gpu["text"]) for cpu, gpu in paired_rows
    )
    backend_truth = candidate_summary["accelerated"]["resolvedBackends"] == [
        expected_backend
    ]
    cpu_truth = candidate_summary["cpu"]["resolvedBackends"] == ["cpu"]
    hardware_device = bool(by_candidate["accelerated"]) and all(
        row["device"]
        and not re.search(
            r"lavapipe|llvmpipe|swiftshader", str(row["device"]), re.IGNORECASE
        )
        for row in by_candidate["accelerated"]
    )
    accelerated_receipt_rows = by_candidate["accelerated"]
    accepted_accelerated_receipts = bool(accelerated_receipt_rows) and all(
        isinstance(row.get("runtimeReceipt"), dict)
        and row.get("runtimeReceiptError") is None
        for row in accelerated_receipt_rows
    )
    accelerated_receipts = (
        accepted_accelerated_receipts
        and len(
            {
                canonical_runtime_receipt(row["runtimeReceipt"])
                for row in accelerated_receipt_rows
                if isinstance(row.get("runtimeReceipt"), dict)
            }
        )
        == 1
    )
    cpu_without_receipts = bool(by_candidate["cpu"]) and all(
        row.get("runtimeReceipt") is None and row.get("runtimeReceiptError") is None
        for row in by_candidate["cpu"]
    )
    pairs_per_audio: dict[tuple[str, str], int] = defaultdict(int)
    for audio, audio_sha256, _repeat in cpu_rows:
        pairs_per_audio[(audio, audio_sha256)] += 1
    sample_size = (
        complete_pairs
        and bool(pairs_per_audio)
        and all(count >= 10 for count in pairs_per_audio.values())
    )
    p95_improved = float(candidate_summary["accelerated"]["p95OuterMs"]) < float(
        candidate_summary["cpu"]["p95OuterMs"]
    )
    gates = {
        "backendTruth": backend_truth and cpu_truth,
        "hardwareDevice": hardware_device,
        "runtimeReceipt": accelerated_receipts and cpu_without_receipts,
        "pairedCompleteness": complete_pairs,
        "sampleSize": sample_size,
        "transcriptParity": transcript_parity,
        "medianSpeedup": speedup_percent >= min_speedup_percent,
        "medianReduction": reduction_ms >= min_speedup_ms,
        "p95Improved": p95_improved,
    }
    return {
        "candidates": candidate_summary,
        "speedupPercent": round(speedup_percent, 3),
        "medianReductionMs": round(reduction_ms, 3),
        "minimumSpeedupPercent": min_speedup_percent,
        "minimumReductionMs": min_speedup_ms,
        "gates": gates,
        "decision": "proceed" if all(gates.values()) else "stop",
        "claimBoundary": (
            "This probe proves backend creation, its selected physical-device receipt, paired "
            "latency, and exact transcript parity only. The receipt does not prove an ICD "
            "manifest or loaded-library digests; launch evidence owns those. It does not replace "
            "the multilingual WER/CER and silence corpus gate."
        ),
    }


def render_summary(summary: dict[str, object]) -> str:
    candidates = summary["candidates"]
    assert isinstance(candidates, dict)
    lines = [
        "# Whisper acceleration probe",
        "",
        "| Candidate | Backend | Median outer ms | p95 outer ms | Runs |",
        "| --- | --- | ---: | ---: | ---: |",
    ]
    for name in ("cpu", "accelerated"):
        value = candidates[name]
        assert isinstance(value, dict)
        lines.append(
            f"| {name} | {', '.join(value['resolvedBackends'])} | "
            f"{value['medianOuterMs']} | {value['p95OuterMs']} | {value['runs']} |"
        )
    lines.extend(
        [
            "",
            f"Paired median speedup: **{summary['speedupPercent']}%**.",
            f"Paired median reduction: **{summary['medianReductionMs']} ms**.",
            f"Decision: **{str(summary['decision']).upper()}**.",
            "",
            "## Gates",
            "",
        ]
    )
    gates = summary["gates"]
    assert isinstance(gates, dict)
    for name, passed in gates.items():
        lines.append(f"- {'PASS' if passed else 'FAIL'}: `{name}`")
    lines.extend(["", str(summary["claimBoundary"]), ""])
    return "\n".join(lines)


def run_probe(args: argparse.Namespace) -> int:
    prepare_output(args.output_dir)
    try:
        required = [("binary", args.binary), ("model", args.model)]
        required.extend(("audio", audio) for audio in args.audio)
        if args.vad is not None:
            required.append(("VAD", args.vad))
        for label, path in required:
            if not path.is_file():
                raise ValueError(f"{label} is missing: {path}")
        environment = {
            "echo": repo_metadata(),
            "host": host_metadata(),
            "binary": artifact(args.binary),
            "runtimeVersion": runtime_version(
                args.binary, args.timeout, args.vk_driver_files
            ),
            "librarySearchPath": portable_path(args.binary.parent),
            "adjacentLibraries": adjacent_libraries(args.binary),
            "vulkanIcds": vulkan_icds(),
            "selectedVulkanDriverFiles": (
                artifact(args.vk_driver_files)
                if args.vk_driver_files is not None
                else None
            ),
            "model": artifact(args.model),
            "vad": artifact(args.vad) if args.vad is not None else None,
            "backend": args.backend,
            "seed": args.seed,
            "warmups": args.warmups,
            "repeats": args.repeats,
            "mesaShaderCacheDir": (
                portable_path(args.mesa_shader_cache_dir)
                if args.mesa_shader_cache_dir is not None
                else None
            ),
            "mesaShaderCacheReuse": args.reuse_mesa_shader_cache,
            "tuning": {
                "threads": args.threads,
                "beamSize": args.beam_size,
                "bestOf": args.best_of,
                "noFallback": args.no_fallback,
                "language": args.language,
                "promptLength": len(args.prompt),
            },
        }
        if args.mesa_shader_cache_dir is not None:
            prepare_mesa_shader_cache(
                args.mesa_shader_cache_dir, args.reuse_mesa_shader_cache
            )
        warmup_rows = []
        for audio in args.audio:
            for candidate in ("cpu", "accelerated"):
                for warmup in range(args.warmups):
                    row = invoke(args, audio, candidate, -(warmup + 1), 0)
                    row["environment"] = environment
                    warmup_rows.append(row)
        rng = random.Random(args.seed)
        rows: list[dict[str, object]] = []
        for repeat in range(1, args.repeats + 1):
            for audio in args.audio:
                order = ["cpu", "accelerated"]
                rng.shuffle(order)
                for index, candidate in enumerate(order, start=1):
                    row = invoke(args, audio, candidate, repeat, index)
                    row["environment"] = environment
                    rows.append(row)
        summary = summarize(
            rows, args.backend, args.min_speedup_percent, args.min_speedup_ms
        )
        summary["warmups"] = {
            candidate: [
                row["outerMs"] for row in warmup_rows if row["candidate"] == candidate
            ]
            for candidate in ("cpu", "accelerated")
        }
        write_atomic(
            args.output_dir / "warmups.jsonl",
            "".join(json.dumps(row, sort_keys=True) + "\n" for row in warmup_rows),
        )
        write_atomic(
            args.output_dir / "runs.jsonl",
            "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        )
        write_atomic(
            args.output_dir / "summary.json",
            json.dumps(summary, indent=2, sort_keys=True) + "\n",
        )
        rendered = render_summary(summary)
        write_atomic(args.output_dir / "summary.md", rendered)
        write_status(args.output_dir, "complete", decision=summary["decision"])
    except Exception as error:
        write_status(
            args.output_dir,
            "failed",
            errorType=type(error).__name__,
            error=str(error),
        )
        raise
    print(rendered)
    return 1 if args.require_gate and summary["decision"] != "proceed" else 0


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-probe-environment-") as temporary:
        root = Path(temporary)
        binary = root / "whisper-cli"
        cache = root / "cache"
        driver = root / "driver.json"
        binary.write_text("binary", encoding="utf-8")
        cache.mkdir()
        driver.write_text("{}", encoding="utf-8")
        poison = {
            "LD_LIBRARY_PATH": "/poison",
            "LD_PRELOAD": "/poison.so",
            "VK_ICD_FILENAMES": "/poison.json",
            "MESA_VK_DEVICE_SELECT": "ffff:ffff!",
            "DRI_PRIME": "1",
            "CUDA_VISIBLE_DEVICES": "0",
            "GGML_VK_VISIBLE_DEVICES": "0",
        }
        with mock.patch.dict(os.environ, poison):
            environment = runtime_environment(binary, cache, driver)
        assert environment["LD_LIBRARY_PATH"] == str(root)
        assert environment["MESA_SHADER_CACHE_DIR"] == str(cache)
        assert environment["VK_DRIVER_FILES"] == str(driver)
        assert all(name not in environment for name in poison if name != "LD_LIBRARY_PATH")
    vulkan_log = """ggml_vulkan: 0 = Intel Iris Xe (Mesa) | fp16: 1
whisper_backend_init_gpu: using Vulkan0 backend
whisper_print_timings: total time = 42.50 ms
"""
    assert detect_backend(vulkan_log, False) == ("vulkan", "Intel Iris Xe (Mesa)")
    multi_device_log = """ggml_vulkan: 0 = llvmpipe (LLVM) | fp16: 0
ggml_vulkan: 1 = Intel Iris Xe (Mesa) | fp16: 1
whisper_backend_init_gpu: using Vulkan1 backend
"""
    assert detect_backend(multi_device_log, False) == ("vulkan", "Intel Iris Xe (Mesa)")
    software_log = """ggml_vulkan: 0 = llvmpipe (LLVM) | fp16: 0
whisper_backend_init_gpu: using Vulkan0 backend
"""
    assert detect_backend(software_log, False) == ("vulkan", "llvmpipe (LLVM)")
    assert detect_backend("whisper_init: use gpu = 1", False) == ("unknown", None)
    assert parse_timings(vulkan_log) == {"totalMs": 42.5}
    cpu_log = "whisper_model_load: CPU total size = 1 MB\nwhisper_backend_init_gpu: no GPU found\n"
    assert detect_backend(cpu_log, True) == ("cpu", None)
    receipt = {
        "schemaVersion": 1,
        "backend": "vulkan",
        "selectedIndex": 0,
        "vendorId": 32902,
        "deviceId": 39497,
        "apiVersion": 4206831,
        "driverVersion": 123,
        "deviceUUID": "0123456789abcdef0123456789abcdef",
        "driverUUID": "fedcba9876543210fedcba9876543210",
        "pipelineCacheUUID": "00112233445566778899aabbccddeeff",
    }
    receipt_line = RUNTIME_RECEIPT_PREFIX + json.dumps(receipt, separators=(",", ":"))
    assert parse_vulkan_runtime_receipt(vulkan_log + receipt_line + "\n") == receipt
    assert receipt_lines('{"schemaVersion":1,"backend":"unrelated"}\n') == []
    assert runtime_receipt_observation(vulkan_log + receipt_line + "\n", False) == (
        receipt,
        None,
    )
    assert runtime_receipt_observation(cpu_log, True) == (None, None)
    assert "CPU control emitted" in str(
        runtime_receipt_observation(cpu_log + receipt_line + "\n", True)[1]
    )
    bad_uuid = dict(receipt)
    bad_uuid["deviceUUID"] = str(bad_uuid["deviceUUID"]).upper()
    try:
        parse_vulkan_runtime_receipt(
            RUNTIME_RECEIPT_PREFIX + json.dumps(bad_uuid) + "\n"
        )
    except ValueError as error:
        assert "lowercase 32-hex" in str(error)
    else:
        raise AssertionError("uppercase UUID receipt should be rejected")
    for uuid_field in ("deviceUUID", "driverUUID", "pipelineCacheUUID"):
        zero_uuid = dict(receipt)
        zero_uuid[uuid_field] = "0" * 32
        try:
            parse_vulkan_runtime_receipt(
                RUNTIME_RECEIPT_PREFIX + json.dumps(zero_uuid) + "\n"
            )
        except ValueError as error:
            assert f"{uuid_field} must not be all zero" in str(error)
        else:
            raise AssertionError("all-zero UUID receipt should be rejected")
    try:
        parse_vulkan_runtime_receipt(
            RUNTIME_RECEIPT_PREFIX
            + '{"schemaVersion":1,"schemaVersion":1,"backend":"vulkan"}\n'
        )
    except ValueError as error:
        assert "duplicate JSON key" in str(error)
    else:
        raise AssertionError("duplicate receipt fields should be rejected")
    assert "exactly one" in str(runtime_receipt_observation(vulkan_log, False)[1])
    wrong_index = dict(receipt)
    wrong_index["selectedIndex"] = 1
    assert "does not match" in str(
        runtime_receipt_observation(
            vulkan_log + RUNTIME_RECEIPT_PREFIX + json.dumps(wrong_index) + "\n", False
        )[1]
    )
    assert detect_backend(
        vulkan_log + "whisper_backend_init_gpu: using Vulkan1 backend\n", False
    ) == ("unknown", None)
    raw = '{"result":{"language":"en"},"transcription":[{"text":" hello"},{"text":" world"}]}'
    assert parse_transcript(raw) == ("hello world", "en")
    rows = []
    for repeat in range(1, 11):
        cpu = 100.0 + repeat
        gpu = 60.0 + repeat
        for candidate, elapsed, backend in [
            ("cpu", cpu, "cpu"),
            ("accelerated", gpu, "vulkan"),
        ]:
            rows.append(
                {
                    "candidate": candidate,
                    "outerMs": elapsed,
                    "resolvedBackend": backend,
                    "device": "GPU" if backend == "vulkan" else None,
                    "runtimeReceipt": receipt if backend == "vulkan" else None,
                    "runtimeReceiptError": None,
                    "audio": "audio.wav",
                    "audioSha256": "audio",
                    "repeat": repeat,
                    "text": "same",
                }
            )
    result = summarize(rows, "vulkan", 20.0, 20.0)
    assert result["decision"] == "proceed"
    rows[-1]["device"] = None
    rejected = summarize(rows, "vulkan", 20.0, 20.0)
    assert not rejected["gates"]["hardwareDevice"]
    assert rejected["decision"] == "stop"
    rows[-1]["device"] = "GPU"
    rows[-1]["runtimeReceipt"] = None
    rows[-1]["runtimeReceiptError"] = "missing Vulkan runtime receipt"
    receipt_rejected = summarize(rows, "vulkan", 20.0, 20.0)
    assert not receipt_rejected["gates"]["runtimeReceipt"]
    rows[-1]["runtimeReceipt"] = receipt
    rows[-1]["runtimeReceiptError"] = None
    different_receipt = dict(receipt)
    different_receipt["driverVersion"] = 124
    rows[-1]["runtimeReceipt"] = different_receipt
    different_receipt_rejected = summarize(rows, "vulkan", 20.0, 20.0)
    assert not different_receipt_rejected["gates"]["runtimeReceipt"]
    rows[-1]["runtimeReceipt"] = receipt
    too_small = summarize(rows[:2], "vulkan", 20.0, 20.0)
    assert not too_small["gates"]["sampleSize"]
    duplicate = summarize(rows + [dict(rows[0])], "vulkan", 20.0, 20.0)
    assert not duplicate["gates"]["pairedCompleteness"]
    rows[-1]["device"] = "llvmpipe (LLVM)"
    software = summarize(rows, "vulkan", 20.0, 20.0)
    assert not software["gates"]["hardwareDevice"]
    with tempfile.TemporaryDirectory(prefix="echo-acceleration-output-") as temporary:
        output = Path(temporary)
        for name in REPORT_FILES:
            (output / name).write_text("stale", encoding="utf-8")
        prepare_output(output)
        assert all(not (output / name).exists() for name in REPORT_FILES)
        status = json.loads((output / "status.json").read_text(encoding="utf-8"))
        assert status["state"] == "running"
        for name in REPORT_FILES:
            (output / name).write_text("stale again", encoding="utf-8")
        failure_args = argparse.Namespace(
            output_dir=output,
            binary=output / "missing-whisper-cli",
            model=output / "missing-model.bin",
            audio=[output / "missing.wav"],
            vad=None,
        )
        try:
            run_probe(failure_args)
        except ValueError:
            pass
        else:
            raise AssertionError("missing probe inputs should fail")
        status = json.loads((output / "status.json").read_text(encoding="utf-8"))
        assert status["state"] == "failed" and status["errorType"] == "ValueError"
        assert all(not (output / name).exists() for name in REPORT_FILES)
    with tempfile.TemporaryDirectory(prefix="echo-acceleration-cache-") as temporary:
        cache = Path(temporary) / "mesa"
        prepare_mesa_shader_cache(cache, False)
        assert cache.is_dir()
        (cache / "entry").write_text("cache", encoding="utf-8")
        prepare_mesa_shader_cache(cache, True)
        try:
            prepare_mesa_shader_cache(cache, False)
        except ValueError as error:
            assert "must be empty" in str(error)
        else:
            raise AssertionError("fresh cache mode should reject populated cache")
    print("whisper acceleration probe self-test passed")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--self-test", action="store_true")
    result.add_argument("--binary", type=Path)
    result.add_argument("--model", type=Path)
    result.add_argument("--audio", action="append", type=Path, default=[])
    result.add_argument("--vad", type=Path)
    result.add_argument("--backend", choices=["vulkan"], default="vulkan")
    result.add_argument("--language", default="auto")
    result.add_argument("--prompt", default="")
    result.add_argument("--threads", type=int, default=4)
    result.add_argument("--beam-size", type=int, default=1)
    result.add_argument("--best-of", type=int, default=1)
    result.add_argument("--no-fallback", action="store_true")
    result.add_argument("--warmups", type=int, default=1)
    result.add_argument("--repeats", type=int, default=10)
    result.add_argument("--seed", type=int, default=20260824)
    result.add_argument("--timeout", type=int, default=600)
    result.add_argument("--min-speedup-percent", type=float, default=20.0)
    result.add_argument("--min-speedup-ms", type=float, default=500.0)
    result.add_argument("--mesa-shader-cache-dir", type=Path)
    result.add_argument("--vk-driver-files", type=Path)
    result.add_argument(
        "--reuse-mesa-shader-cache",
        action="store_true",
        help="reuse an existing non-empty Mesa cache without deleting or modifying it first",
    )
    result.add_argument("--require-gate", action="store_true")
    result.add_argument("--output-dir", type=Path)
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    missing = [
        name
        for name in ("binary", "model", "output_dir")
        if getattr(args, name) is None
    ]
    if not args.audio:
        missing.append("audio")
    if missing:
        raise ValueError("missing required probe arguments: " + ", ".join(missing))
    if args.threads < 1 or args.beam_size < 1 or args.best_of < 1:
        raise ValueError("threads, beam size, and best-of must be positive")
    if args.warmups < 0 or args.repeats < 1:
        raise ValueError("warmups must be non-negative and repeats must be positive")
    if args.timeout < 1 or args.min_speedup_percent < 0 or args.min_speedup_ms < 0:
        raise ValueError(
            "timeout must be positive and speedup gates must be non-negative"
        )
    if args.reuse_mesa_shader_cache and args.mesa_shader_cache_dir is None:
        raise ValueError("--reuse-mesa-shader-cache requires --mesa-shader-cache-dir")
    if args.vk_driver_files is not None and not args.vk_driver_files.is_file():
        raise ValueError("--vk-driver-files must name a file")
    return run_probe(args)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"probe-whisper-acceleration: {error}", file=sys.stderr)
        raise SystemExit(2) from error
