#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib import error, request


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = ROOT / "scripts" / "manifests" / "release-cleanup-2026-08-30.json"
REVIEWED_REPOSITORY = "ddv1982/echo"
REVIEWED_MANIFEST_DIGEST = (
    "sha256:c294f5608023cd79d18e0dd3fb11248edd487aa606c120741c12f09ebbd79bbc"
)
REPOSITORY = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
QUALIFICATION_TAG = re.compile(r"qualification-([0-9a-f]{40})")
SHA256 = re.compile(r"sha256:[0-9a-f]{64}")


class CleanupError(ValueError):
    pass


def read_json(path):
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exception:
        raise CleanupError(f"cannot read JSON from {path}: {exception}") from exception


def read_manifest(path):
    document = read_json(path)
    if not isinstance(document, dict) or type(document.get("schema")) is not int:
        raise CleanupError("cleanup manifest must be a schema 1 object")
    if document["schema"] != 1 or set(document) != {
        "schema",
        "repository",
        "delete_draft_releases",
        "superseded_release_note",
    }:
        raise CleanupError("cleanup manifest must use the exact schema 1 fields")
    repository = document.get("repository")
    if not isinstance(repository, str) or REPOSITORY.fullmatch(repository) is None:
        raise CleanupError("cleanup manifest repository is invalid")
    drafts = document.get("delete_draft_releases")
    note = document.get("superseded_release_note")
    if not isinstance(drafts, list) or not isinstance(note, dict):
        raise CleanupError("cleanup manifest actions are invalid")
    draft_ids = set()
    draft_tags = set()
    for index, draft in enumerate(drafts):
        if not isinstance(draft, dict) or set(draft) != {
            "id",
            "tag_name",
            "target_commitish",
            "assets",
        }:
            raise CleanupError(f"delete_draft_releases[{index}] is invalid")
        release_id = draft["id"]
        tag = draft["tag_name"]
        target = draft["target_commitish"]
        assets = draft["assets"]
        match = QUALIFICATION_TAG.fullmatch(tag) if isinstance(tag, str) else None
        if (
            type(release_id) is not int
            or release_id <= 0
            or match is None
            or target != match.group(1)
            or not manifest_assets_are_valid(assets)
            or release_id in draft_ids
            or tag in draft_tags
        ):
            raise CleanupError(f"delete_draft_releases[{index}] is invalid")
        draft_ids.add(release_id)
        draft_tags.add(tag)
    if set(note) != {
        "id",
        "tag_name",
        "target_commitish",
        "prerelease",
        "assets",
        "expected_body_sha256",
        "applied_body_sha256",
        "notice",
    }:
        raise CleanupError("superseded_release_note is invalid")
    if (
        type(note["id"]) is not int
        or note["id"] <= 0
        or note["id"] in draft_ids
        or not isinstance(note["tag_name"], str)
        or not note["tag_name"].startswith("v")
        or not isinstance(note["target_commitish"], str)
        or not note["target_commitish"]
        or note["prerelease"] is not False
        or not manifest_assets_are_valid(note["assets"])
        or not isinstance(note["expected_body_sha256"], str)
        or SHA256.fullmatch(note["expected_body_sha256"]) is None
        or not isinstance(note["applied_body_sha256"], str)
        or SHA256.fullmatch(note["applied_body_sha256"]) is None
        or not isinstance(note["notice"], str)
        or not note["notice"].startswith("> [!WARNING]\n")
        or not note["notice"].endswith("\n\n")
    ):
        raise CleanupError("superseded_release_note is invalid")
    return document


def manifest_assets_are_valid(assets):
    if not isinstance(assets, list) or not assets:
        return False
    asset_ids = set()
    asset_names = set()
    for asset in assets:
        if (
            not isinstance(asset, dict)
            or set(asset) != {"id", "name", "size", "digest"}
            or type(asset["id"]) is not int
            or asset["id"] <= 0
            or not isinstance(asset["name"], str)
            or not asset["name"]
            or type(asset["size"]) is not int
            or asset["size"] <= 0
            or not isinstance(asset["digest"], str)
            or SHA256.fullmatch(asset["digest"]) is None
            or asset["id"] in asset_ids
            or asset["name"] in asset_names
        ):
            return False
        asset_ids.add(asset["id"])
        asset_names.add(asset["name"])
    return True


def manifest_digest(document):
    canonical = json.dumps(
        document, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    return "sha256:" + hashlib.sha256(canonical).hexdigest()


def validate_apply_manifest(path, document, digest):
    if path.resolve() != DEFAULT_MANIFEST.resolve():
        raise CleanupError("apply mode only accepts the checked-in cleanup manifest")
    if (
        document["repository"] != REVIEWED_REPOSITORY
        or len(document["delete_draft_releases"]) != 5
        or digest != REVIEWED_MANIFEST_DIGEST
    ):
        raise CleanupError("apply mode manifest is not the reviewed five-target inventory")


class GitHubClient:
    def __init__(self, repository, token=None):
        self.repository = repository
        self.token = token

    def _request(self, method, endpoint, payload=None, allow_missing=False):
        url = f"https://api.github.com/repos/{self.repository}{endpoint}"
        body = None if payload is None else json.dumps(payload).encode()
        headers = {
            "Accept": "application/vnd.github+json",
            "User-Agent": "echo-release-history",
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        api_request = request.Request(url, data=body, headers=headers, method=method)
        try:
            with request.urlopen(api_request, timeout=30) as response:
                response_body = response.read()
        except error.HTTPError as exception:
            if allow_missing and exception.code == 404:
                return None
            detail = exception.read().decode(errors="replace")
            raise CleanupError(
                f"GitHub API {method} {endpoint} failed with {exception.code}: {detail}"
            ) from exception
        except (error.URLError, OSError) as exception:
            raise CleanupError(
                f"GitHub API {method} {endpoint} failed: {exception}"
            ) from exception
        if not response_body:
            return None
        try:
            return json.loads(response_body)
        except json.JSONDecodeError as exception:
            raise CleanupError(
                f"GitHub API {method} {endpoint} returned invalid JSON"
            ) from exception

    def _pages(self, endpoint):
        values = []
        page = 1
        while True:
            separator = "&" if "?" in endpoint else "?"
            batch = self._request(
                "GET", f"{endpoint}{separator}per_page=100&page={page}"
            )
            if not isinstance(batch, list):
                raise CleanupError(f"GitHub API {endpoint} did not return a list")
            values.extend(batch)
            if len(batch) < 100:
                return values
            page += 1

    def releases(self):
        return self._pages("/releases")

    def tags(self):
        return self._pages("/tags")

    def release(self, release_id):
        return self._request("GET", f"/releases/{release_id}", allow_missing=True)

    def update_release_body(self, release_id, body):
        self._request("PATCH", f"/releases/{release_id}", {"body": body})

    def delete_release(self, release_id):
        self._request("DELETE", f"/releases/{release_id}")


def load_fixture_list(path, label):
    document = read_json(path)
    if not isinstance(document, list):
        raise CleanupError(f"{label} fixture must be a JSON list")
    return document


def release_body_digest(body):
    return "sha256:" + hashlib.sha256(body.encode()).hexdigest()


def audit_token():
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        return token
    try:
        result = subprocess.run(
            ["gh", "auth", "token"],
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exception:
        raise CleanupError(
            "authenticated audit requires GH_TOKEN, GITHUB_TOKEN, or gh auth login"
        ) from exception
    token = result.stdout.strip()
    if result.returncode != 0 or not token:
        raise CleanupError(
            "authenticated audit requires GH_TOKEN, GITHUB_TOKEN, or gh auth login"
        )
    return token


def release_assets_match(release, expected):
    assets = release.get("assets")
    if not isinstance(assets, list) or not all(
        isinstance(asset, dict) and isinstance(asset.get("name"), str)
        for asset in assets
    ):
        return False
    actual_assets = sorted(
        (asset.get("id"), asset.get("name"), asset.get("size"), asset.get("digest"))
        for asset in assets
    )
    expected_assets = sorted(
        (asset["id"], asset["name"], asset["size"], asset["digest"])
        for asset in expected["assets"]
    )
    return actual_assets == expected_assets


def draft_matches(release, expected):
    return (
        release.get("id") == expected.get("id")
        and release.get("tag_name") == expected.get("tag_name")
        and release.get("target_commitish") == expected.get("target_commitish")
        and release.get("draft") is True
        and release.get("prerelease") is False
        and release_assets_match(release, expected)
    )


def release_note_identity_matches(release, expected):
    return (
        release.get("id") == expected.get("id")
        and release.get("tag_name") == expected.get("tag_name")
        and release.get("target_commitish") == expected.get("target_commitish")
        and release.get("draft") is False
        and release.get("prerelease") is expected.get("prerelease")
        and release_assets_match(release, expected)
    )


def audit(releases, tags, manifest):
    releases_by_id = {release.get("id"): release for release in releases}
    releases_by_tag = {release.get("tag_name"): release for release in releases}
    expected_drafts = manifest["delete_draft_releases"]
    draft_actions = []
    drift = []
    for expected in expected_drafts:
        release = releases_by_id.get(expected["id"])
        if release is None:
            same_tag = releases_by_tag.get(expected["tag_name"])
            state = "missing" if same_tag is None else "drift"
        else:
            state = "delete" if draft_matches(release, expected) else "drift"
        draft_actions.append({"id": expected["id"], "tag": expected["tag_name"], "state": state})
        if state == "drift":
            drift.append(f"draft {expected['tag_name']} does not match the manifest")

    expected_ids = {draft["id"] for draft in expected_drafts}
    unexpected_qualification_drafts = sorted(
        release.get("tag_name")
        for release in releases
        if release.get("draft") is True
        and str(release.get("tag_name", "")).startswith("qualification-")
        and release.get("id") not in expected_ids
    )
    if unexpected_qualification_drafts:
        drift.append(
            "unreviewed qualification drafts exist: "
            + ", ".join(unexpected_qualification_drafts)
        )

    note = manifest["superseded_release_note"]
    release = releases_by_id.get(note["id"])
    if release is None:
        same_tag = releases_by_tag.get(note["tag_name"])
        note_state = "missing" if same_tag is None else "drift"
    elif not release_note_identity_matches(release, note):
        note_state = "drift"
    else:
        body = release.get("body")
        if not isinstance(body, str):
            note_state = "drift"
        elif release_body_digest(body) == note["applied_body_sha256"]:
            note_state = "already-applied"
        elif release_body_digest(body) == note["expected_body_sha256"]:
            note_state = "update"
        else:
            note_state = "drift"
    if note_state in {"missing", "drift"}:
        drift.append(f"release note target {note['tag_name']} is {note_state}")

    release_tags = {release.get("tag_name") for release in releases}
    git_tags = {
        tag.get("name")
        for tag in tags
        if isinstance(tag, dict) and isinstance(tag.get("name"), str)
    }
    orphan_tags = sorted(git_tags - release_tags)
    published = sorted(
        release.get("tag_name")
        for release in releases
        if release.get("draft") is False
    )
    published_asset_count = sum(
        len(release.get("assets", []))
        for release in releases
        if release.get("draft") is False and isinstance(release.get("assets"), list)
    )
    return {
        "draft_actions": draft_actions,
        "note_action": {
            "tag": note["tag_name"],
            "state": note_state,
            "notice": note["notice"],
        },
        "orphan_tags_preserved": orphan_tags,
        "published_releases_preserved": published,
        "published_assets_preserved": published_asset_count,
        "unexpected_qualification_drafts": unexpected_qualification_drafts,
        "drift": drift,
    }


def print_audit(report, digest):
    print(f"manifest: {digest}")
    print("draft releases:")
    for action in report["draft_actions"]:
        print(f"  {action['state']:>15}  {action['tag']} (id {action['id']})")
    note = report["note_action"]
    print(f"release note: {note['state']}  {note['tag']}")
    if note["state"] == "update":
        for line in note["notice"].rstrip().splitlines():
            print(f"  {line}")
    print("orphan tags preserved:")
    for tag in report["orphan_tags_preserved"]:
        print(f"  {tag}")
    print(f"published releases preserved: {len(report['published_releases_preserved'])}")
    print(f"published assets preserved: {report['published_assets_preserved']}")
    if report["drift"]:
        print("drift:", file=sys.stderr)
        for finding in report["drift"]:
            print(f"  {finding}", file=sys.stderr)


def apply_cleanup(client, releases, tags, manifest, approved_digest):
    digest = manifest_digest(manifest)
    if approved_digest != digest:
        raise CleanupError(
            f"approval digest mismatch: expected {digest}, got {approved_digest}"
        )
    target_ids = {
        draft["id"] for draft in manifest["delete_draft_releases"]
    } | {manifest["superseded_release_note"]["id"]}
    refreshed_targets = {
        release_id: client.release(release_id) for release_id in target_ids
    }
    refreshed_releases = [
        release for release in releases if release.get("id") not in target_ids
    ] + [release for release in refreshed_targets.values() if release is not None]
    report = audit(refreshed_releases, tags, manifest)
    if report["drift"]:
        raise CleanupError("live release history drifted from the reviewed manifest")

    note = manifest["superseded_release_note"]
    if report["note_action"]["state"] == "update":
        current = refreshed_targets[note["id"]]
        if (
            current is None
            or not release_note_identity_matches(current, note)
            or not isinstance(current.get("body"), str)
            or release_body_digest(current["body"]) != note["expected_body_sha256"]
        ):
            raise CleanupError(f"release note target {note['tag_name']} changed after preflight")
        client.update_release_body(note["id"], note["notice"] + current["body"])

    states = {action["id"]: action["state"] for action in report["draft_actions"]}
    for expected in manifest["delete_draft_releases"]:
        if states[expected["id"]] == "delete":
            current = client.release(expected["id"])
            if current is None:
                continue
            if not draft_matches(current, expected):
                raise CleanupError(
                    f"draft {expected['tag_name']} changed after preflight"
                )
            client.delete_release(expected["id"])


def main():
    parser = argparse.ArgumentParser(
        description="Audit release history and apply one reviewed cleanup manifest."
    )
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--releases-json", type=Path)
    parser.add_argument("--tags-json", type=Path)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--approve-digest")
    arguments = parser.parse_args()

    try:
        manifest = read_manifest(arguments.manifest)
        digest = manifest_digest(manifest)
        fixture_mode = arguments.releases_json is not None or arguments.tags_json is not None
        if fixture_mode and (arguments.releases_json is None or arguments.tags_json is None):
            raise CleanupError("--releases-json and --tags-json must be used together")
        if arguments.apply and fixture_mode:
            raise CleanupError("apply mode cannot use fixture data")
        apply_token = None
        if arguments.apply:
            validate_apply_manifest(arguments.manifest, manifest, digest)
            apply_token = os.environ.get("ECHO_RELEASE_CLEANUP_TOKEN")
            if not apply_token:
                raise CleanupError("apply mode requires ECHO_RELEASE_CLEANUP_TOKEN")
            if not arguments.approve_digest:
                raise CleanupError("apply mode requires --approve-digest")

        if fixture_mode:
            releases = load_fixture_list(arguments.releases_json, "release")
            tags = load_fixture_list(arguments.tags_json, "tag")
        else:
            client = GitHubClient(
                manifest["repository"], apply_token if arguments.apply else audit_token()
            )
            releases = client.releases()
            tags = client.tags()

        report = audit(releases, tags, manifest)
        print_audit(report, digest)
        if arguments.apply:
            apply_client = GitHubClient(manifest["repository"], apply_token)
            releases = apply_client.releases()
            tags = apply_client.tags()
            apply_cleanup(
                apply_client,
                releases,
                tags,
                manifest,
                arguments.approve_digest,
            )
            verified = audit(
                apply_client.releases(), apply_client.tags(), manifest
            )
            expected_states = {action["state"] for action in verified["draft_actions"]}
            if (
                verified["drift"]
                or expected_states != {"missing"}
                or verified["note_action"]["state"] != "already-applied"
            ):
                raise CleanupError("post-apply verification did not converge")
            print("cleanup applied and verified")
        elif report["drift"]:
            raise CleanupError("release history differs from the reviewed manifest")
    except CleanupError as exception:
        print(f"release-history: {exception}", file=sys.stderr)
        raise SystemExit(1) from exception


if __name__ == "__main__":
    main()
