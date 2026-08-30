#!/usr/bin/env python3
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
MODULE_PATH = ROOT / "scripts" / "release-history.py"
SPEC = importlib.util.spec_from_file_location("release_history", MODULE_PATH)
release_history = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release_history)

class FakeClient:
    def __init__(self, releases):
        self.live_releases = releases
        self.updates = []
        self.deletes = []

    def release(self, release_id):
        return next(
            (release for release in self.live_releases if release["id"] == release_id),
            None,
        )

    def update_release_body(self, release_id, body):
        release = next(item for item in self.live_releases if item["id"] == release_id)
        release["body"] = body
        self.updates.append(release_id)

    def delete_release(self, release_id):
        self.live_releases[:] = [
            release for release in self.live_releases if release["id"] != release_id
        ]
        self.deletes.append(release_id)


class ReleaseHistoryTests(unittest.TestCase):
    def setUp(self):
        self.manifest = release_history.read_manifest(
            ROOT / "scripts" / "manifests" / "release-cleanup-2026-08-30.json"
        )
        fixture = release_history.read_json(
            ROOT / "scripts" / "fixtures" / "release-history.json"
        )
        self.releases = copy.deepcopy(fixture["releases"])
        self.tags = copy.deepcopy(fixture["tags"])

    def test_manifest_digest_is_the_reviewed_identity(self):
        self.assertEqual(
            release_history.manifest_digest(self.manifest),
            "sha256:c294f5608023cd79d18e0dd3fb11248edd487aa606c120741c12f09ebbd79bbc",
        )

    def test_wrapper_audits_fixture_files(self):
        with tempfile.TemporaryDirectory() as directory:
            releases_path = Path(directory) / "releases.json"
            tags_path = Path(directory) / "tags.json"
            releases_path.write_text(json.dumps(self.releases))
            tags_path.write_text(json.dumps(self.tags))
            result = subprocess.run(
                [
                    ROOT / "scripts" / "audit-release-history.sh",
                    "--releases-json",
                    releases_path,
                    "--tags-json",
                    tags_path,
                ],
                text=True,
                capture_output=True,
                check=True,
            )
        self.assertIn("draft releases:", result.stdout)
        self.assertIn("release note: update  v0.12.6", result.stdout)
        self.assertIn("published releases preserved: 2", result.stdout)
        self.assertIn("published assets preserved: 4", result.stdout)

    def test_audit_names_only_reviewed_mutations_and_preserves_orphan_tags(self):
        report = release_history.audit(self.releases, self.tags, self.manifest)
        self.assertEqual(report["drift"], [])
        self.assertEqual(
            [action["state"] for action in report["draft_actions"]],
            ["delete"] * 5,
        )
        self.assertEqual(report["note_action"]["state"], "update")
        self.assertEqual(
            report["orphan_tags_preserved"],
            ["v0.12.0", "v0.12.1", "v0.12.5", "v0.4.1"],
        )
        self.assertEqual(
            report["published_releases_preserved"], ["v0.12.6", "v0.13.0"]
        )
        self.assertEqual(report["published_assets_preserved"], 4)

    def test_changed_asset_digest_blocks_apply(self):
        self.releases[0]["assets"][0]["digest"] = "sha256:" + "0" * 64
        client = FakeClient(self.releases)
        with self.assertRaisesRegex(release_history.CleanupError, "drifted"):
            release_history.apply_cleanup(
                client,
                self.releases,
                self.tags,
                self.manifest,
                release_history.manifest_digest(self.manifest),
            )
        self.assertEqual(client.updates, [])
        self.assertEqual(client.deletes, [])

    def test_tampered_applied_release_body_is_drift(self):
        note = self.manifest["superseded_release_note"]
        target = next(release for release in self.releases if release["id"] == note["id"])
        target["body"] = note["notice"] + "tampered"
        report = release_history.audit(self.releases, self.tags, self.manifest)
        self.assertEqual(report["note_action"]["state"], "drift")

    def test_release_note_identity_drift_blocks_update(self):
        note = self.manifest["superseded_release_note"]
        mutations = {
            "target": lambda release: release.__setitem__("target_commitish", "other"),
            "prerelease": lambda release: release.__setitem__("prerelease", True),
            "asset": lambda release: release["assets"][0].__setitem__("size", 1),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                releases = copy.deepcopy(self.releases)
                target = next(release for release in releases if release["id"] == note["id"])
                mutate(target)
                report = release_history.audit(releases, self.tags, self.manifest)
                self.assertEqual(report["note_action"]["state"], "drift")

    def test_refetches_every_target_before_the_first_write(self):
        preflight_snapshot = copy.deepcopy(self.releases)
        self.releases[0]["assets"][0]["size"] += 1
        client = FakeClient(self.releases)
        with self.assertRaisesRegex(release_history.CleanupError, "drifted"):
            release_history.apply_cleanup(
                client,
                preflight_snapshot,
                self.tags,
                self.manifest,
                release_history.manifest_digest(self.manifest),
            )
        self.assertEqual(client.updates, [])
        self.assertEqual(client.deletes, [])

    def test_apply_rejects_an_alternate_manifest_path(self):
        with self.assertRaisesRegex(release_history.CleanupError, "checked-in"):
            release_history.validate_apply_manifest(
                ROOT / "alternate.json",
                self.manifest,
                release_history.manifest_digest(self.manifest),
            )

    def test_unreviewed_qualification_draft_blocks_apply(self):
        self.releases.append(
            {
                "id": 999,
                "tag_name": "qualification-" + "f" * 40,
                "target_commitish": "f" * 40,
                "draft": True,
                "prerelease": False,
                "body": "",
                "assets": [],
            }
        )
        client = FakeClient(self.releases)
        with self.assertRaisesRegex(release_history.CleanupError, "drifted"):
            release_history.apply_cleanup(
                client,
                self.releases,
                self.tags,
                self.manifest,
                release_history.manifest_digest(self.manifest),
            )
        self.assertEqual(client.updates, [])
        self.assertEqual(client.deletes, [])

    def test_drift_blocks_all_mutations(self):
        self.releases[0]["assets"].pop()
        client = FakeClient(self.releases)
        with self.assertRaisesRegex(release_history.CleanupError, "drifted"):
            release_history.apply_cleanup(
                client,
                self.releases,
                self.tags,
                self.manifest,
                release_history.manifest_digest(self.manifest),
            )
        self.assertEqual(client.updates, [])
        self.assertEqual(client.deletes, [])

    def test_apply_requires_the_exact_reviewed_digest(self):
        client = FakeClient(self.releases)
        with self.assertRaisesRegex(release_history.CleanupError, "digest mismatch"):
            release_history.apply_cleanup(
                client,
                self.releases,
                self.tags,
                self.manifest,
                "sha256:" + "0" * 64,
            )
        self.assertEqual(client.updates, [])
        self.assertEqual(client.deletes, [])

    def test_apply_is_idempotent(self):
        client = FakeClient(self.releases)
        digest = release_history.manifest_digest(self.manifest)
        release_history.apply_cleanup(
            client, self.releases, self.tags, self.manifest, digest
        )
        self.assertEqual(client.updates, [379067749])
        self.assertEqual(len(client.deletes), 5)

        report = release_history.audit(
            client.live_releases, self.tags, self.manifest
        )
        self.assertEqual(report["drift"], [])
        self.assertEqual(
            [action["state"] for action in report["draft_actions"]],
            ["missing"] * 5,
        )
        self.assertEqual(report["note_action"]["state"], "already-applied")

        release_history.apply_cleanup(
            client, client.live_releases, self.tags, self.manifest, digest
        )
        self.assertEqual(client.updates, [379067749])
        self.assertEqual(len(client.deletes), 5)


if __name__ == "__main__":
    unittest.main()
