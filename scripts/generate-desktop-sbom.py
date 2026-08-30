#!/usr/bin/env python3
import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from urllib.parse import quote
import uuid


ROOT = Path(__file__).resolve().parent.parent
FULL_SHA = re.compile(r"[0-9a-f]{40}")


class InputError(ValueError):
    pass


def read_json(path, label):
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise InputError(f"cannot read {label} from {path}: {error}") from error
    if not isinstance(value, dict):
        raise InputError(f"{label} root is not an object")
    return value


def load_cargo_metadata(path):
    if path is not None:
        return read_json(path, "Cargo metadata")
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise InputError(f"cargo metadata failed: {result.stderr.strip()}")
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise InputError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise InputError("Cargo metadata root is not an object")
    return value


def stable_ref(prefix, identity):
    digest = hashlib.sha256(identity.encode()).hexdigest()
    return f"urn:echo:{prefix}:{digest}"


def property_(name, value):
    return {"name": name, "value": value}


def cargo_components(metadata):
    packages = metadata.get("packages")
    workspace_members = metadata.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(workspace_members, list):
        raise InputError("Cargo metadata has invalid packages or workspace_members")
    workspace = set(workspace_members)
    components = []
    references = {}
    workspace_versions = {}
    for index, package in enumerate(packages):
        if not isinstance(package, dict):
            raise InputError(f"Cargo package {index} is not an object")
        package_id = package.get("id")
        name = package.get("name")
        version = package.get("version")
        if not all(
            isinstance(value, str) and value for value in (package_id, name, version)
        ):
            raise InputError(f"Cargo package {index} has no id, name, or version")
        reference = stable_ref("cargo", package_id)
        references[package_id] = reference
        if package_id in workspace:
            workspace_versions[name] = version
        component = {
            "bom-ref": reference,
            "name": name,
            "properties": [
                property_("echo:ecosystem", "cargo"),
                property_("echo:cargo:workspace", str(package_id in workspace).lower()),
            ],
            "purl": f"pkg:cargo/{quote(name, safe='')}@{quote(version, safe='')}",
            "type": "library",
            "version": version,
        }
        license_ = package.get("license")
        if isinstance(license_, str) and license_:
            component["properties"].append(property_("echo:cargo:license", license_))
        components.append(component)
    return components, references, workspace_versions


def npm_package_name(path, package):
    name = package.get("name")
    if isinstance(name, str) and name:
        return name
    marker = "node_modules/"
    if marker not in path:
        raise InputError(f"npm package {path} has no name")
    return path.rsplit(marker, maxsplit=1)[1]


def npm_components(lockfile):
    packages = lockfile.get("packages")
    if lockfile.get("lockfileVersion") != 3 or not isinstance(packages, dict):
        raise InputError("npm lockfile must use lockfileVersion 3 and contain packages")
    merged = {}
    for path, package in packages.items():
        if path == "":
            continue
        if not isinstance(path, str) or not isinstance(package, dict):
            raise InputError("npm package entry is invalid")
        name = npm_package_name(path, package)
        version = package.get("version")
        if not isinstance(version, str) or not version:
            raise InputError(f"npm package {path} has no version")
        key = (name, version)
        development = package.get("dev", False)
        if not isinstance(development, bool):
            raise InputError(f"npm package {path} has invalid dev state")
        if key in merged:
            merged[key] = merged[key] and development
        else:
            merged[key] = development
    components = []
    for (name, version), development in sorted(merged.items()):
        identity = f"{name}@{version}"
        components.append(
            {
                "bom-ref": stable_ref("npm", identity),
                "name": name,
                "properties": [
                    property_("echo:ecosystem", "npm"),
                    property_("echo:npm:development", str(development).lower()),
                ],
                "purl": f"pkg:npm/{quote(name, safe='/')}@{quote(version, safe='')}",
                "type": "library",
                "version": version,
            }
        )
    return components


def cargo_dependencies(metadata, references):
    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict) or not isinstance(resolve.get("nodes"), list):
        raise InputError("Cargo metadata has no resolve.nodes list")
    relationships = []
    for index, node in enumerate(resolve["nodes"]):
        if not isinstance(node, dict):
            raise InputError(f"Cargo resolve node {index} is not an object")
        package_id = node.get("id")
        dependencies = node.get("dependencies")
        if package_id not in references or not isinstance(dependencies, list):
            raise InputError(f"Cargo resolve node {index} is invalid")
        try:
            depends_on = sorted(references[dependency] for dependency in dependencies)
        except KeyError as error:
            raise InputError(
                f"Cargo resolve node references unknown package {error.args[0]}"
            ) from error
        relationships.append({"ref": references[package_id], "dependsOn": depends_on})
    return relationships


def parse_timestamp(value):
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise InputError(f"source timestamp is not ISO 8601: {value}") from error
    if parsed.tzinfo is None:
        raise InputError(f"source timestamp has no UTC offset: {value}")
    return value


def git_value(*arguments):
    result = subprocess.run(
        ["git", *arguments],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise InputError(f"git {' '.join(arguments)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def build_document(cargo, npm, revision, timestamp):
    if FULL_SHA.fullmatch(revision) is None:
        raise InputError("source revision must be a 40-character lowercase commit SHA")
    timestamp = parse_timestamp(timestamp)
    cargo_packages, cargo_refs, versions = cargo_components(cargo)
    npm_packages = npm_components(npm)
    version = versions.get("echo-desktop")
    if version is None:
        raise InputError("Cargo metadata has no echo-desktop package")
    components = sorted(
        cargo_packages + npm_packages, key=lambda value: value["bom-ref"]
    )
    root_ref = f"pkg:github/ddv1982/echo@{revision}"
    dependencies = cargo_dependencies(cargo, cargo_refs)
    dependencies.append(
        {
            "ref": root_ref,
            "dependsOn": sorted(component["bom-ref"] for component in components),
        }
    )
    dependencies.sort(key=lambda value: value["ref"])
    serial = uuid.uuid5(
        uuid.NAMESPACE_URL, f"https://github.com/ddv1982/echo/{revision}"
    )
    return {
        "bomFormat": "CycloneDX",
        "components": components,
        "dependencies": dependencies,
        "metadata": {
            "component": {
                "bom-ref": root_ref,
                "name": "echo-desktop",
                "type": "application",
                "version": version,
            },
            "properties": [
                property_(
                    "echo:sbom:excluded",
                    "operator-built Whisper Vulkan runtime; verified by its separate receipt and checksum",
                ),
                property_(
                    "echo:sbom:scope",
                    "GitHub-built desktop release dependencies from Cargo.lock and frontend/package-lock.json",
                ),
            ],
            "timestamp": timestamp,
            "tools": {
                "components": [
                    {
                        "name": "generate-desktop-sbom.py",
                        "type": "application",
                        "version": "1",
                    }
                ]
            },
        },
        "serialNumber": f"urn:uuid:{serial}",
        "specVersion": "1.6",
        "version": 1,
    }


def write_document(document, output):
    output.parent.mkdir(parents=True, exist_ok=True)
    contents = json.dumps(document, indent=2, sort_keys=True) + "\n"
    with tempfile.NamedTemporaryFile("w", dir=output.parent, delete=False) as temporary:
        temporary.write(contents)
        temporary_path = Path(temporary.name)
    temporary_path.replace(output)


def self_test():
    fixtures = ROOT / "scripts" / "fixtures" / "release-provenance"
    cargo = load_cargo_metadata(fixtures / "cargo-metadata.json")
    npm = read_json(fixtures / "package-lock.json", "npm lockfile")
    document = build_document(cargo, npm, "1" * 40, "2026-08-30T00:00:00Z")
    ecosystems = {
        property_value["value"]
        for component in document["components"]
        for property_value in component["properties"]
        if property_value["name"] == "echo:ecosystem"
    }
    if ecosystems != {"cargo", "npm"}:
        raise RuntimeError(f"SBOM ecosystems differ: {ecosystems}")
    print("generate-desktop-sbom: self-test passed")


def main():
    parser = argparse.ArgumentParser(
        description="Create Echo's deterministic Cargo and npm CycloneDX SBOM."
    )
    parser.add_argument("--cargo-metadata", type=Path)
    parser.add_argument(
        "--npm-lock", type=Path, default=ROOT / "frontend" / "package-lock.json"
    )
    parser.add_argument("--source-revision")
    parser.add_argument("--source-timestamp")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    arguments = parser.parse_args()
    if arguments.self_test:
        self_test()
        return
    if arguments.output is None:
        parser.error("--output is required")
    revision = arguments.source_revision or git_value("rev-parse", "HEAD")
    timestamp = arguments.source_timestamp or git_value(
        "show", "-s", "--format=%cI", revision
    )
    try:
        document = build_document(
            load_cargo_metadata(arguments.cargo_metadata),
            read_json(arguments.npm_lock, "npm lockfile"),
            revision,
            timestamp,
        )
        write_document(document, arguments.output)
    except InputError as error:
        print(f"generate-desktop-sbom: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    print(
        f"wrote {arguments.output} with {len(document['components'])} dependency components"
    )


if __name__ == "__main__":
    main()
