#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


def strict_json(raw: str, label: str) -> dict[str, object]:
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError(f"{label} is not an object")
    return value


def transcribe(
    binary: Path,
    fixture: Path,
    model: str,
    acceleration: str,
    root: Path,
    language: str = "en",
    extra: list[str] | None = None,
    fault: str | None = None,
) -> dict[str, object]:
    environment = dict(os.environ)
    environment.update(
        {
            "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_DATA_HOME": str(root / "data"),
        }
    )
    environment.pop("ECHO_WHISPER_ACCELERATION", None)
    if fault is None:
        environment.pop("ECHO_WHISPER_TEST_FAULT", None)
    else:
        environment["ECHO_WHISPER_TEST_FAULT"] = fault
    command = [
        str(binary),
        "transcribe",
        str(fixture),
        "--engine",
        "whisper",
        "--model",
        model,
        "--language",
        language,
        "--whisper-acceleration",
        acceleration,
        "--format",
        "json",
    ]
    if extra:
        command.extend(extra)
    result = subprocess.run(
        command,
        env=environment,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return strict_json(result.stdout, f"{acceleration} transcription JSON")


def backend(payload: dict[str, object]) -> str:
    return payload["whisper"]["runtime"]["backend"]


def selection(payload: dict[str, object]) -> dict[str, object]:
    return payload["whisper"].get("selection") or {}


def verify_live(args: argparse.Namespace) -> dict[str, object]:
    output = args.output.resolve()
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    output.mkdir(parents=True)
    binary = args.echo_binary.resolve()
    fixture = args.fixture.resolve()
    cpu = transcribe(binary, fixture, args.model, "cpu", output / "cpu")
    if backend(cpu) != "cpu":
        raise ValueError("CPU mode did not use managed CPU")
    gpu = transcribe(binary, fixture, args.model, "gpu", output / "gpu")
    if backend(gpu) != "vulkan":
        raise ValueError("GPU mode did not use Vulkan")
    auto_cold = transcribe(binary, fixture, args.model, "auto", output / "auto")
    if backend(auto_cold) not in {"cpu", "vulkan"}:
        raise ValueError("cold Auto did not use CPU or Vulkan")
    if backend(gpu) == "vulkan" and backend(auto_cold) != "vulkan":
        raise ValueError("cold Auto on a GPU host did not use Vulkan")
    if selection(auto_cold).get("calibrationPending") is True:
        raise ValueError("cold Auto reported pending calibration")
    auto_warm = transcribe(binary, fixture, args.model, "auto", output / "auto")
    if selection(auto_warm).get("calibrationPending") is True:
        raise ValueError("warm Auto reported pending calibration")
    if backend(gpu) == "vulkan" and backend(auto_warm) != "vulkan":
        raise ValueError("warm Auto on a GPU host did not use Vulkan")
    auto_language = transcribe(
        binary, fixture, args.model, "auto", output / "auto-language", language="auto"
    )
    if backend(gpu) == "vulkan" and backend(auto_language) != "vulkan":
        raise ValueError("automatic language did not use Vulkan on a GPU host")
    hints_root = output / "auto-hints"
    (hints_root / "data/echo").mkdir(parents=True)
    (hints_root / "data/echo/dictionary.json").write_text(
        json.dumps(
            {
                "entries": [
                    {
                        "spoken": "clawed code",
                        "written": "Claude Code",
                        "created_at": 1,
                    }
                ]
            }
        )
        + "\n"
    )
    auto_hints = transcribe(binary, fixture, args.model, "auto", hints_root)
    if backend(gpu) == "vulkan" and backend(auto_hints) != "vulkan":
        raise ValueError("recognition hints did not use Vulkan on a GPU host")
    switched = transcribe(binary, fixture, args.model, "cpu", output / "switch")
    if backend(switched) != "cpu":
        raise ValueError("mode switch to CPU did not use CPU")
    switched_gpu = transcribe(binary, fixture, args.model, "gpu", output / "switch")
    if backend(switched_gpu) != "vulkan":
        raise ValueError("mode switch to GPU did not use Vulkan")
    gpu_unavailable = transcribe(
        binary, fixture, args.model, "gpu", output / "gpu-unavailable", fault="no-devices"
    )
    if backend(gpu_unavailable) != "cpu":
        raise ValueError("GPU mode with no devices did not recover on CPU")
    report = {
        "schemaVersion": 1,
        "echoCommit": args.commit,
        "lanes": {
            "cpuMode": "PASS",
            "autoCold": "PASS",
            "autoWarm": "PASS" if backend(auto_warm) in {"cpu", "vulkan"} else "FAIL",
            "gpuMode": "PASS",
            "autoLanguage": "PASS",
            "autoHints": "PASS",
            "modeSwitch": "PASS",
            "gpuUnavailableSimulated": "PASS",
        },
        "autoWarmBackend": backend(auto_warm),
        "autoWarmSelection": selection(auto_warm),
        "gpuDevice": gpu["whisper"]["runtime"].get("device"),
    }
    if report["lanes"]["autoWarm"] != "PASS":
        raise ValueError("warm Auto did not produce a CPU or Vulkan backend")
    (output / "cpu.json").write_text(json.dumps(cpu, indent=2) + "\n")
    (output / "auto-cold.json").write_text(json.dumps(auto_cold, indent=2) + "\n")
    (output / "auto-warm.json").write_text(json.dumps(auto_warm, indent=2) + "\n")
    (output / "gpu.json").write_text(json.dumps(gpu, indent=2) + "\n")
    (output / "auto-language.json").write_text(json.dumps(auto_language, indent=2) + "\n")
    (output / "auto-hints.json").write_text(json.dumps(auto_hints, indent=2) + "\n")
    (output / "mode-switch-cpu.json").write_text(json.dumps(switched, indent=2) + "\n")
    (output / "mode-switch-gpu.json").write_text(json.dumps(switched_gpu, indent=2) + "\n")
    (output / "gpu-unavailable.json").write_text(json.dumps(gpu_unavailable, indent=2) + "\n")
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def self_test() -> None:
    assert backend(
        {"whisper": {"runtime": {"backend": "cpu"}, "selection": {}}}
    ) == "cpu"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-live", action="store_true")
    parser.add_argument("--echo-binary", type=Path)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--model", default="small")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--commit")
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.verify_live:
            if any(
                value is None
                for value in (args.echo_binary, args.fixture, args.output, args.commit)
            ):
                parser.error("--verify-live requires binary, fixture, output, and commit")
            print(json.dumps(verify_live(args), indent=2))
        else:
            parser.error("choose --self-test or --verify-live")
    except (OSError, TypeError, ValueError, subprocess.SubprocessError) as error:
        print(f"verify-whisper-acceleration-modes: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
