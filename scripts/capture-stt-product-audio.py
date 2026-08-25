#!/usr/bin/env python3
from __future__ import annotations

import argparse
import array
import datetime
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import wave
from dataclasses import dataclass
from pathlib import Path


SCHEMA_VERSION = 1
RELEASE_TEXT = (
    "I created these recordings, consent to their use as Echo speech-recognition "
    "test fixtures, and grant them under the selected license."
)


@dataclass(frozen=True)
class Prompt:
    identifier: str
    product_class: str
    seconds: int
    text: str
    direction: str


PROMPTS = (
    Prompt(
        "product-dictation-en",
        "dictation",
        12,
        "Please schedule the design review for Tuesday afternoon and send the notes to the product team.",
        "Speak at your normal dictation pace and volume.",
    ),
    Prompt(
        "product-technical-en",
        "technical-identifiers",
        14,
        "Deploy API version two point four to node alpha seven, then open slash var slash lib slash echo slash config dot JSON.",
        "Read the identifiers naturally, as you would dictate them to a developer tool.",
    ),
    Prompt(
        "product-fast-en",
        "fast-speech",
        12,
        "Before lunch, summarize the customer feedback, update the roadmap, assign the follow-up actions, and notify everyone in the release channel.",
        "Speak naturally but faster than your normal dictation pace. Do not race or slur words.",
    ),
    Prompt(
        "product-quiet-en",
        "quiet-speech",
        12,
        "This is a quiet dictation recorded close to the microphone while the room remains naturally silent.",
        "Speak naturally and quietly. Keep the microphone in its normal position.",
    ),
)


def utc_now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inspect_wav(path: Path) -> dict[str, int | float]:
    with wave.open(str(path), "rb") as source:
        channels = source.getnchannels()
        sample_width = source.getsampwidth()
        rate = source.getframerate()
        frames = source.getnframes()
        raw = source.readframes(frames)
    if (channels, sample_width, rate) != (1, 2, 16_000):
        raise ValueError(
            f"{path.name} must be 16 kHz mono PCM16, got "
            f"{rate} Hz, {channels} channels, {sample_width * 8} bits"
        )
    samples = array.array("h")
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    if not samples:
        raise ValueError(f"{path.name} contains no samples")
    squares = sum(int(sample) * int(sample) for sample in samples)
    rms = (squares / len(samples)) ** 0.5
    peak = max(abs(int(sample)) for sample in samples)
    clipped = sum(abs(int(sample)) >= 32_760 for sample in samples)
    clipped_fraction = clipped / len(samples)
    if rms < 30:
        raise ValueError(f"{path.name} is effectively silent (RMS {rms:.1f})")
    if clipped_fraction > 0.005:
        raise ValueError(
            f"{path.name} clips {clipped_fraction:.2%} of samples; lower the input level"
        )
    return {
        "sampleRateHz": rate,
        "channels": channels,
        "bitsPerSample": sample_width * 8,
        "frames": frames,
        "durationMs": round(frames * 1000 / rate),
        "rms": round(rms, 3),
        "peak": peak,
        "clippedFraction": round(clipped_fraction, 8),
    }


def prepare_output(path: Path) -> None:
    if path.exists():
        if not path.is_dir() or any(path.iterdir()):
            raise ValueError(f"output directory must not exist or must be empty: {path}")
        return
    path.mkdir(parents=True)


def capture(args: argparse.Namespace) -> None:
    recorder = shutil.which("arecord")
    if recorder is None:
        raise RuntimeError("arecord is required")
    output = args.output_dir.resolve()
    prepare_output(output)
    statement = RELEASE_TEXT
    print("Echo product-speech fixture capture")
    print()
    print(statement)
    print(f"Contributor ID: {args.contributor_id}")
    print(f"License: {args.license}")
    print()
    answer = input("Type I CONSENT to continue, or anything else to stop: ")
    if answer != "I CONSENT":
        raise RuntimeError("consent was not granted; no recording started")
    captured_at = utc_now()
    fixtures = []
    for index, prompt in enumerate(PROMPTS, start=1):
        destination = output / f"{prompt.identifier}.wav"
        print()
        print(f"Recording {index}/{len(PROMPTS)}: {prompt.product_class}")
        print(prompt.direction)
        print(f"Say exactly: {prompt.text}")
        input("Press Enter when ready. Recording starts immediately: ")
        completed = subprocess.run(
            [
                recorder,
                "-q",
                "-D",
                args.device,
                "-f",
                "S16_LE",
                "-r",
                "16000",
                "-c",
                "1",
                "-d",
                str(prompt.seconds),
                str(destination),
            ],
            check=False,
        )
        if completed.returncode != 0:
            raise RuntimeError(
                f"arecord failed for {prompt.identifier} with exit {completed.returncode}"
            )
        stats = inspect_wav(destination)
        fixtures.append(
            {
                "id": prompt.identifier,
                "file": destination.name,
                "language": "en",
                "class": prompt.product_class,
                "reference": prompt.text,
                "bytes": destination.stat().st_size,
                "sha256": sha256(destination),
                "naturalClassEvidence": True,
                "synthetic": False,
                "capture": {
                    "capturedAt": captured_at,
                    "device": args.device,
                    "direction": prompt.direction,
                    "stats": stats,
                },
            }
        )
        print(
            f"Captured {destination.name}: {stats['durationMs']} ms, "
            f"RMS {stats['rms']}, peak {stats['peak']}"
        )
    manifest = {
        "schemaVersion": SCHEMA_VERSION,
        "createdAt": utc_now(),
        "contributor": {"id": args.contributor_id},
        "consent": {
            "statement": statement,
            "acceptedAt": captured_at,
            "license": args.license,
        },
        "utterances": fixtures,
    }
    (output / "capture.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print()
    print(f"Wrote {output / 'capture.json'}")


def make_test_wav(path: Path, amplitude: int) -> None:
    samples = array.array("h", [amplitude, -amplitude] * 16_000)
    if sys.byteorder != "little":
        samples.byteswap()
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(16_000)
        output.writeframes(samples.tobytes())


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-product-capture-") as temporary:
        root = Path(temporary)
        healthy = root / "healthy.wav"
        make_test_wav(healthy, 1000)
        stats = inspect_wav(healthy)
        assert stats["durationMs"] == 2000
        assert stats["rms"] == 1000
        silence = root / "silence.wav"
        make_test_wav(silence, 0)
        try:
            inspect_wav(silence)
        except ValueError as error:
            assert "effectively silent" in str(error)
        else:
            raise AssertionError("silent capture was accepted")
        clipped = root / "clipped.wav"
        make_test_wav(clipped, 32767)
        try:
            inspect_wav(clipped)
        except ValueError as error:
            assert "clips" in str(error)
        else:
            raise AssertionError("clipped capture was accepted")
    print("capture-stt-product-audio: self-test ok")


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser()
    value.add_argument("--self-test", action="store_true")
    value.add_argument("--output-dir", type=Path)
    value.add_argument("--device", default="default")
    value.add_argument("--contributor-id")
    value.add_argument(
        "--license",
        choices=("CC0-1.0", "CC-BY-4.0"),
        default="CC0-1.0",
    )
    return value


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.output_dir is None or not args.contributor_id:
        parser().error("--output-dir and --contributor-id are required")
    capture(args)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError) as error:
        print(f"capture-stt-product-audio: {error}", file=sys.stderr)
        raise SystemExit(2) from error
