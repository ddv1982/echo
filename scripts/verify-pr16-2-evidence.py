#!/usr/bin/env python3
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import subprocess
import tarfile
import tempfile
from pathlib import Path

from whisper_identity_v3 import (
    strict_json_file,
    verify_acceleration_set,
    verify_v3_promotion_metadata,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
ARCHIVE = REPO_ROOT / ".audit/pr16-2-evidence/evidence.tar.gz"
SUMMARY = REPO_ROOT / ".audit/pr16-2-evidence/summary.json"

TREES = {
    "small/bundle": "target/pr16-2/sweep-small/cells/pr162-small/bundle",
    "small/analysis": "target/pr16-2/sweep-small/cells/pr162-small/analysis",
    "large/bundle": "target/pr16-2/sweep-large/cells/pr162-large/bundle",
    "large/analysis": "target/pr16-2/sweep-large/cells/pr162-large/analysis",
    "cache/small-prior": "target/pr16-2/cache-small",
    "cache/small-reset": "target/pr16-2/cache-small-reset",
    "cache/large-prior": "target/pr16-2/cache-large",
    "cache/large-reset": "target/pr16-2/cache-large-reset",
}

FILES = {
    "corpus/fixtures.json": "target/stt-product-corpus-main/fixtures.json",
    "small/sweep.json": "target/pr16-2/sweep-small/sweep.json",
    "small/status.json": "target/pr16-2/sweep-small/status.json",
    "small/decision.json": "target/pr16-2/sweep-small/cells/pr162-small/decision.json",
    "large/sweep.json": "target/pr16-2/sweep-large/sweep.json",
    "large/status.json": "target/pr16-2/sweep-large/status.json",
    "large/decision.json": "target/pr16-2/sweep-large/cells/pr162-large/decision.json",
    "reusable/acceleration-set.v3.json": "target/pr16-2/reusable/whisper-acceleration/acceleration-set.v3.json",
    "reusable/promotion-v3.json": "target/pr16-2/reusable/promotion-v3.json",
    "staged/qualified-release.json": "target/pr16-2/staged-final/qualified-release.json",
}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def archive_entry(archive: tarfile.TarFile, source: Path, name: str) -> None:
    data = source.read_bytes()
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = 0o644
    info.mtime = 0
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    archive.addfile(info, io.BytesIO(data))


def build_archive() -> None:
    entries = []
    for archived, source in FILES.items():
        entries.append((archived, REPO_ROOT / source))
    for archived_root, source_root in TREES.items():
        root = REPO_ROOT / source_root
        entries.extend(
            (f"{archived_root}/{path.relative_to(root).as_posix()}", path)
            for path in root.rglob("*")
            if path.is_file() and not path.is_symlink()
        )
    missing = [str(path) for _, path in entries if not path.is_file()]
    if missing:
        raise ValueError(f"PR16.2 evidence sources are missing: {missing}")
    ARCHIVE.parent.mkdir(parents=True, exist_ok=True)
    with ARCHIVE.open("wb") as output:
        with gzip.GzipFile(
            fileobj=output, mode="wb", filename="", mtime=0
        ) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for name, source in sorted(entries):
                    archive_entry(archive, source, name)


def verify_reference(root: Path, reference: object, label: str) -> None:
    if not isinstance(reference, dict) or set(reference) != {"bytes", "path", "sha256"}:
        raise ValueError(f"{label} reference is malformed")
    path = root / reference["path"]
    if (
        not path.is_file()
        or path.stat().st_size != reference["bytes"]
        or sha256_file(path) != reference["sha256"]
    ):
        raise ValueError(f"{label} artifact differs")


def verify_cycle(root: Path) -> dict[str, object]:
    status = strict_json_file(root / "status.json")
    if status.get("state") != "complete":
        raise ValueError("cache cycle status is not complete")
    verify_reference(root, status.get("cycle"), "cache cycle")
    cycle = strict_json_file(root / "cache-cycle.json")
    verify_reference(root, cycle.get("hostEvidence"), "cache host")
    for name, reference in cycle.get("cacheSnapshots", {}).items():
        verify_reference(root, reference, f"cache snapshot {name}")
    for phase, probe in cycle.get("probes", {}).items():
        for name, reference in probe.get("artifacts", {}).items():
            verify_reference(root, reference, f"{phase} probe {name}")
    return cycle


def replay_model(root: Path, model: str) -> None:
    decision = strict_json_file(root / model / "decision.json")
    candidates = decision["candidates"]
    resource = decision["phase2"]["resourceEvidence"]
    with tempfile.TemporaryDirectory(prefix=f"pr16-2-{model}-replay-") as temporary:
        output = Path(temporary)
        subprocess.run(
            [
                "python3",
                str(REPO_ROOT / "scripts/analyze-stt-host-matrix.py"),
                "--runs",
                str(root / model / "bundle/runs.jsonl"),
                "--corpus-manifest",
                str(root / "corpus/fixtures.json"),
                "--cpu-candidate",
                candidates["cpu"],
                "--accelerated-candidate",
                candidates["accelerated"],
                "--expected-backend",
                "vulkan",
                "--output-dir",
                str(output),
                "--minimum-available-memory-bytes",
                str(resource["minimumAvailableMemoryBytes"]),
                "--maximum-sustained-swap-growth-bytes",
                str(resource["maximumSustainedSwapGrowthBytes"]),
                "--require-resource-evidence",
            ],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        replayed = strict_json_file(output / "decision.json")
    recorded = strict_json_file(root / model / "analysis/decision.json")
    if replayed != recorded:
        raise ValueError(f"{model} analyzer replay differs")


def verify_archive() -> None:
    summary = strict_json_file(SUMMARY)
    with tempfile.TemporaryDirectory(prefix="pr16-2-evidence-") as temporary:
        root = Path(temporary)
        with tarfile.open(ARCHIVE, "r:gz") as archive:
            archive.extractall(root, filter="data")

        expected_hashes = {
            "small/sweep.json": summary["physicalQualification"]["small"][
                "sweepSha256"
            ],
            "large/sweep.json": summary["physicalQualification"]["largeTurbo"][
                "sweepSha256"
            ],
            "cache/small-reset/cache-cycle.json": summary["physicalQualification"][
                "small"
            ]["resetCycleSha256"],
            "cache/large-reset/cache-cycle.json": summary["physicalQualification"][
                "largeTurbo"
            ]["resetCycleSha256"],
            "reusable/acceleration-set.v3.json": summary["sourceArtifacts"][
                "accelerationSetFileSha256"
            ],
            "reusable/promotion-v3.json": summary["sourceArtifacts"][
                "promotionFileSha256"
            ],
            "staged/qualified-release.json": summary["releaseArtifacts"][
                "qualifiedReleaseSha256"
            ],
        }
        for relative, expected in expected_hashes.items():
            if sha256_file(root / relative) != expected:
                raise ValueError(f"archived {relative} digest differs")

        for model in ("small", "large"):
            if sum(1 for _ in (root / model / "bundle/runs.jsonl").open()) != 560:
                raise ValueError(f"{model} evidence does not contain 560 runs")
            replay_model(root, model)

        for model in ("small", "large"):
            prior = verify_cycle(root / f"cache/{model}-prior")
            reset = verify_cycle(root / f"cache/{model}-reset")
            if (
                prior["identity"] != reset["identity"]
                or prior["bootId"] == reset["bootId"]
                or reset["resetEvidence"].get("state") != "COMPLETE"
                or reset["resetEvidence"].get("reason") != "distinctBootId"
            ):
                raise ValueError(f"{model} cross-boot reset evidence differs")

        acceleration_set = strict_json_file(root / "reusable/acceleration-set.v3.json")
        identities = verify_acceleration_set(acceleration_set)
        promotion = strict_json_file(root / "reusable/promotion-v3.json")
        verify_v3_promotion_metadata(promotion, acceleration_set)
        if identities["executionArtifactId"] != summary["executionArtifactId"]:
            raise ValueError("archived execution artifact differs")
        staged = strict_json_file(root / "staged/qualified-release.json")
        if (
            staged.get("productionReady") is not False
            or staged.get("productionReadiness") != "proof-only-until-pr16.3"
            or staged.get("physicalRequalificationRequired") is not False
        ):
            raise ValueError("archived staged release is not a proof-only reuse result")
    print("verify-pr16-2-evidence: ok")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build or verify retained PR16.2 evidence"
    )
    parser.add_argument("--build", action="store_true")
    args = parser.parse_args()
    if args.build:
        build_archive()
    verify_archive()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
