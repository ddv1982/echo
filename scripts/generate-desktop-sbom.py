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
ARTIFACT_SHA256 = re.compile(r"[0-9a-f]{64}")
SPDX_LICENSE_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+-]*")
DEFAULT_MANAGED_CATALOG = ROOT / "crates" / "echo" / "src" / "install" / "catalog.rs"


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


def skip_rust_space(source, offset):
    while offset < len(source):
        if source[offset].isspace():
            offset += 1
        elif source.startswith("//", offset):
            newline = source.find("\n", offset + 2)
            offset = len(source) if newline == -1 else newline + 1
        elif source.startswith("/*", offset):
            end = source.find("*/", offset + 2)
            if end == -1:
                raise InputError("managed catalog contains an unterminated comment")
            offset = end + 2
        else:
            break
    return offset


def matching_rust_delimiter(source, offset, opening, closing):
    if offset >= len(source) or source[offset] != opening:
        raise InputError(f"managed catalog parser expected {opening!r}")
    depth = 1
    cursor = offset + 1
    while cursor < len(source):
        if source[cursor] == '"':
            cursor += 1
            while cursor < len(source):
                if source[cursor] == "\\":
                    cursor += 2
                elif source[cursor] == '"':
                    cursor += 1
                    break
                else:
                    cursor += 1
            else:
                raise InputError("managed catalog contains an unterminated string")
            continue
        if source.startswith("//", cursor):
            newline = source.find("\n", cursor + 2)
            cursor = len(source) if newline == -1 else newline + 1
            continue
        if source.startswith("/*", cursor):
            end = source.find("*/", cursor + 2)
            if end == -1:
                raise InputError("managed catalog contains an unterminated comment")
            cursor = end + 2
            continue
        if source[cursor] == opening:
            depth += 1
        elif source[cursor] == closing:
            depth -= 1
            if depth == 0:
                return cursor
        cursor += 1
    raise InputError(f"managed catalog contains an unmatched {opening!r}")


def extract_rust_block(source, declaration, opening, closing):
    matches = list(re.finditer(declaration, source))
    if len(matches) != 1:
        raise InputError(
            f"managed catalog must contain exactly one {declaration.pattern!r} declaration"
        )
    start = matches[0].end() - 1
    end = matching_rust_delimiter(source, start, opening, closing)
    return source[start + 1 : end]


def parse_rust_fields(body, entry_label):
    fields = {}
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+),", stripped)
        if match is None:
            raise InputError(f"malformed field in managed catalog {entry_label}: {stripped}")
        name, value = match.groups()
        if name in fields:
            raise InputError(f"duplicate field {name} in managed catalog {entry_label}")
        fields[name] = value.strip()
    return fields


def parse_rust_struct_array(source, constant, type_name):
    declaration = re.compile(
        rf"\bpub\s+const\s+{re.escape(constant)}\s*:\s*&\s*"
        rf"\[\s*{re.escape(type_name)}\s*\]\s*=\s*&\s*\["
    )
    body = extract_rust_block(source, declaration, "[", "]")
    entries = []
    cursor = 0
    while True:
        cursor = skip_rust_space(body, cursor)
        if cursor == len(body):
            break
        prefix = re.match(rf"{re.escape(type_name)}\s*\{{", body[cursor:])
        if prefix is None:
            raise InputError(
                f"managed catalog {constant} contains an unrecognized entry at offset {cursor}"
            )
        opening = cursor + prefix.end() - 1
        closing = matching_rust_delimiter(body, opening, "{", "}")
        entries.append(
            parse_rust_fields(
                body[opening + 1 : closing], f"{type_name} entry {len(entries)}"
            )
        )
        cursor = skip_rust_space(body, closing + 1)
        if cursor >= len(body) or body[cursor] != ",":
            raise InputError(f"managed catalog {constant} entry has no trailing comma")
        cursor += 1
    if not entries:
        raise InputError(f"managed catalog {constant} is empty")
    return entries


def rust_string(value, label):
    if re.fullmatch(r'"(?:[^"\\]|\\.)*"', value) is None:
        raise InputError(f"managed catalog {label} is not a string literal")
    try:
        decoded = json.loads(value)
    except json.JSONDecodeError as error:
        raise InputError(f"managed catalog {label} is not a valid string: {error}") from error
    if not decoded:
        raise InputError(f"managed catalog {label} is empty")
    return decoded


def rust_variant(value, type_name, label):
    match = re.fullmatch(rf"{re.escape(type_name)}::([A-Za-z_][A-Za-z0-9_]*)", value)
    if match is None:
        raise InputError(f"managed catalog {label} is not a {type_name} variant")
    return match.group(1)


def rust_optional_string(value, label):
    if value == "None":
        return None
    match = re.fullmatch(r"Some\((\"(?:[^\"\\]|\\.)*\")\)", value)
    if match is None:
        raise InputError(f"managed catalog {label} is not None or Some(string)")
    return rust_string(match.group(1), label)


def component_id_mapping(source):
    enum_body = extract_rust_block(
        source, re.compile(r"\bpub\s+enum\s+ComponentId\s*\{"), "{", "}"
    )
    variants = []
    for line in enum_body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*),", stripped)
        if match is None:
            raise InputError(f"malformed ComponentId variant: {stripped}")
        variants.append(match.group(1))
    if len(variants) != len(set(variants)) or not variants:
        raise InputError("managed catalog ComponentId variants are empty or duplicated")

    impl_body = extract_rust_block(
        source, re.compile(r"\bimpl\s+ComponentId\s*\{"), "{", "}"
    )
    pairs = re.findall(
        r"\bSelf::([A-Za-z_][A-Za-z0-9_]*)\s*=>\s*(\"(?:[^\"\\]|\\.)*\")",
        impl_body,
    )
    mapping = {}
    identifiers = set()
    for variant, encoded_identifier in pairs:
        identifier = rust_string(encoded_identifier, f"ComponentId::{variant}")
        if variant in mapping or identifier in identifiers:
            raise InputError("managed catalog ComponentId string mappings are duplicated")
        mapping[variant] = identifier
        identifiers.add(identifier)
    if set(mapping) != set(variants):
        raise InputError("managed catalog does not map every ComponentId to a stable ID")
    return mapping


def load_managed_catalog(path=DEFAULT_MANAGED_CATALOG):
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as error:
        raise InputError(f"cannot read managed catalog from {path}: {error}") from error

    identifiers = component_id_mapping(source)
    specs = parse_rust_struct_array(source, "COMPONENTS", "ComponentSpec")
    provenance_entries = parse_rust_struct_array(
        source, "COMPONENT_PROVENANCE", "ComponentProvenance"
    )
    required_spec_fields = {"id", "label", "version", "url", "artifact_sha256"}
    required_provenance_fields = {
        "id",
        "kind",
        "supplier",
        "distributor",
        "origin",
        "converter",
        "modifications",
        "license_id",
        "license_url",
        "license_scope",
        "bundled_dependency_license_id",
        "bundled_dependency_terms",
        "bundled_dependency_url",
        "provenance_note",
        "provenance_evidence_url",
        "homepage_url",
    }

    components_by_variant = {}
    for index, fields in enumerate(specs):
        missing = required_spec_fields - set(fields)
        if missing:
            raise InputError(f"managed component {index} is missing fields: {sorted(missing)}")
        variant = rust_variant(fields["id"], "ComponentId", f"component {index} id")
        if variant not in identifiers:
            raise InputError(f"managed component {index} uses unknown ComponentId::{variant}")
        if variant in components_by_variant:
            raise InputError(f"duplicate managed component ComponentId::{variant}")
        component = {
            "id": identifiers[variant],
            "name": rust_string(fields["label"], f"component {variant} label"),
            "version": rust_string(fields["version"], f"component {variant} version"),
            "url": rust_string(fields["url"], f"component {variant} url"),
            "sha256": rust_string(
                fields["artifact_sha256"], f"component {variant} artifact_sha256"
            ),
        }
        if not component["url"].startswith("https://"):
            raise InputError(f"managed component {component['id']} URL is not HTTPS")
        if ARTIFACT_SHA256.fullmatch(component["sha256"]) is None:
            raise InputError(f"managed component {component['id']} has an invalid SHA-256")
        components_by_variant[variant] = component

    provenance_by_variant = {}
    for index, fields in enumerate(provenance_entries):
        if set(fields) != required_provenance_fields:
            missing = required_provenance_fields - set(fields)
            unexpected = set(fields) - required_provenance_fields
            raise InputError(
                f"managed provenance {index} has missing {sorted(missing)} "
                f"and unexpected {sorted(unexpected)} fields"
            )
        variant = rust_variant(fields["id"], "ComponentId", f"provenance {index} id")
        if variant not in identifiers:
            raise InputError(f"managed provenance {index} uses unknown ComponentId::{variant}")
        if variant in provenance_by_variant:
            raise InputError(f"duplicate managed provenance ComponentId::{variant}")
        kind_variant = rust_variant(
            fields["kind"], "ComponentKind", f"provenance {variant} kind"
        )
        kinds = {"Runtime": "runtime", "Model": "model"}
        if kind_variant not in kinds:
            raise InputError(f"managed provenance {variant} has invalid kind {kind_variant}")
        license_scope_variant = rust_variant(
            fields["license_scope"],
            "LicenseScope",
            f"provenance {variant} license_scope",
        )
        license_scopes = {
            "Artifact": "artifact",
            "UpstreamSourceWithBundledDependencies": "upstream-source-with-bundled-dependencies",
        }
        if license_scope_variant not in license_scopes:
            raise InputError(
                f"managed provenance {variant} has invalid license scope "
                f"{license_scope_variant}"
            )
        provenance = {
            "kind": kinds[kind_variant],
            "supplier": rust_string(fields["supplier"], f"provenance {variant} supplier"),
            "distributor": rust_string(
                fields["distributor"], f"provenance {variant} distributor"
            ),
            "origin": rust_string(fields["origin"], f"provenance {variant} origin"),
            "converter": rust_optional_string(
                fields["converter"], f"provenance {variant} converter"
            ),
            "modifications": rust_optional_string(
                fields["modifications"], f"provenance {variant} modifications"
            ),
            "license_id": rust_string(
                fields["license_id"], f"provenance {variant} license_id"
            ),
            "license_url": rust_string(
                fields["license_url"], f"provenance {variant} license_url"
            ),
            "license_scope": license_scopes[license_scope_variant],
            "bundled_dependency_license_id": rust_optional_string(
                fields["bundled_dependency_license_id"],
                f"provenance {variant} bundled_dependency_license_id",
            ),
            "bundled_dependency_terms": rust_optional_string(
                fields["bundled_dependency_terms"],
                f"provenance {variant} bundled_dependency_terms",
            ),
            "bundled_dependency_url": rust_optional_string(
                fields["bundled_dependency_url"],
                f"provenance {variant} bundled_dependency_url",
            ),
            "provenance_note": rust_optional_string(
                fields["provenance_note"],
                f"provenance {variant} provenance_note",
            ),
            "provenance_evidence_url": rust_optional_string(
                fields["provenance_evidence_url"],
                f"provenance {variant} provenance_evidence_url",
            ),
            "homepage_url": rust_string(
                fields["homepage_url"], f"provenance {variant} homepage_url"
            ),
        }
        if SPDX_LICENSE_ID.fullmatch(provenance["license_id"]) is None:
            raise InputError(f"managed provenance {variant} has an invalid SPDX license ID")
        bundled_license = provenance["bundled_dependency_license_id"]
        if bundled_license is not None and SPDX_LICENSE_ID.fullmatch(bundled_license) is None:
            raise InputError(
                f"managed provenance {variant} has an invalid bundled dependency SPDX license ID"
            )
        for name in ("license_url", "homepage_url"):
            if not provenance[name].startswith("https://"):
                raise InputError(f"managed provenance {variant} {name} is not HTTPS")
        bundled_values = (
            provenance["bundled_dependency_license_id"],
            provenance["bundled_dependency_terms"],
            provenance["bundled_dependency_url"],
        )
        if provenance["license_scope"] == "artifact":
            if any(value is not None for value in bundled_values):
                raise InputError(
                    f"managed provenance {variant} artifact license has bundled dependency fields"
                )
        elif any(value is None for value in bundled_values):
            raise InputError(
                f"managed provenance {variant} bundled dependency fields are incomplete"
            )
        if (
            provenance["bundled_dependency_url"] is not None
            and not provenance["bundled_dependency_url"].startswith("https://")
        ):
            raise InputError(
                f"managed provenance {variant} bundled_dependency_url is not HTTPS"
            )
        if (
            provenance["provenance_evidence_url"] is not None
            and not provenance["provenance_evidence_url"].startswith("https://")
        ):
            raise InputError(
                f"managed provenance {variant} provenance_evidence_url is not HTTPS"
            )
        provenance_by_variant[variant] = provenance

    expected = set(identifiers)
    if set(components_by_variant) != expected:
        raise InputError("managed catalog does not contain every ComponentId exactly once")
    if set(provenance_by_variant) != expected:
        raise InputError("managed provenance does not contain every ComponentId exactly once")
    return [
        components_by_variant[variant] | provenance_by_variant[variant]
        for variant in sorted(expected, key=lambda value: identifiers[value])
    ]


def managed_components(catalog):
    components = []
    for entry in catalog:
        reference = stable_ref("managed", entry["id"])
        external_references = [
            {"type": "distribution", "url": entry["url"]},
            {"type": "website", "url": entry["homepage_url"]},
        ]
        if entry["provenance_evidence_url"] is not None:
            external_references.append(
                {"type": "evidence", "url": entry["provenance_evidence_url"]}
            )
        licenses = [
            {
                "license": {
                    "acknowledgement": "declared",
                    "id": entry["license_id"],
                    "properties": [
                        property_("echo:managed:license:scope", entry["license_scope"])
                    ],
                    "url": entry["license_url"],
                }
            }
        ]
        if entry["bundled_dependency_license_id"] is not None:
            licenses.append(
                {
                    "license": {
                        "acknowledgement": "declared",
                        "id": entry["bundled_dependency_license_id"],
                        "properties": [
                            property_(
                                "echo:managed:license:scope", "bundled-dependency"
                            )
                        ],
                    }
                }
            )
        properties = [
            property_("echo:ecosystem", "managed"),
            property_("echo:managed:id", entry["id"]),
            property_("echo:managed:kind", entry["kind"]),
            property_("echo:managed:distributor", entry["distributor"]),
            property_("echo:managed:origin", entry["origin"]),
            property_("echo:managed:license:scope", entry["license_scope"]),
        ]
        for field, property_name in (
            ("converter", "echo:managed:converter"),
            ("modifications", "echo:managed:modifications"),
            (
                "bundled_dependency_terms",
                "echo:managed:bundled-dependency:terms",
            ),
            (
                "bundled_dependency_url",
                "echo:managed:bundled-dependency:reference",
            ),
            (
                "provenance_note",
                "echo:managed:provenance:evidence-scope",
            ),
        ):
            if entry[field] is not None:
                properties.append(property_(property_name, entry[field]))
        components.append(
            {
                "bom-ref": reference,
                "externalReferences": external_references,
                "hashes": [{"alg": "SHA-256", "content": entry["sha256"]}],
                "licenses": licenses,
                "name": entry["name"],
                "properties": properties,
                "supplier": {"name": entry["supplier"]},
                "type": (
                    "application"
                    if entry["kind"] == "runtime"
                    else "machine-learning-model"
                ),
                "version": entry["version"],
            }
        )
    return components


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
        source = package.get("source")
        if not all(
            isinstance(value, str) and value for value in (package_id, name, version)
        ):
            raise InputError(f"Cargo package {index} has no id, name, or version")
        if source is not None and (not isinstance(source, str) or not source):
            raise InputError(f"Cargo package {index} has an invalid source")
        if package_id in references:
            raise InputError(f"Cargo metadata contains duplicate package {package_id}")
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
            "type": "library",
            "version": version,
        }
        if source is not None:
            component["properties"].append(property_("echo:cargo:source", source))
        if isinstance(source, str) and source.startswith("registry+"):
            component["purl"] = (
                f"pkg:cargo/{quote(name, safe='')}@{quote(version, safe='')}"
            )
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
    if not isinstance(packages.get(""), dict):
        raise InputError("npm lockfile packages has no root package entry")
    merged = {}
    references_by_path = {}
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
        references_by_path[path] = stable_ref("npm", f"{name}@{version}")
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
    return components, references_by_path


def npm_runtime_dependency_declarations(package, path):
    package_label = path or "<root>"
    regular_names = set()
    peer_names = set()
    for field in ("dependencies", "optionalDependencies", "peerDependencies"):
        declared = package.get(field, {})
        if not isinstance(declared, dict):
            raise InputError(f"npm package {package_label} has invalid {field}")
        if any(not isinstance(name, str) or not name for name in declared):
            raise InputError(f"npm package {package_label} has an invalid {field} name")
        for name in sorted(declared):
            requirement = declared[name]
            if not isinstance(requirement, str) or not requirement:
                raise InputError(
                    f"npm package {package_label} has an invalid {field} requirement for {name}"
                )
            if field == "peerDependencies":
                peer_names.add(name)
            else:
                regular_names.add(name)

    peer_metadata = package.get("peerDependenciesMeta", {})
    if not isinstance(peer_metadata, dict):
        raise InputError(f"npm package {package_label} has invalid peerDependenciesMeta")
    if any(not isinstance(name, str) or not name for name in peer_metadata):
        raise InputError(
            f"npm package {package_label} has an invalid peerDependenciesMeta name"
        )
    optional_peers = set()
    for name in sorted(peer_metadata):
        metadata = peer_metadata[name]
        if not isinstance(metadata, dict):
            raise InputError(
                f"npm package {package_label} has invalid peerDependenciesMeta for {name}"
            )
        optional = metadata.get("optional", False)
        if not isinstance(optional, bool):
            raise InputError(
                f"npm package {package_label} has invalid peerDependenciesMeta optional state for {name}"
            )
        if optional and name in peer_names:
            optional_peers.add(name)

    return [
        (name, name in optional_peers and name not in regular_names)
        for name in sorted(regular_names | peer_names)
    ]


def npm_runtime_dependency_names(package, path):
    return [name for name, _ in npm_runtime_dependency_declarations(package, path)]


def resolve_npm_dependency_path(
    package_path, dependency_name, package_paths, allow_unresolved=False
):
    parts = package_path.split("/") if package_path else []
    dependency_parts = dependency_name.split("/")
    for depth in range(len(parts), -1, -1):
        if depth > 0 and parts[depth - 1] == "node_modules":
            continue
        candidate = "/".join(
            [*parts[:depth], "node_modules", *dependency_parts]
        )
        if candidate in package_paths:
            return candidate
    if allow_unresolved:
        return None
    raise InputError(
        f"npm package {package_path or '<root>'} declares unresolved runtime dependency "
        f"{dependency_name}"
    )


def npm_dependencies(lockfile, references_by_path):
    packages = lockfile["packages"]
    package_paths = set(references_by_path)
    relationships = {reference: set() for reference in references_by_path.values()}
    for path in sorted(package_paths):
        package = packages[path]
        for name, optional_peer in npm_runtime_dependency_declarations(package, path):
            dependency_path = resolve_npm_dependency_path(
                path, name, package_paths, allow_unresolved=optional_peer
            )
            if dependency_path is None:
                continue
            relationships[references_by_path[path]].add(
                references_by_path[dependency_path]
            )
    root_dependencies = []
    for name, optional_peer in npm_runtime_dependency_declarations(packages[""], ""):
        dependency_path = resolve_npm_dependency_path(
            "", name, package_paths, allow_unresolved=optional_peer
        )
        if dependency_path is None:
            continue
        root_dependencies.append(references_by_path[dependency_path])
    return (
        [
            {"ref": reference, "dependsOn": sorted(depends_on)}
            for reference, depends_on in sorted(relationships.items())
        ],
        sorted(set(root_dependencies)),
    )


def cargo_dependencies(metadata, references):
    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict) or not isinstance(resolve.get("nodes"), list):
        raise InputError("Cargo metadata has no resolve.nodes list")
    relationships = []
    seen = set()
    for index, node in enumerate(resolve["nodes"]):
        if not isinstance(node, dict):
            raise InputError(f"Cargo resolve node {index} is not an object")
        package_id = node.get("id")
        dependencies = node.get("dependencies")
        if package_id not in references or not isinstance(dependencies, list):
            raise InputError(f"Cargo resolve node {index} is invalid")
        if package_id in seen:
            raise InputError(f"Cargo resolve contains duplicate node {package_id}")
        seen.add(package_id)
        try:
            depends_on = sorted(references[dependency] for dependency in dependencies)
        except KeyError as error:
            raise InputError(
                f"Cargo resolve node references unknown package {error.args[0]}"
            ) from error
        relationships.append({"ref": references[package_id], "dependsOn": depends_on})
    if seen != set(references):
        raise InputError("Cargo resolve does not contain every package exactly once")
    return relationships


def managed_dependencies(components):
    references = {}
    for component in components:
        properties = {
            property_value["name"]: property_value["value"]
            for property_value in component["properties"]
        }
        references[properties["echo:managed:id"]] = component["bom-ref"]
    runtime_by_component = {
        "whisper-runtime": None,
        "whisper-vulkan-runtime": None,
        "sherpa-runtime": None,
        "whisper-base-q5-1": "whisper-runtime",
        "whisper-small": "whisper-runtime",
        "whisper-large-v3-turbo-q5-0": "whisper-runtime",
        "silero-vad": "whisper-runtime",
        "parakeet-tdt-06b-v3-int8": "sherpa-runtime",
    }
    if set(references) != set(runtime_by_component):
        raise InputError("managed dependency mapping does not match the managed catalog")
    return [
        {
            "ref": references[component_id],
            "dependsOn": (
                [] if runtime_id is None else [references[runtime_id]]
            ),
        }
        for component_id, runtime_id in sorted(runtime_by_component.items())
    ]


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


def build_document(cargo, npm, revision, timestamp, managed=None):
    if FULL_SHA.fullmatch(revision) is None:
        raise InputError("source revision must be a 40-character lowercase commit SHA")
    timestamp = parse_timestamp(timestamp)
    cargo_packages, cargo_refs, versions = cargo_components(cargo)
    npm_packages, npm_refs_by_path = npm_components(npm)
    managed_packages = managed_components(
        load_managed_catalog() if managed is None else managed
    )
    version = versions.get("echo-desktop")
    if version is None:
        raise InputError("Cargo metadata has no echo-desktop package")
    workspace_members = cargo.get("workspace_members")
    desktop_ids = [
        package.get("id")
        for package in cargo["packages"]
        if package.get("name") == "echo-desktop"
        and package.get("id") in workspace_members
    ]
    if len(desktop_ids) != 1:
        raise InputError("Cargo metadata must have exactly one workspace echo-desktop package")
    desktop_ref = cargo_refs[desktop_ids[0]]
    components = sorted(
        cargo_packages + npm_packages + managed_packages,
        key=lambda value: value["bom-ref"],
    )
    root_ref = f"pkg:github/ddv1982/echo@{revision}"
    npm_relationships, direct_npm_refs = npm_dependencies(npm, npm_refs_by_path)
    dependencies = (
        cargo_dependencies(cargo, cargo_refs)
        + npm_relationships
        + managed_dependencies(managed_packages)
    )
    dependencies.append(
        {
            "ref": root_ref,
            "dependsOn": sorted([desktop_ref, *direct_npm_refs]),
        }
    )
    dependencies.sort(key=lambda value: value["ref"])
    dependency_refs = [relationship["ref"] for relationship in dependencies]
    expected_refs = {root_ref, *(component["bom-ref"] for component in components)}
    if len(dependency_refs) != len(set(dependency_refs)):
        raise InputError("generated dependency graph contains duplicate nodes")
    if set(dependency_refs) != expected_refs:
        raise InputError("generated dependency graph does not cover every component exactly once")
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
                    "echo:sbom:scope",
                    "GitHub-built desktop release dependencies: Cargo, npm, and managed catalog components",
                ),
            ],
            "timestamp": timestamp,
            "tools": {
                "components": [
                    {
                        "description": "Generates Cargo, npm, and managed catalog components.",
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
    if ecosystems != {"cargo", "managed", "npm"}:
        raise RuntimeError(f"SBOM ecosystems differ: {ecosystems}")
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
    for component in document["components"]:
        properties = {
            property_value["name"]: property_value["value"]
            for property_value in component.get("properties", [])
        }
        if properties.get("echo:ecosystem") == "managed":
            managed[properties["echo:managed:id"]] = (component, properties)
    if set(managed) != set(expected_kinds):
        raise RuntimeError(f"managed SBOM IDs differ: {set(managed)}")
    for component_id, expected_kind in expected_kinds.items():
        component, properties = managed[component_id]
        references = component.get("externalReferences", [])
        hashes = component.get("hashes", [])
        licenses = component.get("licenses", [])
        if (
            not component.get("version")
            or properties.get("echo:managed:kind") != expected_kind
            or not any(
                hash_value.get("alg") == "SHA-256"
                and re.fullmatch(r"[0-9a-f]{64}", hash_value.get("content", ""))
                for hash_value in hashes
            )
            or not component.get("supplier", {}).get("name")
            or not any(
                reference.get("type") == "distribution" and reference.get("url")
                for reference in references
            )
            or not any(
                reference.get("type") == "website" and reference.get("url")
                for reference in references
            )
            or not licenses
            or not all(license_.get("license", {}).get("id") for license_ in licenses)
        ):
            raise RuntimeError(f"managed SBOM metadata is incomplete: {component_id}")
    license_ids = {
        component_id: {
            license_["license"]["id"] for license_ in component["licenses"]
        }
        for component_id, (component, _) in managed.items()
    }
    if license_ids["parakeet-tdt-06b-v3-int8"] != {"CC-BY-4.0"}:
        raise RuntimeError("Parakeet SBOM license is not CC-BY-4.0")
    if license_ids["sherpa-runtime"] != {"Apache-2.0", "MIT"}:
        raise RuntimeError("sherpa runtime SBOM does not expose its scoped license set")
    print("generate-desktop-sbom: self-test passed")


def main():
    parser = argparse.ArgumentParser(
        description="Create Echo's deterministic Cargo, npm, and managed-component CycloneDX SBOM."
    )
    parser.add_argument("--cargo-metadata", type=Path)
    parser.add_argument(
        "--npm-lock", type=Path, default=ROOT / "frontend" / "package-lock.json"
    )
    parser.add_argument(
        "--managed-catalog",
        "--catalog",
        dest="managed_catalog",
        type=Path,
        default=DEFAULT_MANAGED_CATALOG,
        help="Rust managed-component catalog (default: crates/echo/src/install/catalog.rs)",
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
            load_managed_catalog(arguments.managed_catalog),
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
