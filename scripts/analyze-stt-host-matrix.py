#!/usr/bin/env python3
"""Evaluate paired CPU/accelerator Echo benchmark rows against Phase 2 gates."""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path

PRODUCT_CLASSES = {
    "dictation",
    "technical-identifiers",
    "fast-speech",
    "quiet-speech",
    "noise",
    "false-starts",
    "silence",
    "nonspeech",
}


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def candidate_rows(rows: list[dict[str, object]], label: str) -> list[dict[str, object]]:
    selected = [row for row in rows if row.get("candidate") == label]
    if not selected:
        raise ValueError(f"candidate is absent from runs: {label}")
    return selected


def pair_key(row: dict[str, object]) -> tuple[str, int]:
    return str(row["utterance"]), int(row["repeat"])


def execution_identities(rows: list[dict[str, object]]) -> set[str]:
    identities = set()
    for row in rows:
        engine = row.get("engine") or {}
        echo_binary = row.get("echoBinary") or {}
        runtime = row.get("runtimeArtifact") or {}
        model = row.get("modelArtifact") or {}
        whisper = row.get("whisper") or {}
        tuning = whisper.get("tuning") or {}
        identity = {
            "echoVersion": row.get("echoVersion"),
            "echoCommit": row.get("echoCommit"),
            "echoDirty": row.get("echoDirty"),
            "echoBinarySha256": echo_binary.get("sha256"),
            "host": row.get("host"),
            "seed": row.get("seed"),
            "warmups": row.get("warmups"),
            "engineModel": engine.get("model"),
            "runtimeSha256": runtime.get("sha256"),
            "modelSha256": model.get("sha256"),
            "tuning": tuning,
        }
        identities.add(json.dumps(identity, sort_keys=True))
    return identities


def execution_identity_complete(row: dict[str, object]) -> bool:
    echo_binary = row.get("echoBinary") or {}
    runtime = row.get("runtimeArtifact") or {}
    model = row.get("modelArtifact") or {}
    engine = row.get("engine") or {}
    whisper = row.get("whisper") or {}
    strings = [
        row.get("echoVersion"),
        row.get("echoCommit"),
        echo_binary.get("sha256"),
        engine.get("model"),
        runtime.get("sha256"),
        model.get("sha256"),
    ]
    return (
        isinstance(row.get("echoDirty"), bool)
        and all(isinstance(value, str) and value for value in strings)
        and isinstance(row.get("host"), dict)
        and bool(row["host"])
        and isinstance(row.get("seed"), int)
        and isinstance(row.get("warmups"), int)
        and isinstance(whisper.get("tuning"), dict)
        and bool(whisper["tuning"])
    )


def coverage_complete(corpus: dict[str, object]) -> bool:
    coverage = corpus.get("coverage")
    utterances = corpus.get("utterances")
    if not isinstance(coverage, dict) or not isinstance(utterances, list):
        return False
    pending = coverage.get("pending")
    required_languages = coverage.get("requiredLanguages")
    required_classes = coverage.get("requiredClasses")
    if not isinstance(pending, list) or pending:
        return False
    if (
        not isinstance(required_languages, list)
        or not required_languages
        or not all(isinstance(value, str) and value for value in required_languages)
    ):
        return False
    if (
        not isinstance(required_classes, list)
        or not all(isinstance(value, str) and value for value in required_classes)
        or not PRODUCT_CLASSES.issubset(required_classes)
    ):
        return False
    if len(utterances) < 20 or not all(isinstance(item, dict) for item in utterances):
        return False
    present_languages = {item.get("language") for item in utterances}
    present_classes = {item.get("class") for item in utterances}
    return set(required_languages).issubset(present_languages) and set(required_classes).issubset(
        present_classes
    )


def summarize(
    rows: list[dict[str, object]],
    cpu_label: str,
    accelerated_label: str,
    expected_backend: str,
    coverage_complete: bool,
) -> dict[str, object]:
    cpu = candidate_rows(rows, cpu_label)
    accelerated = candidate_rows(rows, accelerated_label)
    cpu_by_key = {pair_key(row): row for row in cpu}
    accelerated_by_key = {pair_key(row): row for row in accelerated}
    complete_pairs = (
        len(cpu_by_key) == len(cpu)
        and len(accelerated_by_key) == len(accelerated)
        and cpu_by_key.keys() == accelerated_by_key.keys()
    )
    shared_keys = cpu_by_key.keys() & accelerated_by_key.keys()
    if not shared_keys:
        raise ValueError("candidates have no paired observations")
    pairs = [(cpu_by_key[key], accelerated_by_key[key]) for key in sorted(shared_keys)]
    pair_integrity = complete_pairs and all(
        all(
            control.get(field) == candidate.get(field)
            for field in (
                "utterance",
                "repeat",
                "language",
                "audioSha256",
                "reference",
                "referenceWords",
            )
        )
        for control, candidate in pairs
    )
    counts: dict[str, int] = defaultdict(int)
    for utterance, _repeat in cpu_by_key:
        counts[utterance] += 1
    sample_size = complete_pairs and bool(counts) and all(count >= 10 for count in counts.values())
    reductions = [float(control["outerMs"]) - float(candidate["outerMs"]) for control, candidate in pairs]
    speedups = [
        100 * (float(control["outerMs"]) - float(candidate["outerMs"])) / float(control["outerMs"])
        for control, candidate in pairs
    ]
    cpu_outer = [float(row["outerMs"]) for row in cpu]
    accelerated_outer = [float(row["outerMs"]) for row in accelerated]
    language_rows: dict[str, dict[str, object]] = {}
    quality_ok = True
    for language in sorted({str(row["language"]) for row in rows}):
        control = [row for row in cpu if row["language"] == language]
        candidate = [row for row in accelerated if row["language"] == language]
        control_words = sum(int(row["referenceWords"]) for row in control)
        candidate_words = sum(int(row["referenceWords"]) for row in candidate)
        if control_words != candidate_words or control_words == 0:
            raise ValueError(f"invalid reference-word totals for language {language}")
        control_wer = sum(int(row["wordErrors"]) for row in control) / control_words
        candidate_wer = sum(int(row["wordErrors"]) for row in candidate) / candidate_words
        delta = candidate_wer - control_wer
        passed = delta <= 0.005
        quality_ok = quality_ok and passed
        language_rows[language] = {
            "cpuWer": round(control_wer, 6),
            "acceleratedWer": round(candidate_wer, 6),
            "deltaPercentagePoints": round(delta * 100, 3),
            "qualityGate": passed,
            "cpuMedianOuterMs": round(statistics.median(float(row["outerMs"]) for row in control), 3),
            "acceleratedMedianOuterMs": round(
                statistics.median(float(row["outerMs"]) for row in candidate), 3
            ),
        }
    cpu_backends = {
        str((row.get("whisper") or {}).get("runtime", {}).get("backend")) for row in cpu
    }
    accelerated_backends = {
        str((row.get("whisper") or {}).get("runtime", {}).get("backend"))
        for row in accelerated
    }
    devices = [
        (row.get("whisper") or {}).get("runtime", {}).get("device") for row in accelerated
    ]
    backend_truth = cpu_backends == {"cpu"} and accelerated_backends == {expected_backend}
    cpu_identities = execution_identities(cpu)
    accelerated_identities = execution_identities(accelerated)
    identity_match = (
        len(cpu_identities) == 1
        and cpu_identities == accelerated_identities
        and all(execution_identity_complete(row) for row in rows)
    )
    hardware_device = bool(devices) and all(
        device
        and not any(name in str(device).casefold() for name in ("lavapipe", "llvmpipe", "swiftshader"))
        for device in devices
    ) and len({str(device) for device in devices}) == 1
    new_hallucinations = sum(
        not bool(control.get("hallucinatedSilence"))
        and bool(candidate.get("hallucinatedSilence"))
        for control, candidate in pairs
    )
    median_reduction = statistics.median(reductions)
    median_speedup = statistics.median(speedups)
    gates = {
        "completePairs": complete_pairs,
        "pairIntegrity": pair_integrity,
        "sampleSize": sample_size,
        "backendTruth": backend_truth,
        "identityMatch": identity_match,
        "hardwareDevice": hardware_device,
        "medianReduction": median_reduction >= 500,
        "medianSpeedup": median_speedup >= 20,
        "p95Improved": percentile(accelerated_outer, 0.95) < percentile(cpu_outer, 0.95),
        "perLanguageQuality": quality_ok,
        "noNewHallucinations": new_hallucinations == 0,
        "coverageComplete": coverage_complete,
    }
    return {
        "schemaVersion": 1,
        "cpuCandidate": cpu_label,
        "acceleratedCandidate": accelerated_label,
        "expectedBackend": expected_backend,
        "runsPerCandidate": len(cpu),
        "cpuMedianOuterMs": round(statistics.median(cpu_outer), 3),
        "cpuP95OuterMs": round(percentile(cpu_outer, 0.95), 3),
        "acceleratedMedianOuterMs": round(statistics.median(accelerated_outer), 3),
        "acceleratedP95OuterMs": round(percentile(accelerated_outer, 0.95), 3),
        "medianReductionMs": round(median_reduction, 3),
        "medianSpeedupPercent": round(median_speedup, 3),
        "newHallucinations": new_hallucinations,
        "languages": language_rows,
        "gates": gates,
        "decision": "proceed" if all(gates.values()) else "stop",
        "claimBoundary": (
            "This warmed, populated-cache clean-read slice cannot satisfy production coverage "
            "for dictation, silence, nonspeech, noise, fast speech, quiet speech, technical "
            "identifiers, and false starts. Fresh-cache, reset-repeat, explicit driver/ICD, "
            "other-hardware, Turbo, memory, power, and failure-path evidence also remain pending."
        ),
    }


def render(summary: dict[str, object]) -> str:
    lines = [
        "# Echo host matrix decision",
        "",
        "| Mode | Median outer ms | p95 outer ms |",
        "| --- | ---: | ---: |",
        f"| CPU | {summary['cpuMedianOuterMs']} | {summary['cpuP95OuterMs']} |",
        f"| Accelerated | {summary['acceleratedMedianOuterMs']} | {summary['acceleratedP95OuterMs']} |",
        "",
        f"Median reduction: **{summary['medianReductionMs']} ms ({summary['medianSpeedupPercent']}%)**.",
        "",
        "| Language | CPU WER | Accelerated WER | Delta pp | Quality gate |",
        "| --- | ---: | ---: | ---: | --- |",
    ]
    for language, value in summary["languages"].items():
        lines.append(
            f"| {language} | {value['cpuWer']:.2%} | {value['acceleratedWer']:.2%} | "
            f"{value['deltaPercentagePoints']} | {'PASS' if value['qualityGate'] else 'FAIL'} |"
        )
    lines.extend(["", "## Gates", ""])
    for name, passed in summary["gates"].items():
        lines.append(f"- {'PASS' if passed else 'FAIL'}: `{name}`")
    lines.extend(
        [
            "",
            f"Decision: **{str(summary['decision']).upper()}**.",
            "",
            str(summary["claimBoundary"]),
            "",
        ]
    )
    return "\n".join(lines)


def self_test() -> None:
    rows = []
    for repeat in range(10):
        for label, elapsed, errors, backend, device in [
            ("cpu", 1000, 1, "cpu", None),
            ("gpu", 400, 1, "vulkan", "Test GPU"),
        ]:
            rows.append(
                {
                    "candidate": label,
                    "utterance": "sample",
                    "repeat": repeat,
                    "language": "en",
                    "referenceWords": 10,
                    "wordErrors": errors,
                    "hallucinatedSilence": False,
                    "outerMs": elapsed,
                    "echoVersion": "echo-desktop 1.0.0",
                    "echoCommit": "c" * 40,
                    "echoDirty": False,
                    "echoBinary": {"sha256": "d" * 64},
                    "host": {"system": "Linux", "machine": "x86_64"},
                    "seed": 1,
                    "warmups": 1,
                    "engine": {"model": "base-q5_1"},
                    "runtimeArtifact": {"sha256": "a" * 64},
                    "modelArtifact": {"sha256": "b" * 64},
                    "whisper": {
                        "runtime": {"backend": backend, "device": device},
                        "tuning": {"threads": 4, "beamSize": 1},
                    },
                }
            )
    passed = summarize(rows, "cpu", "gpu", "vulkan", True)
    assert passed["decision"] == "proceed"
    for row in rows:
        if row["candidate"] == "gpu":
            row["hallucinatedSilence"] = True
    hallucinated = summarize(rows, "cpu", "gpu", "vulkan", True)
    assert hallucinated["decision"] == "stop"
    assert not hallucinated["gates"]["noNewHallucinations"]
    for row in rows:
        if row["candidate"] == "gpu":
            row["wordErrors"] = 2
            row["hallucinatedSilence"] = False
    failed = summarize(rows, "cpu", "gpu", "vulkan", False)
    assert failed["decision"] == "stop"
    assert not failed["gates"]["perLanguageQuality"]
    assert not failed["gates"]["coverageComplete"]
    print("analyze-stt-host-matrix: self-test ok")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--runs", type=Path)
    parser.add_argument("--corpus-manifest", type=Path)
    parser.add_argument("--cpu-candidate")
    parser.add_argument("--accelerated-candidate")
    parser.add_argument("--expected-backend", default="vulkan")
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    required = [args.runs, args.corpus_manifest, args.cpu_candidate, args.accelerated_candidate, args.output_dir]
    if any(value is None for value in required):
        parser.error("runs, corpus manifest, candidates, and output dir are required")
    try:
        rows = [json.loads(line) for line in args.runs.read_text().splitlines() if line]
        corpus = json.loads(args.corpus_manifest.read_text())
        summary = summarize(
            rows,
            args.cpu_candidate,
            args.accelerated_candidate,
            args.expected_backend,
            coverage_complete=coverage_complete(corpus),
        )
        args.output_dir.mkdir(parents=True, exist_ok=True)
        (args.output_dir / "decision.json").write_text(
            json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        rendered = render(summary)
        (args.output_dir / "decision.md").write_text(rendered, encoding="utf-8")
        print(rendered)
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"analyze-stt-host-matrix: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
