from __future__ import annotations

import hashlib
from pathlib import Path

BUNDLE_MARKER = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
BUNDLE_TOKENS = {
    "deb": b"__TAURI_BUNDLE_TYPE_VAR_DEB",
    "rpm": b"__TAURI_BUNDLE_TYPE_VAR_RPM",
    "appimage": b"__TAURI_BUNDLE_TYPE_VAR_APP",
}


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_sha256(root: Path) -> str:
    files = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError("cache seed must not contain symlinks")
        if path.is_file():
            files.append((path.relative_to(root).as_posix(), path))
        elif not path.is_dir():
            raise ValueError(f"unsupported cache entry: {path}")
    if not files:
        raise ValueError("cache seed must contain files")
    digest = hashlib.sha256(b"echo-whisper-tree-v1\0")
    for relative, path in sorted(files):
        name = relative.encode()
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(path.stat().st_size.to_bytes(8, "little"))
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def runtime_libraries(cli: Path) -> list[Path]:
    by_content: dict[tuple[int, str], Path] = {}
    for candidate in cli.parent.iterdir():
        if ".so" not in candidate.name:
            continue
        try:
            path = candidate.resolve(strict=True)
        except OSError:
            continue
        if not path.is_file():
            continue
        key = (path.stat().st_size, sha256_file(path))
        selected = by_content.get(key)
        if selected is None or (len(path.name), path.name) > (
            len(selected.name),
            selected.name,
        ):
            by_content[key] = path
    return sorted(by_content.values())


def runtime_identity(cli: Path) -> str:
    digest = hashlib.sha256(b"echo-whisper-runtime-v1\0")
    for path in [cli.resolve(), *runtime_libraries(cli)]:
        name = path.name.encode()
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(path.stat().st_size.to_bytes(8, "little"))
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def bundle_variant(canonical: bytes, bundle_type: str) -> bytes:
    if canonical.count(BUNDLE_MARKER) != 1:
        raise ValueError(
            "canonical binary must contain one unknown Tauri bundle marker"
        )
    return canonical.replace(BUNDLE_MARKER, BUNDLE_TOKENS[bundle_type], 1)


def verify_contained_symlinks(root: Path) -> None:
    resolved_root = root.resolve()
    for path in root.rglob("*"):
        if path.is_symlink() and not path.resolve(strict=True).is_relative_to(
            resolved_root
        ):
            raise ValueError(f"symlink escapes package root: {path}")
