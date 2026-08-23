#!/usr/bin/env python3
import argparse
import fnmatch
import hashlib
import json
import tarfile
from pathlib import PurePosixPath


ARCHIVES = {
    "whisper-runtime": (
        "whisper.tar.gz",
        [
            "whisper-bin-ubuntu-x64/whisper-cli",
            "whisper-bin-ubuntu-x64/libwhisper*",
            "whisper-bin-ubuntu-x64/libggml*",
        ],
    ),
    "sherpa-runtime": (
        "sherpa.tar.bz2",
        ["sherpa-onnx-v1.13.6-linux-x64-static-no-tts/bin/sherpa-onnx-offline"],
    ),
    "parakeet-model": (
        "parakeet.tar.bz2",
        [
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/encoder.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/decoder.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/joiner.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/tokens.txt",
        ],
    ),
}


def selected(name, patterns):
    return any(fnmatch.fnmatchcase(name, pattern) for pattern in patterns)


def digest_regular(archive, member):
    source = archive.extractfile(member)
    if source is None:
        raise ValueError(f"cannot read {member.name}")
    digest = hashlib.sha256()
    for chunk in iter(lambda: source.read(1024 * 1024), b""):
        digest.update(chunk)
    return digest.hexdigest()


def resolve_symlink(member, by_name):
    seen = set()
    current = member
    while current.issym():
        if current.name in seen:
            raise ValueError(f"symlink cycle at {member.name}")
        seen.add(current.name)
        target = str(PurePosixPath(current.name).parent / current.linkname)
        current = by_name.get(target)
        if current is None:
            raise ValueError(f"missing symlink target for {member.name}")
    if not current.isfile():
        raise ValueError(f"unsafe symlink target for {member.name}")
    return current


def inventory(root):
    result = {"schemaVersion": 1, "components": {}}
    for component, (filename, patterns) in ARCHIVES.items():
        archive_path = root / filename
        with tarfile.open(archive_path, "r:*") as archive:
            members = archive.getmembers()
            by_name = {member.name: member for member in members}
            payload = []
            for member in members:
                if not selected(member.name, patterns):
                    continue
                if member.isfile():
                    sha256 = digest_regular(archive, member)
                    kind = "file"
                    link_target = None
                elif member.issym():
                    target_member = resolve_symlink(member, by_name)
                    sha256 = digest_regular(archive, target_member)
                    kind = "symlink"
                    link_target = member.linkname
                else:
                    raise ValueError(f"selected special member {member.name}")
                payload.append(
                    {
                        "path": member.name,
                        "kind": kind,
                        "size": member.size,
                        "mode": member.mode & 0o777,
                        "sha256": sha256,
                        "linkTarget": link_target,
                    }
                )
            result["components"][component] = {
                "archive": filename,
                "entries": len(members),
                "expandedBytes": sum(member.size for member in members),
                "payload": payload,
            }
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("probe_dir", type=PurePosixPath)
    args = parser.parse_args()
    print(json.dumps(inventory(args.probe_dir), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
