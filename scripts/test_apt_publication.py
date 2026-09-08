#!/usr/bin/env python3
"""Offline fixtures for monotonic APT publication and authenticated identity."""

from __future__ import annotations

import copy
import pathlib
import shutil
import subprocess
import tempfile
import unittest
import urllib.error
from unittest.mock import patch

import build_apt_repository as builder
import check_apt_repository as fixtures
import guard_apt_publication as publication


def identity(version="1.0.0", digest="a" * 64):
    return {"version": version, "packages": {"amd64": {
        "filename": f"pool/main/e/echo/echo_{version}_amd64.deb", "size": 123, "sha256": digest,
    }}}


class MemoryState:
    def __init__(self, value=None):
        self.value = copy.deepcopy(value)
        self.writes = 0

    def read(self):
        return None if self.value is None else publication.validate_identity(self.value)

    def reserve(self, candidate):
        self.value = copy.deepcopy(candidate)
        self.writes += 1


class PublicationTests(unittest.TestCase):
    def test_newer_and_identical_retry_are_accepted(self):
        current = identity()
        state = MemoryState(current)
        newer = identity("1.0.1")
        publication.guard(newer, state, lambda: current, allow_first_publication=False)
        # A failed deployment leaves a reservation but still serves the old site.
        publication.guard(newer, state, lambda: current, allow_first_publication=False)
        self.assertEqual(state.value, newer)
        publication.guard(newer, state, lambda: newer, allow_first_publication=False)
        self.assertEqual(state.writes, 3)

    def test_equal_version_different_bytes_are_rejected(self):
        state = MemoryState(identity())
        with self.assertRaisesRegex(ValueError, "different package bytes"):
            publication.guard(identity(digest="b" * 64), state, lambda: identity(), allow_first_publication=False)
        self.assertEqual(state.writes, 0)

    def test_debian_epoch_tilde_and_numeric_ordering(self):
        self.assertLess(publication.compare_versions("1.0~rc1", "1.0"), 0)
        self.assertGreater(publication.compare_versions("1:1.0", "2.0"), 0)
        self.assertGreater(publication.compare_versions("1.0-10", "1.0-2"), 0)
        # Debian-equivalent spelling must not permit different package bytes.
        with self.assertRaisesRegex(ValueError, "different package bytes"):
            publication.require_monotonic(identity("1.0-0"), identity("1.0"))

    def test_delayed_old_release_cannot_replace_newer_even_with_stale_cdn(self):
        old, newer = identity(), identity("1.0.1")
        state = MemoryState(old)
        publication.guard(newer, state, lambda: old, allow_first_publication=False)
        # The old build finishes late and reads a stale but valid signed site.
        with self.assertRaisesRegex(ValueError, "downgrade"):
            publication.guard(old, state, lambda: old, allow_first_publication=False)
        self.assertEqual(state.value, newer)
        self.assertEqual(state.writes, 1)

    def test_existing_site_bootstraps_without_opt_in_but_never_downgrades(self):
        state = MemoryState()
        with self.assertRaisesRegex(ValueError, "downgrade"):
            publication.guard(identity(), state, lambda: identity("2.0"), allow_first_publication=False)
        self.assertEqual(state.writes, 0)
        publication.guard(identity("2.1"), state, lambda: identity("2.0"), allow_first_publication=False)
        self.assertEqual(state.value, identity("2.1"))

    def test_first_publication_is_explicit_and_identical_failed_attempt_can_retry(self):
        state = MemoryState()
        with self.assertRaisesRegex(ValueError, "explicit opt-in"):
            publication.guard(identity(), state, lambda: None, allow_first_publication=False)
        publication.guard(identity(), state, lambda: None, allow_first_publication=True)
        publication.guard(identity(), state, lambda: None, allow_first_publication=True)
        self.assertEqual(state.writes, 2)
        # A missing established site cannot silently be treated as a new one.
        with self.assertRaises(ValueError):
            publication.guard(identity("2.0"), state, lambda: None, allow_first_publication=True)

    def test_malformed_state_or_site_never_reserves(self):
        for malformed in ({}, identity("not-a-version"), identity(digest="not-a-hash")):
            with self.subTest(malformed=malformed):
                state = MemoryState(malformed)
                with self.assertRaises(ValueError):
                    publication.guard(identity("2.0"), state, lambda: identity(), allow_first_publication=True)
                self.assertEqual(state.writes, 0)
        state = MemoryState(identity())
        with self.assertRaises(ValueError):
            publication.guard(identity("2.0"), state, lambda: {}, allow_first_publication=True)
        self.assertEqual(state.writes, 0)

    def test_unavailable_site_fails_closed_even_with_bootstrap_enabled(self):
        def unavailable():
            raise urllib.error.URLError("fixture connection refused")
        state = MemoryState()
        with self.assertRaises(urllib.error.URLError):
            publication.guard(identity(), state, unavailable, allow_first_publication=True)
        self.assertEqual(state.writes, 0)

    def test_only_http_404_is_missing_not_permission_or_server_errors(self):
        url = "https://example.invalid/apt/dists/stable/InRelease"
        for code in (403, 404, 500):
            with self.subTest(code=code), patch.object(publication.urllib.request, "urlopen", side_effect=
                    urllib.error.HTTPError(url, code, "fixture", {}, None)):
                if code == 404:
                    self.assertIsNone(publication.fetch(url, missing_ok=True))
                else:
                    with self.assertRaises(urllib.error.HTTPError):
                        publication.fetch(url, missing_ok=True)

    def test_state_api_errors_are_not_first_publication(self):
        state = publication.GitHubState("owner/repo", "fixture-token")
        with patch.object(state, "api", side_effect=urllib.error.URLError("unavailable")):
            with self.assertRaises(urllib.error.URLError):
                publication.guard(identity(), state, lambda: None, allow_first_publication=True)


class SignedIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.work = pathlib.Path(tempfile.mkdtemp(prefix="echoap-"))
        cls.home = cls.work / "gpg"
        fixtures.prepare_gnupg_home(cls.home)
        fingerprint = fixtures.generate_signing_key(cls.home, cls.work / "key-parameters")
        cls.deb = fixtures.write_fixture_deb(cls.work / "echo_1.0.0_amd64.deb")
        cls.repo = cls.work / "repository"
        cls.keyring = cls.work / "keyring.gpg"
        builder.build_repository(builder.build_arg_parser().parse_args([
            str(cls.deb), "--output", str(cls.repo), "--gpg-key", fingerprint,
            "--gpg-homedir", str(cls.home), "--public-key-out", str(cls.keyring),
        ]))

    @classmethod
    def tearDownClass(cls):
        fixtures.kill_agent(cls.home)
        shutil.rmtree(cls.work)

    def load(self, path):
        return (self.repo / path).read_bytes()

    def test_signed_candidate_identity_matches_real_package(self):
        import hashlib
        actual = publication.repository_identity(self.load, self.keyring, verify_packages=True)
        self.assertEqual(actual["version"], "1.0.0")
        self.assertEqual(actual["packages"]["amd64"]["sha256"], hashlib.sha256(self.deb.read_bytes()).hexdigest())

    def test_altered_index_is_rejected_by_signed_checksum(self):
        def load(path):
            data = self.load(path)
            return data.replace(b"Version: 1.0.0", b"Version: 0.9.0") if path.endswith("/Packages") else data
        with self.assertRaisesRegex(ValueError, "Packages does not match"):
            publication.repository_identity(load, self.keyring)

    def test_altered_deb_is_rejected_before_publication(self):
        def load(path):
            data = self.load(path)
            return data + b"different package" if path.endswith(".deb") else data
        with self.assertRaisesRegex(ValueError, "candidate .deb"):
            publication.repository_identity(load, self.keyring, verify_packages=True)

    def test_bad_signature_is_rejected(self):
        def load(path):
            data = self.load(path)
            return data.replace(b"Suite: stable", b"Suite: broken") if path.endswith("/InRelease") else data
        with self.assertRaises(subprocess.CalledProcessError):
            publication.repository_identity(load, self.keyring)


if __name__ == "__main__":
    unittest.main()
