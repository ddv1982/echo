#!/usr/bin/env python3
"""Verify settings projections and filesystem collection counts in isolated processes.

Managed status checks count repair-marker reads, attempted once per status call.
Whisper preparation rereads its launch identity after validating selected leases.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TEST = "settings::tests::settings_collection_probe"


def run(command, **kwargs):
    result = subprocess.run(command, text=True, capture_output=True, cwd=ROOT, **kwargs)
    if result.returncode:
        raise RuntimeError(f"Command failed: {command}\n{result.stdout}\n{result.stderr}")
    return result


def source_identity():
    paths = run([
        "git", "ls-files", "--cached", "--others", "--exclude-standard", "--",
        "crates", "src-tauri", ".cargo", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
        "scripts/verify-settings-collections.py",
    ]).stdout.splitlines()
    digest = hashlib.sha256()
    for name in sorted(set(paths)):
        digest.update(name.encode() + b"\0")
        digest.update((ROOT / name).read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def test_binary():
    built = run([
        "cargo", "test", "-p", "echo-desktop", "--bin", "echo-desktop",
        "--no-run", "--message-format=json",
    ])
    for line in built.stdout.splitlines():
        artifact = json.loads(line)
        if artifact.get("executable") and artifact.get("target", {}).get("name") == "echo-desktop":
            return Path(artifact["executable"])
    raise RuntimeError("Cargo did not report the desktop test executable")


def fixture(root, models, runtimes, config):
    for name in ["models", "bin", "config/echo", "data", "cache", "home", "runtime"]:
        (root / name).mkdir(parents=True, exist_ok=True)
    (root / "runtime").chmod(0o700)
    for model in models:
        path = root / "models" / model
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"")
    for runtime in runtimes:
        path = root / "bin" / runtime
        path.write_text("#!/bin/sh\nexit 0\n")
        path.chmod(0o755)
    (root / "config/echo/config.json").write_text(json.dumps(config))


def probe(binary, root, overrides):
    env = {key: value for key, value in os.environ.items() if not key.startswith("ECHO_")}
    env.update({
        "ECHO_MODEL_DIR": str(root / "models"),
        "HOME": str(root / "home"),
        "XDG_CONFIG_HOME": str(root / "config"),
        "XDG_CACHE_HOME": str(root / "cache"),
        "XDG_DATA_HOME": str(root / "data"),
        "XDG_RUNTIME_DIR": str(root / "runtime"),
        "ECHO_WHISPER_ACCELERATION": "cpu",
        "GDK_BACKEND": "x11",
        "XDG_SESSION_TYPE": "x11",
        **overrides,
    })
    trace = root / "openat.log"
    result = run([
        "xvfb-run", "-a", "strace", "-f", "-s", "4096", "-e", "trace=openat", "-o", str(trace),
        "/usr/bin/env", f"PATH={root / 'bin'}", str(binary), TEST,
        "--exact", "--ignored", "--nocapture",
    ], env=env)
    lines = trace.read_text().splitlines()
    counts = {
        "model_directory": sum(f'"{root / "models"}"' in line and "O_DIRECTORY" in line for line in lines),
        "managed_status_checks": sum(f'"{root / "models/managed/repair"}/' in line for line in lines),
        "whisper_identity_open": sum(f'"{root / "bin/whisper-cli"}"' in line for line in lines),
    }
    assert counts["model_directory"] == 1, counts
    assert counts["managed_status_checks"] == 8, counts
    output = result.stdout + result.stderr
    snapshots = [json.loads(line.split("SETTINGS_SNAPSHOT ", 1)[1]) for line in output.splitlines() if "SETTINGS_SNAPSHOT " in line]
    assert len(snapshots) == 1, output
    return snapshots[0], counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("/tmp/echo-settings-collections.json"))
    args = parser.parse_args()
    for command in ["cargo", "xvfb-run", "strace"]:
        if not shutil.which(command):
            raise RuntimeError(f"Required command is missing: {command}")
    source_sha256 = source_identity()
    source_revision = run(["git", "rev-parse", "HEAD"]).stdout.strip()
    binary = test_binary()
    parakeet = [f"parakeet-tdt-0.6b-v3/{name}" for name in ["tokens.txt", "encoder.onnx", "decoder.onnx", "joiner.onnx"]]
    cases = [
        ("unavailable", [], [], {}, {}, "unavailable", "multilingual", "en"),
        ("english", ["ggml-base.en.bin"], ["whisper-cli"], {}, {}, "ready", "english", "en"),
        ("english-rejects-german", ["ggml-base.en.bin"], ["whisper-cli"], {}, {"ECHO_LANGUAGE": "de"}, "unavailable", "english", "de"),
        ("multilingual", ["ggml-small.bin"], ["whisper-cli"], {}, {}, "ready", "multilingual", "auto"),
        ("parakeet", parakeet, ["sherpa-onnx-offline"], {}, {}, "ready", "parakeet", "auto"),
        ("missing-pinned", [], ["whisper-cli"], {"whisper_model": "missing.en"}, {}, "unavailable", "multilingual", "auto"),
        ("custom-override", ["personal.bin"], ["whisper-cli"], {"whisper_model": "missing"}, {"ECHO_WHISPER_MODEL": "personal"}, "ready", "multilingual", "auto"),
        ("language-override", ["ggml-small.bin"], ["whisper-cli"], {"language": "en"}, {"ECHO_LANGUAGE": "de"}, "ready", "multilingual", "de"),
        ("engine-override", ["ggml-small.bin"], ["whisper-cli"], {"engine": "whisper"}, {"ECHO_ENGINE": "parakeet"}, "unavailable", "parakeet", "auto"),
        ("fake", [], [], {}, {"ECHO_ENGINE": "fake"}, "ready", "multilingual", "en"),
    ]
    report = {
        "source_revision": source_revision,
        "source_sha256": source_sha256,
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "binary": str(binary),
        "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(),
        "cases": [],
    }
    with tempfile.TemporaryDirectory(prefix="echo-settings-collections-") as scratch:
        for name, models, runtimes, config, overrides, kind, mode, language in cases:
            root = Path(scratch) / name
            fixture(root, models, runtimes, config)
            snapshot, counts = probe(binary, root, overrides)
            transcription = snapshot["transcription"]
            expected_identities = 0
            if "whisper-cli" in runtimes:
                expected_identities = 2 if kind == "ready" else 1
            assert counts["whisper_identity_open"] == expected_identities, (name, counts)
            assert transcription["nextRun"]["kind"] == kind, (name, transcription["nextRun"])
            assert transcription["languages"]["mode"] == mode, (name, transcription["languages"])
            assert snapshot["preferences"]["language"]["effective"] == language, (name, snapshot["preferences"]["language"])
            assert snapshot["readiness"]["speechReady"] == (kind == "ready"), name
            if name == "custom-override":
                assert transcription["nextRun"]["engine"]["model"] == "personal"
                assert transcription["models"]["whisper"] == []
                assert snapshot["preferences"]["whisperModel"]["source"] == "env"
            if name == "english":
                assert [option["code"] for option in transcription["languages"]["options"]] == ["en"]
            if name == "language-override":
                assert transcription["nextRun"]["language"] == "de"
            report["cases"].append({"name": name, "collections": counts, "next_run": transcription["nextRun"], "language_mode": mode})
            print(f"{name}: {counts}")
    assert source_identity() == source_sha256, "Source changed while settings checks were running"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Settings collection and projection checks passed. Report: {args.output}")


if __name__ == "__main__":
    main()
