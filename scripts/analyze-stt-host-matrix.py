#!/usr/bin/env python3
"""Evaluate paired CPU/accelerator Echo benchmark rows against Phase 2 gates."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import statistics
import sys
import unicodedata
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

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
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}")
REPO_ROOT = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class CorpusBinding:
    """The immutable fixture fields a measurement is allowed to claim."""

    identifier: str
    language: str
    reference: str
    audio_sha256: str | None
    audio_path: Path | None


def require_object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    return value


def require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label} must be a non-empty string")
    return value


def require_int(value: object, label: str, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise ValueError(f"{label} must be an integer at least {minimum}")
    return value


def require_number(value: object, label: str, minimum: float = 0) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{label} must be a number")
    number = float(value)
    if not math.isfinite(number) or number < minimum:
        raise ValueError(f"{label} must be a finite number at least {minimum}")
    return number


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def is_sha256(value: object) -> bool:
    return isinstance(value, str) and bool(SHA256_PATTERN.fullmatch(value))


def portable_path(path: Path) -> str:
    resolved = path.resolve()
    for label, root in (("$REPO", REPO_ROOT), ("$HOME", Path.home())):
        try:
            return str(Path(label) / resolved.relative_to(root.resolve()))
        except ValueError:
            pass
    return str(resolved)


def recorded_path(value: object, label: str) -> Path:
    raw = require_string(value, label)
    if raw == "$REPO" or raw.startswith("$REPO/"):
        return REPO_ROOT / raw.removeprefix("$REPO/")
    if raw == "$HOME" or raw.startswith("$HOME/"):
        return Path.home() / raw.removeprefix("$HOME/")
    return Path(raw)


def same_recorded_path(left: object, right: object, label: str) -> bool:
    try:
        return (
            recorded_path(left, f"{label} path").resolve()
            == recorded_path(right, f"{label} path").resolve()
        )
    except OSError as error:
        raise ValueError(f"{label} path cannot be resolved: {error}") from error


def read_json(path: Path, label: str) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except UnicodeDecodeError as error:
        raise ValueError(f"{label} is not UTF-8") from error


def safe_artifact_file(
    bundle_root: Path, reference: object, label: str, *, require_bytes: bool = True
) -> tuple[Path, bytes]:
    item = require_object(reference, label)
    raw_path = require_string(item.get("path"), f"{label}.path")
    relative = PurePosixPath(raw_path)
    if (
        relative.is_absolute()
        or raw_path != relative.as_posix()
        or not relative.parts
        or any(part in {"", ".", ".."} for part in relative.parts)
    ):
        raise ValueError(f"{label}.path must be a safe relative artifact path")
    root = bundle_root.resolve()
    candidate = bundle_root.joinpath(*relative.parts)
    try:
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{label}.path is missing: {raw_path}") from error
    if not resolved.is_file() or root not in (resolved, *resolved.parents):
        raise ValueError(f"{label}.path escapes the run bundle: {raw_path}")
    contents = resolved.read_bytes()
    if not is_sha256(item.get("sha256")):
        raise ValueError(f"{label}.sha256 must be a SHA-256")
    if sha256_bytes(contents) != item["sha256"]:
        raise ValueError(f"{label} SHA-256 mismatch")
    size = item.get("bytes")
    if require_bytes and (isinstance(size, bool) or not isinstance(size, int)):
        raise ValueError(f"{label}.bytes must be an integer")
    if size is not None and (
        isinstance(size, bool)
        or not isinstance(size, int)
        or size < 0
        or size != len(contents)
    ):
        raise ValueError(f"{label} byte count mismatch")
    return resolved, contents


def verify_identity(value: object, label: str) -> dict[str, object]:
    identity = require_object(value, label)
    path = recorded_path(identity.get("path"), f"{label}.path")
    if not is_sha256(identity.get("sha256")):
        raise ValueError(f"{label}.sha256 must be a SHA-256")
    try:
        if not path.is_file():
            raise ValueError(f"{label}.path is not a file: {path}")
        digest = sha256_file(path)
    except OSError as error:
        raise ValueError(f"{label}.path cannot be read: {path}") from error
    if digest != identity["sha256"]:
        raise ValueError(f"{label} SHA-256 mismatch")
    return {"path": portable_path(path), "sha256": digest}


def normalized_words(text: str) -> list[str]:
    normalized = unicodedata.normalize("NFKC", text).casefold()
    return re.findall(r"\w+", normalized, flags=re.UNICODE)


def edit_distance(left: list[str], right: list[str]) -> int:
    previous = list(range(len(right) + 1))
    for left_index, left_word in enumerate(left, start=1):
        current = [left_index]
        for right_index, right_word in enumerate(right, start=1):
            current.append(
                min(
                    current[-1] + 1,
                    previous[right_index] + 1,
                    previous[right_index - 1] + (left_word != right_word),
                )
            )
        previous = current
    return previous[-1]


def corpus_bindings(
    manifest_path: Path,
) -> tuple[dict[str, object], dict[str, CorpusBinding]]:
    source = manifest_path.read_bytes()
    value = require_object(json.loads(source), "corpus manifest")
    if value.get("schemaVersion") != 1:
        raise ValueError("corpus manifest schemaVersion must be 1")
    utterances = value.get("utterances")
    if not isinstance(utterances, list) or not utterances:
        raise ValueError("corpus manifest must contain utterances")
    bindings: dict[str, CorpusBinding] = {}
    for index, raw in enumerate(utterances):
        item = require_object(raw, f"corpus utterances[{index}]")
        identifier = require_string(item.get("id"), f"corpus utterances[{index}].id")
        if identifier in bindings:
            raise ValueError(f"duplicate corpus utterance id: {identifier}")
        language = require_string(item.get("language"), f"corpus {identifier}.language")
        reference = item.get("reference")
        if not isinstance(reference, str):
            raise ValueError(f"corpus {identifier}.reference must be a string")
        audio_hash = item.get("audioSha256", item.get("sha256"))
        if audio_hash is not None and not is_sha256(audio_hash):
            raise ValueError(f"corpus {identifier} has an invalid audio SHA-256")
        audio_path: Path | None = None
        if isinstance(item.get("file"), str) and item["file"]:
            audio_path = (manifest_path.parent / str(item["file"])).resolve()
            if not audio_path.is_file():
                raise ValueError(f"corpus audio fixture is missing: {audio_path}")
            actual_hash = sha256_file(audio_path)
            if audio_hash is not None and actual_hash != audio_hash:
                raise ValueError(f"corpus {identifier} audio SHA-256 mismatch")
            audio_hash = actual_hash
        bindings[identifier] = CorpusBinding(
            identifier, language, reference, audio_hash, audio_path
        )
    return value, bindings


def validate_status_and_manifest(bundle_root: Path) -> dict[str, object]:
    status = require_object(
        read_json(bundle_root / "status.json", "status.json"), "status.json"
    )
    manifest = require_object(
        read_json(bundle_root / "run-manifest.json", "run-manifest.json"),
        "run-manifest.json",
    )
    if status.get("schemaVersion") != 1 or manifest.get("schemaVersion") != 1:
        raise ValueError("run bundle schemaVersion must be 1")
    if status.get("state") != "complete":
        raise ValueError("run bundle status must be complete")
    run_id = require_string(manifest.get("runId"), "run-manifest.runId")
    if status.get("runId") != run_id:
        raise ValueError("status.json runId does not match run-manifest.json")
    for field in ("startedAt", "completedAt"):
        require_string(status.get(field), f"status.{field}")
    require_string(status.get("updatedAt"), "status.updatedAt")
    started_at = require_string(manifest.get("startedAt"), "run-manifest.startedAt")
    if status["startedAt"] != started_at:
        raise ValueError("status.json startedAt does not match run-manifest.json")
    verify_identity(manifest.get("binary"), "run-manifest.binary")
    return manifest


def candidate_configs(manifest: dict[str, object]) -> dict[str, dict[str, object]]:
    raw_candidates = manifest.get("candidates")
    if not isinstance(raw_candidates, list) or not raw_candidates:
        raise ValueError("run-manifest candidates must be a non-empty array")
    candidates: dict[str, dict[str, object]] = {}
    for index, raw in enumerate(raw_candidates):
        candidate = require_object(raw, f"run-manifest candidates[{index}]")
        label = require_string(candidate.get("label"), f"candidate[{index}].label")
        if label in candidates:
            raise ValueError(f"duplicate run-manifest candidate: {label}")
        require_string(candidate.get("engine"), f"candidate {label}.engine")
        if "model" not in candidate:
            raise ValueError(f"candidate {label}.model is required")
        if candidate["model"] is not None and not isinstance(candidate["model"], str):
            raise ValueError(f"candidate {label}.model must be a string or null")
        for field in ("threads", "beamSize", "bestOf"):
            if field not in candidate:
                raise ValueError(f"candidate {label}.{field} is required")
            if candidate.get(field) is not None:
                require_int(candidate[field], f"candidate {label}.{field}", 1)
        for field in ("noFallback", "forceCpu"):
            if not isinstance(candidate.get(field), bool):
                raise ValueError(f"candidate {label}.{field} must be a boolean")
        candidates[label] = candidate
    return candidates


def snapshot_fixtures(source: bytes, label: str) -> dict[str, dict[str, object]]:
    value = require_object(json.loads(source), label)
    if value.get("schemaVersion") != 1:
        raise ValueError(f"{label} schemaVersion must be 1")
    utterances = value.get("utterances")
    if not isinstance(utterances, list) or not utterances:
        raise ValueError(f"{label} must contain utterances")
    fixtures: dict[str, dict[str, object]] = {}
    for index, raw in enumerate(utterances):
        fixture = require_object(raw, f"{label} utterances[{index}]")
        identifier = require_string(
            fixture.get("id"), f"{label} utterances[{index}].id"
        )
        if identifier in fixtures:
            raise ValueError(f"duplicate {label} fixture id: {identifier}")
        require_string(fixture.get("language"), f"{label} {identifier}.language")
        if not isinstance(fixture.get("reference"), str):
            raise ValueError(f"{label} {identifier}.reference must be a string")
        require_string(fixture.get("file"), f"{label} {identifier}.file")
        fixtures[identifier] = fixture
    return fixtures


def validate_corpus_snapshot(
    bundle_root: Path, manifest: dict[str, object], bindings: dict[str, CorpusBinding]
) -> dict[str, dict[str, object]]:
    corpus = require_object(manifest.get("corpus"), "run-manifest.corpus")
    _snapshot, snapshot_contents = safe_artifact_file(
        bundle_root,
        corpus.get("snapshot"),
        "run-manifest.corpus.snapshot",
        require_bytes=False,
    )
    snapshot = snapshot_fixtures(snapshot_contents, "run bundle corpus snapshot")
    if set(snapshot) != set(bindings):
        raise ValueError(
            "run bundle corpus snapshot fixture IDs do not match --corpus-manifest"
        )
    for identifier, binding in bindings.items():
        fixture = snapshot[identifier]
        if (
            fixture["language"] != binding.language
            or fixture["reference"] != binding.reference
        ):
            raise ValueError(
                f"run bundle corpus snapshot fixture mismatch: {identifier}"
            )
    source_path = recorded_path(
        corpus.get("sourcePath"), "run-manifest.corpus.sourcePath"
    )
    source_fixtures: dict[str, dict[str, object]] | None = None
    if source_path.is_file():
        source_fixtures = snapshot_fixtures(
            source_path.read_bytes(), "run-manifest.corpus.sourcePath"
        )
        if set(source_fixtures) != set(snapshot):
            raise ValueError("run source fixture IDs do not match the bundled snapshot")
        for identifier, fixture in snapshot.items():
            source_fixture = source_fixtures[identifier]
            if any(
                source_fixture[field] != fixture[field]
                for field in ("language", "reference", "file")
            ):
                raise ValueError(f"run source fixture mismatch: {identifier}")
    raw_fixtures = corpus.get("utterances")
    if not isinstance(raw_fixtures, list) or len(raw_fixtures) != len(bindings):
        raise ValueError("run-manifest corpus fixtures do not match --corpus-manifest")
    fixtures: dict[str, dict[str, object]] = {}
    for index, raw in enumerate(raw_fixtures):
        fixture = require_object(raw, f"run-manifest corpus utterances[{index}]")
        identifier = require_string(
            fixture.get("id"), f"run-manifest corpus fixture[{index}].id"
        )
        if identifier in fixtures or identifier not in bindings:
            raise ValueError(
                f"unexpected or duplicate run-manifest fixture: {identifier}"
            )
        require_string(fixture.get("audio"), f"run-manifest fixture {identifier}.audio")
        expected_hash = bindings[identifier].audio_sha256
        actual_hash = fixture.get("audioSha256")
        if not is_sha256(actual_hash):
            raise ValueError(
                f"run-manifest fixture has an invalid audio digest: {identifier}"
            )
        if expected_hash is not None and actual_hash != expected_hash:
            raise ValueError(
                f"run-manifest fixture audio digest mismatch: {identifier}"
            )
        audio = recorded_path(
            fixture["audio"], f"run-manifest fixture {identifier}.audio"
        )
        if not audio.is_file():
            raise ValueError(f"run-manifest fixture audio is missing: {audio}")
        observed_hash = sha256_file(audio)
        if observed_hash != actual_hash or (
            expected_hash is not None and observed_hash != expected_hash
        ):
            raise ValueError(
                f"run-manifest fixture audio contents changed: {identifier}"
            )
        audio_path = bindings[identifier].audio_path
        if audio_path is not None and not same_recorded_path(
            fixture["audio"], str(audio_path), f"fixture {identifier}"
        ):
            raise ValueError(f"run-manifest fixture audio path mismatch: {identifier}")
        if source_fixtures is not None:
            source_audio = source_path.parent / str(source_fixtures[identifier]["file"])
            if source_audio.is_file() and not same_recorded_path(
                fixture["audio"], str(source_audio), f"fixture {identifier}"
            ):
                raise ValueError(
                    f"run source fixture audio path mismatch: {identifier}"
                )
        fixtures[identifier] = fixture
    if set(fixtures) != set(bindings):
        raise ValueError(
            "run-manifest corpus fixture IDs do not match --corpus-manifest"
        )
    return fixtures


def command_for_candidate(
    command: object,
    binary: object,
    fixture_audio: object,
    binding: CorpusBinding,
    candidate: dict[str, object],
    label: str,
) -> None:
    if not isinstance(command, list) or not all(
        isinstance(item, str) for item in command
    ):
        raise ValueError(f"{label} command must be a string array")
    if len(command) < 3:
        raise ValueError(f"{label} command is incomplete")
    language = "auto" if candidate["engine"] == "parakeet" else binding.language
    expected: list[object] = [
        "transcribe",
        portable_path(recorded_path(fixture_audio, f"{label} fixture audio")),
        "--language",
        language,
        "--format",
        "json",
    ]
    if candidate["engine"] != "fake":
        expected.extend(["--engine", candidate["engine"]])
    if candidate["model"] is not None:
        expected.extend(["--model", candidate["model"]])
    for flag, value in (
        ("--whisper-threads", candidate["threads"]),
        ("--whisper-beam-size", candidate["beamSize"]),
        ("--whisper-best-of", candidate["bestOf"]),
    ):
        if value is not None:
            expected.extend([flag, str(value)])
    if candidate["noFallback"]:
        expected.append("--whisper-no-fallback")
    if candidate["forceCpu"]:
        expected.append("--whisper-no-gpu")
    actual = list(command[1:])
    actual[1] = portable_path(recorded_path(actual[1], f"{label} command audio"))
    if len(command) != len(expected) + 1 or actual != expected:
        raise ValueError(f"{label} command does not match its run-manifest candidate")
    if not same_recorded_path(command[0], binary, f"{label} binary"):
        raise ValueError(f"{label} command binary does not match run-manifest binary")
    if not same_recorded_path(command[2], fixture_audio, f"{label} audio"):
        raise ValueError(f"{label} command audio does not match its fixture")


def identity_from_product(
    row: dict[str, object],
    engine: dict[str, object],
    row_field: str,
    engine_field: str,
    label: str,
) -> dict[str, object]:
    product_path = engine.get(engine_field)
    if not isinstance(product_path, str) or not product_path:
        raise ValueError(f"{label} product engine.{engine_field} is missing")
    identity = require_object(row.get(row_field), f"{label} row.{row_field}")
    if not same_recorded_path(
        identity.get("path"), product_path, f"{label} {row_field}"
    ):
        raise ValueError(f"{label} {row_field} path does not match product JSON")
    return verify_identity(identity, f"{label} row.{row_field}")


def replay_artifact(
    bundle_root: Path,
    artifact: dict[str, object],
    row: dict[str, object],
    binding: CorpusBinding,
    fixture: dict[str, object],
    candidate: dict[str, object],
    binary: object,
) -> dict[str, object]:
    row_id = require_string(row.get("rowId"), "runs rowId")
    label = f"observation {row_id}"
    paths: set[Path] = set()
    contents: dict[str, bytes] = {}
    for name in ("command", "environment", "stdout", "stderr", "result", "timing"):
        path, value = safe_artifact_file(
            bundle_root, artifact.get(name), f"{label}.{name}"
        )
        if path in paths:
            raise ValueError(f"{label} reuses an artifact path")
        paths.add(path)
        contents[name] = value
    try:
        command = json.loads(contents["command"].decode("utf-8"))
        environment = json.loads(contents["environment"].decode("utf-8"))
        stdout_product = json.loads(contents["stdout"].decode("utf-8"))
        result = json.loads(contents["result"].decode("utf-8"))
        timing = json.loads(contents["timing"].decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{label} has invalid raw JSON") from error
    if not isinstance(environment, dict) or not all(
        isinstance(key, str) and isinstance(value, str)
        for key, value in environment.items()
    ):
        raise ValueError(f"{label} environment must be a string object")
    command_for_candidate(command, binary, fixture["audio"], binding, candidate, label)
    result = require_object(result, f"{label} result")
    timing = require_object(timing, f"{label} timing")
    if result.get("schemaVersion") != 1 or timing.get("schemaVersion") != 1:
        raise ValueError(f"{label} result and timing schemaVersion must be 1")
    if result.get("returnCode") != 0 or timing.get("returnCode") != 0:
        raise ValueError(f"{label} did not complete successfully")
    if (
        result.get("parseError") is not None
        or result.get("invocationError") is not None
    ):
        raise ValueError(f"{label} has a recorded invocation or parse error")
    product = require_object(stdout_product, f"{label} stdout product JSON")
    if result.get("productJson") != product:
        raise ValueError(f"{label} result productJson does not match stdout")
    text = product.get("text")
    raw = product.get("raw")
    if not isinstance(text, str) or not isinstance(raw, str):
        raise ValueError(
            f"{label} product transcript must contain text and raw strings"
        )
    audio_ms = require_int(product.get("audioMs"), f"{label} product audioMs")
    infer_ms = require_int(product.get("inferMs"), f"{label} product inferMs")
    outer_ms = require_number(timing.get("wallMs"), f"{label} timing wallMs")
    if outer_ms < infer_ms:
        raise ValueError(f"{label} wall time is lower than inference time")
    for field in ("startedAt", "finishedAt"):
        require_string(timing.get(field), f"{label} timing.{field}")
    engine = require_object(product.get("engine"), f"{label} product engine")
    if engine.get("id") != candidate["engine"]:
        raise ValueError(f"{label} product engine does not match its candidate")
    if candidate["model"] is not None and engine.get("model") != candidate["model"]:
        raise ValueError(f"{label} product model does not match its candidate")
    if engine.get("vad") is not None and not isinstance(engine["vad"], bool):
        raise ValueError(f"{label} product engine.vad must be a boolean or null")
    runtime = identity_from_product(row, engine, "runtimeArtifact", "binary", label)
    model = identity_from_product(row, engine, "modelArtifact", "modelPath", label)
    vad_artifact: dict[str, object] | None = None
    if "vadArtifact" in row:
        if not isinstance(engine.get("vadPath"), str) or not engine["vadPath"]:
            raise ValueError(
                f"{label} records vadArtifact without product engine.vadPath"
            )
        vad_artifact = identity_from_product(
            row, engine, "vadArtifact", "vadPath", label
        )
    whisper = product.get("whisper")
    if whisper is not None and not isinstance(whisper, dict):
        raise ValueError(f"{label} product whisper must be an object or null")
    reference_words = normalized_words(binding.reference)
    errors = edit_distance(reference_words, normalized_words(text))
    verified = {
        "schemaVersion": 2,
        "rowId": row_id,
        "candidate": candidate["label"],
        "utterance": binding.identifier,
        "language": binding.language,
        "repeat": row.get("repeat"),
        "audio": fixture["audio"],
        "audioSha256": binding.audio_sha256,
        "reference": binding.reference,
        "text": text,
        "raw": raw,
        "wordErrors": errors,
        "referenceWords": len(reference_words),
        "wer": errors / len(reference_words) if reference_words else None,
        "hallucinatedSilence": not reference_words and bool(text.strip()),
        "audioMs": audio_ms,
        "inferMs": infer_ms,
        "outerMs": outer_ms,
        "rtf": infer_ms / audio_ms if audio_ms else None,
        "engine": engine,
        "runtimeArtifact": runtime,
        "modelArtifact": model,
        "whisper": whisper,
    }
    if vad_artifact is not None:
        verified["vadArtifact"] = vad_artifact
    return verified


def verify_bundle(
    runs_path: Path, corpus_manifest: Path
) -> tuple[list[dict[str, object]], dict[str, object]]:
    if runs_path.resolve().name != "runs.jsonl":
        raise ValueError("--runs must name a runs.jsonl file in its Phase 1 bundle")
    bundle_root = runs_path.resolve().parent
    manifest = validate_status_and_manifest(bundle_root)
    corpus, bindings = corpus_bindings(corpus_manifest.resolve())
    fixtures = validate_corpus_snapshot(bundle_root, manifest, bindings)
    candidates = candidate_configs(manifest)
    repeats = require_int(manifest.get("repeats"), "run-manifest.repeats", 1)
    warmups = require_int(manifest.get("warmups"), "run-manifest.warmups")
    seed = require_int(manifest.get("seed"), "run-manifest.seed")
    echo = require_object(manifest.get("echo"), "run-manifest.echo")
    host = require_object(manifest.get("host"), "run-manifest.host")
    binary = require_object(manifest.get("binary"), "run-manifest.binary")
    artifact_index = manifest.get("artifactIndex")
    if not isinstance(artifact_index, list):
        raise ValueError("run-manifest.artifactIndex must be an array")
    artifacts: dict[str, dict[str, object]] = {}
    measurement_ids: set[str] = set()
    warmup_ids: set[str] = set()
    warmup_coverage: dict[tuple[str, str], int] = defaultdict(int)
    artifact_paths: set[str] = set()
    for index, raw in enumerate(artifact_index):
        artifact = require_object(raw, f"artifactIndex[{index}]")
        if artifact.get("schemaVersion") != 1:
            raise ValueError(f"artifactIndex[{index}] schemaVersion must be 1")
        row_id = require_string(artifact.get("rowId"), f"artifactIndex[{index}].rowId")
        phase = artifact.get("phase")
        if phase not in {"measurement", "warmup"}:
            raise ValueError(
                f"artifactIndex[{index}].phase must be measurement or warmup"
            )
        if row_id in artifacts:
            raise ValueError(f"duplicate artifact rowId: {row_id}")
        candidate = require_string(
            artifact.get("candidate"), f"artifactIndex[{index}].candidate"
        )
        utterance = require_string(
            artifact.get("utterance"), f"artifactIndex[{index}].utterance"
        )
        if candidate not in candidates or utterance not in bindings:
            raise ValueError(
                f"artifactIndex[{index}] has an unknown candidate or fixture"
            )
        for field in ("command", "environment", "stdout", "stderr", "result", "timing"):
            reference = require_object(
                artifact.get(field), f"artifactIndex[{index}].{field}"
            )
            path = require_string(
                reference.get("path"), f"artifactIndex[{index}].{field}.path"
            )
            if path in artifact_paths:
                raise ValueError(f"artifactIndex reuses artifact path: {path}")
            artifact_paths.add(path)
        artifacts[row_id] = artifact
        (measurement_ids if phase == "measurement" else warmup_ids).add(row_id)
        if phase == "warmup":
            warmup_coverage[(candidate, utterance)] += 1
    if len(warmup_ids) != len(candidates) * len(bindings) * warmups:
        raise ValueError(
            "warmup artifacts do not match run-manifest candidates, fixtures, and warmups"
        )
    expected_warmup_coverage = (
        {
            (candidate, utterance): warmups
            for candidate in candidates
            for utterance in bindings
        }
        if warmups
        else {}
    )
    if dict(warmup_coverage) != expected_warmup_coverage:
        raise ValueError(
            "warmup artifacts do not provide exact candidate/fixture coverage"
        )
    for row_id, artifact in artifacts.items():
        for field in ("command", "environment", "stdout", "stderr", "result", "timing"):
            safe_artifact_file(
                bundle_root, artifact[field], f"observation {row_id}.{field}"
            )
    try:
        raw_rows = [
            json.loads(line)
            for line in runs_path.read_text(encoding="utf-8").splitlines()
            if line
        ]
    except json.JSONDecodeError as error:
        raise ValueError("runs.jsonl contains invalid JSON") from error
    if not raw_rows:
        raise ValueError("runs.jsonl has no measurements")
    rows: list[dict[str, object]] = []
    row_ids: set[str] = set()
    coverage: set[tuple[str, str, int]] = set()
    for index, raw in enumerate(raw_rows):
        row = require_object(raw, f"runs row {index}")
        if row.get("schemaVersion") != 2:
            raise ValueError(f"runs row {index} schemaVersion must be 2")
        row_id = require_string(row.get("rowId"), f"runs row {index}.rowId")
        if row_id in row_ids:
            raise ValueError(f"duplicate runs rowId: {row_id}")
        row_ids.add(row_id)
        artifact = artifacts.get(row_id)
        if artifact is None or artifact.get("phase") != "measurement":
            raise ValueError(f"runs row {row_id} does not have a measurement artifact")
        observation_artifact = require_object(
            row.get("observationArtifact"), f"runs row {row_id}.observationArtifact"
        )
        if (
            observation_artifact.get("rowId") != row_id
            or observation_artifact != artifact
        ):
            raise ValueError(
                f"runs row {row_id} observationArtifact does not match artifactIndex"
            )
        candidate_label = require_string(
            row.get("candidate"), f"runs row {row_id}.candidate"
        )
        utterance = require_string(row.get("utterance"), f"runs row {row_id}.utterance")
        if (
            artifact.get("candidate") != candidate_label
            or artifact.get("utterance") != utterance
        ):
            raise ValueError(
                f"runs row {row_id} does not match its observation artifact"
            )
        if candidate_label not in candidates or utterance not in fixtures:
            raise ValueError(f"runs row {row_id} has an unknown candidate or fixture")
        repeat = require_int(row.get("repeat"), f"runs row {row_id}.repeat", 1)
        if repeat > repeats:
            raise ValueError(f"runs row {row_id} repeat exceeds run-manifest.repeats")
        key = (candidate_label, utterance, repeat)
        if key in coverage:
            raise ValueError(f"duplicate candidate/fixture/repeat evidence: {key}")
        coverage.add(key)
        if row.get("runId") is not None and row.get("runId") != manifest["runId"]:
            raise ValueError(f"runs row {row_id} has a foreign runId")
        verified = replay_artifact(
            bundle_root,
            artifact,
            row,
            bindings[utterance],
            fixtures[utterance],
            candidates[candidate_label],
            binary["path"],
        )
        verified.update(
            {
                "echoVersion": echo.get("version"),
                "echoCommit": echo.get("commit"),
                "echoDirty": echo.get("dirty"),
                "echoBinary": verify_identity(binary, "run-manifest.binary"),
                "host": host,
                "seed": seed,
                "cacheState": manifest.get("cacheState"),
                "resetCycle": manifest.get("resetCycle"),
                "warmups": warmups,
            }
        )
        rows.append(verified)
    if row_ids != measurement_ids:
        raise ValueError(
            "runs.jsonl and measurement artifact row IDs do not match exactly"
        )
    expected_coverage = {
        (candidate, utterance, repeat)
        for candidate in candidates
        for utterance in bindings
        for repeat in range(1, repeats + 1)
    }
    if coverage != expected_coverage:
        raise ValueError(
            "measurement rows do not provide exact candidate/fixture/repeat coverage"
        )
    return rows, corpus


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def candidate_rows(
    rows: list[dict[str, object]], label: str
) -> list[dict[str, object]]:
    selected = [row for row in rows if row.get("candidate") == label]
    if not selected:
        raise ValueError(f"candidate is absent from runs: {label}")
    return selected


def pair_key(row: dict[str, object]) -> tuple[str, str, str, int]:
    return (
        str(row.get("resetCycle") or "unverified"),
        str(row.get("cacheState") or "unverified"),
        str(row["utterance"]),
        int(row["repeat"]),
    )


def execution_identities(rows: list[dict[str, object]]) -> set[str]:
    identities = set()
    for row in rows:
        engine = row.get("engine") or {}
        echo_binary = row.get("echoBinary") or {}
        runtime = row.get("runtimeArtifact") or {}
        model = row.get("modelArtifact") or {}
        vad_artifact = row.get("vadArtifact") or {}
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
            "engineVad": engine.get("vad"),
            "runtimeSha256": runtime.get("sha256"),
            "modelSha256": model.get("sha256"),
            "vadSha256": vad_artifact.get("sha256"),
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
        and isinstance(engine.get("vad"), (bool, type(None)))
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
    return set(required_languages).issubset(present_languages) and set(
        required_classes
    ).issubset(present_classes)


def summarize(
    rows: list[dict[str, object]],
    cpu_label: str,
    accelerated_label: str,
    expected_backend: str,
    coverage_complete: bool,
) -> dict[str, object]:
    cpu = candidate_rows(rows, cpu_label)
    accelerated = candidate_rows(rows, accelerated_label)
    selected_rows = cpu + accelerated
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
                "cacheState",
                "resetCycle",
            )
        )
        for control, candidate in pairs
    )
    counts: dict[tuple[str, str, str], int] = defaultdict(int)
    for reset_cycle, cache_state, utterance, _repeat in cpu_by_key:
        counts[(reset_cycle, cache_state, utterance)] += 1
    sample_size = (
        complete_pairs
        and bool(counts)
        and all(count >= 10 for count in counts.values())
    )
    reductions = [
        float(control["outerMs"]) - float(candidate["outerMs"])
        for control, candidate in pairs
    ]
    speedups = [
        100
        * (float(control["outerMs"]) - float(candidate["outerMs"]))
        / float(control["outerMs"])
        for control, candidate in pairs
    ]
    cpu_outer = [float(row["outerMs"]) for row in cpu]
    accelerated_outer = [float(row["outerMs"]) for row in accelerated]
    language_rows: dict[str, dict[str, object]] = {}
    quality_ok = True
    for language in sorted({str(row["language"]) for row in selected_rows}):
        control = [row for row in cpu if row["language"] == language]
        candidate = [row for row in accelerated if row["language"] == language]
        control_spoken = [row for row in control if int(row["referenceWords"]) > 0]
        candidate_spoken = [row for row in candidate if int(row["referenceWords"]) > 0]
        control_words = sum(int(row["referenceWords"]) for row in control_spoken)
        candidate_words = sum(int(row["referenceWords"]) for row in candidate_spoken)
        if control_words != candidate_words:
            raise ValueError(f"invalid reference-word totals for language {language}")
        control_wer = (
            sum(int(row["wordErrors"]) for row in control_spoken) / control_words
            if control_words
            else None
        )
        candidate_wer = (
            sum(int(row["wordErrors"]) for row in candidate_spoken) / candidate_words
            if candidate_words
            else None
        )
        delta = (
            candidate_wer - control_wer
            if candidate_wer is not None and control_wer is not None
            else None
        )
        passed = delta is None or delta <= 0.005
        quality_ok = quality_ok and passed
        language_rows[language] = {
            "cpuWer": round(control_wer, 6) if control_wer is not None else None,
            "acceleratedWer": (
                round(candidate_wer, 6) if candidate_wer is not None else None
            ),
            "deltaPercentagePoints": round(delta * 100, 3)
            if delta is not None
            else None,
            "qualityGate": passed,
            "cpuMedianOuterMs": round(
                statistics.median(float(row["outerMs"]) for row in control), 3
            ),
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
        (row.get("whisper") or {}).get("runtime", {}).get("device")
        for row in accelerated
    ]
    backend_truth = cpu_backends == {"cpu"} and accelerated_backends == {
        expected_backend
    }
    cpu_identities = execution_identities(cpu)
    accelerated_identities = execution_identities(accelerated)
    identity_match = (
        len(cpu_identities) == 1
        and cpu_identities == accelerated_identities
        and all(execution_identity_complete(row) for row in selected_rows)
    )
    hardware_device = (
        bool(devices)
        and all(
            device
            and not any(
                name in str(device).casefold()
                for name in ("lavapipe", "llvmpipe", "swiftshader")
            )
            for device in devices
        )
        and len({str(device) for device in devices}) == 1
    )
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
        # Phase 4 replaces these caller labels with collected receipts. They cannot admit a run.
        "driverIcdIdentity": False,
        "freshAndPopulatedCacheEvidence": False,
        "resetEvidence": False,
        "medianReduction": median_reduction >= 500,
        "medianSpeedup": median_speedup >= 20,
        "p95Improved": percentile(accelerated_outer, 0.95)
        < percentile(cpu_outer, 0.95),
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
        cpu_wer = "n/a" if value["cpuWer"] is None else f"{value['cpuWer']:.2%}"
        accelerated_wer = (
            "n/a"
            if value["acceleratedWer"] is None
            else f"{value['acceleratedWer']:.2%}"
        )
        delta = (
            "n/a"
            if value["deltaPercentagePoints"] is None
            else str(value["deltaPercentagePoints"])
        )
        lines.append(
            f"| {language} | {cpu_wer} | {accelerated_wer} | "
            f"{delta} | {'PASS' if value['qualityGate'] else 'FAIL'} |"
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
    for reset_cycle in ("before-reset", "after-reset"):
        for cache_state in ("fresh", "populated"):
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
                            "reference": "ten reference words are represented by this test fixture row",
                            "referenceWords": 10,
                            "wordErrors": errors,
                            "hallucinatedSilence": False,
                            "outerMs": elapsed,
                            "cacheState": cache_state,
                            "resetCycle": reset_cycle,
                            "echoVersion": "echo-desktop 1.0.0",
                            "echoCommit": "c" * 40,
                            "echoDirty": False,
                            "echoBinary": {"sha256": "d" * 64},
                            "host": {
                                "system": "Linux",
                                "machine": "x86_64",
                                "driverIdentity": "test-driver-1",
                                "icdIdentity": "test-icd-1",
                            },
                            "seed": 1,
                            "warmups": 1,
                            "audioSha256": "e" * 64,
                            "engine": {"model": "base-q5_1", "vad": False},
                            "runtimeArtifact": {"sha256": "a" * 64},
                            "modelArtifact": {"sha256": "b" * 64},
                            "whisper": {
                                "runtime": {"backend": backend, "device": device},
                                "tuning": {"threads": 4, "beamSize": 1},
                            },
                        }
                    )
    rows.append({"candidate": "unrelated", "language": "it"})
    passed = summarize(rows, "cpu", "gpu", "vulkan", True)
    assert passed["decision"] == "stop"
    assert not passed["gates"]["driverIcdIdentity"]
    assert not passed["gates"]["freshAndPopulatedCacheEvidence"]
    assert not passed["gates"]["resetEvidence"]
    silence_rows = json.loads(json.dumps(rows[:-1]))
    for row in silence_rows:
        row["language"] = "auto"
        row["referenceWords"] = 0
        row["wordErrors"] = 0
    silence = summarize(silence_rows, "cpu", "gpu", "vulkan", False)
    assert silence["languages"]["auto"]["cpuWer"] is None
    assert silence["languages"]["auto"]["acceleratedWer"] is None
    unverified = json.loads(json.dumps(rows))
    for row in unverified:
        if row.get("candidate") not in {"cpu", "gpu"}:
            continue
        row["cacheState"] = "populated"
        row["resetCycle"] = "unverified"
        row["host"]["driverIdentity"] = None
        row["host"]["icdIdentity"] = None
    evidence_failed = summarize(unverified, "cpu", "gpu", "vulkan", True)
    assert evidence_failed["decision"] == "stop"
    assert not evidence_failed["gates"]["driverIcdIdentity"]
    assert not evidence_failed["gates"]["freshAndPopulatedCacheEvidence"]
    assert not evidence_failed["gates"]["resetEvidence"]
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
    required = [
        args.runs,
        args.corpus_manifest,
        args.cpu_candidate,
        args.accelerated_candidate,
        args.output_dir,
    ]
    if any(value is None for value in required):
        parser.error("runs, corpus manifest, candidates, and output dir are required")
    try:
        rows, corpus = verify_bundle(args.runs, args.corpus_manifest)
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
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"analyze-stt-host-matrix: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
