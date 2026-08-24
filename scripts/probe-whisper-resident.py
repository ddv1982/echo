#!/usr/bin/env python3
"""Measure first and warm requests against a pinned whisper-server."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import secrets
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_identity(path: Path) -> dict[str, object]:
    return {"path": str(path.resolve()), "sha256": sha256(path), "bytes": path.stat().st_size}


def host_metadata() -> dict[str, object]:
    uname = platform.uname()
    return {
        "system": uname.system,
        "release": uname.release,
        "machine": uname.machine,
        "processor": uname.processor,
        "cpuCount": os.cpu_count(),
    }


def reserve_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def process_listens_on(pid: int, port: int) -> bool | None:
    descriptors = Path(f"/proc/{pid}/fd")
    sockets = Path(f"/proc/{pid}/net/tcp")
    if not descriptors.is_dir() or not sockets.is_file():
        return None
    inodes = set()
    for descriptor in descriptors.iterdir():
        try:
            target = os.readlink(descriptor)
        except OSError:
            continue
        if target.startswith("socket:[") and target.endswith("]"):
            inodes.add(target[8:-1])
    expected_port = f"{port:04X}"
    for line in sockets.read_text(encoding="utf-8").splitlines()[1:]:
        fields = line.split()
        if len(fields) > 9 and fields[1].endswith(f":{expected_port}"):
            if fields[3] == "0A" and fields[9] in inodes:
                return True
    return False


def wait_ready(process: subprocess.Popen[bytes], port: int, log: Path) -> float:
    started = time.perf_counter_ns()
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        if process.poll() is not None:
            detail = log.read_text(encoding="utf-8", errors="replace").strip()
            raise RuntimeError(f"whisper-server exited before ready: {detail}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                owned = process_listens_on(process.pid, port)
                if owned is False:
                    time.sleep(0.05)
                    continue
                return (time.perf_counter_ns() - started) / 1_000_000
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("whisper-server did not become ready within 180 seconds")


def multipart(audio: Path, boundary: str) -> bytes:
    audio_bytes = audio.read_bytes()
    parts = [
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n".encode(),
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"temperature\"\r\n\r\n0.0\r\n".encode(),
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"temperature_inc\"\r\n\r\n0.2\r\n".encode(),
        (
            f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; "
            "filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        ).encode(),
        audio_bytes,
        f"\r\n--{boundary}--\r\n".encode(),
    ]
    return b"".join(parts)


def post_audio(url: str, audio: Path) -> tuple[str, float]:
    boundary = f"echo-{secrets.token_hex(16)}"
    request = urllib.request.Request(
        url,
        data=multipart(audio, boundary),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        method="POST",
    )
    started = time.perf_counter_ns()
    with urllib.request.urlopen(request, timeout=600) as response:
        raw = response.read()
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    payload = json.loads(raw)
    text = payload.get("text")
    if not isinstance(text, str):
        raise ValueError("whisper-server response has no text field")
    return text.strip(), elapsed_ms


def resident_rss_bytes(pid: int) -> int | None:
    status = Path(f"/proc/{pid}/status")
    if not status.is_file():
        return None
    for line in status.read_text(encoding="utf-8").splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1]) * 1024
    return None


def server_command(args: argparse.Namespace, port: int, endpoint: str, public: Path) -> list[str]:
    command = [
        str(args.server),
        "-m",
        str(args.model),
        "-t",
        str(args.threads),
        "-bs",
        str(args.beam_size),
        "-bo",
        str(args.best_of),
        "-l",
        args.language,
        "-nt",
        "--host",
        "127.0.0.1",
        "--port",
        str(port),
        "--public",
        str(public),
        "--inference-path",
        endpoint,
    ]
    if args.no_fallback:
        command.append("-nf")
    if args.prompt:
        command.extend(["--prompt", args.prompt])
    if args.vad is not None:
        command.extend(["--vad", "-vm", str(args.vad)])
    return command


def run_probe(args: argparse.Namespace) -> None:
    for label, path in [("server", args.server), ("model", args.model), ("audio", args.audio)]:
        if not path.is_file():
            raise ValueError(f"{label} is missing: {path}")
    if args.vad is not None and not args.vad.is_file():
        raise ValueError(f"VAD model is missing: {args.vad}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    endpoint = f"/{secrets.token_hex(24)}"
    port = reserve_port()
    with tempfile.TemporaryDirectory(prefix="echo-whisper-resident-") as temporary:
        root = Path(temporary)
        public = root / "public"
        public.mkdir()
        log = root / "server.log"
        with log.open("wb") as output:
            process = subprocess.Popen(
                server_command(args, port, endpoint, public),
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=subprocess.STDOUT,
            )
            try:
                load_ms = wait_ready(process, port, log)
                rows = []
                for index in range(args.repeats + 1):
                    text, request_ms = post_audio(
                        f"http://127.0.0.1:{port}{endpoint}", args.audio
                    )
                    rows.append(
                        {
                            "schemaVersion": 1,
                            "kind": "residentFirst" if index == 0 else "residentWarm",
                            "repeat": index,
                            "loadMs": round(load_ms, 3),
                            "requestMs": round(request_ms, 3),
                            "rssBytes": resident_rss_bytes(process.pid),
                            "text": text,
                            "language": args.language,
                            "threads": args.threads,
                            "beamSize": args.beam_size,
                            "bestOf": args.best_of,
                            "noFallback": args.no_fallback,
                            "vad": args.vad is not None,
                            "serverPid": process.pid,
                        }
                    )
            finally:
                stop_process(process)
        artifacts = {
            "server": artifact_identity(args.server),
            "model": artifact_identity(args.model),
            "vad": artifact_identity(args.vad) if args.vad is not None else None,
            "audio": artifact_identity(args.audio),
        }
        prompt = {
            "length": len(args.prompt),
            "sha256": hashlib.sha256(args.prompt.encode("utf-8")).hexdigest(),
        }
        host = host_metadata()
        for row in rows:
            row["artifacts"] = artifacts
            row["prompt"] = prompt
            row["host"] = host
        (args.output_dir / "resident-runs.jsonl").write_text(
            "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
            encoding="utf-8",
        )
        warm = [row["requestMs"] for row in rows if row["kind"] == "residentWarm"]
        summary = {
            "loadMs": rows[0]["loadMs"],
            "firstRequestMs": rows[0]["requestMs"],
            "warmRequestMs": warm,
            "serverLog": log.read_text(encoding="utf-8", errors="replace"),
        }
        (args.output_dir / "resident-summary.json").write_text(
            json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )


class FakeHandler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:
        length = int(self.headers["Content-Length"])
        self.rfile.read(length)
        body = json.dumps({"text": "resident works"}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *_args: object) -> None:
        return


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-resident-self-test-") as temporary:
        audio = Path(temporary) / "sample.wav"
        audio.write_bytes(b"RIFFfake")
        server = ThreadingHTTPServer(("127.0.0.1", 0), FakeHandler)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            port = int(server.server_address[1])
            text, elapsed = post_audio(f"http://127.0.0.1:{port}/secret", audio)
            assert text == "resident works"
            assert elapsed >= 0
        finally:
            server.shutdown()
            thread.join()
            server.server_close()
        fake = Path(temporary) / "fake-whisper-server"
        fake.write_text(
            """#!/usr/bin/env python3
import argparse
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

parser = argparse.ArgumentParser(add_help=False)
parser.add_argument('-m', dest='model', type=Path, required=True)
parser.add_argument('--port', type=int, required=True)
parser.add_argument('--inference-path', required=True)
args, _ = parser.parse_known_args()
args.model.with_suffix('.pid').write_text(str(os.getpid()))
if args.model.name == 'exit-ready.bin':
    sys.exit(7)

class Handler(BaseHTTPRequestHandler):
    requests = 0
    def do_POST(self):
        Handler.requests += 1
        self.rfile.read(int(self.headers['Content-Length']))
        if args.model.name == 'fail-request.bin' and Handler.requests == 2:
            self.send_error(500)
            return
        body = json.dumps({'text': 'resident works'}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, _format, *_args):
        return

ThreadingHTTPServer(('127.0.0.1', args.port), Handler).serve_forever()
""",
            encoding="utf-8",
        )
        fake.chmod(0o755)

        def probe_args(model: Path, output: Path) -> argparse.Namespace:
            model.write_bytes(b"model")
            return argparse.Namespace(
                server=fake,
                model=model,
                audio=audio,
                vad=None,
                language="auto",
                prompt="",
                threads=2,
                beam_size=1,
                best_of=1,
                no_fallback=True,
                repeats=1,
                output_dir=output,
            )

        def assert_reaped(model: Path) -> None:
            pid = int(model.with_suffix(".pid").read_text(encoding="utf-8"))
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
            raise AssertionError(f"fake server {pid} is still running")

        success_model = Path(temporary) / "success.bin"
        success_output = Path(temporary) / "success-output"
        run_probe(probe_args(success_model, success_output))
        rows = [
            json.loads(line)
            for line in (success_output / "resident-runs.jsonl")
            .read_text(encoding="utf-8")
            .splitlines()
        ]
        assert [row["kind"] for row in rows] == ["residentFirst", "residentWarm"]
        assert rows[0]["artifacts"]["model"]["path"] == str(success_model.resolve())
        assert len(rows[0]["artifacts"]["audio"]["sha256"]) == 64
        assert rows[0]["prompt"]["length"] == 0
        assert rows[0]["host"]["system"]
        assert_reaped(success_model)

        exit_model = Path(temporary) / "exit-ready.bin"
        exit_output = Path(temporary) / "exit-output"
        try:
            run_probe(probe_args(exit_model, exit_output))
        except RuntimeError as error:
            assert "exited before ready" in str(error)
        else:
            raise AssertionError("exit-before-ready probe unexpectedly succeeded")
        assert_reaped(exit_model)
        assert not (exit_output / "resident-runs.jsonl").exists()

        failure_model = Path(temporary) / "fail-request.bin"
        failure_output = Path(temporary) / "failure-output"
        try:
            run_probe(probe_args(failure_model, failure_output))
        except urllib.error.HTTPError as error:
            assert error.code == 500
        else:
            raise AssertionError("failed-request probe unexpectedly succeeded")
        assert_reaped(failure_model)
        assert not (failure_output / "resident-runs.jsonl").exists()


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--server", type=Path, required=True)
    value.add_argument("--model", type=Path, required=True)
    value.add_argument("--audio", type=Path, required=True)
    value.add_argument("--vad", type=Path)
    value.add_argument("--language", default="auto")
    value.add_argument("--prompt", default="")
    value.add_argument("--threads", type=int, default=min(os.cpu_count() or 4, 4))
    value.add_argument("--beam-size", type=int, default=5)
    value.add_argument("--best-of", type=int, default=5)
    value.add_argument("--no-fallback", action="store_true")
    value.add_argument("--repeats", type=int, default=10)
    value.add_argument("--output-dir", type=Path, required=True)
    return value


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        print("probe-whisper-resident: self-test ok")
        return 0
    args = parser().parse_args()
    if min(args.threads, args.beam_size, args.best_of, args.repeats) < 1:
        parser().error("threads, beam size, best of, and repeats must be at least 1")
    try:
        run_probe(args)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"probe-whisper-resident: {error}", file=sys.stderr)
        return 1
    print(f"probe-whisper-resident: wrote {args.output_dir / 'resident-runs.jsonl'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
