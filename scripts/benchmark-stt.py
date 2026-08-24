#!/usr/bin/env python3
"""Benchmark Echo speech candidates through the shipping transcribe CLI."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import re
import statistics
import subprocess
import sys
import tempfile
import time
import unicodedata
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

CHARACTER_ERROR_LANGUAGES = {"ja", "zh", "yue", "th", "lo", "km", "my"}


@dataclass(frozen=True)
class Candidate:
    label: str
    engine: str
    model: str | None
    threads: int | None = None
    beam_size: int | None = None
    best_of: int | None = None
    no_fallback: bool = False


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
                if key == "no-fallback" and not value_separator:
                    values[key] = True
                    continue
                if key not in {"threads", "beam", "best-of", "no-fallback"} or not value_separator:
                    raise argparse.ArgumentTypeError(f"invalid Whisper candidate option: {entry}")
                if key == "no-fallback":
                    if value not in {"true", "false"}:
                        raise argparse.ArgumentTypeError("no-fallback must be true or false")
                    values[key] = value == "true"
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
        )
    raise argparse.ArgumentTypeError(
        "candidate must be fake, parakeet, or whisper:MODEL[@threads=N,beam=N,best-of=N,no-fallback]"
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest(path: Path) -> list[dict[str, object]]:
    value = json.loads(path.read_text(encoding="utf-8"))
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
        "path": str(path),
        "sha256": sha256(path) if path.is_file() else None,
    }
    cache[raw_path] = identity
    return identity


def invoke_candidate(
    binary: Path,
    candidate: Candidate,
    utterance: dict[str, object],
    environment: dict[str, str],
) -> tuple[subprocess.CompletedProcess[str], dict[str, object], float]:
    command = command_for(binary, candidate, utterance)
    started = time.perf_counter_ns()
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    outer_ms = (time.perf_counter_ns() - started) / 1_000_000
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"{candidate.label} failed for {utterance['id']}: {detail}")
    payload = json.loads(completed.stdout)
    if not isinstance(payload, dict):
        raise ValueError("Echo JSON output must be an object")
    return completed, payload, outer_ms


def run_benchmark(args: argparse.Namespace) -> None:
    binary = args.binary.resolve()
    if not binary.is_file():
        raise ValueError(f"Echo binary is missing: {binary}")
    utterances = load_manifest(args.manifest.resolve())
    labels = [candidate.label for candidate in args.candidate]
    if len(labels) != len(set(labels)):
        raise ValueError("candidate labels must be unique")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, object]] = []
    host = host_metadata()
    binary_identity = {"path": str(binary), "sha256": sha256(binary)}
    artifact_identities: dict[str, dict[str, object]] = {}
    rng = random.Random(args.seed)
    with tempfile.TemporaryDirectory(prefix="echo-stt-benchmark-") as temporary:
        root = Path(temporary)
        environment = os.environ.copy()
        environment.update(
            {
                "ECHO_CONFIG_DIR": str(root / "config"),
                "ECHO_DATA_DIR": str(root / "data"),
                "ECHO_CLEANUP": "off",
            }
        )
        version = subprocess.run(
            [str(binary), "--version"],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
        ).stdout.strip()
        environments: dict[str, dict[str, str]] = {}
        for candidate in args.candidate:
            candidate_environment = environment.copy()
            if candidate.engine == "fake":
                candidate_environment["ECHO_ENGINE"] = "fake"
            environments[candidate.label] = candidate_environment
            for utterance in utterances:
                for _ in range(args.warmups):
                    _, payload, _ = invoke_candidate(
                        binary, candidate, utterance, candidate_environment
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
                    _, payload, outer_ms = invoke_candidate(
                        binary,
                        candidate,
                        utterance,
                        environments[candidate.label],
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
                    whisper = payload.get("whisper")
                    rows.append(
                        {
                            "schemaVersion": 2,
                            "echoVersion": version,
                            "echoBinary": binary_identity,
                            "host": host,
                            "seed": args.seed,
                            "candidate": candidate.label,
                            "candidateOrder": order_index + 1,
                            "utterance": utterance["id"],
                            "language": utterance["language"],
                            "repeat": repeat + 1,
                            "audio": str(utterance["audio"]),
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
                            "engine": engine,
                            "runtimeArtifact": artifact_identity(
                                engine.get("binary"), artifact_identities
                            ),
                            "modelArtifact": artifact_identity(
                                engine.get("modelPath"), artifact_identities
                            ),
                            "whisper": whisper if isinstance(whisper, dict) else None,
                            "warmups": args.warmups,
                        }
                    )
    jsonl = args.output_dir / "runs.jsonl"
    jsonl.write_text(
        "".join(json.dumps(row, sort_keys=True, ensure_ascii=False) + "\n" for row in rows),
        encoding="utf-8",
    )
    (args.output_dir / "summary.md").write_text(render_summary(rows), encoding="utf-8")


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
        "whisper:large-v3-turbo-q5_0@threads=4,beam=1,best-of=2,no-fallback"
    )
    tuned_command = command_for(
        Path("echo-desktop"),
        tuned,
        {"audio": Path("speech.wav"), "language": "nl"},
    )
    assert tuned_command[-7:] == [
        "--whisper-threads",
        "4",
        "--whisper-beam-size",
        "1",
        "--whisper-best-of",
        "2",
        "--whisper-no-fallback",
    ]
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


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--binary", required=True, type=Path)
    value.add_argument("--manifest", required=True, type=Path)
    value.add_argument("--candidate", required=True, action="append", type=parse_candidate)
    value.add_argument("--repeats", type=int, default=3)
    value.add_argument("--warmups", type=int, default=1)
    value.add_argument("--seed", type=int, default=20260824)
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
