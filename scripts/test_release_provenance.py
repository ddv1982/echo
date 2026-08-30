#!/usr/bin/env python3
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "scripts" / "fixtures"


def run_script(name, *arguments):
    return subprocess.run(
        ["python3", str(ROOT / "scripts" / name), *map(str, arguments)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


class WorkflowPinningTests(unittest.TestCase):
    def test_accepts_full_commit_and_local_action(self):
        result = run_script(
            "verify-workflow-pinning.py",
            FIXTURES / "workflow-pinning" / "pinned.yml",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_every_floating_reference(self):
        result = run_script(
            "verify-workflow-pinning.py",
            FIXTURES / "workflow-pinning" / "floating.yml",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("actions/checkout@v7", result.stderr)
        self.assertIn("owner/repository/path@main", result.stderr)
        self.assertIn("docker://alpine:latest", result.stderr)


class TagPolicyTests(unittest.TestCase):
    def test_allows_repeated_run_at_same_commit(self):
        result = run_script(
            "verify-tag-policy.py",
            "--runs-json",
            FIXTURES / "release-provenance" / "tag-runs-same-commit.json",
            "--tag",
            "v1.2.3",
            "--sha",
            "1111111111111111111111111111111111111111",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_tag_seen_at_another_commit(self):
        result = run_script(
            "verify-tag-policy.py",
            "--runs-json",
            FIXTURES / "release-provenance" / "tag-runs-moved.json",
            "--tag",
            "v1.2.3",
            "--sha",
            "1111111111111111111111111111111111111111",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("create a new patch version", result.stderr)


class DesktopSbomTests(unittest.TestCase):
    def test_combines_cargo_and_npm_in_deterministic_cyclonedx(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.json"
            second = Path(directory) / "second.json"
            arguments = (
                "--cargo-metadata",
                FIXTURES / "release-provenance" / "cargo-metadata.json",
                "--npm-lock",
                FIXTURES / "release-provenance" / "package-lock.json",
                "--source-revision",
                "1111111111111111111111111111111111111111",
                "--source-timestamp",
                "2026-08-30T00:00:00Z",
            )
            result = run_script(
                "generate-desktop-sbom.py", *arguments, "--output", first
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            result = run_script(
                "generate-desktop-sbom.py", *arguments, "--output", second
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(first.read_bytes(), second.read_bytes())

            document = json.loads(first.read_text())
            components = document["components"]
            ecosystems = {
                property_["value"]
                for component in components
                for property_ in component.get("properties", [])
                if property_["name"] == "echo:ecosystem"
            }
            self.assertEqual(ecosystems, {"cargo", "npm"})
            self.assertTrue(
                any(component["name"] == "serde" for component in components)
            )
            workspace = next(
                component for component in components if component["name"] == "echo-desktop"
            )
            self.assertNotIn("purl", workspace)
            serde = next(
                component for component in components if component["name"] == "serde"
            )
            self.assertEqual(serde["purl"], "pkg:cargo/serde@1.0.219")
            vite = next(
                component for component in components if component["name"] == "vite"
            )
            self.assertIn(
                {"name": "echo:npm:development", "value": "true"},
                vite["properties"],
            )


class AttestationPermissionsTests(unittest.TestCase):
    def test_attestation_job_has_only_required_permissions(self):
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text()
        job = workflow.split("\n  attest-assets:\n", maxsplit=1)[1].split(
            "\n  github-release:\n", maxsplit=1
        )[0]
        permissions = job.split("\n    permissions:\n", maxsplit=1)[1].split(
            "\n    env:\n", maxsplit=1
        )[0]
        actual = {
            key.strip(): value.strip()
            for line in permissions.splitlines()
            for key, value in [line.split(":", maxsplit=1)]
        }
        self.assertEqual(
            actual,
            {
                "actions": "read",
                "attestations": "write",
                "contents": "read",
                "id-token": "write",
            },
        )


if __name__ == "__main__":
    unittest.main()
