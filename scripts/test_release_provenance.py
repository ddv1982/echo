#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "scripts" / "fixtures"


def load_sbom_module():
    path = ROOT / "scripts" / "generate-desktop-sbom.py"
    spec = importlib.util.spec_from_file_location("generate_desktop_sbom", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SBOM = load_sbom_module()


def run_script(name, *arguments):
    return subprocess.run(
        ["python3", str(ROOT / "scripts" / name), *map(str, arguments)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def rewrite_catalog_entries(source, constant, type_name, transform):
    array = re.compile(
        rf"(pub const {constant}\s*:.*?=\s*&\[)(.*?)(\n\];)", re.DOTALL
    )
    match = array.search(source)
    if match is None:
        raise AssertionError(f"test fixture has no {constant} array")
    entries = re.findall(
        rf"\n\s+{type_name}\s*\{{.*?\n\s+\}},", match.group(2), re.DOTALL
    )
    if not entries:
        raise AssertionError(f"test fixture has no {type_name} entries")
    replacement = match.group(1) + "".join(transform(entries)) + match.group(3)
    return source[: match.start()] + replacement + source[match.end() :]


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
    def test_combines_release_dependencies_in_deterministic_cyclonedx(self):
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
            self.assertEqual(ecosystems, {"cargo", "managed", "npm"})
            expected_kinds = {
                "parakeet-tdt-06b-v3-int8": "model",
                "sherpa-runtime": "runtime",
                "silero-vad": "model",
                "whisper-base-q5-1": "model",
                "whisper-large-v3-turbo-q5-0": "model",
                "whisper-runtime": "runtime",
                "whisper-small": "model",
                "whisper-vulkan-runtime": "runtime",
            }
            managed = {}
            for component in components:
                properties = {
                    property_["name"]: property_["value"]
                    for property_ in component.get("properties", [])
                }
                if properties.get("echo:ecosystem") == "managed":
                    managed[properties["echo:managed:id"]] = (component, properties)
            self.assertEqual(set(managed), set(expected_kinds))
            for component_id, expected_kind in expected_kinds.items():
                component, properties = managed[component_id]
                references = component["externalReferences"]
                self.assertTrue(component["version"])
                self.assertEqual(properties["echo:managed:kind"], expected_kind)
                self.assertRegex(
                    next(
                        hash_["content"]
                        for hash_ in component["hashes"]
                        if hash_["alg"] == "SHA-256"
                    ),
                    r"^[0-9a-f]{64}$",
                )
                self.assertTrue(component["supplier"]["name"])
                self.assertTrue(
                    any(
                        reference["type"] == "distribution" and reference["url"]
                        for reference in references
                    )
                )
                self.assertTrue(
                    any(
                        reference["type"] == "website" and reference["url"]
                        for reference in references
                    )
                )
                self.assertTrue(component["licenses"])
                self.assertTrue(
                    all(license_["license"]["id"] for license_ in component["licenses"])
                )
            licenses = {
                component_id: {
                    license_["license"]["id"] for license_ in component["licenses"]
                }
                for component_id, (component, _) in managed.items()
            }
            self.assertEqual(
                licenses["parakeet-tdt-06b-v3-int8"], {"CC-BY-4.0"}
            )
            self.assertIn("Apache-2.0", licenses["sherpa-runtime"])
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


class ThirdPartyProvenanceSemanticsTests(unittest.TestCase):
    REVISION = "1" * 40
    TIMESTAMP = "2026-08-30T00:00:00Z"

    @classmethod
    def setUpClass(cls):
        cls.catalog_path = ROOT / "crates" / "echo" / "src" / "install" / "catalog.rs"
        cls.catalog_source = cls.catalog_path.read_text(encoding="utf-8")
        cls.cargo = json.loads(
            (FIXTURES / "release-provenance" / "cargo-metadata.json").read_text()
        )
        cls.npm = json.loads(
            (FIXTURES / "release-provenance" / "package-lock.json").read_text()
        )
        cls.actual_npm = json.loads(
            (ROOT / "frontend" / "package-lock.json").read_text()
        )
        cls.document = SBOM.build_document(
            cls.cargo,
            cls.npm,
            cls.REVISION,
            cls.TIMESTAMP,
            SBOM.load_managed_catalog(cls.catalog_path),
        )
        cls.actual_lock_document = SBOM.build_document(
            cls.cargo,
            cls.actual_npm,
            cls.REVISION,
            cls.TIMESTAMP,
            SBOM.load_managed_catalog(cls.catalog_path),
        )

    def _components(self, document=None):
        return (document or self.document)["components"]

    def _properties(self, component):
        return {
            property_["name"]: property_["value"]
            for property_ in component.get("properties", [])
        }

    def _managed(self, component_id, document=None):
        for component in self._components(document):
            properties = self._properties(component)
            if properties.get("echo:managed:id") == component_id:
                return component
        self.fail(f"SBOM has no managed component {component_id}")

    def _ecosystem_component(self, ecosystem, name, document=None):
        for component in self._components(document):
            properties = self._properties(component)
            if properties.get("echo:ecosystem") == ecosystem and component["name"] == name:
                return component
        self.fail(f"SBOM has no {ecosystem} component named {name}")

    def _semantic_property(self, component, component_id, *term_sets):
        properties = self._properties(component)
        for terms in term_sets:
            matches = [
                value
                for name, value in properties.items()
                if all(term in name.lower() for term in terms)
            ]
            if len(matches) == 1:
                return matches[0]
        descriptions = ["+".join(terms) for terms in term_sets]
        self.fail(
            f"{component_id} must expose exactly one machine-readable property "
            f"matching one of {descriptions}; properties were {sorted(properties)}"
        )

    def _provenance_scope_note(self, component, component_id):
        return self._semantic_property(
            component,
            component_id,
            ("provenance", "note"),
            ("provenance", "scope"),
            ("evidence", "scope"),
        ).lower()

    def _evidence_reference(self, component, component_id):
        references = [
            reference["url"]
            for reference in component.get("externalReferences", [])
            if reference.get("type") == "evidence"
        ]
        self.assertEqual(
            len(references),
            1,
            f"{component_id} must expose exactly one CycloneDX evidence reference",
        )
        return references[0]

    def _assert_parakeet_revision_boundary(self, text, source):
        lowered = text.lower()
        self.assertIn("575de92b31b2f60855bca9b70968bde5afb069ba", lowered)
        self.assertRegex(
            lowered,
            r"(?:\bunpinned\b[\s\S]{0,80}\b(?:nvidia|model)\b|"
            r"\b(?:nvidia|model)\b[\s\S]{0,80}\bunpinned\b)",
            f"{source} must identify the conversion input as unpinned",
        )
        self.assertRegex(
            lowered,
            r"(?=[^.]*\battribution\b)(?=[^.]*\blicen[cs]e\b)"
            r"(?=[^.]*\bsnapshot\b)(?=[^.]*\bonly\b)[^.]*",
            f"{source} must limit the pinned revision to an attribution/license snapshot",
        )
        self.assertRegex(
            lowered,
            r"(?:\bdoes not\b|\bcannot\b|\bis not\b)[\s\S]{0,120}"
            r"\b(?:attest|prove|establish)(?:s|ed)?\b[\s\S]{0,120}"
            r"\b(?:source-revision\s+byte\s+lineage|byte\s+lineage)\b",
            f"{source} must distinguish the snapshot from attested byte lineage",
        )

    def _dependency_map(self, document):
        return {node["ref"]: node["dependsOn"] for node in document["dependencies"]}

    def _document_with_runtime_npm_dependency(self):
        npm = json.loads(json.dumps(self.npm))
        npm["packages"]["node_modules/react"]["dependencies"] = {
            "scheduler": "^0.27.0"
        }
        npm["packages"]["node_modules/scheduler"] = {
            "version": "0.27.0",
            "resolved": "https://registry.npmjs.org/scheduler/-/scheduler-0.27.0.tgz",
            "peerDependencies": {"react": "^19.0.0"},
        }
        return SBOM.build_document(
            self.cargo,
            npm,
            self.REVISION,
            self.TIMESTAMP,
            SBOM.load_managed_catalog(self.catalog_path),
        )

    def _run_catalog(self, source, directory):
        catalog = directory / "catalog.rs"
        output = directory / "sbom.json"
        catalog.write_text(source, encoding="utf-8")
        result = run_script(
            "generate-desktop-sbom.py",
            "--cargo-metadata",
            FIXTURES / "release-provenance" / "cargo-metadata.json",
            "--npm-lock",
            FIXTURES / "release-provenance" / "package-lock.json",
            "--source-revision",
            self.REVISION,
            "--source-timestamp",
            self.TIMESTAMP,
            "--catalog",
            catalog,
            "--output",
            output,
        )
        return result, output

    def _assert_catalog_rejected(self, source, expected_diagnostic):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            first, output = self._run_catalog(source, directory)
            second, _ = self._run_catalog(source, directory)
            self.assertNotEqual(first.returncode, 0, "malformed catalog was accepted")
            self.assertEqual(first.stderr, second.stderr, "catalog error is not deterministic")
            self.assertIn(expected_diagnostic, first.stderr)
            self.assertFalse(output.exists(), "rejected catalog produced an SBOM")

    def test_echo_vulkan_artifact_identifies_its_upstream(self):
        vulkan = self._managed("whisper-vulkan-runtime")
        with self.subTest(component="whisper-vulkan-runtime", field="supplier"):
            self.assertEqual(vulkan["supplier"]["name"], "Echo")
        with self.subTest(component="whisper-vulkan-runtime", field="distributor"):
            self.assertEqual(
                self._semantic_property(vulkan, "whisper-vulkan-runtime", ("distributor",)),
                "Echo",
            )
        with self.subTest(component="whisper-vulkan-runtime", field="origin"):
            self.assertEqual(
                self._semantic_property(
                    vulkan, "whisper-vulkan-runtime", ("origin",), ("upstream",)
                ),
                "ggml-org",
            )
            self.assertTrue(
                any(
                    "github.com/ggml-org/whisper.cpp" in reference["url"]
                    for reference in vulkan["externalReferences"]
                ),
                "whisper-vulkan-runtime lacks a pinned whisper.cpp upstream reference",
            )

    def test_parakeet_artifact_identifies_conversion_provenance(self):
        parakeet = self._managed("parakeet-tdt-06b-v3-int8")
        expected_roles = {
            "supplier": "k2-fsa",
            "distributor": "k2-fsa",
            "converter": "k2-fsa",
            "origin": "NVIDIA",
        }
        for role, expected in expected_roles.items():
            with self.subTest(component="parakeet-tdt-06b-v3-int8", role=role):
                role_terms = ((role,), ("upstream",)) if role == "origin" else ((role,),)
                actual = (
                    parakeet["supplier"]["name"]
                    if role == "supplier"
                    else self._semantic_property(
                        parakeet,
                        "parakeet-tdt-06b-v3-int8",
                        *role_terms,
                    )
                )
                self.assertEqual(actual, expected)
        with self.subTest(component="parakeet-tdt-06b-v3-int8", field="modifications"):
            modifications = self._semantic_property(
                parakeet, "parakeet-tdt-06b-v3-int8", ("modification",)
            ).lower()
            for required_text in ("onnx conversion", "int8 quantization"):
                self.assertIn(required_text, modifications)

    def test_sherpa_provenance_note_scopes_named_build_evidence(self):
        sherpa = self._managed("sherpa-runtime")
        note = self._provenance_scope_note(sherpa, "sherpa-runtime")
        commit = "1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911"
        archive = "sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2"
        self.assertIn(commit, note)
        self.assertIn(archive, note)
        self.assertRegex(
            note, r"v1\.13\.6 tag.{0,40}(?:resolves|points).{0,20}commit"
        )
        self.assertIn(".github/workflows/linux.yaml", note)
        for action in (
            r"\bbuild(?:s|ing)?\b",
            r"\bnam(?:e|es|ed|ing)\b",
            r"\bupload(?:s|ed|ing)?\b",
        ):
            with self.subTest(component="sherpa-runtime", workflow_action=action):
                self.assertRegex(note, action)
        self.assertRegex(
            note, r"(?:sha-256|digest).{0,80}\bexact\b.{0,30}\bbytes\b"
        )
        self.assertRegex(
            note, r"\bstatic\b.{0,30}\bonnx runtime\b.{0,20}\b1\.27\.1\b"
        )
        self.assertRegex(
            note, r"bundled dependencies.{0,80}(?:own|their).{0,20}terms"
        )
        self.assertEqual(
            self._evidence_reference(sherpa, "sherpa-runtime"),
            f"https://github.com/k2-fsa/sherpa-onnx/blob/{commit}/.github/workflows/linux.yaml",
        )

    def test_parakeet_provenance_note_scopes_export_evidence(self):
        parakeet = self._managed("parakeet-tdt-06b-v3-int8")
        note = self._provenance_scope_note(parakeet, "parakeet-tdt-06b-v3-int8")
        self.assertIn(
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2", note
        )
        self.assertRegex(
            note, r"\bmerged\b.{0,40}\b(?:pr|pull request)\b.{0,20}\b2500\b"
        )
        self.assertIn("export record", note)
        self.assertIn("nvidia", note)
        self.assertRegex(note, r"\bonnx\b.{0,30}\bconversion\b")
        self.assertRegex(note, r"\bint8\b.{0,30}\bquantization\b")
        self._assert_parakeet_revision_boundary(note, "Parakeet SBOM provenance note")
        self.assertEqual(
            self._evidence_reference(parakeet, "parakeet-tdt-06b-v3-int8"),
            "https://github.com/k2-fsa/sherpa-onnx/pull/2500",
        )

    def test_provenance_evidence_is_limited_to_sherpa_and_parakeet(self):
        expected = {"sherpa-runtime", "parakeet-tdt-06b-v3-int8"}
        with_notes = set()
        with_evidence = set()
        for component in self._components():
            properties = self._properties(component)
            component_id = properties.get("echo:managed:id")
            if component_id is None:
                continue
            if any(
                all(term in name.lower() for term in terms)
                for name in properties
                for terms in (
                    ("provenance", "scope"),
                    ("evidence", "scope"),
                )
            ):
                with_notes.add(component_id)
            if any(
                reference.get("type") == "evidence"
                for reference in component.get("externalReferences", [])
            ):
                with_evidence.add(component_id)
        self.assertEqual(with_notes, expected)
        self.assertEqual(with_evidence, expected)

    def test_third_party_notice_disclaims_exact_parakeet_source_revision(self):
        notice = (ROOT / "THIRD_PARTY.md").read_text(encoding="utf-8")
        match = re.search(
            r"(?ms)^- \*\*`parakeet-tdt-06b-v3-int8`.*?"
            r"(?=^- \*\*`|^## |\Z)",
            notice,
        )
        self.assertIsNotNone(match, "THIRD_PARTY.md has no Parakeet component notice")
        parakeet_notice = match.group(0)
        self.assertIn(
            "https://github.com/k2-fsa/sherpa-onnx/pull/2500", parakeet_notice
        )
        self.assertIn(
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
            parakeet_notice,
        )
        self.assertRegex(
            parakeet_notice.lower(), r"\bonnx\b[\s\S]{0,40}\bconversion\b"
        )
        self.assertRegex(
            parakeet_notice.lower(), r"\bint8\b[\s\S]{0,40}\bquantiz"
        )
        self._assert_parakeet_revision_boundary(
            parakeet_notice, "THIRD_PARTY.md Parakeet notice"
        )

    def test_third_party_notice_links_sherpa_build_evidence(self):
        notice = (ROOT / "THIRD_PARTY.md").read_text(encoding="utf-8")
        match = re.search(
            r"(?ms)^- \*\*`sherpa-runtime`.*?" r"(?=^- \*\*`|^## |\Z)",
            notice,
        )
        self.assertIsNotNone(match, "THIRD_PARTY.md has no sherpa component notice")
        sherpa_notice = match.group(0).lower()
        commit = "1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911"
        self.assertIn(
            f"https://github.com/k2-fsa/sherpa-onnx/blob/{commit}/.github/workflows/linux.yaml",
            sherpa_notice,
        )
        self.assertIn(
            "sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2",
            sherpa_notice,
        )
        self.assertRegex(
            sherpa_notice,
            r"(?:sha-256|digest)[\s\S]{0,100}\bexact\b[\s\S]{0,30}\bbytes\b",
        )
        self.assertRegex(
            sherpa_notice,
            r"\bstatic\b[\s\S]{0,40}\bonnx runtime\b[\s\S]{0,20}\b1\.27\.1\b",
        )

    def test_sherpa_static_distribution_scopes_its_license_claims(self):
        sherpa = self._managed("sherpa-runtime")
        license_ids = {
            license_["license"]["id"]
            for license_ in sherpa.get("licenses", [])
            if "license" in license_ and "id" in license_["license"]
        }
        with self.subTest(component="sherpa-runtime", field="artifact-license"):
            self.assertNotEqual(
                license_ids,
                {"Apache-2.0"},
                "the downloaded static distribution must not be represented as exclusively Apache-2.0",
            )
        with self.subTest(component="sherpa-runtime", field="source-license-scope"):
            scope = self._semantic_property(sherpa, "sherpa-runtime", ("license", "scope"))
            self.assertIn("upstream", scope.lower())
            self.assertIn("source", scope.lower())
        with self.subTest(component="sherpa-runtime", field="bundled-dependency-terms"):
            terms = self._semantic_property(
                sherpa, "sherpa-runtime", ("bundled", "dependency", "term")
            )
            self.assertTrue(terms.strip(), "sherpa bundled-dependency terms are empty")
        with self.subTest(component="sherpa-runtime", field="bundled-dependency-reference"):
            reference = self._semantic_property(
                sherpa, "sherpa-runtime", ("bundled", "dependency", "reference")
            )
            self.assertTrue(
                reference.startswith("https://"),
                "sherpa bundled-dependency reference must be an HTTPS URL",
            )

    def test_root_has_only_direct_required_application_dependencies(self):
        document = self._document_with_runtime_npm_dependency()
        root_ref = document["metadata"]["component"]["bom-ref"]
        desktop = self._ecosystem_component("cargo", "echo-desktop", document)
        react = self._ecosystem_component("npm", "react", document)
        actual = self._dependency_map(document).get(root_ref)
        self.assertEqual(
            actual,
            sorted([desktop["bom-ref"], react["bom-ref"]]),
            "root must include only top-level desktop Cargo and direct production npm dependencies",
        )
        vite = self._ecosystem_component("npm", "vite", document)
        self.assertNotIn(vite["bom-ref"], actual, "Vite is a development dependency")
        managed_refs = {
            component["bom-ref"]
            for component in self._components(document)
            if self._properties(component).get("echo:ecosystem") == "managed"
        }
        self.assertTrue(
            managed_refs.isdisjoint(actual),
            "optional managed downloads must not be direct root dependencies",
        )

    def test_every_component_has_one_node_and_npm_edges_follow_lockfile(self):
        document = self._document_with_runtime_npm_dependency()
        node_refs = [node["ref"] for node in document["dependencies"]]
        expected_refs = {
            document["metadata"]["component"]["bom-ref"],
            *(component["bom-ref"] for component in self._components(document)),
        }
        with self.subTest(rule="one dependency node per component"):
            self.assertEqual(
                len(node_refs), len(set(node_refs)), "duplicate dependency nodes found"
            )
            self.assertEqual(
                set(node_refs),
                expected_refs,
                "dependency nodes must cover the root and every component exactly once",
            )
        dependencies = self._dependency_map(document)
        react = self._ecosystem_component("npm", "react", document)
        scheduler = self._ecosystem_component("npm", "scheduler", document)
        vite = self._ecosystem_component("npm", "vite", document)
        with self.subTest(rule="npm runtime dependencies"):
            self.assertEqual(dependencies.get(react["bom-ref"]), [scheduler["bom-ref"]])
            self.assertEqual(dependencies.get(scheduler["bom-ref"]), [react["bom-ref"]])
            self.assertEqual(dependencies.get(vite["bom-ref"]), [])

    def test_actual_lock_required_peer_edges_are_included(self):
        packages = self.actual_npm["packages"]
        self.assertIn("react", packages["node_modules/lucide-react"]["peerDependencies"])
        self.assertIn("react", packages["node_modules/react-dom"]["peerDependencies"])

        dependencies = self._dependency_map(self.actual_lock_document)
        react = self._ecosystem_component("npm", "react", self.actual_lock_document)
        for package_name in ("lucide-react", "react-dom"):
            with self.subTest(package=package_name):
                package = self._ecosystem_component(
                    "npm", package_name, self.actual_lock_document
                )
                self.assertIn(react["bom-ref"], dependencies[package["bom-ref"]])

    def test_actual_lock_resolved_optional_peer_edge_is_included(self):
        packages = self.actual_npm["packages"]
        oxlint = packages["node_modules/oxlint"]
        self.assertTrue(
            oxlint["peerDependenciesMeta"]["oxlint-tsgolint"]["optional"]
        )
        self.assertIn("node_modules/oxlint-tsgolint", packages)

        dependencies = self._dependency_map(self.actual_lock_document)
        oxlint_component = self._ecosystem_component(
            "npm", "oxlint", self.actual_lock_document
        )
        tsgolint_component = self._ecosystem_component(
            "npm", "oxlint-tsgolint", self.actual_lock_document
        )
        self.assertIn(
            tsgolint_component["bom-ref"], dependencies[oxlint_component["bom-ref"]]
        )

    def test_actual_lock_unresolved_optional_peer_is_omitted(self):
        packages = self.actual_npm["packages"]
        oxlint = packages["node_modules/oxlint"]
        self.assertTrue(oxlint["peerDependenciesMeta"]["vite-plus"]["optional"])
        self.assertNotIn("node_modules/vite-plus", packages)
        self.assertFalse(
            any(
                component["name"] == "vite-plus"
                and self._properties(component).get("echo:ecosystem") == "npm"
                for component in self._components(self.actual_lock_document)
            )
        )

    def test_actual_lock_mutation_rejects_unresolved_required_peer(self):
        npm = json.loads(json.dumps(self.actual_npm))
        peer_name = "echo-unresolved-required-peer"
        npm["packages"]["node_modules/lucide-react"]["peerDependencies"][
            peer_name
        ] = "1.0.0"
        with self.assertRaisesRegex(
            SBOM.InputError,
            rf"npm package node_modules/lucide-react declares unresolved runtime dependency {peer_name}",
        ):
            SBOM.build_document(
                self.cargo,
                npm,
                self.REVISION,
                self.TIMESTAMP,
                SBOM.load_managed_catalog(self.catalog_path),
            )

    def test_managed_edges_connect_models_to_their_runtimes(self):
        references = {
            self._properties(component)["echo:managed:id"]: component["bom-ref"]
            for component in self._components()
            if self._properties(component).get("echo:ecosystem") == "managed"
        }
        dependencies = self._dependency_map(self.document)
        expected = {
            "whisper-runtime": [],
            "whisper-vulkan-runtime": [],
            "sherpa-runtime": [],
            "whisper-base-q5-1": [references["whisper-runtime"]],
            "whisper-small": [references["whisper-runtime"]],
            "whisper-large-v3-turbo-q5-0": [references["whisper-runtime"]],
            "silero-vad": [references["whisper-runtime"]],
            "parakeet-tdt-06b-v3-int8": [references["sherpa-runtime"]],
        }
        for component_id, expected_edges in expected.items():
            with self.subTest(component=component_id):
                self.assertEqual(
                    dependencies.get(references[component_id]),
                    expected_edges,
                    f"managed dependency edges are wrong for {component_id}",
                )

    def test_catalog_parser_rejects_malformed_and_incomplete_inputs(self):
        malformed = rewrite_catalog_entries(
            self.catalog_source,
            "COMPONENT_PROVENANCE",
            "ComponentProvenance",
            lambda entries: [
                re.sub(r"(\n\s*)supplier\s*:", r"\1supplier ", entries[0], count=1),
                *entries[1:],
            ],
        )
        missing_component = rewrite_catalog_entries(
            self.catalog_source,
            "COMPONENTS",
            "ComponentSpec",
            lambda entries: entries[1:],
        )
        missing_provenance = rewrite_catalog_entries(
            self.catalog_source,
            "COMPONENT_PROVENANCE",
            "ComponentProvenance",
            lambda entries: entries[1:],
        )
        duplicate_component = rewrite_catalog_entries(
            self.catalog_source,
            "COMPONENTS",
            "ComponentSpec",
            lambda entries: [entries[0], *entries],
        )
        missing_provenance_note = rewrite_catalog_entries(
            self.catalog_source,
            "COMPONENT_PROVENANCE",
            "ComponentProvenance",
            lambda entries: [
                re.sub(
                    r"\n\s*provenance_note:\s*None,",
                    "",
                    entries[0],
                    count=1,
                ),
                *entries[1:],
            ],
        )
        invalid_evidence_url = self.catalog_source.replace(
            "https://github.com/k2-fsa/sherpa-onnx/blob/"
            "1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/.github/workflows/linux.yaml",
            "http://github.com/k2-fsa/sherpa-onnx/blob/"
            "1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/.github/workflows/linux.yaml",
            1,
        )

        mappings = list(
            re.finditer(
                r"(Self::[A-Za-z_][A-Za-z0-9_]*\s*=>\s*)\"([^\"]+)\"",
                self.catalog_source,
            )
        )
        self.assertGreaterEqual(len(mappings), 2, "test fixture has too few stable ID mappings")
        second = mappings[1]
        duplicate_mapping = (
            self.catalog_source[: second.start(2)]
            + mappings[0].group(2)
            + self.catalog_source[second.end(2) :]
        )

        cases = (
            (malformed, "malformed field in managed catalog"),
            (
                missing_component,
                "managed catalog does not contain every ComponentId exactly once",
            ),
            (
                missing_provenance,
                "managed provenance does not contain every ComponentId exactly once",
            ),
            (duplicate_component, "duplicate managed component ComponentId::"),
            (missing_provenance_note, "missing ['provenance_note']"),
            (invalid_evidence_url, "provenance_evidence_url is not HTTPS"),
            (duplicate_mapping, "ComponentId string mappings are duplicated"),
        )
        for source, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic):
                self._assert_catalog_rejected(source, diagnostic)

    def test_catalog_array_order_does_not_change_generated_document(self):
        reversed_source = rewrite_catalog_entries(
            self.catalog_source,
            "COMPONENTS",
            "ComponentSpec",
            lambda entries: list(reversed(entries)),
        )
        reversed_source = rewrite_catalog_entries(
            reversed_source,
            "COMPONENT_PROVENANCE",
            "ComponentProvenance",
            lambda entries: list(reversed(entries)),
        )
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            original_dir = directory / "original"
            reversed_dir = directory / "reversed"
            original_dir.mkdir()
            reversed_dir.mkdir()
            original, original_output = self._run_catalog(
                self.catalog_source, original_dir
            )
            reordered, reordered_output = self._run_catalog(
                reversed_source, reversed_dir
            )
            self.assertEqual(original.returncode, 0, original.stderr)
            self.assertEqual(reordered.returncode, 0, reordered.stderr)
            self.assertEqual(
                original_output.read_bytes(),
                reordered_output.read_bytes(),
                "COMPONENTS/COMPONENT_PROVENANCE order changed generated SBOM bytes",
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


class ReleaseStagingTests(unittest.TestCase):
    def test_stages_third_party_attribution(self):
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text()
        stage = workflow.split("      - name: Stage GitHub Release files\n", maxsplit=1)[
            1
        ].split("\n      - name:", maxsplit=1)[0]
        self.assertIn("THIRD_PARTY.md", stage)


if __name__ == "__main__":
    unittest.main()
