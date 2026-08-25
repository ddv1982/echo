#!/usr/bin/env python3
"""Benchmark Echo speech candidates through the shipping transcribe CLI."""

from __future__ import annotations

import argparse
import contextlib
import datetime
import hashlib
import json
import os
import platform
import random
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time
import unicodedata
import uuid
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

CHARACTER_ERROR_LANGUAGES = {"ja", "zh", "yue", "th", "lo", "km", "my"}
REPO_ROOT = Path(__file__).resolve().parent.parent
RUN_BUNDLE_SCHEMA_VERSION = 1
STATUS_SCHEMA_VERSION = 1
ENVIRONMENT_NAMES = {
    "ECHO_MODEL_DIR",
    "ECHO_WHISPER_MODEL",
    "HOME",
    "LANG",
    "LANGUAGE",
    "PATH",
    "TEMP",
    "TMP",
    "TMPDIR",
    "TZ",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
}
ENVIRONMENT_PREFIXES = (
    "CUDA_",
    "DYLD_",
    "GGML_",
    "HIP_",
    "HSA_",
    "LC_",
    "LD_",
    "LIBVA_",
    "MESA_",
    "OMP_",
    "OPENBLAS_",
    "OPENVINO_",
    "RAYON_",
    "ROCR_",
    "VK_",
    "ZE_",
)


class BenchmarkInterrupted(RuntimeError):
    """Raised when a signal interrupts a benchmark attempt."""


def utc_now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def write_atomic(path: Path, value: bytes) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{uuid.uuid4().hex}.tmp")
    try:
        temporary.write_bytes(value)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_json_atomic(path: Path, value: object) -> None:
    write_atomic(
        path,
        (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8"),
    )


def write_text_atomic(path: Path, value: str) -> None:
    write_atomic(path, value.encode("utf-8"))


def prepare_output_dir(output_dir: Path) -> None:
    if output_dir.exists():
        if not output_dir.is_dir():
            raise ValueError(f"output path is not a directory: {output_dir}")
        if any(output_dir.iterdir()):
            raise ValueError(f"output directory must be empty: {output_dir}")
        return
    output_dir.mkdir(parents=True)


def write_status(
    output_dir: Path,
    run_id: str,
    state: str,
    started_at: str,
    **detail: object,
) -> None:
    payload = {
        "schemaVersion": STATUS_SCHEMA_VERSION,
        "runId": run_id,
        "state": state,
        "startedAt": started_at,
        "updatedAt": utc_now(),
        **detail,
    }
    write_json_atomic(output_dir / "status.json", payload)


def selected_environment(root: Path) -> dict[str, str]:
    environment = {
        name: value
        for name, value in os.environ.items()
        if value
        and (name in ENVIRONMENT_NAMES or name.startswith(ENVIRONMENT_PREFIXES))
    }
    environment.update(
        {
            "ECHO_CONFIG_DIR": str(root / "config"),
            "ECHO_DATA_DIR": str(root / "data"),
            "ECHO_CLEANUP": "off",
            "TMPDIR": str(root / "tmp"),
        }
    )
    return environment


def relative_artifact_path(output_dir: Path, path: Path) -> str:
    return path.relative_to(output_dir).as_posix()


def write_artifact(output_dir: Path, path: Path, value: bytes) -> dict[str, object]:
    write_atomic(path, value)
    return {
        "path": relative_artifact_path(output_dir, path),
        "sha256": sha256_bytes(value),
        "bytes": len(value),
    }


def verify_manifest_unchanged(manifest_path: Path, manifest_digest: str) -> None:
    if sha256(manifest_path) != manifest_digest:
        raise ValueError("corpus manifest changed during benchmark")


def verify_utterance_unchanged(utterance: dict[str, object]) -> None:
    audio = Path(utterance["audio"])
    if sha256(audio) != utterance["audioSha256"]:
        raise ValueError(f"audio fixture changed during benchmark: {audio}")


def verify_all_utterances_unchanged(utterances: list[dict[str, object]]) -> None:
    for utterance in utterances:
        verify_utterance_unchanged(utterance)


@contextlib.contextmanager
def interruption_handler() -> object:
    def interrupt(signum: int, _frame: object) -> None:
        raise BenchmarkInterrupted(f"interrupted by {signal.Signals(signum).name}")

    originals = {signum: signal.getsignal(signum) for signum in (signal.SIGINT, signal.SIGTERM)}
    try:
        for signum in originals:
            signal.signal(signum, interrupt)
        yield
    finally:
        for signum, handler in originals.items():
            signal.signal(signum, handler)


def portable_path(path: Path) -> str:
    resolved = path.resolve()
    for label, root in (("$REPO", REPO_ROOT), ("$HOME", Path.home())):
        try:
            relative = resolved.relative_to(root.resolve())
        except ValueError:
            continue
        return str(Path(label) / relative)
    return str(resolved)


def portable_whisper_telemetry(value: object) -> dict[str, object] | None:
    if not isinstance(value, dict):
        return None
    telemetry = dict(value)
    runtime = telemetry.get("runtime")
    if isinstance(runtime, dict):
        portable_runtime = dict(runtime)
        for key in (
            "binary",
            "libraryPath",
            "vulkanDriverFiles",
            "mesaShaderCacheDir",
        ):
            if isinstance(portable_runtime.get(key), str):
                portable_runtime[key] = portable_path(Path(portable_runtime[key]))
        telemetry["runtime"] = portable_runtime
    return telemetry


@dataclass(frozen=True)
class Candidate:
    label: str
    engine: str
    model: str | None
    threads: int | None = None
    beam_size: int | None = None
    best_of: int | None = None
    no_fallback: bool = False
    force_cpu: bool = False


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


def hallucinated_silence(reference_words: list[str], transcript: str) -> bool:
    return not reference_words and bool(transcript.strip())


def parse_candidate(raw: str) -> Candidate:
    if raw in {"fake", "parakeet"}:
        return Candidate(label=raw, engine=raw, model=None)
    base, option_separator, option_text = raw.partition("@")
    engine, separator, model = base.partition(":")
    if engine == "whisper" and separator and model:
        values: dict[str, int | bool] = {}
        if option_separator:
            for entry in option_text.split(","):
                key, value_separator, value = entry.partition("=")
                if key in {"no-fallback", "cpu-only"} and not value_separator:
                    values[key] = True
                    continue
                if key not in {"threads", "beam", "best-of", "no-fallback"} or not value_separator:
                    raise argparse.ArgumentTypeError(f"invalid Whisper candidate option: {entry}")
                if key == "no-fallback":
                    if value not in {"true", "false"}:
                        raise argparse.ArgumentTypeError("no-fallback must be true or false")
                    if value == "false":
                        raise argparse.ArgumentTypeError(
                            "omit no-fallback to keep the runtime fallback default"
                        )
                    values[key] = True
                    continue
                try:
                    parsed = int(value)
                except ValueError as error:
                    raise argparse.ArgumentTypeError(f"{key} must be an integer") from error
                if parsed < 1:
                    raise argparse.ArgumentTypeError(f"{key} must be at least 1")
                values[key] = parsed
        return Candidate(
            label=raw,
            engine=engine,
            model=model,
            threads=values.get("threads"),
            beam_size=values.get("beam"),
            best_of=values.get("best-of"),
            no_fallback=bool(values.get("no-fallback", False)),
            force_cpu=bool(values.get("cpu-only", False)),
        )
    raise argparse.ArgumentTypeError(
        "candidate must be fake, parakeet, or "
        "whisper:MODEL[@threads=N,beam=N,best-of=N,no-fallback,cpu-only]"
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest(path: Path, source: bytes | None = None) -> list[dict[str, object]]:
    value = json.loads(source if source is not None else path.read_bytes())
    if value.get("schemaVersion") != 1 or not isinstance(value.get("utterances"), list):
        raise ValueError("manifest must have schemaVersion 1 and an utterances array")
    utterances: list[dict[str, object]] = []
    seen: set[str] = set()
    for item in value["utterances"]:
        if not isinstance(item, dict):
            raise ValueError("each utterance must be an object")
        identifier = item.get("id")
        relative_file = item.get("file")
        language = item.get("language")
        reference = item.get("reference")
        if not all(isinstance(field, str) for field in [identifier, relative_file, language, reference]):
            raise ValueError("utterance id, file, language, and reference must be strings")
        if identifier in seen:
            raise ValueError(f"duplicate utterance id: {identifier}")
        if language in CHARACTER_ERROR_LANGUAGES and reference.strip():
            raise ValueError(
                f"{identifier} uses {language}, which needs a character-error-rate scorer"
            )
        seen.add(identifier)
        audio = (path.parent / relative_file).resolve()
        if not audio.is_file():
            raise ValueError(f"audio fixture is missing: {audio}")
        utterances.append(
            {
                "id": identifier,
                "audio": audio,
                "language": language,
                "reference": reference,
                "audioSha256": sha256(audio),
            }
        )
    if not utterances:
        raise ValueError("manifest has no utterances")
    return utterances


def command_for(binary: Path, candidate: Candidate, utterance: dict[str, object]) -> list[str]:
    requested_language = "auto" if candidate.engine == "parakeet" else str(utterance["language"])
    command = [
        str(binary),
        "transcribe",
        str(utterance["audio"]),
        "--language",
        requested_language,
        "--format",
        "json",
    ]
    if candidate.engine != "fake":
        command.extend(["--engine", candidate.engine])
    if candidate.model is not None:
        command.extend(["--model", candidate.model])
    for flag, value in [
        ("--whisper-threads", candidate.threads),
        ("--whisper-beam-size", candidate.beam_size),
        ("--whisper-best-of", candidate.best_of),
    ]:
        if value is not None:
            command.extend([flag, str(value)])
    if candidate.no_fallback:
        command.append("--whisper-no-fallback")
    if candidate.force_cpu:
        command.append("--whisper-no-gpu")
    return command


def host_metadata() -> dict[str, object]:
    uname = platform.uname()
    return {
        "system": uname.system,
        "release": uname.release,
        "machine": uname.machine,
        "processor": uname.processor,
        "cpuCount": os.cpu_count(),
    }


def artifact_identity(
    raw_path: object, cache: dict[str, dict[str, object]]
) -> dict[str, object] | None:
    if not isinstance(raw_path, str) or not raw_path:
        return None
    if raw_path in cache:
        return cache[raw_path]
    path = Path(raw_path)
    identity = {
        "path": portable_path(path),
        "sha256": sha256(path) if path.is_file() else None,
    }
    cache[raw_path] = identity
    return identity


def invoke_candidate(
    command: list[str], environment: dict[str, str]
) -> tuple[subprocess.CompletedProcess[str], float]:
    started = time.perf_counter_ns()
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    outer_ms = (time.perf_counter_ns() - started) / 1_000_000
    return completed, outer_ms


def capture_observation(
    output_dir: Path,
    manifest_path: Path,
    run_manifest: dict[str, object],
    binary: Path,
    candidate: Candidate,
    utterance: dict[str, object],
    environment: dict[str, str],
    observation_id: str,
    phase: str,
) -> tuple[dict[str, object], float, dict[str, object]]:
    verify_utterance_unchanged(utterance)
    command = command_for(binary, candidate, utterance)
    started_at = utc_now()
    completed: subprocess.CompletedProcess[str] | None = None
    invocation_error: OSError | None = None
    outer_ms = 0.0
    try:
        completed, outer_ms = invoke_candidate(command, environment)
    except OSError as error:
        invocation_error = error
    product: object | None = None
    parse_error: str | None = None
    if completed is not None and completed.returncode == 0:
        try:
            product = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            parse_error = str(error)

    artifact_dir = output_dir / "artifacts" / observation_id
    artifact_dir.mkdir(parents=True, exist_ok=True)
    stdout = completed.stdout if completed is not None else ""
    stderr = completed.stderr if completed is not None else str(invocation_error or "")
    return_code = completed.returncode if completed is not None else None
    raw_files = {
        "command": ("command.json", json.dumps(command, indent=2, ensure_ascii=False) + "\n"),
        "environment": (
            "environment.json",
            json.dumps(environment, indent=2, sort_keys=True) + "\n",
        ),
        "stdout": ("stdout.txt", stdout),
        "stderr": ("stderr.txt", stderr),
        "result": (
            "result.json",
            json.dumps(
                {
                    "schemaVersion": RUN_BUNDLE_SCHEMA_VERSION,
                    "returnCode": return_code,
                    "productJson": product,
                    "parseError": parse_error,
                    "invocationError": str(invocation_error) if invocation_error else None,
                },
                indent=2,
                sort_keys=True,
                ensure_ascii=False,
            )
            + "\n",
        ),
        "timing": (
            "timing.json",
            json.dumps(
                {
                    "schemaVersion": RUN_BUNDLE_SCHEMA_VERSION,
                    "startedAt": started_at,
                    "finishedAt": utc_now(),
                    "wallMs": round(outer_ms, 3),
                    "returnCode": return_code,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
        ),
    }
    artifacts = {
        name: write_artifact(output_dir, artifact_dir / filename, value.encode("utf-8"))
        for name, (filename, value) in raw_files.items()
    }
    artifact = {
        "schemaVersion": RUN_BUNDLE_SCHEMA_VERSION,
        "rowId": observation_id,
        "phase": phase,
        "candidate": candidate.label,
        "utterance": utterance["id"],
        **artifacts,
    }
    artifact_index = run_manifest["artifactIndex"]
    assert isinstance(artifact_index, list)
    artifact_index.append(artifact)
    write_json_atomic(manifest_path, run_manifest)

    verify_utterance_unchanged(utterance)
    if invocation_error is not None:
        raise RuntimeError(
            f"could not invoke {candidate.label} for {utterance['id']}: {invocation_error}"
        )
    assert completed is not None
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"{candidate.label} failed for {utterance['id']}: {detail}")
    if parse_error is not None:
        raise ValueError(f"Echo JSON output is invalid: {parse_error}")
    if not isinstance(product, dict):
        raise ValueError("Echo JSON output must be an object")
    return product, outer_ms, artifact


def run_benchmark(args: argparse.Namespace) -> None:
    output_dir = args.output_dir.resolve()
    prepare_output_dir(output_dir)
    run_id = f"stt-{datetime.datetime.now(datetime.timezone.utc):%Y%m%dT%H%M%SZ}-{uuid.uuid4().hex}"
    started_at = utc_now()
    write_status(output_dir, run_id, "running", started_at)
    try:
        with interruption_handler():
            binary = args.binary.resolve()
            if not binary.is_file():
                raise ValueError(f"Echo binary is missing: {binary}")
            corpus_path = args.manifest.resolve()
            corpus_source = corpus_path.read_bytes()
            manifest_digest = sha256_bytes(corpus_source)
            utterances = load_manifest(corpus_path, corpus_source)
            labels = [candidate.label for candidate in args.candidate]
            if len(labels) != len(set(labels)):
                raise ValueError("candidate labels must be unique")

            write_atomic(output_dir / "corpus-manifest.json", corpus_source)
            binary_identity = {"path": portable_path(binary), "sha256": sha256(binary)}
            run_manifest: dict[str, object] = {
                "schemaVersion": RUN_BUNDLE_SCHEMA_VERSION,
                "runId": run_id,
                "startedAt": started_at,
                "binary": binary_identity,
                "corpus": {
                    "sourcePath": portable_path(corpus_path),
                    "snapshot": {
                        "path": "corpus-manifest.json",
                        "sha256": manifest_digest,
                    },
                    "utterances": [
                        {
                            "id": utterance["id"],
                            "audio": portable_path(Path(utterance["audio"])),
                            "audioSha256": utterance["audioSha256"],
                        }
                        for utterance in utterances
                    ],
                },
                "candidates": [
                    {
                        "label": candidate.label,
                        "engine": candidate.engine,
                        "model": candidate.model,
                        "threads": candidate.threads,
                        "beamSize": candidate.beam_size,
                        "bestOf": candidate.best_of,
                        "noFallback": candidate.no_fallback,
                        "forceCpu": candidate.force_cpu,
                    }
                    for candidate in args.candidate
                ],
                "seed": args.seed,
                "repeats": args.repeats,
                "warmups": args.warmups,
                "cacheState": args.cache_state,
                "resetCycle": args.reset_cycle,
                "artifactIndex": [],
            }
            run_manifest_path = output_dir / "run-manifest.json"
            write_json_atomic(run_manifest_path, run_manifest)

            rows: list[dict[str, object]] = []
            host = host_metadata()
            host["driverIdentity"] = args.driver_identity
            host["icdIdentity"] = args.icd_identity
            artifact_identities: dict[str, dict[str, object]] = {}
            rng = random.Random(args.seed)
            observation_count = 0
            with tempfile.TemporaryDirectory(prefix="echo-stt-benchmark-") as temporary:
                root = Path(temporary)
                for directory in (root / "config", root / "data", root / "tmp"):
                    directory.mkdir(parents=True, exist_ok=True)
                environment = selected_environment(root)
                version = subprocess.run(
                    [str(binary), "--version"],
                    check=True,
                    capture_output=True,
                    text=True,
                    env=environment,
                ).stdout.strip()
                echo_commit = subprocess.run(
                    ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
                    check=True,
                    capture_output=True,
                    text=True,
                ).stdout.strip()
                echo_dirty = bool(
                    subprocess.run(
                        ["git", "-C", str(REPO_ROOT), "status", "--porcelain"],
                        check=True,
                        capture_output=True,
                        text=True,
                    ).stdout.strip()
                )
                run_manifest["echo"] = {
                    "version": version,
                    "commit": echo_commit,
                    "dirty": echo_dirty,
                }
                run_manifest["host"] = host
                write_json_atomic(run_manifest_path, run_manifest)
                verify_manifest_unchanged(corpus_path, manifest_digest)

                environments: dict[str, dict[str, str]] = {}
                for candidate in args.candidate:
                    candidate_environment = environment.copy()
                    if candidate.engine == "fake":
                        candidate_environment["ECHO_ENGINE"] = "fake"
                    environments[candidate.label] = candidate_environment
                    for utterance in utterances:
                        for _ in range(args.warmups):
                            observation_count += 1
                            payload, _, _ = capture_observation(
                                output_dir,
                                run_manifest_path,
                                run_manifest,
                                binary,
                                candidate,
                                utterance,
                                candidate_environment,
                                f"warmup-{observation_count:06d}",
                                "warmup",
                            )
                            engine = payload.get("engine")
                            if isinstance(engine, dict):
                                artifact_identity(engine.get("binary"), artifact_identities)
                                artifact_identity(engine.get("modelPath"), artifact_identities)
                for repeat in range(args.repeats):
                    for utterance in utterances:
                        ordered = list(args.candidate)
                        rng.shuffle(ordered)
                        for order_index, candidate in enumerate(ordered):
                            observation_count += 1
                            row_id = f"observation-{observation_count:06d}"
                            payload, outer_ms, observation_artifact = capture_observation(
                                output_dir,
                                run_manifest_path,
                                run_manifest,
                                binary,
                                candidate,
                                utterance,
                                environments[candidate.label],
                                row_id,
                                "measurement",
                            )
                            reference_words = normalized_words(str(utterance["reference"]))
                            transcript = str(payload["text"])
                            output_words = normalized_words(transcript)
                            errors = edit_distance(reference_words, output_words)
                            audio_ms = int(payload["audioMs"])
                            infer_ms = int(payload["inferMs"])
                            engine = payload.get("engine")
                            if not isinstance(engine, dict):
                                raise ValueError("Echo JSON output has no engine object")
                            portable_engine = dict(engine)
                            for key in ("binary", "modelPath"):
                                if isinstance(portable_engine.get(key), str):
                                    portable_engine[key] = portable_path(Path(portable_engine[key]))
                            whisper = payload.get("whisper")
                            rows.append(
                                {
                                    "schemaVersion": 2,
                                    "rowId": row_id,
                                    "observationArtifact": observation_artifact,
                                    "echoVersion": version,
                                    "echoCommit": echo_commit,
                                    "echoDirty": echo_dirty,
                                    "echoBinary": binary_identity,
                                    "host": host,
                                    "seed": args.seed,
                                    "cacheState": args.cache_state,
                                    "resetCycle": args.reset_cycle,
                                    "candidate": candidate.label,
                                    "candidateOrder": order_index + 1,
                                    "utterance": utterance["id"],
                                    "language": utterance["language"],
                                    "repeat": repeat + 1,
                                    "audio": portable_path(Path(utterance["audio"])),
                                    "audioSha256": utterance["audioSha256"],
                                    "reference": utterance["reference"],
                                    "text": transcript,
                                    "raw": payload["raw"],
                                    "wordErrors": errors,
                                    "referenceWords": len(reference_words),
                                    "wer": errors / len(reference_words) if reference_words else None,
                                    "hallucinatedSilence": hallucinated_silence(
                                        reference_words, transcript
                                    ),
                                    "audioMs": audio_ms,
                                    "inferMs": infer_ms,
                                    "outerMs": round(outer_ms, 3),
                                    "rtf": infer_ms / audio_ms if audio_ms else None,
                                    "engine": portable_engine,
                                    "runtimeArtifact": artifact_identity(
                                        engine.get("binary"), artifact_identities
                                    ),
                                    "modelArtifact": artifact_identity(
                                        engine.get("modelPath"), artifact_identities
                                    ),
                                    "whisper": portable_whisper_telemetry(whisper),
                                    "warmups": args.warmups,
                                }
                            )
                verify_manifest_unchanged(corpus_path, manifest_digest)
                verify_all_utterances_unchanged(utterances)
            write_text_atomic(
                output_dir / "runs.jsonl",
                "".join(json.dumps(row, sort_keys=True, ensure_ascii=False) + "\n" for row in rows),
            )
            write_text_atomic(output_dir / "summary.md", render_summary(rows))
    except BaseException as error:
        write_status(
            output_dir,
            run_id,
            "failed",
            started_at,
            failedAt=utc_now(),
            failure={"type": type(error).__name__, "message": str(error)},
        )
        raise
    write_status(
        output_dir,
        run_id,
        "complete",
        started_at,
        completedAt=utc_now(),
    )


def render_summary(rows: list[dict[str, object]]) -> str:
    grouped: dict[tuple[str, str], list[dict[str, object]]] = defaultdict(list)
    for row in rows:
        grouped[(str(row["candidate"]), str(row["language"]))].append(row)
    lines = [
        "# Echo speech benchmark",
        "",
        "| Candidate | Language | WER | Median outer ms | Median RTF | Silence hallucinations | Runs |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for (candidate, language), values in sorted(grouped.items()):
        errors = sum(int(row["wordErrors"]) for row in values if int(row["referenceWords"]) > 0)
        words = sum(int(row["referenceWords"]) for row in values)
        rtf_values = [float(row["rtf"]) for row in values if row["rtf"] is not None]
        outer_values = [float(row["outerMs"]) for row in values]
        hallucinations = sum(bool(row["hallucinatedSilence"]) for row in values)
        wer = f"{errors / words:.2%}" if words else "n/a"
        rtf = f"{statistics.median(rtf_values):.3f}" if rtf_values else "n/a"
        outer = f"{statistics.median(outer_values):.1f}"
        lines.append(
            f"| {candidate} | {language} | {wer} | {outer} | {rtf} | {hallucinations} | {len(values)} |"
        )
    lines.extend(["", "RTF is `inferMs / audioMs`. Lower is faster.", ""])
    return "\n".join(lines)


def self_test() -> None:
    assert portable_path(REPO_ROOT / "target" / "fixture.wav") == "$REPO/target/fixture.wav"
    portable = portable_whisper_telemetry(
        {
            "runtime": {
                "binary": str(REPO_ROOT / "target" / "whisper-cli"),
                "libraryPath": str(REPO_ROOT / "target" / "runtime"),
            }
        }
    )
    assert portable == {
        "runtime": {
            "binary": "$REPO/target/whisper-cli",
            "libraryPath": "$REPO/target/runtime",
        }
    }
    assert normalized_words(" Héllo, WORLD! ") == ["héllo", "world"]
    assert normalized_words("ＡＢＣ 123") == ["abc", "123"]
    assert edit_distance(["one", "two"], ["one", "too"]) == 1
    assert edit_distance([], ["invented"]) == 1
    assert hallucinated_silence([], "...")
    assert not hallucinated_silence([], "  ")
    assert not hallucinated_silence(["spoken"], "spoken")
    parakeet_command = command_for(
        Path("echo-desktop"),
        Candidate(label="parakeet", engine="parakeet", model=None),
        {"audio": Path("speech.wav"), "language": "nl"},
    )
    assert parakeet_command[parakeet_command.index("--language") + 1] == "auto"
    tuned = parse_candidate(
        "whisper:large-v3-turbo-q5_0@threads=4,beam=1,best-of=2,no-fallback,cpu-only"
    )
    tuned_command = command_for(
        Path("echo-desktop"),
        tuned,
        {"audio": Path("speech.wav"), "language": "nl"},
    )
    assert tuned_command[-8:] == [
        "--whisper-threads",
        "4",
        "--whisper-beam-size",
        "1",
        "--whisper-best-of",
        "2",
        "--whisper-no-fallback",
        "--whisper-no-gpu",
    ]
    try:
        parse_candidate("whisper:small@no-fallback=false")
    except argparse.ArgumentTypeError as error:
        assert "omit no-fallback" in str(error)
    else:
        raise AssertionError("explicit fallback-enabled candidate was accepted")
    fake_rows = [
        {
            "candidate": "fake",
            "language": "en",
            "wordErrors": 0,
            "referenceWords": 2,
            "outerMs": 1.0,
            "rtf": 0.0,
            "hallucinatedSilence": False,
        }
    ]
    assert "| fake | en | 0.00% | 1.0 | 0.000 | 0 | 1 |" in render_summary(fake_rows)
    with tempfile.TemporaryDirectory(prefix="echo-stt-benchmark-test-") as temporary:
        root = Path(temporary)
        output = root / "output"
        prepare_output_dir(output)
        write_status(output, "test-run", "running", "before")
        write_status(output, "test-run", "complete", "before", completedAt="after")
        assert json.loads((output / "status.json").read_text())["state"] == "complete"
        artifact = write_artifact(output, output / "artifact.txt", b"artifact")
        assert artifact == {
            "path": "artifact.txt",
            "sha256": sha256_bytes(b"artifact"),
            "bytes": len(b"artifact"),
        }
        assert (output / artifact["path"]).read_bytes() == b"artifact"

        source_manifest = root / "fixtures.json"
        source_manifest.write_text('{"schemaVersion":1,"utterances":[]}', encoding="utf-8")
        manifest_digest = sha256(source_manifest)
        verify_manifest_unchanged(source_manifest, manifest_digest)
        source_manifest.write_text('{"schemaVersion":2,"utterances":[]}', encoding="utf-8")
        try:
            verify_manifest_unchanged(source_manifest, manifest_digest)
        except ValueError as error:
            assert "manifest changed" in str(error)
        else:
            raise AssertionError("mutated corpus manifest was accepted")

        stale = root / "stale"
        stale.mkdir()
        (stale / "old.txt").write_text("old", encoding="utf-8")
        try:
            prepare_output_dir(stale)
        except ValueError as error:
            assert "must be empty" in str(error)
        else:
            raise AssertionError("stale benchmark directory was accepted")


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--binary", required=True, type=Path)
    value.add_argument("--manifest", required=True, type=Path)
    value.add_argument("--candidate", required=True, action="append", type=parse_candidate)
    value.add_argument("--repeats", type=int, default=3)
    value.add_argument("--warmups", type=int, default=1)
    value.add_argument("--seed", type=int, default=20260824)
    value.add_argument(
        "--cache-state",
        choices=("fresh", "populated", "unverified"),
        default="unverified",
    )
    value.add_argument("--reset-cycle", default="unverified")
    value.add_argument("--driver-identity")
    value.add_argument("--icd-identity")
    value.add_argument("--output-dir", required=True, type=Path)
    return value


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        print("benchmark-stt: self-test ok")
        return 0
    args = parser().parse_args()
    if args.repeats < 1:
        parser().error("--repeats must be at least 1")
    if args.warmups < 0:
        parser().error("--warmups cannot be negative")
    try:
        run_benchmark(args)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"benchmark-stt: {error}", file=sys.stderr)
        return 1
    print(f"benchmark-stt: wrote {args.output_dir / 'runs.jsonl'} and summary.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
