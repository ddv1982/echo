#!/usr/bin/env python3
"""Run real Tauri/WebKit recording and settings contracts in an isolated desktop."""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import wave


ROOT = Path(__file__).resolve().parents[1]


def command_output(args: list[str]) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def source_paths() -> list[Path]:
    listed = subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT
    )
    return [Path(path.decode()) for path in listed.split(b"\0") if path]


def hash_source_file(digest, relative: Path) -> None:
    path = ROOT / relative
    digest.update(str(relative).encode())
    digest.update(b"\0")
    if path.is_symlink():
        digest.update(b"symlink\0")
        digest.update(os.readlink(path).encode())
    elif path.is_file():
        digest.update(b"file\0")
        with path.open("rb") as source:
            for block in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(block)
    else:
        digest.update(b"missing\0")


def selected_file_hashes() -> dict[str, str]:
    files = (
        Path("frontend/src/verification/nativeRecordingProbe.ts"),
        Path("frontend/src/api/tauriDesktopApi.ts"),
        Path("frontend/src/perf/statusPerf.ts"),
    )
    return {str(path): sha256(ROOT / path) for path in files}


def source_identity() -> dict[str, object]:
    digest = hashlib.sha256()
    for path in source_paths():
        hash_source_file(digest, path)
    status = command_output(["git", "status", "--porcelain=v1"])
    return {
        "commit": command_output(["git", "rev-parse", "HEAD"]),
        "dirty": bool(status),
        "sourceFingerprint": digest.hexdigest(),
        "probeSourceSha256": selected_file_hashes(),
    }


def redact_status_perf(payload: dict[str, object]) -> dict[str, object]:
    report = payload.get("report")
    if isinstance(report, dict):
        report.pop("userAgent", None)
    return payload


def valid_contract_report(payload: dict[str, object] | None, commit: str, version: str) -> bool:
    if payload is None or payload.get("commit") != commit:
        return False
    report = payload.get("report", {})
    if report.get("appVersion") != version:
        return False
    verification = report.get("verification", {})
    checks = verification.get("checks", [])
    if len(checks) != 10 or len({check.get("name") for check in checks}) != 10:
        return False
    if not all(check.get("passed") is True for check in checks):
        return False
    timings = verification.get("timingsMs", {})
    expected = {"startReceipt", "staleStopReceipt", "stopReceipt", "terminalObservation"}
    if timings.keys() != expected or not all(
        isinstance(value, (int, float)) and math.isfinite(value) and value >= 0
        for value in timings.values()
    ):
        return False
    revisions = verification.get("settingsRevisions", [])
    return len(revisions) == 4 and all(a < b for a, b in zip(revisions, revisions[1:]))


def redacted_stdout(stdout: str) -> tuple[str, dict[str, object] | None]:
    lines: list[str] = []
    status_perf: dict[str, object] | None = None
    for line in stdout.splitlines():
        if line.startswith("STATUS_PERF_JSON "):
            status_perf = redact_status_perf(json.loads(line.removeprefix("STATUS_PERF_JSON ")))
            lines.append(f"STATUS_PERF_JSON {json.dumps(status_perf, separators=(',', ':'))}")
        else:
            lines.append(line)
    return "\n".join(lines) + ("\n" if stdout.endswith("\n") else ""), status_perf


def write_probe(root: Path, app_version: str) -> Path:
    probe = root / "probe"
    probe.mkdir()
    entry = (ROOT / "frontend/src/verification/nativeRecordingProbe.ts").resolve()
    (probe / "index.html").write_text('<div id="root"></div><script type="module" src="/main.ts"></script>\n')
    (probe / "main.ts").write_text(f"import {json.dumps(str(entry))}\n")
    (probe / "vite.config.mjs").write_text(
        "export default {"
        f"root: {json.dumps(str(probe))},"
        f"define: {{ __APP_VERSION__: {json.dumps(json.dumps(app_version))} }},"
        f"build: {{ outDir: {json.dumps(str(probe / 'dist'))}, emptyOutDir: true }}"
        "}\n"
    )
    return probe


def fixture(path: Path) -> None:
    with wave.open(str(path), "wb") as audio:
        audio.setnchannels(1)
        audio.setsampwidth(2)
        audio.setframerate(16_000)
        audio.writeframes(struct.pack("<h", 8_000) * 48_000)


def run(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--release", action="store_true", help="build an optimized binary for comparable measurements")
    parser.add_argument("--output", type=Path, help="write the JSON record to this path")
    parser.add_argument("--log-dir", type=Path, help="write captured native stdout and stderr here")
    parser.add_argument(
        "--lock-file",
        type=Path,
        help="shared build-and-run lock (defaults to ECHO_COORDINATION_LOCK_FILE or the target directory)",
    )
    parser.add_argument("--timeout", type=int, default=120)
    args = parser.parse_args(argv)
    if args.timeout <= 0:
        parser.error("--timeout must be greater than zero")

    profile = "release" if args.release else "debug"
    target_base = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    target = target_base / "recording-native-probe"
    lock = args.lock_file or Path(
        os.environ.get("ECHO_COORDINATION_LOCK_FILE", target / ".verify-recording-native.lock")
    )
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = manifest["workspace"]["package"]["version"]
    identity = source_identity()
    lock.parent.mkdir(parents=True, exist_ok=True)
    with lock.open("w") as lock_file:
        fcntl.flock(lock_file, fcntl.LOCK_EX)
        with tempfile.TemporaryDirectory(prefix="echo-native-recording-") as temporary:
            root = Path(temporary)
            probe = write_probe(root, version)
            vite = ROOT / "frontend/node_modules/.bin/vite"
            subprocess.run([str(vite), "build", "--config", str(probe / "vite.config.mjs")], cwd=ROOT, check=True)
            build_env = {
                **os.environ,
                "CARGO_TARGET_DIR": str(target),
                "ECHO_BUILD_SHA": str(identity["commit"]),
                "TAURI_CONFIG": json.dumps({"build": {"frontendDist": str(probe / "dist")}}),
            }
            build = ["cargo", "build", "-p", "echo-desktop", "--features", "status-perf-probe"]
            if args.release:
                build.append("--release")
            subprocess.run(build, cwd=ROOT, env=build_env, check=True)
            binary = target / profile / "echo-desktop"
            audio = root / "fixture.wav"
            fixture(audio)
            runtime_root = root / "runtime"
            for name in ("data", "config", "models", "xdg-data", "xdg-config", "xdg-cache"):
                (runtime_root / name).mkdir(parents=True, exist_ok=True)
            environment = {
                **os.environ,
                "ECHO_DATA_DIR": str(runtime_root / "data"),
                "ECHO_CONFIG_DIR": str(runtime_root / "config"),
                "ECHO_MODEL_DIR": str(runtime_root / "models"),
                "ECHO_AUDIO_FIXTURE": str(audio),
                "ECHO_ENGINE": "fake",
                "ECHO_SKIP_INJECT": "1",
                "ECHO_HUD": "0",
                "XDG_DATA_HOME": str(runtime_root / "xdg-data"),
                "XDG_CONFIG_HOME": str(runtime_root / "xdg-config"),
                "XDG_CACHE_HOME": str(runtime_root / "xdg-cache"),
                "GDK_BACKEND": "x11",
                "XDG_SESSION_TYPE": "x11",
            }
            launched = ["dbus-run-session", "--", "xvfb-run", "-a", str(binary)]
            started_at = time.time()
            process = subprocess.Popen(launched, cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
            try:
                stdout, stderr = process.communicate(timeout=args.timeout)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    stdout, stderr = process.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    stdout, stderr = process.communicate()
                raise RuntimeError(f"native probe timed out after {args.timeout}s")
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
            safe_stdout, status_perf = redacted_stdout(stdout)
            if args.log_dir:
                args.log_dir.mkdir(parents=True, exist_ok=True)
                (args.log_dir / "native.stdout.log").write_text(safe_stdout)
                (args.log_dir / "native.stderr.log").write_text(stderr)
            source_at_end = source_identity()
            source_changed = source_at_end != identity
            contracts_passed = valid_contract_report(status_perf, str(identity["commit"]), version)
            result: dict[str, object] = {
                "schemaVersion": 1,
                "valid": not source_changed and contracts_passed and process.returncode == 0,
                "source": identity,
                "sourceAtEnd": source_at_end,
                "sourceChangedDuringRun": source_changed,
                "binary": {"path": str(binary), "sha256": sha256(binary), "profile": profile},
                "environment": {
                    "desktop": "dbus-run-session + Xvfb",
                    "audio": "synthetic fixture",
                    "engine": "fake",
                    "injection": "disabled",
                    "microphone": "not used",
                },
                "command": launched,
                "startedAtUnixMs": round(started_at * 1000),
                "durationMs": round((time.time() - started_at) * 1000),
                "exitCode": process.returncode,
                "statusPerf": status_perf,
            }
            output = args.output
            if output:
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text(json.dumps(result, indent=2) + "\n")
            if output:
                print(json.dumps({"valid": result["valid"], "report": str(output), "commit": identity["commit"]}))
            else:
                print(json.dumps(result, indent=2))
            if stderr:
                print(stderr, file=sys.stderr)
            if process.returncode != 0 or not contracts_passed:
                print(safe_stdout, file=sys.stderr)
                raise RuntimeError("native probe failed; see stdout/stderr above")
            if source_changed:
                raise RuntimeError("source changed during the native probe; record is invalid")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(run(sys.argv[1:]))
    except (subprocess.CalledProcessError, RuntimeError) as error:
        print(f"verify-recording-native: {error}", file=sys.stderr)
        raise SystemExit(1)
