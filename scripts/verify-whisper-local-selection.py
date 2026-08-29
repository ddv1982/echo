#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import hashlib
import subprocess
import sys
import time
from pathlib import Path


def strict_json(raw: str, label: str) -> dict[str, object]:
    def unique(pairs: list[tuple[str, object]]) -> dict[str, object]:
        value = {}
        for key, item in pairs:
            if key in value:
                raise ValueError(f"{label} has duplicate key {key!r}")
            value[key] = item
        return value

    value = json.loads(raw, object_pairs_hook=unique)
    if not isinstance(value, dict):
        raise ValueError(f"{label} is not an object")
    return value


def run_transcription(
    binary: Path,
    fixture: Path,
    model: str,
    mode: str,
    root: Path,
    fault: str | None = None,
) -> tuple[dict[str, object], float]:
    environment = dict(os.environ)
    environment.update(
        {
            "ECHO_WHISPER_ACCELERATION": mode,
            "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_DATA_HOME": str(root / "data"),
        }
    )
    if fault is not None:
        environment["ECHO_WHISPER_TEST_FAULT"] = fault
    else:
        environment.pop("ECHO_WHISPER_TEST_FAULT", None)
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
    return strict_json(result.stdout, "transcription JSON"), time.monotonic() - started


def selection(payload: dict[str, object]) -> dict[str, object]:
    return payload["whisper"]["selection"]


def runtime_backend(payload: dict[str, object]) -> str:
    return payload["whisper"]["runtime"]["backend"]


def state_root(root: Path) -> Path:
    return root / "data/echo/whisper-local-selection/v1"


def clone_calibrated_state(source_root: Path, destination_root: Path) -> None:
    destination = state_root(destination_root)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(state_root(source_root), destination)


def run_test(repo: Path, name: str, build_commit: str | None = None) -> str:
    environment = dict(os.environ)
    dependency_prefix = (
        repo.parent / "echo/target/pr16-1/deps/usr/lib/x86_64-linux-gnu/pkgconfig"
    )
    if dependency_prefix.is_dir():
        environment["PKG_CONFIG_PATH"] = str(dependency_prefix)
    if build_commit is not None:
        environment["ECHO_BUILD_COMMIT"] = build_commit
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
    portable = strict_json(package.read_text(encoding="utf-8"), "portable selection")
    binding = strict_json(
        (package.parent / "portable-selection-binding.v1.json").read_text(
            encoding="utf-8"
        ),
        "portable selection binding",
    )
    if hashlib.sha256(binary.read_bytes()).hexdigest() != binding["echoBinarySha256"]:
        raise ValueError("portable selection package belongs to another Echo binary")
    repo = Path(__file__).resolve().parent.parent
    source_check = subprocess.run(
        [
            "git",
            "diff",
            "--quiet",
            args.trunk_commit,
            "--",
            "crates/echo/src/stt/whisper_acceleration.rs",
        ],
        cwd=repo,
    )
    if source_check.returncode != 0:
        raise ValueError("production Whisper decision differs from the trunk baseline")

    trunk, trunk_wall = run_transcription(
        args.trunk_binary.resolve(), fixture, args.model, "cpu", output / "trunk"
    )
    if runtime_backend(trunk) != "cpu":
        raise ValueError("trunk baseline did not use managed CPU")

    baseline_root = output / "cold"
    cpu, cpu_wall = run_transcription(binary, fixture, args.model, "cpu", baseline_root)
    cold, cold_wall = run_transcription(
        binary, fixture, args.model, "auto", baseline_root
    )
    if runtime_backend(cpu) != "cpu":
        raise ValueError("cold baseline did not use managed CPU")
    if runtime_backend(cold) not in {"cpu", "vulkan"}:
        raise ValueError("cold Auto did not use CPU or Vulkan")
    cold_selection = selection(cold)
    if cold_selection.get("calibrationPending") is True:
        raise ValueError("cold Auto reported pending calibration")
    gpu_cold, gpu_cold_wall = run_transcription(
        binary, fixture, args.model, "gpu", output / "gpu-cold"
    )
    if runtime_backend(gpu_cold) != "vulkan":
        raise ValueError("explicit GPU with an empty local store did not use Vulkan")
    if runtime_backend(gpu_cold) == "vulkan" and runtime_backend(cold) != "vulkan":
        raise ValueError("cold Auto on a GPU host did not use Vulkan")
    warm, warm_wall = run_transcription(
        binary, fixture, args.model, "auto", baseline_root
    )
    gpu, gpu_wall = run_transcription(binary, fixture, args.model, "gpu", baseline_root)
    warm_selection = selection(warm)
    gpu_selection = selection(gpu)
    if (
        runtime_backend(gpu) != "vulkan"
        or runtime_backend(warm) != "vulkan"
        or warm_selection["cachedDecision"] != "vulkan"
        or warm_selection["calibrationPending"] is not False
        or gpu_selection["localKey"] != warm_selection["localKey"]
    ):
        raise ValueError("warm Auto or explicit GPU did not share the Vulkan route")
    expected_failures = {
        "wrong-receipt": "receiptMismatch",
        "backend-fallback": "cpuFallback",
        "gpu-timeout": "timeout",
    }
    failure_payloads = {}
    for fault, expected_reason in expected_failures.items():
        lane_root = output / fault
        clone_calibrated_state(baseline_root, lane_root)
        payload, _ = run_transcription(
            binary, fixture, args.model, "gpu", lane_root, fault
        )
        recovery = payload["whisper"].get("recovery")
        if (
            runtime_backend(payload) != "cpu"
            or recovery is None
            or recovery.get("acceleratedAttempted") is not True
            or recovery.get("fallbackReason") != expected_reason
        ):
            raise ValueError(f"{fault} did not quarantine and recover exactly once")
        if not list((state_root(lane_root) / "keys").glob("*/quarantine/*.json")):
            raise ValueError(f"{fault} wrote no immutable local quarantine")
        failure_payloads[fault] = payload

    corrupt_root = output / "corrupt-cache"
    clone_calibrated_state(baseline_root, corrupt_root)
    [view] = list((state_root(corrupt_root) / "views/models").glob("*.json"))
    view.write_text("{corrupt\n", encoding="utf-8")
    corrupt, _ = run_transcription(binary, fixture, args.model, "auto", corrupt_root)
    if view.read_text(encoding="utf-8") != "{corrupt\n":
        raise ValueError("corrupt cache was not preserved")
    if selection(corrupt).get("calibrationPending") is True:
        raise ValueError("corrupt cache Auto reported pending calibration")
    if runtime_backend(gpu) == "vulkan" and runtime_backend(corrupt) != "vulkan":
        raise ValueError("corrupt cache Auto on a GPU host did not use Vulkan")

    driver_root = output / "driver-change"
    clone_calibrated_state(baseline_root, driver_root)
    route_files = list((state_root(driver_root) / "scopes").glob("*/*/routes/*.json"))
    driver, _ = run_transcription(
        binary, fixture, args.model, "gpu", driver_root, "driver-change"
    )
    if (
        runtime_backend(driver) != "vulkan"
        or selection(driver)["localKey"] == warm_selection["localKey"]
        or any(not route.is_file() for route in route_files)
    ):
        raise ValueError("driver fingerprint did not rotate local selection safely")

    reorder_root = output / "device-reorder"
    clone_calibrated_state(baseline_root, reorder_root)
    reordered, _ = run_transcription(
        binary, fixture, args.model, "gpu", reorder_root, "device-reorder"
    )
    if (
        runtime_backend(reordered) != "vulkan"
        or selection(reordered)["localKey"] != warm_selection["localKey"]
    ):
        raise ValueError("device reorder did not follow the stable UUID")

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
        concurrent.append(strict_json(stdout, "concurrent transcription JSON"))
    if any(runtime_backend(payload) not in {"cpu", "vulkan"} for payload in concurrent):
        raise ValueError("concurrent Auto did not use CPU or Vulkan")

    perf_output = run_test(
        repo,
        "stt::whisper_planner::tests::cached_warm_planner_p95_is_bounded",
    )
    match = re.search(r"cached_warm_planner_p95_us=(\d+)", perf_output)
    if match is None:
        raise ValueError("warm planner performance test emitted no p95")
    cached_p95_ms = int(match.group(1)) / 1000
    if cached_p95_ms > 25:
        raise ValueError(f"cached planner p95 is {cached_p95_ms:.3f} ms")
    decision_output = run_test(
        repo,
        "stt::whisper_acceleration::tests::production_decision_p95_is_bounded",
        args.trunk_commit,
    )
    match = re.search(r"production_decision_p95_ns=(\d+)", decision_output)
    if match is None:
        raise ValueError("production decision performance test emitted no p95")
    decision_p95_ms = int(match.group(1)) / 1_000_000

    delay_percent = ((cold_wall / cpu_wall) - 1) * 100
    if delay_percent > 5:
        raise ValueError(f"cold Auto delay is {delay_percent:.3f} percent")
    report = {
        "schemaVersion": 1,
        "echoCommit": binding["echoCommit"],
        "echoBinarySha256": binding["echoBinarySha256"],
        "portableSelectionBindingSha256": hashlib.sha256(
            (package.parent / "portable-selection-binding.v1.json").read_bytes()
        ).hexdigest(),
        "runtimeArtifactId": strict_json(
            (package.parent / "runtime/build-receipt.json").read_text(encoding="utf-8"),
            "runtime build receipt",
        )["artifactId"],
        "executionArtifactId": portable["executionArtifact"]["id"],
        "inferenceContractIds": [
            contract["id"] for contract in portable["inferenceContracts"]
        ],
        "cold": {
            "cpuWallMs": round(cpu_wall * 1000, 3),
            "autoWallMs": round(cold_wall * 1000, 3),
            "delayPercent": round(delay_percent, 3),
        },
        "trunk": {
            "commit": args.trunk_commit,
            "productionDecisionSourceUnchanged": True,
            "productionDecisionP95Ms": decision_p95_ms,
            "managedCpuFirstResultMs": round(trunk_wall * 1000, 3),
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
        "faultInjection": "debug-build-only installed CLI",
    }
    (output / "cpu.json").write_text(json.dumps(cpu, indent=2) + "\n")
    (output / "trunk.json").write_text(json.dumps(trunk, indent=2) + "\n")
    (output / "auto-cold.json").write_text(json.dumps(cold, indent=2) + "\n")
    (output / "auto-warm.json").write_text(json.dumps(warm, indent=2) + "\n")
    (output / "gpu-cold.json").write_text(json.dumps(gpu_cold, indent=2) + "\n")
    (output / "gpu-warm.json").write_text(json.dumps(gpu, indent=2) + "\n")
    for fault, payload in failure_payloads.items():
        (output / f"{fault}.json").write_text(json.dumps(payload, indent=2) + "\n")
    (output / "corrupt-cache.json").write_text(json.dumps(corrupt, indent=2) + "\n")
    (output / "driver-change.json").write_text(json.dumps(driver, indent=2) + "\n")
    (output / "device-reorder.json").write_text(json.dumps(reordered, indent=2) + "\n")
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
    parser.add_argument("--trunk-binary", type=Path)
    parser.add_argument("--trunk-commit")
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--model", default="small")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.verify_live:
            if any(
                value is None
                for value in (
                    args.echo_binary,
                    args.trunk_binary,
                    args.trunk_commit,
                    args.fixture,
                    args.output,
                )
            ):
                parser.error(
                    "--verify-live requires both binaries, trunk commit, fixture, and output"
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
