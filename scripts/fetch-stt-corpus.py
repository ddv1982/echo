#!/usr/bin/env python3
"""Fetch and verify a pinned speech benchmark corpus manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import tempfile
import threading
import urllib.request
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse


ID_PATTERN = re.compile(r"[a-z0-9][a-z0-9_-]*")
MAX_FILE_BYTES = 100 * 1024 * 1024
MAX_CORPUS_BYTES = 1024 * 1024 * 1024


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def write_atomic(path: Path, value: bytes) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        temporary.write_bytes(value)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def load_manifest(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schemaVersion") != 1:
        raise ValueError("corpus manifest schemaVersion must be 1")
    source = value.get("source")
    coverage = value.get("coverage")
    utterances = value.get("utterances")
    if (
        not isinstance(source, dict)
        or not isinstance(coverage, dict)
        or not isinstance(utterances, list)
        or not utterances
    ):
        raise ValueError(
            "corpus manifest needs source, coverage, and non-empty utterances"
        )
    revision = source.get("revision")
    allowed_hosts = source.get("allowedHosts")
    redirect_host_suffixes = source.get("redirectAllowedHostSuffixes", [])
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("source revision must be a full hexadecimal commit")
    if (
        not isinstance(allowed_hosts, list)
        or not allowed_hosts
        or not all(isinstance(host, str) and host for host in allowed_hosts)
    ):
        raise ValueError("source allowedHosts must be a non-empty string array")
    if not isinstance(redirect_host_suffixes, list) or not all(
        isinstance(suffix, str) and suffix.startswith(".") and len(suffix) > 1
        for suffix in redirect_host_suffixes
    ):
        raise ValueError("source redirectAllowedHostSuffixes must contain DNS suffixes")
    for field in ("attribution", "license", "licenseUrl", "homepage", "repository"):
        if not isinstance(source.get(field), str) or not source[field]:
            raise ValueError(f"source {field} must be a non-empty string")
    repository = urlparse(str(source["repository"]))
    repository_local_http = repository.scheme == "http" and repository.hostname in {
        "127.0.0.1",
        "localhost",
    }
    if (
        not (repository.scheme == "https" or repository_local_http)
        or not repository.hostname
    ):
        raise ValueError("source repository must use HTTPS")
    repository_path = repository.path.rstrip("/")
    seen = set()
    total_bytes = 0
    for item in utterances:
        if not isinstance(item, dict):
            raise ValueError("each corpus utterance must be an object")
        identifier = item.get("id")
        language = item.get("language")
        fixture_class = item.get("class")
        reference = item.get("reference")
        url = item.get("sourceUrl")
        digest = item.get("sha256")
        size = item.get("bytes")
        if not isinstance(identifier, str) or not ID_PATTERN.fullmatch(identifier):
            raise ValueError(f"invalid utterance id: {identifier}")
        if identifier in seen:
            raise ValueError(f"duplicate utterance id: {identifier}")
        if not all(
            isinstance(field, str) and field
            for field in [language, fixture_class, reference, url]
        ):
            raise ValueError(
                f"{identifier} needs language, class, reference, and sourceUrl"
            )
        parsed = urlparse(url)
        allowed_scheme = parsed.scheme == "https" or (
            parsed.scheme == "http" and parsed.hostname in {"127.0.0.1", "localhost"}
        )
        if not allowed_scheme:
            raise ValueError(f"{identifier} sourceUrl must use HTTPS")
        if parsed.hostname not in allowed_hosts:
            raise ValueError(f"{identifier} source host is not allowed")
        expected_prefix = f"{repository_path}/resolve/{revision}/"
        if parsed.hostname != repository.hostname or not parsed.path.startswith(
            expected_prefix
        ):
            raise ValueError(
                f"{identifier} sourceUrl must resolve the declared repository revision"
            )
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError(f"{identifier} needs a SHA-256")
        if not isinstance(size, int) or not 44 <= size <= MAX_FILE_BYTES:
            raise ValueError(f"{identifier} needs a plausible WAV byte size")
        total_bytes += size
        if total_bytes > MAX_CORPUS_BYTES:
            raise ValueError("corpus manifest exceeds the download byte limit")
        seen.add(identifier)
    return value


def verify_wav(path: Path) -> None:
    with wave.open(str(path), "rb") as audio:
        if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth()) != (
            16000,
            1,
            2,
        ):
            raise ValueError(f"audio must be 16 kHz mono PCM16: {path}")
        if audio.getnframes() == 0:
            raise ValueError(f"audio has no frames: {path}")


def host_allowed(hostname: str | None, exact: list[str], suffixes: list[str]) -> bool:
    return bool(hostname) and (
        hostname in exact or any(hostname.endswith(suffix) for suffix in suffixes)
    )


def fetch_one(
    item: dict[str, object],
    output_dir: Path,
    allowed_hosts: list[str],
    redirect_host_suffixes: list[str],
) -> Path:
    identifier = str(item["id"])
    destination = output_dir / f"{identifier}.wav"
    expected_size = int(item["bytes"])
    expected_hash = str(item["sha256"])
    if destination.is_file():
        existing = destination.read_bytes()
        if len(existing) == expected_size and sha256_bytes(existing) == expected_hash:
            verify_wav(destination)
            return destination
    temporary_name = ""
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb",
            prefix=f".{identifier}.",
            suffix=".tmp",
            dir=output_dir,
            delete=False,
        ) as temporary:
            temporary_name = temporary.name
            digest = hashlib.sha256()
            downloaded = 0
            with urllib.request.urlopen(
                str(item["sourceUrl"]), timeout=120
            ) as response:
                final = urlparse(response.geturl())
                local_http = final.scheme == "http" and final.hostname in {
                    "127.0.0.1",
                    "localhost",
                }
                if not (final.scheme == "https" or local_http) or not host_allowed(
                    final.hostname, allowed_hosts, redirect_host_suffixes
                ):
                    raise ValueError(
                        f"{identifier} redirected to a disallowed source host"
                    )
                while chunk := response.read(
                    min(1024 * 1024, expected_size + 1 - downloaded)
                ):
                    downloaded += len(chunk)
                    if downloaded > expected_size:
                        raise ValueError(f"{identifier} byte size mismatch")
                    digest.update(chunk)
                    temporary.write(chunk)
            if downloaded != expected_size:
                raise ValueError(f"{identifier} byte size mismatch")
            if digest.hexdigest() != expected_hash:
                raise ValueError(f"{identifier} SHA-256 mismatch")
        temporary_path = Path(temporary_name)
        verify_wav(temporary_path)
        os.replace(temporary_path, destination)
    finally:
        if temporary_name:
            Path(temporary_name).unlink(missing_ok=True)
    return destination


def run_fetch(manifest_path: Path, output_dir: Path) -> None:
    manifest = load_manifest(manifest_path)
    output_dir.mkdir(parents=True, exist_ok=True)
    for name in ("fixtures.json", "NOTICE.md"):
        (output_dir / name).unlink(missing_ok=True)
    write_atomic(output_dir / "status.json", b'{"schemaVersion":1,"state":"running"}\n')
    try:
        fixtures = []
        source = manifest["source"]
        assert isinstance(source, dict)
        allowed_hosts = [str(host) for host in source["allowedHosts"]]
        redirect_host_suffixes = [
            str(suffix) for suffix in source.get("redirectAllowedHostSuffixes", [])
        ]
        for item in manifest["utterances"]:
            assert isinstance(item, dict)
            audio = fetch_one(item, output_dir, allowed_hosts, redirect_host_suffixes)
            fixtures.append(
                {
                    "id": item["id"],
                    "file": audio.name,
                    "language": item["language"],
                    "class": item["class"],
                    "reference": item["reference"],
                    "bytes": item["bytes"],
                    "sha256": item["sha256"],
                    "provenance": {
                        "sourceUrl": item["sourceUrl"],
                        "sourceSha256": item["sha256"],
                        "sourceBytes": item["bytes"],
                        "repository": source["repository"],
                        "revision": source["revision"],
                        "attribution": source["attribution"],
                        "license": {
                            "id": source["license"],
                            "url": source["licenseUrl"],
                        },
                    },
                    "derivation": {
                        "kind": "verbatim-copy",
                        "sourceSha256": item["sha256"],
                        "outputSha256": item["sha256"],
                    },
                }
            )
        benchmark = {
            "schemaVersion": 1,
            "source": source,
            "coverage": manifest["coverage"],
            "utterances": fixtures,
        }
        write_atomic(
            output_dir / "fixtures.json",
            (json.dumps(benchmark, indent=2, ensure_ascii=False) + "\n").encode(),
        )
        notice = "\n".join(
            [
                "# Corpus notice",
                "",
                str(source["attribution"]),
                "",
                f"License: {source['license']} ({source['licenseUrl']})",
                f"Source: {source['homepage']}",
                f"Pinned redistribution: {source['repository']} at {source['revision']}",
                "",
            ]
        )
        write_atomic(output_dir / "NOTICE.md", notice.encode())
        write_atomic(
            output_dir / "status.json",
            json.dumps(
                {"schemaVersion": 1, "state": "complete", "utterances": len(fixtures)},
                sort_keys=True,
            ).encode()
            + b"\n",
        )
    except Exception as error:
        write_atomic(
            output_dir / "status.json",
            json.dumps(
                {
                    "schemaVersion": 1,
                    "state": "failed",
                    "errorType": type(error).__name__,
                    "error": str(error),
                },
                sort_keys=True,
            ).encode()
            + b"\n",
        )
        raise


class FixtureHandler(BaseHTTPRequestHandler):
    payload = b""

    def do_GET(self) -> None:
        self.send_response(200)
        self.send_header("Content-Length", str(len(self.payload)))
        self.end_headers()
        self.wfile.write(self.payload)

    def log_message(self, _format: str, *_args: object) -> None:
        return


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-corpus-test-") as temporary:
        root = Path(temporary)
        wav = root / "sample.wav"
        with wave.open(str(wav), "wb") as audio:
            audio.setnchannels(1)
            audio.setsampwidth(2)
            audio.setframerate(16000)
            audio.writeframes(b"\0\0" * 160)
        FixtureHandler.payload = wav.read_bytes()
        server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            revision = "a" * 40
            manifest = {
                "schemaVersion": 1,
                "source": {
                    "revision": revision,
                    "allowedHosts": ["127.0.0.1"],
                    "redirectAllowedHostSuffixes": [],
                    "attribution": "Test corpus",
                    "license": "CC0",
                    "licenseUrl": "https://example.com/license",
                    "homepage": "https://example.com",
                    "repository": f"http://127.0.0.1:{server.server_port}/repo",
                },
                "coverage": {
                    "included": ["test"],
                    "requiredLanguages": ["en"],
                    "requiredClasses": ["dictation"],
                    "pending": ["dictation"],
                },
                "utterances": [
                    {
                        "id": "test_en",
                        "language": "en",
                        "class": "dictation",
                        "reference": "test",
                        "sourceUrl": (
                            f"http://127.0.0.1:{server.server_port}/repo/resolve/"
                            f"{revision}/sample.wav"
                        ),
                        "bytes": len(FixtureHandler.payload),
                        "sha256": sha256_bytes(FixtureHandler.payload),
                    }
                ],
            }
            manifest_path = root / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            load_manifest(manifest_path)
            manifest["utterances"][0]["sourceUrl"] = (
                f"http://127.0.0.1:{server.server_port}/repo/resolve/main/"
                f"sample-{revision}.wav"
            )
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            try:
                load_manifest(manifest_path)
            except ValueError as error:
                assert "declared repository revision" in str(error)
            else:
                raise AssertionError("unresolved revision unexpectedly passed")
            manifest["utterances"][0]["sourceUrl"] = (
                f"http://127.0.0.1:{server.server_port}/repo/resolve/"
                f"{revision}/sample.wav"
            )
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            output = root / "output"
            run_fetch(manifest_path, output)
            assert (
                json.loads((output / "status.json").read_text())["state"] == "complete"
            )
            fetched = json.loads((output / "fixtures.json").read_text())
            fixture = fetched["utterances"][0]
            assert fixture["id"] == "test_en" and fixture["class"] == "dictation"
            assert fetched["coverage"] == manifest["coverage"]
            assert fixture["provenance"]["license"]["id"] == "CC0"
            assert (
                fixture["derivation"]["outputSha256"]
                == fixture["provenance"]["sourceSha256"]
            )
            (output / "test_en.wav").write_bytes(b"corrupt")
            run_fetch(manifest_path, output)
            assert (
                sha256_bytes((output / "test_en.wav").read_bytes())
                == manifest["utterances"][0]["sha256"]
            )
            manifest["utterances"][0]["sha256"] = "0" * 64
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            try:
                run_fetch(manifest_path, output)
            except ValueError:
                pass
            else:
                raise AssertionError("bad hash unexpectedly passed")
            assert json.loads((output / "status.json").read_text())["state"] == "failed"
            assert not (output / "fixtures.json").exists()
        finally:
            server.shutdown()
            thread.join()
            server.server_close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--check-manifest", action="store_true")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("fetch-stt-corpus: self-test ok")
        return 0
    if args.check_manifest:
        if args.manifest is None:
            parser.error("--manifest is required with --check-manifest")
        try:
            manifest = load_manifest(args.manifest)
        except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
            print(f"fetch-stt-corpus: {error}", file=sys.stderr)
            return 1
        print(
            f"fetch-stt-corpus: manifest ok ({len(manifest['utterances'])} utterances)"
        )
        return 0
    if args.manifest is None or args.output_dir is None:
        parser.error("--manifest and --output-dir are required")
    try:
        run_fetch(args.manifest, args.output_dir)
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"fetch-stt-corpus: {error}", file=sys.stderr)
        return 1
    print(f"fetch-stt-corpus: wrote {args.output_dir / 'fixtures.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
