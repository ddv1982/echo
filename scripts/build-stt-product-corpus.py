#!/usr/bin/env python3
from __future__ import annotations

import argparse
import array
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import uuid
import wave
from collections import Counter
from pathlib import Path
from urllib.parse import urlparse


REPO_ROOT = Path(__file__).resolve().parent.parent
RELEASE_TEXT = (
    "I created these recordings, consent to their use as Echo speech-recognition "
    "test fixtures, and grant them under the selected license."
)
SUPPORTED_LICENSES = {
    "CC-BY-4.0": "https://creativecommons.org/licenses/by/4.0/",
    "CC0-1.0": "https://creativecommons.org/publicdomain/zero/1.0/",
}
ALLOWED_HOSTS = {"huggingface.co", "upload.wikimedia.org"}


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_atomic(path: Path, value: bytes) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{uuid.uuid4().hex}.tmp")
    try:
        temporary.write_bytes(value)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_json(path: Path, value: object) -> None:
    write_atomic(
        path,
        (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode(),
    )


def load_json(path: Path, label: str) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"could not read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be a JSON object")
    return value


def prepare_output(path: Path) -> None:
    if path.exists():
        if not path.is_dir() or any(path.iterdir()):
            raise ValueError(f"output directory must not exist or be empty: {path}")
    else:
        path.mkdir(parents=True)
    write_json(path / "status.json", {"schemaVersion": 1, "state": "running"})


def pcm16(samples: list[int]) -> bytes:
    values = array.array("h", [max(-32768, min(32767, value)) for value in samples])
    if sys.byteorder != "little":
        values.byteswap()
    return values.tobytes()


def wav_bytes(samples: list[int], rate: int = 16_000) -> bytes:
    with tempfile.SpooledTemporaryFile() as temporary:
        with wave.open(temporary, "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(rate)
            output.writeframes(pcm16(samples))
        temporary.seek(0)
        return temporary.read()


def read_wav(path: Path) -> tuple[int, int, list[int]]:
    with wave.open(str(path), "rb") as source:
        rate = source.getframerate()
        channels = source.getnchannels()
        width = source.getsampwidth()
        frames = source.getnframes()
        raw = source.readframes(frames)
    if width != 2 or channels not in {1, 2} or not raw:
        raise ValueError(f"{path.name} must be non-empty mono or stereo PCM16")
    values = array.array("h")
    values.frombytes(raw)
    if sys.byteorder != "little":
        values.byteswap()
    return rate, channels, list(values)


def mono(samples: list[int], channels: int) -> list[int]:
    if channels == 1:
        return samples
    return [int((samples[index] + samples[index + 1]) / 2) for index in range(0, len(samples), 2)]


def resample(samples: list[int], source_rate: int, target_rate: int = 16_000) -> list[int]:
    if source_rate == target_rate:
        return samples
    output_length = len(samples) * target_rate // source_rate
    output = []
    for index in range(output_length):
        position = index * source_rate
        left = position // target_rate
        fraction = position % target_rate
        right = min(left + 1, len(samples) - 1)
        numerator = samples[left] * (target_rate - fraction) + samples[right] * fraction
        output.append(round(numerator / target_rate))
    return output


def canonical_wav(path: Path) -> bytes:
    rate, channels, samples = read_wav(path)
    return wav_bytes(resample(mono(samples, channels), rate))


def wav_stats(value: bytes) -> dict[str, int | float]:
    with tempfile.NamedTemporaryFile(suffix=".wav") as temporary:
        temporary.write(value)
        temporary.flush()
        rate, channels, samples = read_wav(Path(temporary.name))
    if rate != 16_000 or channels != 1:
        raise ValueError("generated WAV is not 16 kHz mono")
    rms = math.sqrt(sum(sample * sample for sample in samples) / len(samples))
    peak = max(abs(sample) for sample in samples)
    clipped = sum(abs(sample) >= 32760 for sample in samples) / len(samples)
    return {
        "frames": len(samples),
        "durationMs": round(len(samples) * 1000 / rate),
        "rms": round(rms, 3),
        "peak": peak,
        "clippedFraction": round(clipped, 8),
    }


def obtain_external(name: str, item: dict[str, object], cache: Path) -> Path:
    url = item.get("url")
    expected_bytes = item.get("bytes")
    expected_hash = item.get("sha256")
    if (
        not isinstance(url, str)
        or not isinstance(expected_bytes, int)
        or not isinstance(expected_hash, str)
    ):
        raise ValueError(f"external {name} is incomplete")
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname not in ALLOWED_HOSTS:
        raise ValueError(f"external {name} has a disallowed URL")
    suffix = Path(parsed.path).suffix or ".source"
    destination = cache / f"{name}{suffix}"
    if destination.is_file():
        if destination.stat().st_size == expected_bytes and sha256(destination) == expected_hash:
            return destination
        raise ValueError(f"cached external {name} does not match its identity")
    cache.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.{uuid.uuid4().hex}.tmp")
    try:
        with urllib.request.urlopen(url, timeout=120) as response:
            final = urlparse(response.geturl())
            if final.scheme != "https" or not (
                final.hostname in ALLOWED_HOSTS
                or (final.hostname or "").endswith(".hf.co")
            ):
                raise ValueError(f"external {name} redirected to a disallowed host")
            data = response.read(expected_bytes + 1)
        if len(data) != expected_bytes or sha256_bytes(data) != expected_hash:
            raise ValueError(f"external {name} bytes or digest changed")
        temporary.write_bytes(data)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)
    return destination


def validate_license(identifier: object, url: object, label: str) -> None:
    if (
        not isinstance(identifier, str)
        or identifier not in SUPPORTED_LICENSES
        or SUPPORTED_LICENSES[identifier] != url
    ):
        raise ValueError(f"{label} has an unsupported license")


def validate_fleurs(path: Path) -> list[dict[str, object]]:
    manifest = load_json(path / "fixtures.json", "FLEURS fixtures")
    source = manifest.get("source")
    utterances = manifest.get("utterances")
    if not isinstance(source, dict) or not isinstance(utterances, list):
        raise ValueError("FLEURS fixtures lack source or utterances")
    validate_license(source.get("license"), source.get("licenseUrl"), "FLEURS")
    if len(utterances) != 20:
        raise ValueError("FLEURS input must contain exactly twenty fixtures")
    languages = Counter()
    output = []
    for raw in utterances:
        if not isinstance(raw, dict) or raw.get("class") != "clean-read":
            raise ValueError("every FLEURS fixture must be clean-read")
        identifier = raw.get("id")
        filename = raw.get("file")
        language = raw.get("language")
        if not all(isinstance(value, str) and value for value in (identifier, filename, language)):
            raise ValueError("FLEURS fixture identity is incomplete")
        audio = path / str(filename)
        if not audio.is_file() or sha256(audio) != raw.get("sha256"):
            raise ValueError(f"FLEURS audio identity mismatch: {identifier}")
        if not isinstance(raw.get("provenance"), dict) or not isinstance(
            raw.get("derivation"), dict
        ):
            raise ValueError(f"FLEURS provenance is missing: {identifier}")
        canonical_wav(audio)
        languages[str(language)] += 1
        output.append({**raw, "_audio": audio})
    if languages != Counter({"en": 4, "nl": 4, "de": 4, "fr": 4, "es": 4}):
        raise ValueError(f"unexpected FLEURS language distribution: {languages}")
    return output


def validate_captures(path: Path, recipe: dict[str, object]) -> list[dict[str, object]]:
    manifest = load_json(path / "capture.json", "capture manifest")
    consent = manifest.get("consent")
    contributor = manifest.get("contributor")
    utterances = manifest.get("utterances")
    if not isinstance(consent, dict) or consent.get("statement") != RELEASE_TEXT:
        raise ValueError("capture consent statement is missing or changed")
    validate_license(consent.get("license"), SUPPORTED_LICENSES.get(str(consent.get("license"))), "capture")
    if not isinstance(contributor, dict) or not isinstance(contributor.get("id"), str):
        raise ValueError("capture contributor ID is missing")
    expected = recipe.get("captures")
    if not isinstance(expected, list) or not isinstance(utterances, list):
        raise ValueError("capture recipe or manifest is incomplete")
    expected_by_id = {str(item["id"]): item for item in expected if isinstance(item, dict)}
    actual_by_id = {str(item["id"]): item for item in utterances if isinstance(item, dict)}
    if set(expected_by_id) != set(actual_by_id) or len(actual_by_id) != 4:
        raise ValueError("capture IDs do not match the recipe")
    output = []
    for identifier, expected_item in expected_by_id.items():
        item = actual_by_id[identifier]
        for field in ("language", "class", "reference"):
            if item.get(field) != expected_item.get(field):
                raise ValueError(f"capture {identifier} changed {field}")
        if item.get("naturalClassEvidence") is not True or item.get("synthetic") is not False:
            raise ValueError(f"capture {identifier} is not natural class evidence")
        audio = path / str(item.get("file"))
        if (
            not audio.is_file()
            or audio.stat().st_size != item.get("bytes")
            or sha256(audio) != item.get("sha256")
        ):
            raise ValueError(f"capture audio identity mismatch: {identifier}")
        value = canonical_wav(audio)
        if sha256_bytes(value) != item.get("sha256"):
            raise ValueError(f"capture {identifier} is not canonical 16 kHz mono PCM16")
        output.append({**item, "_audio": audio, "_contributor": contributor, "_consent": consent})
    return output


def decode_keyboard(path: Path) -> tuple[list[int], str]:
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        raise RuntimeError("ffmpeg is required to decode the pinned keyboard OGG")
    version = subprocess.run(
        [ffmpeg, "-version"], check=True, capture_output=True, text=True
    ).stdout.splitlines()[0]
    completed = subprocess.run(
        [ffmpeg, "-v", "error", "-i", str(path), "-f", "s16le", "-ac", "1", "-ar", "16000", "-"],
        check=True,
        capture_output=True,
    )
    values = array.array("h")
    values.frombytes(completed.stdout)
    if sys.byteorder != "little":
        values.byteswap()
    if not values:
        raise ValueError("decoded keyboard audio is empty")
    return list(values), version


def mix_noise(speech: list[int], noise: list[int], snr_db: float) -> tuple[list[int], float]:
    speech_rms = math.sqrt(sum(sample * sample for sample in speech) / len(speech))
    noise_rms = math.sqrt(sum(sample * sample for sample in noise) / len(noise))
    if speech_rms == 0 or noise_rms == 0:
        raise ValueError("speech and noise must both be non-silent")
    gain = speech_rms / (noise_rms * (10 ** (snr_db / 20)))
    mixed = [
        round(speech[index] + noise[index % len(noise)] * gain)
        for index in range(len(speech))
    ]
    peak = max(abs(sample) for sample in mixed)
    if peak > 32700:
        attenuation = 32700 / peak
        mixed = [round(sample * attenuation) for sample in mixed]
    actual_noise_rms = noise_rms * gain
    actual_snr = 20 * math.log10(speech_rms / actual_noise_rms)
    return mixed, actual_snr


def provenance(
    source_url: str,
    source_hash: str,
    source_bytes: int,
    attribution: str,
    license_id: str,
    license_url: str,
    **extra: object,
) -> dict[str, object]:
    return {
        "sourceUrl": source_url,
        "sourceSha256": source_hash,
        "sourceBytes": source_bytes,
        "attribution": attribution,
        "license": {"id": license_id, "url": license_url},
        **extra,
    }


def fixture(
    identifier: str,
    filename: str,
    language: str,
    product_class: str,
    reference: str,
    value: bytes,
    source: dict[str, object],
    derivation: dict[str, object],
    natural: bool | None = None,
    synthetic: bool | None = None,
    capture: object = None,
) -> dict[str, object]:
    result = {
        "id": identifier,
        "file": filename,
        "language": language,
        "class": product_class,
        "reference": reference,
        "bytes": len(value),
        "sha256": sha256_bytes(value),
        "provenance": source,
        "derivation": {**derivation, "outputSha256": sha256_bytes(value)},
    }
    if natural is not None:
        result["naturalClassEvidence"] = natural
    if synthetic is not None:
        result["synthetic"] = synthetic
    if capture is not None:
        result["capture"] = capture
    return result


def copy_fixture(item: dict[str, object], output: Path) -> dict[str, object]:
    source = item.pop("_audio")
    assert isinstance(source, Path)
    rate, channels, _samples = read_wav(source)
    if (rate, channels) != (16_000, 1):
        raise ValueError(f"fixture is not 16 kHz mono PCM16: {item['id']}")
    value = source.read_bytes()
    filename = str(item["file"])
    write_atomic(output / filename, value)
    if len(value) != item.get("bytes") or sha256_bytes(value) != item.get("sha256"):
        raise ValueError(f"verbatim fixture changed during canonical validation: {item['id']}")
    return item


def build(args: argparse.Namespace) -> None:
    recipe = load_json(args.recipe, "product corpus recipe")
    if recipe.get("schemaVersion") != 1:
        raise ValueError("product corpus recipe schemaVersion must be 1")
    output = args.output_dir.resolve()
    prepare_output(output)
    try:
        fleurs = validate_fleurs(args.fleurs_dir.resolve())
        captures = validate_captures(args.capture_dir.resolve(), recipe)
        externals = recipe.get("externals")
        if not isinstance(externals, dict):
            raise ValueError("product corpus recipe has no externals")
        source_cache = args.source_cache.resolve()
        ami_item = externals["ami"]
        cough_item = externals["cough"]
        keyboard_item = externals["keyboard"]
        if not all(isinstance(item, dict) for item in (ami_item, cough_item, keyboard_item)):
            raise ValueError("external fixture records must be objects")
        ami_path = obtain_external("ami", ami_item, source_cache)
        cough_path = obtain_external("cough", cough_item, source_cache)
        keyboard_path = obtain_external("keyboard", keyboard_item, source_cache)

        fixtures = [copy_fixture(dict(item), output) for item in fleurs]
        capture_by_class: dict[str, dict[str, object]] = {}
        for item in captures:
            local = dict(item)
            audio = local.pop("_audio")
            contributor = local.pop("_contributor")
            consent = local.pop("_consent")
            assert isinstance(audio, Path) and isinstance(contributor, dict) and isinstance(consent, dict)
            value = canonical_wav(audio)
            filename = f"{local['id']}.wav"
            write_atomic(output / filename, value)
            source_hash = str(local["sha256"])
            record = fixture(
                str(local["id"]),
                filename,
                str(local["language"]),
                str(local["class"]),
                str(local["reference"]),
                value,
                provenance(
                    f"project://echo/product-capture/{local['id']}",
                    source_hash,
                    int(local["bytes"]),
                    str(contributor["id"]),
                    str(consent["license"]),
                    SUPPORTED_LICENSES[str(consent["license"])],
                    consent=consent,
                ),
                {"kind": "verbatim-copy", "sourceSha256": source_hash},
                True,
                False,
                local.get("capture"),
            )
            fixtures.append(record)
            capture_by_class[str(local["class"])] = {**record, "_value": value}

        ami_value = canonical_wav(ami_path)
        ami_filename = "product-false-starts-en.wav"
        write_atomic(output / ami_filename, ami_value)
        fixtures.append(
            fixture(
                str(ami_item["id"]),
                ami_filename,
                "en",
                "false-starts",
                str(ami_item["reference"]),
                ami_value,
                provenance(
                    str(ami_item["url"]),
                    str(ami_item["sha256"]),
                    int(ami_item["bytes"]),
                    str(ami_item["attribution"]),
                    str(ami_item["license"]),
                    str(ami_item["licenseUrl"]),
                    modifications=ami_item.get("modifications"),
                ),
                {"kind": "verbatim-copy", "sourceSha256": ami_item["sha256"]},
                True,
                False,
            )
        )

        cough_value = canonical_wav(cough_path)
        cough_filename = "product-nonspeech-cough-en.wav"
        write_atomic(output / cough_filename, cough_value)
        fixtures.append(
            fixture(
                str(cough_item["id"]),
                cough_filename,
                "en",
                "nonspeech",
                "",
                cough_value,
                provenance(
                    str(cough_item["url"]),
                    str(cough_item["sha256"]),
                    int(cough_item["bytes"]),
                    str(cough_item["attribution"]),
                    str(cough_item["license"]),
                    str(cough_item["licenseUrl"]),
                    revision=cough_item.get("revision"),
                ),
                {
                    "kind": "stereo-44100-to-mono-16000-linear-v1",
                    "sourceSha256": cough_item["sha256"],
                },
                True,
                False,
            )
        )

        dictation = capture_by_class["dictation"]
        dictation_value = dictation["_value"]
        assert isinstance(dictation_value, bytes)
        with tempfile.NamedTemporaryFile(suffix=".wav") as temporary:
            temporary.write(dictation_value)
            temporary.flush()
            _rate, _channels, speech = read_wav(Path(temporary.name))
        keyboard, ffmpeg_version = decode_keyboard(keyboard_path)
        snr_db = float(recipe["derivations"]["noiseSnrDb"])
        mixed, actual_snr = mix_noise(speech, keyboard, snr_db)
        noise_value = wav_bytes(mixed)
        noise_filename = "product-noise-keyboard-en.wav"
        write_atomic(output / noise_filename, noise_value)
        fixtures.append(
            fixture(
                "product-noise-keyboard-en",
                noise_filename,
                "en",
                "noise",
                str(dictation["reference"]),
                noise_value,
                dict(dictation["provenance"]),
                {
                    "kind": "additive-keyboard-noise-v1",
                    "sourceSha256": dictation["sha256"],
                    "snrDb": snr_db,
                    "measuredSnrDb": round(actual_snr, 6),
                    "secondarySource": {
                        "url": keyboard_item["url"],
                        "sha256": keyboard_item["sha256"],
                        "bytes": keyboard_item["bytes"],
                        "license": keyboard_item["license"],
                        "licenseUrl": keyboard_item["licenseUrl"],
                        "attribution": keyboard_item["attribution"],
                    },
                    "decoder": ffmpeg_version,
                },
                False,
                True,
            )
        )

        silence_frames = int(recipe["derivations"]["silenceDurationMs"]) * 16
        silence_value = wav_bytes([0] * silence_frames)
        silence_filename = "product-silence-en.wav"
        write_atomic(output / silence_filename, silence_value)
        silence_hash = sha256_bytes(silence_value)
        fixtures.append(
            fixture(
                "product-silence-en",
                silence_filename,
                "en",
                "silence",
                "",
                silence_value,
                provenance(
                    "project://echo/generated/digital-silence-v1",
                    silence_hash,
                    len(silence_value),
                    "Echo project",
                    "CC0-1.0",
                    SUPPORTED_LICENSES["CC0-1.0"],
                ),
                {"kind": "generated-digital-silence-v1", "sourceSha256": silence_hash},
                False,
                True,
            )
        )

        if len(fixtures) != 28 or len({str(item["id"]) for item in fixtures}) != 28:
            raise ValueError("product corpus must contain exactly 28 unique fixtures")
        by_id = {str(item["id"]): item for item in fixtures}
        first_language_ids = {
            language: next(
                str(item["id"])
                for item in fixtures
                if item["language"] == language and item["class"] == "clean-read"
            )
            for language in ("nl", "de", "fr", "es")
        }
        class_ids = {
            str(item["class"]): str(item["id"])
            for item in fixtures
            if item["class"] in set(recipe["requiredClasses"])
        }
        bindings = [
            {
                "id": class_ids[product_class],
                "language": by_id[class_ids[product_class]]["language"],
                "class": product_class,
            }
            for product_class in recipe["requiredClasses"]
        ] + [
            {
                "id": identifier,
                "language": language,
                "class": "clean-read",
            }
            for language, identifier in first_language_ids.items()
        ]
        source = recipe["source"]
        assert isinstance(source, dict)
        manifest = {
            "schemaVersion": 1,
            "source": source,
            "coverage": {
                "included": [
                    "twenty multilingual FLEURS clean-read fixtures",
                    "four consented natural product recordings",
                    "natural AMI false starts",
                    "natural cough nonspeech",
                    "controlled keyboard noise",
                    "digital silence",
                ],
                "requiredLanguages": recipe["requiredLanguages"],
                "requiredClasses": recipe["requiredClasses"],
                "requiredBindings": bindings,
                "pending": [],
                "claimBoundary": "One natural fixture per product class plus multilingual clean speech qualifies CPU/Vulkan parity for this exact host identity, not general model quality.",
            },
            "utterances": fixtures,
        }
        write_json(output / "fixtures.json", manifest)
        write_json(
            output / "status.json",
            {"schemaVersion": 1, "state": "complete", "utterances": len(fixtures)},
        )
    except Exception as error:
        (output / "fixtures.json").unlink(missing_ok=True)
        write_json(
            output / "status.json",
            {
                "schemaVersion": 1,
                "state": "failed",
                "errorType": type(error).__name__,
                "error": str(error),
            },
        )
        raise


def make_test_wav(path: Path, rate: int, channels: int, amplitude: int) -> None:
    frames = rate
    samples = []
    for index in range(frames):
        value = amplitude if index % 2 == 0 else -amplitude
        samples.extend([value] * channels)
    values = array.array("h", samples)
    if sys.byteorder != "little":
        values.byteswap()
    with wave.open(str(path), "wb") as output:
        output.setnchannels(channels)
        output.setsampwidth(2)
        output.setframerate(rate)
        output.writeframes(values.tobytes())


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-product-corpus-") as temporary:
        root = Path(temporary)
        recipe = load_json(
            REPO_ROOT / "benchmarks/stt/product-corpus-recipe.json",
            "product corpus recipe",
        )
        stereo = root / "stereo.wav"
        make_test_wav(stereo, 44_100, 2, 1000)
        first = canonical_wav(stereo)
        second = canonical_wav(stereo)
        assert first == second
        stats = wav_stats(first)
        assert stats["durationMs"] == 1000 and stats["rms"] > 0
        speech = [1000, -1000] * 8000
        noise = [500, -500] * 4000
        mixed, snr = mix_noise(speech, noise, 10)
        assert len(mixed) == len(speech) and abs(snr - 10) < 1e-9
        silence = wav_bytes([0] * 16_000)
        assert wav_stats(silence)["peak"] == 0
        nonempty = root / "nonempty"
        nonempty.mkdir()
        (nonempty / "owned").write_text("keep", encoding="utf-8")
        try:
            prepare_output(nonempty)
        except ValueError:
            pass
        else:
            raise AssertionError("nonempty output was accepted")
        cache = root / "cache"
        cache.mkdir()
        item = {
            "url": "https://upload.wikimedia.org/test.wav",
            "bytes": stereo.stat().st_size,
            "sha256": sha256(stereo),
        }
        cached = cache / "test.wav"
        shutil.copyfile(stereo, cached)
        assert obtain_external("test", item, cache) == cached
        cached.write_bytes(b"tampered")
        try:
            obtain_external("test", item, cache)
        except ValueError as error:
            assert "does not match" in str(error)
        else:
            raise AssertionError("tampered external cache was accepted")
        capture_dir = root / "captures"
        capture_dir.mkdir()
        capture_items = []
        for prompt in recipe["captures"]:
            assert isinstance(prompt, dict)
            path = capture_dir / f"{prompt['id']}.wav"
            value = wav_bytes([1000, -1000] * 16_000)
            path.write_bytes(value)
            capture_items.append(
                {
                    **prompt,
                    "file": path.name,
                    "bytes": len(value),
                    "sha256": sha256_bytes(value),
                    "naturalClassEvidence": True,
                    "synthetic": False,
                    "capture": {"stats": wav_stats(value)},
                }
            )
        capture_manifest = {
            "schemaVersion": 1,
            "contributor": {"id": "test-contributor"},
            "consent": {
                "statement": RELEASE_TEXT,
                "acceptedAt": "test",
                "license": "CC0-1.0",
            },
            "utterances": capture_items,
        }
        write_json(capture_dir / "capture.json", capture_manifest)
        assert len(validate_captures(capture_dir, recipe)) == 4
        capture_manifest["consent"]["statement"] = "changed"
        write_json(capture_dir / "capture.json", capture_manifest)
        try:
            validate_captures(capture_dir, recipe)
        except ValueError as error:
            assert "consent statement" in str(error)
        else:
            raise AssertionError("changed consent was accepted")
        capture_manifest["consent"]["statement"] = RELEASE_TEXT
        capture_manifest["utterances"][0]["class"] = "wrong"
        write_json(capture_dir / "capture.json", capture_manifest)
        try:
            validate_captures(capture_dir, recipe)
        except ValueError as error:
            assert "changed class" in str(error)
        else:
            raise AssertionError("changed capture class was accepted")
    print("build-stt-product-corpus: self-test ok")


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser()
    value.add_argument("--self-test", action="store_true")
    value.add_argument(
        "--recipe",
        type=Path,
        default=REPO_ROOT / "benchmarks/stt/product-corpus-recipe.json",
    )
    value.add_argument("--fleurs-dir", type=Path)
    value.add_argument("--capture-dir", type=Path)
    value.add_argument("--source-cache", type=Path)
    value.add_argument("--output-dir", type=Path)
    return value


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    if any(
        value is None
        for value in (args.fleurs_dir, args.capture_dir, args.source_cache, args.output_dir)
    ):
        parser().error(
            "--fleurs-dir, --capture-dir, --source-cache, and --output-dir are required"
        )
    build(args)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"build-stt-product-corpus: {error}", file=sys.stderr)
        raise SystemExit(2) from error
