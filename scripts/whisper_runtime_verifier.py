#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import pathlib
import re
import stat
import subprocess
import sys
import unittest


SCHEMA_VERSION = 2
UINT32_MAX = 2**32 - 1
REPRODUCIBLE_FLAGS = (
    "-ffile-prefix-map=<SCRATCH_DIR>=/usr/src/echo-whisper-runtime "
    "-fmacro-prefix-map=<SCRATCH_DIR>=/usr/src/echo-whisper-runtime "
    "-fdebug-prefix-map=<SCRATCH_DIR>=/usr/src/echo-whisper-runtime"
)
PINNED_REVISIONS = {
    "v1.9.2": ("306c88f4d1286aec1bf96e544632897886af5501", 1785851811),
    "v1.9.3": ("371b5a7561823ab2bb32142d2751e35e7534727b", 1787219223),
}
REQUIRED_OPTIONS = {
    "BUILD_SHARED_LIBS": "ON",
    "CMAKE_BUILD_TYPE": "Release",
    "CMAKE_CXX_FLAGS": REPRODUCIBLE_FLAGS,
    "CMAKE_C_FLAGS": REPRODUCIBLE_FLAGS,
    "CMAKE_SKIP_RPATH": "ON",
    "GGML_BACKEND_DL": "ON",
    "GGML_CPU_ALL_VARIANTS": "ON",
    "GGML_NATIVE": "OFF",
    "GGML_VULKAN": "ON",
    "WHISPER_BUILD_EXAMPLES": "ON",
    "WHISPER_BUILD_SERVER": "OFF",
    "WHISPER_BUILD_TESTS": "OFF",
}
EXPECTED_CPU_VARIANTS = [
    "libggml-cpu-alderlake.so",
    "libggml-cpu-cannonlake.so",
    "libggml-cpu-cascadelake.so",
    "libggml-cpu-cooperlake.so",
    "libggml-cpu-haswell.so",
    "libggml-cpu-icelake.so",
    "libggml-cpu-ivybridge.so",
    "libggml-cpu-piledriver.so",
    "libggml-cpu-sandybridge.so",
    "libggml-cpu-sapphirerapids.so",
    "libggml-cpu-skylakex.so",
    "libggml-cpu-sse42.so",
    "libggml-cpu-x64.so",
    "libggml-cpu-zen4.so",
]
PRIVATE_LIBRARY = re.compile(r"^lib(?:ggml|whisper)(?:[-.]|$)")
CPU_LOAD = re.compile(r"loaded CPU backend from (.+/libggml-cpu-[^/\s]+\.so)")
VULKAN_LOAD = re.compile(r"loaded Vulkan backend from (.+/libggml-vulkan\.so)")
VERSIONED_LIBRARY = re.compile(
    r"^(?:libggml(?:-base)?\.so(?:\.0(?:\.\d+\.\d+)?)?|libwhisper\.so(?:\.1(?:\.\d+\.\d+)?)?)$"
)
FIXED_PACKAGE_FILES = {
    "cmake-cache.txt",
    "echo-whisper-runtime-probe",
    "libggml-vulkan.so",
    "whisper-cli",
    *EXPECTED_CPU_VARIANTS,
}
REQUIRED_REGULAR_FILES = FIXED_PACKAGE_FILES
CACHE_KEYS = sorted(
    set(REQUIRED_OPTIONS)
    | {
        "CMAKE_C_COMPILER",
        "CMAKE_CXX_COMPILER",
    }
)
ABI_SYMBOL = re.compile(r"\b(GLIBC|GLIBCXX|CXXABI)_([0-9]+(?:\.[0-9]+)+)\b")
EXPECTED_ELF_IDENTITY = {
    "architecture": "x86_64",
    "elfClass": "ELF64",
    "machine": "Advanced Micro Devices X86-64",
}
SYSTEM_VULKAN_ROOTS = [
    pathlib.Path(value)
    for value in [
        "/lib",
        "/usr/lib",
        "/lib64",
        "/usr/lib64",
        "/nix/store",
        "/run/current-system/sw/lib",
    ]
]


class VerificationError(RuntimeError):
    pass


def fail(message):
    raise VerificationError(message)


def strict_json_loads(raw):
    def reject_duplicate_keys(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                fail(f"duplicate JSON key: {key}")
            value[key] = item
        return value

    return json.loads(raw, object_pairs_hook=reject_duplicate_keys)


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def parse_cache(path):
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith(("#", "//")) or "=" not in line:
            continue
        key_and_type, value = line.split("=", 1)
        key = key_and_type.split(":", 1)[0]
        if key in values:
            fail(f"duplicate CMake cache key: {key}")
        values[key] = value
    return values


def validate_patch_counts(path):
    lines = path.read_text(encoding="utf-8").splitlines()
    index = 0
    hunks = 0
    while index < len(lines):
        header = re.match(
            r"@@ -(?:\d+)(?:,(\d+))? \+(?:\d+)(?:,(\d+))? @@", lines[index]
        )
        if not header:
            index += 1
            continue
        hunks += 1
        expected_old = int(header.group(1) or 1)
        expected_new = int(header.group(2) or 1)
        old_count = 0
        new_count = 0
        index += 1
        while index < len(lines) and not lines[index].startswith(
            ("@@ ", "diff --git ")
        ):
            line = lines[index]
            if line.startswith("+"):
                new_count += 1
            elif line.startswith("-"):
                old_count += 1
            elif line.startswith(" "):
                old_count += 1
                new_count += 1
            elif line == "":
                # Normalized repository patches omit the trailing space from
                # blank context lines; git apply still treats them as context.
                old_count += 1
                new_count += 1
            elif line.startswith("\\ No newline"):
                pass
            else:
                break
            index += 1
        if (old_count, new_count) != (expected_old, expected_new):
            fail(
                f"patch hunk count differs in {path.name}: "
                f"expected {expected_old}/{expected_new}, got {old_count}/{new_count}"
            )
    if hunks == 0:
        fail(f"patch has no unified-diff hunk: {path.name}")


def write_cache_contract(source, destination):
    cache = parse_cache(source)
    source_dir = pathlib.Path(cache.get("CMAKE_HOME_DIRECTORY", ""))
    if not source_dir.is_absolute() or source_dir.name != "source":
        fail("CMake cache does not identify the disposable source worktree")
    scratch_dir = str(source_dir.parent)
    missing = [key for key in CACHE_KEYS if key not in cache]
    if missing:
        fail(f"CMake cache is missing {', '.join(missing)}")
    destination.write_text(
        "".join(
            f"{key}={cache[key].replace(scratch_dir, '<SCRATCH_DIR>')}\n"
            for key in CACHE_KEYS
        ),
        encoding="utf-8",
    )


def normalized_files(package):
    files = []
    for path in sorted(package.iterdir(), key=lambda item: item.name):
        if path.name == "build-receipt.json":
            continue
        if path.is_symlink():
            files.append(
                {"path": path.name, "type": "symlink", "target": os.readlink(path)}
            )
        elif path.is_file():
            files.append(
                {
                    "mode": stat.S_IMODE(path.stat().st_mode),
                    "path": path.name,
                    "sha256": sha256_file(path),
                    "size": path.stat().st_size,
                    "type": "file",
                }
            )
        else:
            fail(f"package contains a non-file entry: {path.name}")
    return files


def normalize_package_modes(package):
    for path in package.iterdir():
        if path.is_symlink() or not path.is_file():
            continue
        path.chmod(
            0o644 if path.name in {"build-receipt.json", "cmake-cache.txt"} else 0o755
        )


def validate_package_filenames(files):
    for entry in files:
        name = entry["path"]
        if name.startswith("libvulkan.so"):
            fail("package must not contain the host-owned Vulkan loader")
        if name not in FIXED_PACKAGE_FILES and not VERSIONED_LIBRARY.fullmatch(name):
            fail(f"package contains an unexpected file: {name}")


def file_entries_by_path(files):
    if not isinstance(files, list):
        fail("package inventory is not an array")
    indexed = {}
    for entry in files:
        if not isinstance(entry, dict):
            fail("package inventory entry is not an object")
        entry_type = entry.get("type")
        expected_keys = (
            {"mode", "path", "sha256", "size", "type"}
            if entry_type == "file"
            else {"path", "target", "type"}
            if entry_type == "symlink"
            else set()
        )
        if set(entry) != expected_keys:
            fail("package inventory entry keys differ")
        name = entry["path"]
        if not isinstance(name, str) or not name or pathlib.PurePath(name).name != name:
            fail("package inventory path is invalid")
        if name in indexed:
            fail(f"duplicate package inventory path: {name}")
        if entry_type == "file":
            if (
                isinstance(entry["mode"], bool)
                or not isinstance(entry["mode"], int)
                or isinstance(entry["size"], bool)
                or not isinstance(entry["size"], int)
                or entry["size"] < 0
                or not isinstance(entry["sha256"], str)
                or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"])
            ):
                fail(f"package file metadata is invalid: {name}")
        elif (
            not isinstance(entry["target"], str)
            or not entry["target"]
            or pathlib.PurePath(entry["target"]).name != entry["target"]
        ):
            fail(f"package symlink target is invalid: {name}")
        indexed[name] = entry
    return indexed


def validate_runtime_inventory(files):
    indexed = file_entries_by_path(files)
    validate_package_filenames(files)
    missing = sorted(REQUIRED_REGULAR_FILES - set(indexed))
    if missing:
        fail(f"missing required regular file: {', '.join(missing)}")
    for name in sorted(REQUIRED_REGULAR_FILES):
        if indexed[name]["type"] != "file":
            fail(f"required runtime file is not regular: {name}")
    return indexed


def verify_builder_contract(path):
    text = path.read_text(encoding="utf-8")
    required = [
        "-DGGML_NATIVE=OFF",
        "-DGGML_BACKEND_DL=ON",
        "-DGGML_CPU_ALL_VARIANTS=ON",
        "-DGGML_VULKAN=ON",
        "-ffile-prefix-map=${scratch_dir}=",
        "-fmacro-prefix-map=${scratch_dir}=",
        "-fdebug-prefix-map=${scratch_dir}=",
        "--create",
        "--verify",
        "--revision-info",
    ]
    missing = [value for value in required if value not in text]
    if missing:
        fail(f"runtime builder contract is missing {', '.join(missing)}")


def artifact_id(files):
    encoded = json.dumps(
        files, ensure_ascii=True, separators=(",", ":"), sort_keys=True
    ).encode("ascii")
    return hashlib.sha256(b"echo-whisper-runtime-v1\0" + encoded).hexdigest()


def read_needed(path):
    output = subprocess.run(
        ["readelf", "-d", str(path)],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout
    return sorted(
        match.group(1)
        for line in output.splitlines()
        if (match := re.search(r"\(NEEDED\).*\[(.+)\]", line))
    )


def private_dependencies(package, files):
    dependencies = {}
    for entry in files:
        if entry["type"] != "file":
            continue
        path = package / entry["path"]
        try:
            needed = read_needed(path)
        except subprocess.CalledProcessError:
            continue
        private = [name for name in needed if PRIVATE_LIBRARY.match(name)]
        if private:
            dependencies[entry["path"]] = private
    return dependencies


def compiler_identity(cache, key):
    path = pathlib.Path(cache[key]).resolve()
    result = subprocess.run(
        [str(path), "--version"],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return {
        "path": str(path),
        "reportedVersionLine": result.stdout.splitlines()[0],
        "sha256": sha256_file(path),
    }


def version_key(value):
    return tuple(int(part) for part in value.split("."))


def parse_elf_header(output):
    fields = {}
    for line in output.splitlines():
        match = re.match(r"\s*(Class|Data|Machine):\s*(.+?)\s*$", line)
        if match:
            fields[match.group(1)] = match.group(2)
    if (
        fields.get("Class") != EXPECTED_ELF_IDENTITY["elfClass"]
        or fields.get("Data") != "2's complement, little endian"
        or fields.get("Machine") != EXPECTED_ELF_IDENTITY["machine"]
    ):
        fail(f"unsupported ELF identity: {fields}")
    return EXPECTED_ELF_IDENTITY.copy()


def read_elf_identity(path):
    output = subprocess.run(
        ["readelf", "-h", str(path)],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout
    return parse_elf_header(output)


def platform_abi(package, files):
    required = {}
    for path in elf_files(package, {"files": files}):
        output = subprocess.run(
            ["readelf", "--version-info", str(path)],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ).stdout
        for family, version in ABI_SYMBOL.findall(output):
            current = required.get(family)
            if current is None or version_key(version) > version_key(current):
                required[family] = version
    return EXPECTED_ELF_IDENTITY | {
        "minimumSymbolVersions": dict(sorted(required.items()))
    }


def require_fresh_receipt_outputs(package):
    for name in ["build-receipt.json", "cmake-cache.txt"]:
        path = package / name
        if path.exists() or path.is_symlink():
            fail(f"receipt output already exists: {name}")


def create_receipt(args):
    if args.package.is_symlink():
        fail("package directory must not be a symlink")
    package = args.package.resolve()
    if not package.is_dir():
        fail(f"package is not a directory: {package}")
    expected = PINNED_REVISIONS.get(args.revision)
    if expected != (args.commit, args.source_date_epoch):
        fail("revision, commit, and SOURCE_DATE_EPOCH are not the pinned tuple")

    require_fresh_receipt_outputs(package)
    cache_contract = package / "cmake-cache.txt"
    write_cache_contract(args.cmake_cache, cache_contract)
    normalize_package_modes(package)
    cache = parse_cache(cache_contract)
    files = normalized_files(package)
    validate_runtime_inventory(files)
    patches = []
    for path in sorted(args.patch):
        validate_patch_counts(path)
        patches.append(
            {
                "path": path.relative_to(args.repo_root).as_posix(),
                "sha256": sha256_file(path),
            }
        )
    variants = sorted(
        entry["path"]
        for entry in files
        if entry["type"] == "file"
        and re.fullmatch(r"libggml-cpu-[a-z0-9]+\.so", entry["path"])
    )
    receipt = {
        "artifactId": artifact_id(files),
        "cmake": {
            "cachePath": "cmake-cache.txt",
            "cacheSha256": sha256_file(cache_contract),
            "options": {key: cache[key] for key in sorted(REQUIRED_OPTIONS)},
        },
        "compiler": {
            "c": compiler_identity(cache, "CMAKE_C_COMPILER"),
            "cxx": compiler_identity(cache, "CMAKE_CXX_COMPILER"),
        },
        "files": files,
        "patches": patches,
        "platformAbi": platform_abi(package, files),
        "privateElfDependencies": private_dependencies(package, files),
        "reproducibility": {
            "pathMapping": "/usr/src/echo-whisper-runtime",
            "scope": "sameToolchain",
        },
        "runtime": {
            "cpuVariants": variants,
            "portable": True,
            "vulkanModule": "libggml-vulkan.so",
        },
        "schemaVersion": SCHEMA_VERSION,
        "source": {"commit": args.commit, "revision": args.revision},
        "sourceDateEpoch": args.source_date_epoch,
        "trustBoundary": "buildObservation",
    }
    (package / "build-receipt.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    normalize_package_modes(package)


def require_exact_keys(value, expected, context):
    actual = set(value) if isinstance(value, dict) else set()
    if actual != set(expected):
        fail(f"{context} keys differ: {sorted(actual ^ set(expected))}")


def require_private_resolution(package, resolved, artifact):
    if package.resolve() not in resolved.resolve().parents:
        fail(f"private library for {artifact} resolved outside the package: {resolved}")


def require_system_vulkan_loader(resolved):
    path = resolved.resolve()
    roots = [root.resolve() for root in SYSTEM_VULKAN_ROOTS if root.exists()]
    if not any(root == path or root in path.parents for root in roots):
        fail(f"Vulkan loader resolved outside an approved system library root: {path}")


def validate_contract(receipt, cache):
    require_exact_keys(
        receipt,
        {
            "artifactId",
            "cmake",
            "compiler",
            "files",
            "patches",
            "platformAbi",
            "privateElfDependencies",
            "reproducibility",
            "runtime",
            "schemaVersion",
            "source",
            "sourceDateEpoch",
            "trustBoundary",
        },
        "receipt",
    )
    if receipt["schemaVersion"] != SCHEMA_VERSION:
        fail("unsupported receipt schema")
    require_exact_keys(receipt["compiler"], {"c", "cxx"}, "compiler")
    for language in ["c", "cxx"]:
        observation = receipt["compiler"][language]
        require_exact_keys(
            observation,
            {"path", "reportedVersionLine", "sha256"},
            f"compiler.{language}",
        )
        if (
            not isinstance(observation["path"], str)
            or not pathlib.Path(observation["path"]).is_absolute()
        ):
            fail(f"compiler.{language}.path is invalid")
        if not isinstance(observation["sha256"], str) or not re.fullmatch(
            r"[0-9a-f]{64}", observation["sha256"]
        ):
            fail(f"compiler.{language}.sha256 is invalid")
        if (
            not isinstance(observation["reportedVersionLine"], str)
            or not observation["reportedVersionLine"]
        ):
            fail(f"compiler.{language}.reportedVersionLine is empty")
    require_exact_keys(receipt["source"], {"commit", "revision"}, "source")
    require_exact_keys(
        receipt["platformAbi"],
        {"architecture", "elfClass", "machine", "minimumSymbolVersions"},
        "platformAbi",
    )
    for key, expected in EXPECTED_ELF_IDENTITY.items():
        if receipt["platformAbi"][key] != expected:
            fail(f"platform ABI {key} differs")
    minimum_versions = receipt["platformAbi"]["minimumSymbolVersions"]
    if not isinstance(minimum_versions, dict):
        fail("platform ABI minimumSymbolVersions is not an object")
    for family, version in minimum_versions.items():
        if (
            family not in {"GLIBC", "GLIBCXX", "CXXABI"}
            or not isinstance(version, str)
            or not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", version)
        ):
            fail("platform ABI has an invalid symbol version")
    if receipt["reproducibility"] != {
        "pathMapping": "/usr/src/echo-whisper-runtime",
        "scope": "sameToolchain",
    }:
        fail("reproducibility scope differs")
    if receipt["trustBoundary"] != "buildObservation":
        fail("build receipt trust boundary differs")
    require_exact_keys(
        receipt["cmake"], {"cachePath", "cacheSha256", "options"}, "cmake"
    )
    revision = receipt["source"]["revision"]
    expected = PINNED_REVISIONS.get(revision)
    if expected != (receipt["source"]["commit"], receipt["sourceDateEpoch"]):
        fail("receipt does not name a pinned revision")
    for key, expected_value in REQUIRED_OPTIONS.items():
        if cache.get(key) != expected_value:
            fail(f"CMake cache requires {key}={expected_value}")
    if receipt["cmake"]["options"] != dict(sorted(REQUIRED_OPTIONS.items())):
        fail("receipt CMake options differ from the required build contract")
    require_exact_keys(
        receipt["runtime"], {"cpuVariants", "portable", "vulkanModule"}, "runtime"
    )
    variants = receipt["runtime"].get("cpuVariants")
    if not isinstance(variants, list) or any(
        not isinstance(variant, str) for variant in variants
    ):
        fail("CPU variants are invalid")
    if variants != EXPECTED_CPU_VARIANTS:
        missing = sorted(set(EXPECTED_CPU_VARIANTS) - set(variants or []))
        extra = sorted(set(variants or []) - set(EXPECTED_CPU_VARIANTS))
        fail(f"CPU variants differ; missing={missing}, extra={extra}")
    if receipt["runtime"] != {
        "cpuVariants": EXPECTED_CPU_VARIANTS,
        "portable": True,
        "vulkanModule": "libggml-vulkan.so",
    }:
        fail("runtime contract is not portable CPU plus Vulkan")
    validate_runtime_inventory(receipt["files"])
    for entry in receipt["files"]:
        if entry["type"] != "file":
            continue
        expected_mode = 0o644 if entry["path"] == "cmake-cache.txt" else 0o755
        if entry["mode"] != expected_mode:
            fail(f"staged file mode differs for {entry['path']}")


def receipt_cache_path(package, receipt):
    cmake = receipt.get("cmake")
    if not isinstance(cmake, dict) or cmake.get("cachePath") != "cmake-cache.txt":
        fail("receipt CMake cache path is invalid")
    path = package / "cmake-cache.txt"
    if (
        path.is_symlink()
        or not path.is_file()
        or path.parent.resolve() != package.resolve()
    ):
        fail("receipt CMake cache path is not a package-owned regular file")
    return path


def validate_receipt(package, repo_root):
    receipt_path = package / "build-receipt.json"
    if receipt_path.is_symlink():
        fail("build receipt must be a package-owned regular file")
    if receipt_path.parent.resolve() != package.resolve():
        fail("build receipt escapes the package")
    if receipt_path.is_file() and stat.S_IMODE(receipt_path.stat().st_mode) != 0o644:
        fail("build receipt mode differs from 0644")
    try:
        receipt = strict_json_loads(receipt_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"could not read build receipt: {error}")
    cache_path = receipt_cache_path(package, receipt)
    cache = parse_cache(cache_path)
    validate_contract(receipt, cache)
    if receipt["cmake"]["cacheSha256"] != sha256_file(cache_path):
        fail("CMake cache digest differs")
    files = normalized_files(package)
    if files != receipt["files"]:
        fail("staged files differ from the build receipt")
    for entry in files:
        if entry["type"] != "symlink":
            continue
        target = pathlib.Path(entry["target"])
        if target.is_absolute() or target.name != entry["target"]:
            fail(f"staged symlink target escapes the package: {entry['path']}")
        resolved = (package / entry["path"]).resolve()
        if package.resolve() not in resolved.parents:
            fail(f"staged symlink resolves outside the package: {entry['path']}")
    if receipt["artifactId"] != artifact_id(files):
        fail("runtime artifact ID differs")
    if receipt["privateElfDependencies"] != private_dependencies(package, files):
        fail("private ELF dependency receipt differs")
    if receipt["platformAbi"] != platform_abi(package, files):
        fail("platform ABI receipt differs")
    expected_patches = []
    for relative in [
        "patches/whisper.cpp/runtime-probe.patch",
        "patches/whisper.cpp/runtime-receipt.patch",
        "patches/whisper.cpp/runtime-selector.patch",
    ]:
        path = repo_root / relative
        validate_patch_counts(path)
        expected_patches.append({"path": relative, "sha256": sha256_file(path)})
    if receipt["patches"] != expected_patches:
        fail("patch digests differ from the receipt")
    return receipt


def elf_files(package, receipt):
    paths = []
    for entry in receipt["files"]:
        if entry["type"] != "file":
            continue
        path = package / entry["path"]
        if path.name == "cmake-cache.txt":
            continue
        try:
            read_elf_identity(path)
        except subprocess.CalledProcessError:
            fail(f"required runtime file is not an ELF binary: {path.name}")
        paths.append(path)
    return paths


def runtime_environment(package, base=None):
    source = os.environ if base is None else base
    environment = {
        key: value
        for key, value in source.items()
        if not key.startswith(("LD_", "VK_", "MESA_", "GGML_"))
        and key
        not in {
            "ECHO_WHISPER_VULKAN_DEVICE_UUID",
            "ECHO_WHISPER_VULKAN_DRIVER_UUID",
        }
    }
    environment["LD_LIBRARY_PATH"] = str(package)
    return environment


def verify_elf_resolution(package, receipt, require_vulkan):
    stage = package.resolve()
    environment = runtime_environment(stage)
    for path in elf_files(package, receipt):
        found_vulkan_loader = False
        dynamic = subprocess.run(
            ["readelf", "-d", str(path)],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout
        if re.search(r"\((?:RPATH|RUNPATH)\)", dynamic):
            fail(f"ELF has an RPATH or RUNPATH: {path.name}")
        if path.name == "libggml-vulkan.so" and not require_vulkan:
            continue
        output = subprocess.run(
            ["ldd", str(path)],
            check=True,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        ).stdout
        if "not found" in output:
            fail(f"ELF has an unresolved dependency: {path.name}")
        for line in output.splitlines():
            match = re.match(r"\s*(\S+) => (\S+)", line)
            if not match:
                continue
            if path.name == "libggml-vulkan.so" and match.group(1) == "libvulkan.so.1":
                require_system_vulkan_loader(pathlib.Path(match.group(2)))
                found_vulkan_loader = True
            if not PRIVATE_LIBRARY.match(match.group(1)):
                continue
            resolved = pathlib.Path(match.group(2)).resolve()
            require_private_resolution(stage, resolved, path.name)
        if path.name == "libggml-vulkan.so" and not found_vulkan_loader:
            fail("Vulkan module did not resolve the host-owned libvulkan.so.1")


def run_probe(package, *arguments, environment=None):
    command = [str(package / "echo-whisper-runtime-probe")]
    command.extend(arguments)
    return subprocess.run(
        command,
        env=runtime_environment(package) if environment is None else environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def validate_vulkan_receipt_value(receipt):
    require_exact_keys(
        receipt,
        {
            "apiVersion",
            "backend",
            "deviceId",
            "deviceUUID",
            "driverUUID",
            "driverVersion",
            "pipelineCacheUUID",
            "schemaVersion",
            "selectedIndex",
            "vendorId",
        },
        "Vulkan device receipt",
    )
    integer_fields = [
        "apiVersion",
        "deviceId",
        "driverVersion",
        "schemaVersion",
        "selectedIndex",
        "vendorId",
    ]
    if any(type(receipt[key]) is not int for key in integer_fields):
        fail("Vulkan runtime probe emitted invalid integer fields")
    if any(not 0 <= receipt[key] <= UINT32_MAX for key in integer_fields):
        fail("Vulkan runtime probe emitted a non-unsigned 32-bit integer")
    if receipt["schemaVersion"] != 1 or receipt["backend"] != "vulkan":
        fail("Vulkan runtime probe emitted an invalid receipt identity")
    if (
        receipt["vendorId"] <= 0
        or receipt["deviceId"] <= 0
        or receipt["apiVersion"] <= 0
    ):
        fail("Vulkan runtime probe emitted an invalid selected device")
    for key in ["deviceUUID", "driverUUID", "pipelineCacheUUID"]:
        if not isinstance(receipt[key], str) or not re.fullmatch(
            r"(?!0{32})[0-9a-f]{32}", receipt[key]
        ):
            fail(f"Vulkan runtime probe emitted an invalid {key}")
    return receipt


def validate_vulkan_receipt(stderr):
    prefix = "echo_whisper_runtime_receipt: "
    lines = [
        line[len(prefix) :] for line in stderr.splitlines() if line.startswith(prefix)
    ]
    if len(lines) != 1:
        fail(f"Vulkan runtime probe emitted {len(lines)} device receipts")
    return validate_vulkan_receipt_value(strict_json_loads(lines[0]))


def enumerate_vulkan_receipts(package):
    result = run_probe(package, "--list-vulkan-json")
    if result.returncode != 0:
        fail(f"Vulkan enumeration failed: {result.stderr.strip()}")
    prefix = "echo_whisper_vulkan_device: "
    receipts = [
        validate_vulkan_receipt_value(strict_json_loads(line[len(prefix) :]))
        for line in result.stdout.splitlines()
        if line.startswith(prefix)
    ]
    if not receipts or [receipt["selectedIndex"] for receipt in receipts] != list(
        range(len(receipts))
    ):
        fail("Vulkan enumeration is empty or has unstable indices")
    stable = [
        tuple(receipt[key] for key in ("deviceUUID", "driverUUID"))
        for receipt in receipts
    ]
    if len(stable) != len(set(stable)):
        fail("Vulkan enumeration has duplicate stable device identities")
    return receipts


def validate_cpu_probe(stderr, package, receipt, require_vulkan):
    selected_match = CPU_LOAD.search(stderr)
    if not selected_match:
        fail("CPU runtime probe did not report the selected CPU module")
    selected_path = pathlib.Path(selected_match.group(1)).resolve()
    if package.resolve() not in selected_path.parents:
        fail(f"CPU runtime probe loaded an external module: {selected_path}")
    selected = selected_path.name
    if selected not in receipt["runtime"]["cpuVariants"]:
        fail(f"CPU runtime probe selected an unreceipted variant: {selected}")
    vulkan_match = VULKAN_LOAD.search(stderr)
    if require_vulkan:
        if not vulkan_match:
            fail("runtime loader did not load the packaged Vulkan module")
        require_private_resolution(
            package, pathlib.Path(vulkan_match.group(1)), "runtime loader"
        )
    return selected


def verify_runtime_loading(package, receipt, require_vulkan):
    cpu = run_probe(package, "--cpu")
    if cpu.returncode != 0:
        fail(f"CPU runtime probe failed: {cpu.stderr.strip()}")
    selected_cpu = validate_cpu_probe(cpu.stderr, package, receipt, require_vulkan)
    help_result = subprocess.run(
        [str(package / "whisper-cli"), "--no-gpu", "--help"],
        env=runtime_environment(package),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if help_result.returncode != 0 or "--no-gpu" not in help_result.stdout:
        fail("whisper-cli does not expose the managed CPU --no-gpu switch")
    if require_vulkan:
        vulkan = run_probe(package, "--ready-vulkan")
        if vulkan.returncode != 0:
            fail(f"Vulkan runtime probe failed: {vulkan.stderr.strip()}")
        validate_vulkan_receipt(vulkan.stderr)
        enumerated = enumerate_vulkan_receipts(package)
        selected_vulkan = enumerated[0]
        selector_environment = runtime_environment(package)
        selector_environment["ECHO_WHISPER_VULKAN_DEVICE_UUID"] = selected_vulkan[
            "deviceUUID"
        ]
        selector_environment["ECHO_WHISPER_VULKAN_DRIVER_UUID"] = selected_vulkan[
            "driverUUID"
        ]
        ready = run_probe(package, "--ready-vulkan", environment=selector_environment)
        if ready.returncode != 0:
            fail(f"Vulkan UUID selection failed: {ready.stderr.strip()}")
        ready_receipt = validate_vulkan_receipt(ready.stderr)
        for key in (
            "apiVersion",
            "backend",
            "deviceId",
            "deviceUUID",
            "driverUUID",
            "driverVersion",
            "pipelineCacheUUID",
            "schemaVersion",
            "vendorId",
        ):
            if ready_receipt[key] != selected_vulkan[key]:
                fail("Vulkan UUID selection returned another stable device")
        if ready_receipt["selectedIndex"] != 0:
            fail("Vulkan UUID selection did not isolate one logical device")
    print(f"selectedCpuVariant={selected_cpu}")
    print("cpuBackendVerified=true")
    print("noGpuSwitchAvailable=true")
    if require_vulkan:
        print("vulkanBackendVerified=true")
        print("vulkanUuidSelectorVerified=true")
    print(f"artifactId={receipt['artifactId']}")
    print("portable=true")


def verify_package(args):
    package = args.package.resolve()
    if not package.is_dir():
        fail(f"package is not a directory: {package}")
    repo_root = pathlib.Path(__file__).resolve().parent.parent
    receipt = validate_receipt(package, repo_root)
    verify_elf_resolution(package, receipt, args.require_vulkan)
    verify_runtime_loading(package, receipt, args.require_vulkan)


def parser():
    result = argparse.ArgumentParser()
    subcommands = result.add_subparsers(dest="command", required=True)
    create = subcommands.add_parser("create", add_help=False)
    create.add_argument("package", type=pathlib.Path)
    create.add_argument("cmake_cache", type=pathlib.Path)
    create.add_argument("revision")
    create.add_argument("commit")
    create.add_argument("source_date_epoch", type=int)
    create.add_argument("repo_root", type=pathlib.Path)
    create.add_argument("patch", type=pathlib.Path, nargs="+")
    verify = subcommands.add_parser("verify", add_help=False)
    verify.add_argument("package", type=pathlib.Path)
    verify.add_argument("--require-vulkan", action="store_true")
    revision_info = subcommands.add_parser("revision-info", add_help=False)
    revision_info.add_argument("revision")
    subcommands.add_parser("self-test", add_help=False)
    return result


def main():
    arguments = sys.argv[1:]
    if arguments and arguments[0] in {
        "--create",
        "--revision-info",
        "--verify",
        "--self-test",
    }:
        arguments[0] = arguments[0][2:]
    args = parser().parse_args(arguments)
    if args.command == "create":
        create_receipt(args)
    elif args.command == "revision-info":
        value = PINNED_REVISIONS.get(args.revision)
        if value is None:
            fail(f"unsupported revision: {args.revision}")
        print(value[0], value[1])
    elif args.command == "verify":
        verify_package(args)
    else:
        from test_whisper_runtime_verifier import ContractTests

        suite = unittest.defaultTestLoader.loadTestsFromTestCase(ContractTests)
        outcome = unittest.TextTestRunner(verbosity=2).run(suite)
        if not outcome.wasSuccessful():
            return 1
        print("verify-whisper-vulkan-runtime: self-test ok")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except VerificationError as error:
        print(f"verify-whisper-vulkan-runtime: {error}", file=sys.stderr)
        sys.exit(2)
