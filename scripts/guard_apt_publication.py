#!/usr/bin/env python3
"""Verify signed APT identities and reserve a monotonic Pages publication.

Run only while holding the publish-apt concurrency lock, through deploy completion.
The state branch is a high-water mark, not a claim that a deployment succeeded:
failed deployments can retry identical package bytes but cannot enable a rollback.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

STATE_BRANCH = "apt-publication-state"
STATE_FILE = "publication.json"


def validate_version(version: str) -> None:
    if not isinstance(version, str) or not version or version.startswith("-"):
        raise ValueError("invalid Debian version")
    result = subprocess.run(["dpkg", "--validate-version", version], capture_output=True, text=True)
    if result.returncode != 0:
        raise ValueError(f"invalid Debian version {version!r}: {result.stderr.strip()}")


def compare_versions(left: str, right: str) -> int:
    validate_version(left)
    validate_version(right)
    for operator, result in (("lt", -1), ("eq", 0)):
        compared = subprocess.run(["dpkg", "--compare-versions", left, operator, right])
        if compared.returncode == 0:
            return result
        if compared.returncode != 1:
            raise RuntimeError("dpkg version comparison failed")
    return 1


def parse_stanzas(data: bytes) -> list[dict[str, str]]:
    stanzas = []
    fields: dict[str, str] = {}
    key = None
    for line in data.decode("utf-8").splitlines() + [""]:
        if not line:
            if fields:
                stanzas.append(fields)
            fields, key = {}, None
        elif line[0].isspace():
            if key is None:
                raise ValueError("orphan Debian control continuation")
            fields[key] += "\n" + line.strip()
        else:
            key, separator, value = line.partition(":")
            if not separator or not key or key in fields:
                raise ValueError("malformed or duplicate Debian control field")
            fields[key] = value.strip()
    return stanzas


def safe_path(value: str) -> str:
    path = pathlib.PurePosixPath(value)
    if path.is_absolute() or str(path) != value or ".." in path.parts or not path.parts:
        raise ValueError(f"unsafe repository path: {value!r}")
    return value


def validate_identity(identity: object) -> dict:
    if not isinstance(identity, dict) or set(identity) != {"version", "packages"}:
        raise ValueError("invalid publication identity")
    validate_version(identity["version"])
    packages = identity["packages"]
    if not isinstance(packages, dict) or not packages:
        raise ValueError("publication has no packages")
    for arch, package in packages.items():
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", arch):
            raise ValueError("invalid package architecture")
        if not isinstance(package, dict) or set(package) != {"sha256", "size", "filename"}:
            raise ValueError("invalid package identity")
        if not isinstance(package["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", package["sha256"]):
            raise ValueError("invalid package SHA256")
        if type(package["size"]) is not int or package["size"] <= 0:
            raise ValueError("invalid package size")
        if not safe_path(package["filename"]).startswith("pool/main/e/echo/echo_"):
            raise ValueError("unexpected package pool path")
    return identity


def require_monotonic(candidate: dict, previous: dict) -> None:
    validate_identity(candidate)
    validate_identity(previous)
    order = compare_versions(candidate["version"], previous["version"])
    if order < 0:
        raise ValueError(f"refusing APT downgrade {previous['version']} -> {candidate['version']}")
    if order == 0 and candidate != previous:
        raise ValueError("same Debian version has different package bytes or identity")


def fetch(url: str, *, missing_ok: bool = False) -> bytes | None:
    request = urllib.request.Request(url, headers={"Cache-Control": "no-cache", "Pragma": "no-cache"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        if missing_ok and error.code == 404:
            return None
        raise


def repository_identity(load, keyring: pathlib.Path, *, verify_packages: bool = False) -> dict:
    """Authenticate Release, Packages, and (for the candidate) actual .deb bytes."""
    with tempfile.TemporaryDirectory(prefix="echo-apt-identity-") as tmp:
        root = pathlib.Path(tmp)
        (root / "InRelease").write_bytes(load("dists/stable/InRelease"))
        subprocess.run(
            ["gpgv", "--homedir", str(root), "--keyring", str(keyring.resolve()),
             "--output", str(root / "Release"), str(root / "InRelease")],
            check=True, capture_output=True,
        )
        releases = parse_stanzas((root / "Release").read_bytes())
    if len(releases) != 1:
        raise ValueError("expected one signed Release stanza")
    release = releases[0]
    if release.get("Suite") != "stable" or release.get("Components") != "main":
        raise ValueError("unexpected APT suite or components")
    hashes = {}
    for row in release["SHA256"].splitlines():
        if not row:
            continue
        digest, size, path = row.split()
        if path in hashes or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError("invalid or duplicate Release checksum")
        hashes[safe_path(path)] = (digest, int(size))
    architectures = release["Architectures"].split()
    if not architectures or len(set(architectures)) != len(architectures):
        raise ValueError("invalid Release architectures")
    identity = {"version": None, "packages": {}}
    for arch in architectures:
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", arch):
            raise ValueError("invalid Release architecture")
        path = f"main/binary-{arch}/Packages"
        data = load(f"dists/stable/{path}")
        if (hashlib.sha256(data).hexdigest(), len(data)) != hashes[path]:
            raise ValueError("Packages does not match signed Release")
        packages = parse_stanzas(data)
        if len(packages) != 1 or packages[0].get("Package") != "echo":
            raise ValueError("expected exactly one Echo package per architecture")
        fields = packages[0]
        if fields["Architecture"] != arch:
            raise ValueError("package architecture disagrees with index")
        version = fields["Version"]
        if identity["version"] not in (None, version):
            raise ValueError("mixed package versions in publication")
        identity["version"] = version
        package = {"sha256": fields["SHA256"], "size": int(fields["Size"]),
                   "filename": safe_path(fields["Filename"])}
        identity["packages"][arch] = package
        if verify_packages:
            data = load(package["filename"])
            if (hashlib.sha256(data).hexdigest(), len(data)) != (package["sha256"], package["size"]):
                raise ValueError("candidate .deb does not match signed Packages")
    return validate_identity(identity)


class GitHubState:
    def __init__(self, repository: str, token: str):
        self.base = f"{os.environ.get('GITHUB_API_URL', 'https://api.github.com')}/repos/{repository}"
        self.token = token
        self.repository = repository
        self.commit = None

    def api(self, path: str, data=None, *, method=None, missing_ok=False):
        request = urllib.request.Request(
            self.base + path,
            data=None if data is None else json.dumps(data).encode(),
            method=method,
            headers={"Authorization": f"Bearer {self.token}", "Accept": "application/vnd.github+json",
                     "X-GitHub-Api-Version": "2022-11-28", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            if missing_ok and error.code == 404:
                return None
            raise

    def read(self) -> dict | None:
        # A ref 404 is meaningful only after verifying access to the repository.
        repository = self.api("")
        if repository["full_name"].lower() != self.repository.lower() or not repository["permissions"]["push"]:
            raise ValueError("cannot authenticate publication state repository write access")
        ref = self.api(f"/git/ref/heads/{STATE_BRANCH}", missing_ok=True)
        if ref is None:
            return None
        if ref["object"]["type"] != "commit":
            raise ValueError("publication state ref is not a commit")
        self.commit = ref["object"]["sha"]
        # Pin the content read to the observed commit, not a moving branch name.
        content = self.api(f"/contents/{STATE_FILE}?ref={self.commit}")
        if content["encoding"] != "base64":
            raise ValueError("invalid publication state encoding")
        return validate_identity(json.loads(base64.b64decode(content["content"])))

    def reserve(self, identity: dict) -> None:
        tree = self.api("/git/trees", {"tree": [{"path": STATE_FILE, "mode": "100644", "type": "blob",
                                               "content": json.dumps(validate_identity(identity), sort_keys=True) + "\n"}]})
        commit = self.api("/git/commits", {"message": f"Reserve APT {identity['version']}", "tree": tree["sha"],
                                          "parents": [self.commit] if self.commit else []})
        if self.commit:
            # No force: stale concurrent writers cannot overwrite an intervening reservation.
            self.api(f"/git/refs/heads/{STATE_BRANCH}", {"sha": commit["sha"], "force": False}, method="PATCH")
        else:
            self.api("/git/refs", {"ref": f"refs/heads/{STATE_BRANCH}", "sha": commit["sha"]})


def guard(candidate: dict, state: GitHubState, published, *, allow_first_publication: bool) -> None:
    previous = state.read()
    if previous is not None:
        require_monotonic(candidate, previous)
    current = published()
    if current is None:
        if not allow_first_publication or (previous is not None and previous != candidate):
            raise ValueError("published InRelease is missing; first publication or its identical retry requires explicit opt-in")
    else:
        require_monotonic(candidate, current)
    state.reserve(candidate)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=pathlib.Path, required=True, help="extracted Pages artifact apt directory")
    parser.add_argument("--repository-url", required=True)
    parser.add_argument("--keyring", type=pathlib.Path, required=True, help="trusted signing key, not downloaded from the site")
    parser.add_argument("--allow-first-publication", action="store_true")
    args = parser.parse_args()
    try:
        candidate = repository_identity(lambda path: (args.candidate / path).read_bytes(), args.keyring, verify_packages=True)
        base = args.repository_url.rstrip("/") + "/"
        if urllib.parse.urlsplit(base).scheme != "https":
            raise ValueError("published repository URL must use HTTPS")

        def published():
            inrelease = fetch(base + "dists/stable/InRelease", missing_ok=True)
            if inrelease is None:
                return None
            return repository_identity(
                lambda path: inrelease if path == "dists/stable/InRelease" else fetch(base + path), args.keyring,
            )

        state = GitHubState(os.environ["GITHUB_REPOSITORY"], os.environ["GH_TOKEN"])
        guard(candidate, state, published, allow_first_publication=args.allow_first_publication)
        print(f"Reserved verified APT publication {candidate['version']}; retain concurrency lock through deployment.")
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
