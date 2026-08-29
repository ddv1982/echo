#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import hashlib
import subprocess
import sys
import time
from pathlib import Path


def run_transcription(
    binary: Path,
    fixture: Path,
    model: str,
    mode: str,
    root: Path,
) -> tuple[dict[str, object], float]:
    environment = dict(os.environ)
    environment.update(
        {
            "ECHO_WHISPER_ACCELERATION": mode,
            "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_DATA_HOME": str(root / "data"),
        }
    )
    (root / "config").mkdir(parents=True, exist_ok=True)
    (root / "data").mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    result = subprocess.run(
        [
            str(binary),
            "transcribe",
            str(fixture),
            "--engine",
            "whisper",
            "--model",
            model,
            "--language",
            "en",
            "--format",
            "json",
        ],
        env=environment,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return json.loads(result.stdout), time.monotonic() - started


def selection(payload: dict[str, object]) -> dict[str, object]:
    return payload["whisper"]["selection"]


def runtime_backend(payload: dict[str, object]) -> str:
    return payload["whisper"]["runtime"]["backend"]


def state_root(root: Path) -> Path:
    return root / "data/echo/whisper-local-selection/v1"


def wait_for_calibration(root: Path, expected_jobs: int, timeout: float = 180) -> None:
    deadline = time.monotonic() + timeout
    state = state_root(root)
    while time.monotonic() < deadline:
        jobs = list((state / "jobs").glob("*.json"))
        results = list((state / "job-results").glob("*.json"))
        routes = list((state / "scopes").glob("*/*/routes/*.json"))
        if len(jobs) == expected_jobs and len(results) == expected_jobs and routes:
            return
        time.sleep(0.25)
    raise ValueError("background calibration did not finish within its deadline")


def run_test(repo: Path, name: str) -> str:
    environment = dict(os.environ)
    dependency_prefix = (
        repo.parent / "echo/target/pr16-1/deps/usr/lib/x86_64-linux-gnu/pkgconfig"
    )
    if dependency_prefix.is_dir():
        environment["PKG_CONFIG_PATH"] = str(dependency_prefix)
    result = subprocess.run(
        ["cargo", "test", "-p", "echo", name, "--", "--nocapture"],
        cwd=repo,
        env=environment,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return result.stdout


def verify_live(args: argparse.Namespace) -> dict[str, object]:
    output = args.output.resolve()
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    output.mkdir(parents=True)
    binary = args.echo_binary.resolve()
    fixture = args.fixture.resolve()
    package = binary.parent / "whisper-acceleration/portable-selection.v1.json"
    portable = json.loads(package.read_text(encoding="utf-8"))
    binding = json.loads(
        (package.parent / "portable-selection-binding.v1.json").read_text(
            encoding="utf-8"
        )
    )
    if hashlib.sha256(binary.read_bytes()).hexdigest() != binding["echoBinarySha256"]:
        raise ValueError("portable selection package belongs to another Echo binary")

    baseline_root = output / "cold"
    cpu, cpu_wall = run_transcription(binary, fixture, args.model, "cpu", baseline_root)
    cold, cold_wall = run_transcription(
        binary, fixture, args.model, "auto", baseline_root
    )
    if runtime_backend(cpu) != "cpu" or runtime_backend(cold) != "cpu":
        raise ValueError("cold baseline or Auto did not use managed CPU")
    cold_selection = selection(cold)
    if (
        cold_selection["cachedDecision"] != "unknown"
        or cold_selection["calibrationPending"] is not True
    ):
        raise ValueError("cold Auto did not report pending calibration")
    wait_for_calibration(baseline_root, 1)
    gpu_cold, gpu_cold_wall = run_transcription(
        binary, fixture, args.model, "gpu", output / "gpu-cold"
    )
    if runtime_backend(gpu_cold) != "vulkan":
        raise ValueError("explicit GPU with an empty local store did not use Vulkan")
    warm, warm_wall = run_transcription(
        binary, fixture, args.model, "auto", baseline_root
    )
    gpu, gpu_wall = run_transcription(binary, fixture, args.model, "gpu", baseline_root)
    warm_selection = selection(warm)
    gpu_selection = selection(gpu)
    if (
        runtime_backend(warm) != "cpu"
        or warm_selection["cachedDecision"] != "vulkan"
        or warm_selection["calibrationPending"] is not False
        or runtime_backend(gpu) != "vulkan"
        or gpu_selection["localKey"] != warm_selection["localKey"]
    ):
        raise ValueError("warm Auto or explicit GPU differs from the calibrated route")

    concurrent_root = output / "concurrent"
    command = [
        str(binary),
        "transcribe",
        str(fixture),
        "--engine",
        "whisper",
        "--model",
        args.model,
        "--language",
        "en",
        "--format",
        "json",
    ]
    environment = dict(os.environ)
    environment.update(
        {
            "ECHO_WHISPER_ACCELERATION": "auto",
            "XDG_CONFIG_HOME": str(concurrent_root / "config"),
            "XDG_DATA_HOME": str(concurrent_root / "data"),
        }
    )
    (concurrent_root / "config").mkdir(parents=True)
    (concurrent_root / "data").mkdir(parents=True)
    processes = [
        subprocess.Popen(
            command,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for _ in range(2)
    ]
    concurrent = []
    for process in processes:
        stdout, stderr = process.communicate(timeout=60)
        if process.returncode != 0:
            raise ValueError(f"concurrent Auto failed: {stderr}")
        concurrent.append(json.loads(stdout))
    wait_for_calibration(concurrent_root, 2)
    state = state_root(concurrent_root)
    if len(list((state / "keys").glob("*/calibration/*.json"))) != 1:
        raise ValueError("concurrent owners repeated calibration")

    repo = Path(__file__).resolve().parent.parent
    unit_lanes = {
        "wrongReceipt": "stt::whisper_recovery::tests::every_accelerator_failure_quarantines_once_and_runs_one_cpu_retry",
        "backendFallback": "stt::whisper_recovery::tests::every_accelerator_failure_quarantines_once_and_runs_one_cpu_retry",
        "gpuTimeout": "stt::whisper_recovery::tests::every_accelerator_failure_quarantines_once_and_runs_one_cpu_retry",
        "corruptCache": "stt::whisper_accel_cache::tests::corrupt_record_is_preserved_and_fails_closed",
        "driverChange": "stt::whisper_accel_cache::tests::key_changes_for_identity_but_not_selected_index",
        "deviceReorder": "stt::backend::vulkan::tests::stable_receipt_ignores_diagnostic_index",
    }
    for test in sorted(set(unit_lanes.values())):
        run_test(repo, test)
    perf_output = run_test(
        repo,
        "stt::whisper_accel_cache::tests::model_view_is_disposable_and_rotates_on_file_change",
    )
    match = re.search(r"cached_model_view_p95_us=(\d+)", perf_output)
    if match is None:
        raise ValueError("warm planner performance test emitted no p95")
    cached_p95_ms = int(match.group(1)) / 1000
    if cached_p95_ms > 25:
        raise ValueError(f"cached planner p95 is {cached_p95_ms:.3f} ms")

    delay_percent = ((cold_wall / cpu_wall) - 1) * 100
    if delay_percent > 5:
        raise ValueError(f"cold Auto delay is {delay_percent:.3f} percent")
    report = {
        "schemaVersion": 1,
        "executionArtifactId": portable["executionArtifact"]["id"],
        "inferenceContractIds": [
            contract["id"] for contract in portable["inferenceContracts"]
        ],
        "cold": {
            "cpuWallMs": round(cpu_wall * 1000, 3),
            "autoWallMs": round(cold_wall * 1000, 3),
            "delayPercent": round(delay_percent, 3),
        },
        "warm": {
            "autoWallMs": round(warm_wall * 1000, 3),
            "gpuWallMs": round(gpu_wall * 1000, 3),
            "localKey": warm_selection["localKey"],
        },
        "gpuColdWallMs": round(gpu_cold_wall * 1000, 3),
        "cachedPlannerP95Ms": cached_p95_ms,
        "lanes": {
            "autoCold": "PASS",
            "autoWarm": "PASS",
            "gpuCold": "PASS",
            "wrongReceipt": "PASS",
            "backendFallback": "PASS",
            "gpuTimeout": "PASS",
            "corruptCache": "PASS",
            "driverChange": "PASS",
            "deviceReorder": "PASS",
            "concurrentState": "PASS",
        },
        "unitLaneTests": unit_lanes,
    }
    (output / "cpu.json").write_text(json.dumps(cpu, indent=2) + "\n")
    (output / "auto-cold.json").write_text(json.dumps(cold, indent=2) + "\n")
    (output / "auto-warm.json").write_text(json.dumps(warm, indent=2) + "\n")
    (output / "gpu-cold.json").write_text(json.dumps(gpu_cold, indent=2) + "\n")
    (output / "gpu-warm.json").write_text(json.dumps(gpu, indent=2) + "\n")
    (output / "concurrent.json").write_text(json.dumps(concurrent, indent=2) + "\n")
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def self_test() -> None:
    payload = {
        "whisper": {
            "runtime": {"backend": "cpu"},
            "selection": {"cachedDecision": "unknown"},
        }
    }
    assert runtime_backend(payload) == "cpu"
    assert selection(payload)["cachedDecision"] == "unknown"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-live", action="store_true")
    parser.add_argument("--echo-binary", type=Path)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--model", default="small")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.verify_live:
            if args.echo_binary is None or args.fixture is None or args.output is None:
                parser.error(
                    "--verify-live requires --echo-binary, --fixture, and --output"
                )
            print(json.dumps(verify_live(args), indent=2))
        else:
            parser.error("choose --self-test or --verify-live")
    except (OSError, TypeError, ValueError, subprocess.SubprocessError) as error:
        print(f"verify-whisper-local-selection: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
