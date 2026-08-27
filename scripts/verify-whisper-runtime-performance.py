#!/usr/bin/env python3
import argparse
import csv
import hashlib
import json
import math
import pathlib
import re
import statistics
import sys

from whisper_runtime_verifier import (
    VerificationError as RuntimeVerificationError,
    validate_vulkan_receipt,
)


SHA256 = re.compile(r"[0-9a-f]{64}")
LABEL = re.compile(r"(baseline|candidate)-(cpu|vulkan)-(small|largeTurbo)")
RUN_KEYS = {
    "artifactId",
    "backend",
    "buildReceiptSha256",
    "candidate",
    "model",
    "observedUseGpu",
    "outerMs",
    "resultJsonSha256",
    "run",
    "runtimeId",
    "stderrSha256",
    "transcriptSha256",
    "vulkanReceiptSha256",
}


class EvidenceError(RuntimeError):
    pass


def fail(message):
    raise EvidenceError(message)


def strict_json(path):
    def unique(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                fail(f"duplicate JSON key: {key}")
            value[key] = item
        return value

    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"could not read {path}: {error}")


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require_keys(value, expected, context):
    if not isinstance(value, dict) or set(value) != set(expected):
        fail(f"{context} keys differ")


def require_sha(value, context):
    if not isinstance(value, str) or not SHA256.fullmatch(value):
        fail(f"{context} is not a SHA-256 digest")


def percentile95(values):
    ordered = sorted(values)
    position = (len(ordered) - 1) * 0.95
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return float(ordered[lower])
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def group_summary(baseline, candidate):
    baseline_median = statistics.median(baseline)
    candidate_median = statistics.median(candidate)
    baseline_p95 = percentile95(baseline)
    candidate_p95 = percentile95(candidate)
    return {
        "baselineMedianMs": baseline_median,
        "baselineP95Ms": baseline_p95,
        "candidateMedianMs": candidate_median,
        "candidateP95Ms": candidate_p95,
        "medianDeltaPercent": (candidate_median - baseline_median)
        / baseline_median
        * 100,
        "p95DeltaPercent": (candidate_p95 - baseline_p95) / baseline_p95 * 100,
        "pairs": len(baseline),
    }


def validate_receipts(receipts):
    if not isinstance(receipts, dict) or not receipts:
        fail("Vulkan receipt map is empty")
    for digest, receipt in receipts.items():
        require_sha(digest, "Vulkan receipt key")
        canonical = json.dumps(receipt, separators=(",", ":"), sort_keys=True).encode()
        if sha256_bytes(canonical) != digest:
            fail("Vulkan receipt digest differs")
        stderr = "echo_whisper_runtime_receipt: " + json.dumps(receipt)
        try:
            validate_vulkan_receipt(stderr)
        except RuntimeVerificationError as error:
            fail(f"invalid Vulkan receipt: {error}")


def validate_inputs(manifest):
    require_keys(manifest["command"], {"arguments", "cpuFlag", "vulkanFlag"}, "command")
    if manifest["command"] != {
        "arguments": [
            "--language",
            "en",
            "--threads",
            "4",
            "--beam-size",
            "3",
            "--best-of",
            "5",
            "--no-timestamps",
            "--output-json",
        ],
        "cpuFlag": "--no-gpu",
        "vulkanFlag": None,
    }:
        fail("performance command contract differs")
    require_keys(manifest["inputs"], {"audio", "cpuMask", "models", "vad"}, "inputs")
    require_keys(manifest["inputs"]["audio"], {"id", "sha256", "size"}, "audio")
    require_sha(manifest["inputs"]["audio"]["sha256"], "audio digest")
    if (
        type(manifest["inputs"]["audio"]["size"]) is not int
        or manifest["inputs"]["audio"]["size"] <= 0
    ):
        fail("audio size is invalid")
    if (
        manifest["inputs"]["vad"] is not None
        or manifest["inputs"]["cpuMask"] is not None
    ):
        fail("performance evidence changed the VAD or CPU-mask contract")
    require_keys(manifest["inputs"]["models"], {"largeTurbo", "small"}, "models")
    for model in manifest["inputs"]["models"].values():
        require_keys(model, {"sha256", "size"}, "model")
        require_sha(model["sha256"], "model digest")
        if type(model["size"]) is not int or model["size"] <= 0:
            fail("model size is invalid")


def validate_runtimes(manifest):
    require_keys(
        manifest["runtimes"],
        {"baseline-cpu", "baseline-vulkan", "candidate"},
        "runtimes",
    )
    for name, runtime in manifest["runtimes"].items():
        require_keys(runtime, {"runtimeTreeId", "whisperCliSha256"}, f"runtime {name}")
        require_sha(runtime["runtimeTreeId"], f"runtime {name} tree identity")
        require_sha(runtime["whisperCliSha256"], f"runtime {name} CLI digest")
    candidate = manifest["candidate"]
    if manifest["runtimes"]["candidate"] != {
        "runtimeTreeId": candidate["runtimeTreeId"],
        "whisperCliSha256": candidate["whisperCliSha256"],
    }:
        fail("candidate runtime identity differs")


def validate_runs(manifest):
    receipts = manifest["vulkanReceipts"]
    validate_receipts(receipts)
    if not isinstance(manifest["runs"], list) or len(manifest["runs"]) != 80:
        fail("performance evidence must contain 80 runs")
    groups = {}
    transcripts = {}
    candidate_vulkan_receipts = set()
    for row in manifest["runs"]:
        require_keys(row, RUN_KEYS, "run")
        match = LABEL.fullmatch(
            row["candidate"] if isinstance(row["candidate"], str) else ""
        )
        if not match:
            fail("run candidate label is invalid")
        side, backend, model = match.groups()
        if row["backend"] != backend or row["model"] != model:
            fail("run label differs from backend or model")
        if type(row["run"]) is not int or not 1 <= row["run"] <= 10:
            fail("run number is invalid")
        if type(row["outerMs"]) is not int or row["outerMs"] <= 0:
            fail("run duration is invalid")
        if row["observedUseGpu"] != (1 if backend == "vulkan" else 0):
            fail("observed backend differs")
        for key in ["resultJsonSha256", "stderrSha256", "transcriptSha256"]:
            require_sha(row[key], key)
        expected_runtime = "candidate" if side == "candidate" else f"baseline-{backend}"
        if row["runtimeId"] != expected_runtime:
            fail("run runtime identity differs")
        if side == "candidate":
            if row["artifactId"] != manifest["candidate"]["artifactId"]:
                fail("candidate artifact ID differs")
            if row["buildReceiptSha256"] != manifest["candidate"]["buildReceiptSha256"]:
                fail("candidate build receipt differs")
        elif row["artifactId"] is not None or row["buildReceiptSha256"] is not None:
            fail("baseline run claims a candidate receipt")
        if backend == "vulkan":
            if row["vulkanReceiptSha256"] not in receipts:
                fail("run Vulkan receipt is unbound")
            if side == "candidate":
                candidate_vulkan_receipts.add(row["vulkanReceiptSha256"])
        elif row["vulkanReceiptSha256"] is not None:
            fail("CPU run claims a Vulkan receipt")
        groups.setdefault(row["candidate"], []).append(row)
        transcripts.setdefault(model, set()).add(row["transcriptSha256"])
    expected_groups = {
        f"{side}-{backend}-{model}"
        for side in ["baseline", "candidate"]
        for backend in ["cpu", "vulkan"]
        for model in ["small", "largeTurbo"]
    }
    if set(groups) != expected_groups:
        fail("performance groups differ")
    for label, rows in groups.items():
        if sorted(row["run"] for row in rows) != list(range(1, 11)):
            fail(f"{label} run sequence differs")
    referenced_receipts = {
        row["vulkanReceiptSha256"]
        for row in manifest["runs"]
        if row["vulkanReceiptSha256"] is not None
    }
    if set(receipts) != referenced_receipts:
        fail("Vulkan receipt map contains unreferenced evidence")
    return (
        groups,
        all(len(values) == 1 for values in transcripts.values()),
        len(candidate_vulkan_receipts) == 1,
    )


def calculated_summary(manifest, manifest_path, tsv_path):
    groups, transcript_parity, receipts_exact = validate_runs(manifest)
    results = {}
    for backend in ["cpu", "vulkan"]:
        results[backend] = {}
        for model in ["small", "largeTurbo"]:
            baseline = [row["outerMs"] for row in groups[f"baseline-{backend}-{model}"]]
            candidate = [
                row["outerMs"] for row in groups[f"candidate-{backend}-{model}"]
            ]
            results[backend][model] = group_summary(baseline, candidate)
    candidate = manifest["candidate"]
    return {
        "archiveSha256": candidate["archiveSha256"],
        "artifactId": candidate["artifactId"],
        "buildReceiptSha256": candidate["buildReceiptSha256"],
        "candidateVulkanReceiptsExact": receipts_exact,
        "candidateWhisperCliSha256": candidate["whisperCliSha256"],
        "interleavedSha256": sha256_file(tsv_path),
        "packageBytes": candidate["packageBytes"],
        "results": results,
        "runManifestSha256": sha256_file(manifest_path),
        "runs": len(manifest["runs"]),
        "schemaVersion": 1,
        "transcriptParity": transcript_parity,
    }


def verify(manifest_path, summary_path, tsv_path):
    manifest = strict_json(manifest_path)
    require_keys(
        manifest,
        {
            "candidate",
            "command",
            "inputs",
            "runs",
            "runtimes",
            "schemaVersion",
            "vulkanReceipts",
        },
        "manifest",
    )
    if manifest["schemaVersion"] != 1:
        fail("unsupported performance evidence schema")
    require_keys(
        manifest["candidate"],
        {
            "archiveSha256",
            "artifactId",
            "buildReceiptSha256",
            "packageBytes",
            "runtimeTreeId",
            "whisperCliSha256",
        },
        "candidate",
    )
    for key in [
        "archiveSha256",
        "artifactId",
        "buildReceiptSha256",
        "runtimeTreeId",
        "whisperCliSha256",
    ]:
        require_sha(manifest["candidate"][key], f"candidate {key}")
    if (
        type(manifest["candidate"]["packageBytes"]) is not int
        or manifest["candidate"]["packageBytes"] <= 0
    ):
        fail("candidate package size is invalid")
    validate_inputs(manifest)
    validate_runtimes(manifest)
    with tsv_path.open(newline="", encoding="utf-8") as stream:
        timing_rows = list(csv.DictReader(stream, delimiter="\t"))
        if len(timing_rows) != 80:
            fail("interleaved TSV must contain 80 rows")
    manifest_timings = [
        {
            "candidate": row["candidate"],
            "run": str(row["run"]),
            "outer_ms": str(row["outerMs"]),
        }
        for row in manifest["runs"]
    ]
    if timing_rows != manifest_timings:
        fail("interleaved TSV differs from the run manifest")
    expected = calculated_summary(manifest, manifest_path, tsv_path)
    if strict_json(summary_path) != expected:
        fail("performance summary differs from the run manifest")
    print("verify-whisper-runtime-performance: ok")


def self_test():
    if not math.isclose(percentile95([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), 9.55):
        fail("p95 interpolation self-test failed")
    print("verify-whisper-runtime-performance: self-test ok")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("paths", nargs="*", type=pathlib.Path)
    args = parser.parse_args()
    if args.self_test and not args.paths and not args.verify:
        self_test()
    elif args.verify and not args.self_test and len(args.paths) == 3:
        verify(*args.paths)
    else:
        parser.error("use --self-test or --verify MANIFEST SUMMARY TSV")


if __name__ == "__main__":
    try:
        main()
    except EvidenceError as error:
        print(f"verify-whisper-runtime-performance: {error}", file=sys.stderr)
        sys.exit(2)
