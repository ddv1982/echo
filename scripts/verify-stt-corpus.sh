#!/usr/bin/env bash
set -euo pipefail

python3 scripts/fetch-stt-corpus.py --self-test
python3 scripts/fetch-stt-corpus.py \
  --check-manifest \
  --manifest benchmarks/stt/corpus-fleurs.json
python3 scripts/analyze-stt-host-matrix.py --self-test
verify_root="$(mktemp -d -t echo-stt-replay-XXXXXX)"
trap 'rm -rf "$verify_root"' EXIT
python3 - "$PWD" "$verify_root" <<'PY'
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys

repo = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
sys.path.insert(0, str(repo / "scripts"))
from whisper_release_common import runtime_identity as launch_identity

analyzer = repo / "scripts" / "analyze-stt-host-matrix.py"
bundle = root / "bundle"
bundle.mkdir()
audio = bundle / "audio.bin"
runtime = bundle / "runtime.bin"
model = bundle / "model.bin"
vad = bundle / "vad.bin"
library = bundle / "libwhisper.so"
audio.write_bytes(b"audio fixture")
runtime.write_bytes(b"runtime receipt")
model.write_bytes(b"model receipt")
vad.write_bytes(b"vad receipt")
library.write_bytes(b"library receipt")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ref(relative, contents):
    path = bundle / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(contents)
    return {"path": relative, "sha256": digest(path), "bytes": len(contents)}


coverage = {
    "schemaVersion": 1,
    "source": {
        "name": "Replay fixture",
        "homepage": "https://example.com/corpus",
        "repository": "https://example.com/repository",
        "revision": "b" * 40,
        "license": "CC-BY-4.0",
        "licenseUrl": "https://creativecommons.org/licenses/by/4.0/",
        "attribution": "Replay fixture authors",
    },
    "coverage": {
        "pending": ["missing product classes"],
        "requiredLanguages": ["en"],
        "requiredClasses": [
            "dictation", "technical-identifiers", "fast-speech", "quiet-speech",
            "noise", "false-starts", "silence", "nonspeech",
        ],
    },
    "utterances": [
        {
            "id": "fixture",
            "language": "en",
            "class": "clean-read",
            "reference": "hello world",
            "sourceUrl": "https://example.com/repository/audio.bin",
            "bytes": audio.stat().st_size,
            "sha256": digest(audio),
        }
    ],
}
coverage_path = root / "coverage-manifest.json"
coverage_path.write_text(json.dumps(coverage, indent=2) + "\n", encoding="utf-8")
snapshot = {
    "schemaVersion": 1,
    "source": coverage["source"],
    "coverage": coverage["coverage"],
    "utterances": [
        {
            "id": "fixture", "file": "audio.bin", "language": "en",
            "class": "clean-read", "reference": "hello world",
            "provenance": {
                "sourceUrl": "https://example.com/repository/audio.bin",
                "sourceSha256": digest(audio),
                "sourceBytes": audio.stat().st_size,
                "repository": coverage["source"]["repository"],
                "revision": coverage["source"]["revision"],
                "attribution": coverage["source"]["attribution"],
                "license": {
                    "id": coverage["source"]["license"],
                    "url": coverage["source"]["licenseUrl"],
                },
            },
            "derivation": {
                "kind": "verbatim-copy", "sourceSha256": digest(audio),
                "outputSha256": digest(audio),
            },
        }
    ],
}
corpus_path = bundle / "corpus-manifest.json"
corpus_path.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8")

manifest = {
    "schemaVersion": 1,
    "runId": "replay-fixture",
    "startedAt": "2026-08-25T00:00:00+00:00",
    "binary": {"path": sys.executable, "sha256": digest(pathlib.Path(sys.executable))},
    "corpus": {
        "sourcePath": str(corpus_path),
        "snapshot": {"path": "corpus-manifest.json", "sha256": digest(corpus_path)},
        "utterances": [{
            "id": "fixture", "audio": str(audio), "audioSha256": digest(audio),
            "class": snapshot["utterances"][0]["class"],
            "provenance": snapshot["utterances"][0]["provenance"],
            "derivation": snapshot["utterances"][0]["derivation"],
        }],
    },
    "candidates": [
        {
            "label": label,
            "engine": "whisper",
            "model": "base",
            "threads": None,
            "beamSize": None,
            "bestOf": None,
            "noFallback": False,
            "forceCpu": False,
        }
        for label in ("cpu", "gpu")
    ],
    "seed": 7,
    "repeats": 1,
    "warmups": 1,
    "cacheState": "fresh",
    "resetCycle": "forged-reset",
    "echo": {"version": "echo 1", "commit": "a" * 40, "dirty": False},
    "host": {"system": "Linux", "driverIdentity": "forged", "icdIdentity": "forged"},
    "artifactIndex": [],
}


def observation(row_id, candidate, phase, backend):
    product = {
        "schemaVersion": 1,
        "text": "hello world",
        "raw": "HELLO WORLD",
        "audioMs": 1000,
        "inferMs": 50,
        "engine": {
            "id": "whisper",
            "model": "base",
            "binary": str(runtime),
            "modelPath": str(model),
            "vad": False,
            "vadPath": str(vad),
        },
        "whisper": {
            "runtime": {
                "backend": backend,
                "device": "Test GPU" if backend == "vulkan" else None,
                "identitySha256": launch_identity(runtime),
                "libraryPath": str(root),
                "vulkanDriverFiles": str(root / "driver.json"),
                "mesaShaderCacheDir": str(root / "cache"),
            },
            "tuning": {"threads": 1, "beamSize": 1},
        },
    }
    prefix = f"artifacts/{row_id}"
    stdout = json.dumps(product, sort_keys=True).encode() + b"\n"
    artifact = {
        "schemaVersion": 1,
        "rowId": row_id,
        "phase": phase,
        "candidate": candidate,
        "utterance": "fixture",
        "command": ref(
            f"{prefix}/command.json",
            json.dumps(
                [
                    sys.executable, "transcribe", str(audio), "--language", "en", "--format", "json",
                    "--engine", "whisper", "--model", "base",
                ]
            ).encode() + b"\n",
        ),
        "environment": ref(f"{prefix}/environment.json", b"{}\n"),
        "stdout": ref(f"{prefix}/stdout.txt", stdout),
        "stderr": ref(f"{prefix}/stderr.txt", b""),
        "result": ref(
            f"{prefix}/result.json",
            json.dumps(
                {
                    "schemaVersion": 1,
                    "returnCode": 0,
                    "productJson": product,
                    "parseError": None,
                    "invocationError": None,
                },
                sort_keys=True,
            ).encode() + b"\n",
        ),
        "timing": ref(
            f"{prefix}/timing.json",
            b'{"schemaVersion":1,"startedAt":"start","finishedAt":"finish","wallMs":100,"returnCode":0}\n',
        ),
        "runtimeArtifact": {"path": str(runtime), "sha256": digest(runtime)},
        "modelArtifact": {"path": str(model), "sha256": digest(model)},
        "vadArtifact": {"path": str(vad), "sha256": digest(vad)},
    }
    manifest["artifactIndex"].append(artifact)
    return {
        "schemaVersion": 2,
        "rowId": row_id,
        "observationArtifact": artifact,
        "candidate": candidate,
        "utterance": "fixture",
        "repeat": 1,
        "text": "forged transcript",
        "raw": "forged raw",
        "reference": "forged reference",
        "referenceWords": 999,
        "wordErrors": 999,
        "wer": 999,
        "hallucinatedSilence": True,
        "outerMs": 1,
        "audioMs": 1,
        "inferMs": 1,
        "engine": {"id": "forged"},
        "whisper": {"runtime": {"backend": "forged"}},
        "runtimeArtifact": {"path": str(runtime), "sha256": digest(runtime)},
        "modelArtifact": {"path": str(model), "sha256": digest(model)},
        "vadArtifact": {"path": str(vad), "sha256": digest(vad)},
    }


observation("warmup-cpu", "cpu", "warmup", "cpu")
observation("warmup-gpu", "gpu", "warmup", "vulkan")
rows = [
    observation("measurement-cpu", "cpu", "measurement", "cpu"),
    observation("measurement-gpu", "gpu", "measurement", "vulkan"),
]
(bundle / "runs.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
(bundle / "run-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
(bundle / "status.json").write_text(
    json.dumps(
        {
            "schemaVersion": 1,
            "runId": "replay-fixture",
            "state": "complete",
            "startedAt": "2026-08-25T00:00:00+00:00",
            "completedAt": "2026-08-25T00:01:00+00:00",
            "updatedAt": "2026-08-25T00:01:00+00:00",
        }
    )
    + "\n",
    encoding="utf-8",
)


def run(target, corpus=coverage_path):
    return subprocess.run(
        [
            sys.executable, str(analyzer), "--runs", str(target / "runs.jsonl"),
            "--corpus-manifest", str(corpus),
            "--cpu-candidate", "cpu", "--accelerated-candidate", "gpu", "--output-dir", str(target / "decision"),
        ],
        check=False,
        capture_output=True,
        text=True,
    )


completed = run(bundle)
assert completed.returncode == 0, completed.stderr
assert coverage_path.read_bytes() != corpus_path.read_bytes()
decision = json.loads((bundle / "decision" / "decision.json").read_text())
assert decision["languages"]["en"]["cpuWer"] == 0
assert decision["languages"]["en"]["acceleratedWer"] == 0
assert not decision["gates"]["coverageComplete"]
assert not decision["gates"]["identityMatch"]
assert not decision["gates"]["driverIcdIdentity"]
assert not decision["gates"]["freshAndPopulatedCacheEvidence"]
assert not decision["gates"]["resetEvidence"]

zero_warmups = root / "zero-warmups"
shutil.copytree(bundle, zero_warmups, ignore=shutil.ignore_patterns("decision"))
zero_manifest_path = zero_warmups / "run-manifest.json"
zero_manifest = json.loads(zero_manifest_path.read_text())
zero_manifest["warmups"] = 0
zero_manifest["artifactIndex"] = [
    artifact
    for artifact in zero_manifest["artifactIndex"]
    if artifact["phase"] == "measurement"
]
zero_manifest_path.write_text(json.dumps(zero_manifest), encoding="utf-8")
zero_completed = run(zero_warmups)
assert zero_completed.returncode == 0, zero_completed.stderr


def tampered(name, mutate):
    target = root / name
    shutil.copytree(bundle, target, ignore=shutil.ignore_patterns("decision"))
    mutate(target)
    rejected = run(target)
    assert rejected.returncode == 1, (name, rejected.stdout, rejected.stderr)


def tampered_external(name, mutate):
    changed_path = root / f"{name}.json"
    changed = json.loads(coverage_path.read_text())
    mutate(changed)
    changed_path.write_text(json.dumps(changed), encoding="utf-8")
    rejected = run(bundle, changed_path)
    assert rejected.returncode == 1, (name, rejected.stdout, rejected.stderr)


def sync_row_artifact(target, changed, row_id):
    artifact = next(item for item in changed["artifactIndex"] if item["rowId"] == row_id)
    rows = [json.loads(line) for line in (target / "runs.jsonl").read_text().splitlines()]
    next(row for row in rows if row["rowId"] == row_id)["observationArtifact"] = artifact
    (target / "runs.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))


tampered("running", lambda target: (target / "status.json").write_text((target / "status.json").read_text().replace('"complete"', '"running"')))
tampered("failed", lambda target: (target / "status.json").write_text((target / "status.json").read_text().replace('"complete"', '"failed"')))
tampered("stale", lambda target: (target / "status.json").write_text((target / "status.json").read_text().replace("replay-fixture", "old-run")))
tampered("duplicate", lambda target: (target / "runs.jsonl").write_text((target / "runs.jsonl").read_text() * 2))
tampered("missing", lambda target: (target / "runs.jsonl").write_text((target / "runs.jsonl").read_text().splitlines()[0] + "\n"))
tampered(
    "mutation",
    lambda target: (target / "artifacts/measurement-cpu/stdout.txt").write_bytes(
        (target / "artifacts/measurement-cpu/stdout.txt").read_bytes() + b"!"
    ),
)


def vad_digest_mismatch(target):
    rows = [json.loads(line) for line in (target / "runs.jsonl").read_text().splitlines()]
    rows[0]["vadArtifact"]["sha256"] = "0" * 64
    (target / "runs.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))


tampered("vad-digest", vad_digest_mismatch)


def warmup_runtime_digest_mismatch(target):
    manifest_path = target / "run-manifest.json"
    changed = json.loads(manifest_path.read_text())
    warmup = next(item for item in changed["artifactIndex"] if item["rowId"] == "warmup-cpu")
    warmup["runtimeArtifact"]["sha256"] = "0" * 64
    manifest_path.write_text(json.dumps(changed), encoding="utf-8")


tampered("warmup-runtime-digest", warmup_runtime_digest_mismatch)

def composite_runtime_mismatch(target):
    manifest_path = target / "run-manifest.json"
    changed = json.loads(manifest_path.read_text())
    row_id = "measurement-cpu"
    stdout_path = target / f"artifacts/{row_id}/stdout.txt"
    product = json.loads(stdout_path.read_text())
    product["whisper"]["runtime"]["identitySha256"] = "0" * 64
    stdout_path.write_text(json.dumps(product), encoding="utf-8")
    result_path = target / f"artifacts/{row_id}/result.json"
    result = json.loads(result_path.read_text())
    result["productJson"] = product
    result_path.write_text(json.dumps(result), encoding="utf-8")
    artifact = next(item for item in changed["artifactIndex"] if item["rowId"] == row_id)
    for name, path in (("stdout", stdout_path), ("result", result_path)):
        artifact[name]["sha256"] = digest(path)
        artifact[name]["bytes"] = path.stat().st_size
    sync_row_artifact(target, changed, row_id)
    manifest_path.write_text(json.dumps(changed), encoding="utf-8")


tampered("composite-runtime-digest", composite_runtime_mismatch)


def reference_mismatch(target):
    snapshot_path = target / "corpus-manifest.json"
    changed = json.loads(snapshot_path.read_text())
    changed["utterances"][0]["reference"] = "wrong reference"
    snapshot_path.write_text(json.dumps(changed), encoding="utf-8")
    manifest_path = target / "run-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    snapshot = manifest["corpus"]["snapshot"]
    snapshot["sha256"] = digest(snapshot_path)
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")


tampered("reference-mismatch", reference_mismatch)


tampered_external(
    "external-class-tamper",
    lambda changed: changed["utterances"][0].__setitem__("class", "dictation"),
)
tampered_external(
    "external-coverage-tamper",
    lambda changed: changed["coverage"].__setitem__("pending", []),
)
tampered_external(
    "external-license-tamper",
    lambda changed: changed["source"].__setitem__("license", "CC0-1.0"),
)
tampered_external(
    "external-source-tamper",
    lambda changed: changed["source"].__setitem__(
        "repository", "https://example.com/other"
    ),
)


def bundled_class_mismatch(target):
    snapshot_path = target / "corpus-manifest.json"
    changed = json.loads(snapshot_path.read_text())
    changed["utterances"][0]["class"] = "dictation"
    snapshot_path.write_text(json.dumps(changed), encoding="utf-8")
    manifest_path = target / "run-manifest.json"
    run_manifest = json.loads(manifest_path.read_text())
    run_manifest["corpus"]["snapshot"]["sha256"] = digest(snapshot_path)
    run_manifest["corpus"]["utterances"][0]["class"] = "dictation"
    manifest_path.write_text(json.dumps(run_manifest), encoding="utf-8")


tampered("bundled-class-mismatch", bundled_class_mismatch)


def audio_mismatch(target):
    manifest_path = target / "run-manifest.json"
    changed = json.loads(manifest_path.read_text())
    changed["corpus"]["utterances"][0]["audioSha256"] = "0" * 64
    manifest_path.write_text(json.dumps(changed), encoding="utf-8")


tampered("audio-mismatch", audio_mismatch)


def mismatch(target):
    manifest_path = target / "run-manifest.json"
    changed = json.loads(manifest_path.read_text())
    result = target / "artifacts/measurement-cpu/result.json"
    payload = json.loads(result.read_text())
    payload["productJson"]["text"] = "mismatch"
    result.write_text(json.dumps(payload), encoding="utf-8")
    reference = next(item for item in changed["artifactIndex"] if item["rowId"] == "measurement-cpu")["result"]
    reference["sha256"] = digest(result)
    reference["bytes"] = result.stat().st_size
    sync_row_artifact(target, changed, "measurement-cpu")
    manifest_path.write_text(json.dumps(changed), encoding="utf-8")


tampered("result-mismatch", mismatch)


def candidate_mismatch(target):
    manifest_path = target / "run-manifest.json"
    changed = json.loads(manifest_path.read_text())
    next(item for item in changed["artifactIndex"] if item["rowId"] == "measurement-gpu")["candidate"] = "cpu"
    sync_row_artifact(target, changed, "measurement-gpu")
    manifest_path.write_text(json.dumps(changed), encoding="utf-8")


tampered("candidate-mismatch", candidate_mismatch)


def path_escape(target):
    manifest_path = target / "run-manifest.json"
    changed = json.loads(manifest_path.read_text())
    next(item for item in changed["artifactIndex"] if item["rowId"] == "measurement-cpu")["command"]["path"] = "../escape"
    manifest_path.write_text(json.dumps(changed), encoding="utf-8")


tampered("path-escape", path_escape)
print("verify-stt-corpus replay: ok")
PY
python3 - <<'PY'
import json
from pathlib import Path

manifest = json.loads(Path("benchmarks/stt/corpus-fleurs.json").read_text())
assert manifest["schemaVersion"] == 1
assert manifest["source"]["license"] == "CC-BY-4.0"
assert len(manifest["utterances"]) == 20
languages = [item["language"] for item in manifest["utterances"]]
assert {language: languages.count(language) for language in set(languages)} == {
    "de": 4,
    "en": 4,
    "es": 4,
    "fr": 4,
    "nl": 4,
}
assert all(item["class"] == "clean-read" for item in manifest["utterances"])
print("verify-stt-corpus: ok")
PY
