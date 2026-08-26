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
import tempfile
import unittest


SCHEMA_VERSION = 1
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
    "build-receipt.json",
    "cmake-cache.txt",
    "echo-whisper-runtime-probe",
    "libggml-vulkan.so",
    "whisper-cli",
    *EXPECTED_CPU_VARIANTS,
}
CACHE_KEYS = sorted(
    set(REQUIRED_OPTIONS)
    | {
        "CMAKE_C_COMPILER",
        "CMAKE_CXX_COMPILER",
    }
)


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
    while index < len(lines):
        header = re.match(
            r"@@ -(?:\d+)(?:,(\d+))? \+(?:\d+)(?:,(\d+))? @@", lines[index]
        )
        if not header:
            index += 1
            continue
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
        "sha256": sha256_file(path),
        "version": result.stdout.splitlines()[0],
    }


def create_receipt(args):
    package = args.package.resolve()
    if not package.is_dir():
        fail(f"package is not a directory: {package}")
    expected = PINNED_REVISIONS.get(args.revision)
    if expected != (args.commit, args.source_date_epoch):
        fail("revision, commit, and SOURCE_DATE_EPOCH are not the pinned tuple")

    cache_contract = package / "cmake-cache.txt"
    write_cache_contract(args.cmake_cache, cache_contract)
    normalize_package_modes(package)
    cache = parse_cache(cache_contract)
    files = normalized_files(package)
    validate_package_filenames(files)
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
        "privateElfDependencies": private_dependencies(package, files),
        "runtime": {
            "cpuVariants": variants,
            "portable": True,
            "vulkanModule": "libggml-vulkan.so",
        },
        "schemaVersion": SCHEMA_VERSION,
        "source": {"commit": args.commit, "revision": args.revision},
        "sourceDateEpoch": args.source_date_epoch,
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
    roots = [pathlib.Path("/lib"), pathlib.Path("/usr/lib")]
    if not any(root == path or root in path.parents for root in roots):
        fail(f"Vulkan loader resolved outside an approved system library root: {path}")


def validate_contract(receipt, cache, package):
    require_exact_keys(
        receipt,
        {
            "artifactId",
            "cmake",
            "compiler",
            "files",
            "patches",
            "privateElfDependencies",
            "runtime",
            "schemaVersion",
            "source",
            "sourceDateEpoch",
        },
        "receipt",
    )
    if receipt["schemaVersion"] != SCHEMA_VERSION:
        fail("unsupported receipt schema")
    require_exact_keys(receipt["compiler"], {"c", "cxx"}, "compiler")
    for language in ["c", "cxx"]:
        require_exact_keys(
            receipt["compiler"][language],
            {"path", "sha256", "version"},
            f"compiler.{language}",
        )
        if not re.fullmatch(r"[0-9a-f]{64}", receipt["compiler"][language]["sha256"]):
            fail(f"compiler.{language}.sha256 is invalid")
        if not receipt["compiler"][language]["version"]:
            fail(f"compiler.{language}.version is empty")
    require_exact_keys(receipt["source"], {"commit", "revision"}, "source")
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
    variants = receipt["runtime"].get("cpuVariants")
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
    if not (package / "libggml-vulkan.so").is_file():
        fail("Vulkan backend module is missing")
    validate_package_filenames(receipt["files"])
    for entry in receipt["files"]:
        if entry["type"] != "file":
            continue
        expected_mode = 0o644 if entry["path"] == "cmake-cache.txt" else 0o755
        if entry["mode"] != expected_mode:
            fail(f"staged file mode differs for {entry['path']}")


def validate_receipt(package, repo_root):
    receipt_path = package / "build-receipt.json"
    if receipt_path.is_file() and stat.S_IMODE(receipt_path.stat().st_mode) != 0o644:
        fail("build receipt mode differs from 0644")
    try:
        receipt = strict_json_loads(receipt_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"could not read build receipt: {error}")
    cache_path = package / receipt.get("cmake", {}).get("cachePath", "")
    if cache_path != package / "cmake-cache.txt" or not cache_path.is_file():
        fail("receipt does not bind the packaged CMake cache")
    cache = parse_cache(cache_path)
    validate_contract(receipt, cache, package)
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
    expected_patches = []
    for relative in [
        "patches/whisper.cpp/runtime-probe.patch",
        "patches/whisper.cpp/runtime-receipt.patch",
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
        result = subprocess.run(
            ["readelf", "-h", str(path)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if result.returncode == 0:
            paths.append(path)
    return paths


def verify_elf_resolution(package, receipt):
    stage = package.resolve()
    environment = os.environ | {"LD_LIBRARY_PATH": str(stage)}
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


def run_probe(package, cpu):
    command = [str(package / "echo-whisper-runtime-probe")]
    if cpu:
        command.append("--cpu")
    return subprocess.run(
        command,
        env=os.environ | {"LD_LIBRARY_PATH": str(package)},
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def validate_vulkan_receipt(stderr):
    prefix = "echo_whisper_runtime_receipt: "
    lines = [
        line[len(prefix) :] for line in stderr.splitlines() if line.startswith(prefix)
    ]
    if len(lines) != 1:
        fail(f"Vulkan runtime probe emitted {len(lines)} device receipts")
    receipt = strict_json_loads(lines[0])
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
    if receipt["schemaVersion"] != 1 or receipt["backend"] != "vulkan":
        fail("Vulkan runtime probe emitted an invalid receipt identity")
    if (
        receipt["selectedIndex"] < 0
        or receipt["vendorId"] <= 0
        or receipt["deviceId"] <= 0
    ):
        fail("Vulkan runtime probe emitted an invalid selected device")
    for key in ["deviceUUID", "driverUUID", "pipelineCacheUUID"]:
        if not re.fullmatch(r"(?!0{32})[0-9a-f]{32}", receipt[key]):
            fail(f"Vulkan runtime probe emitted an invalid {key}")


def verify_runtime_loading(package, receipt, require_vulkan):
    cpu = run_probe(package, True)
    if cpu.returncode != 0:
        fail(f"CPU runtime probe failed: {cpu.stderr.strip()}")
    selected_match = CPU_LOAD.search(cpu.stderr)
    if not selected_match:
        fail("CPU runtime probe did not report the selected CPU module")
    selected_path = pathlib.Path(selected_match.group(1)).resolve()
    if package.resolve() not in selected_path.parents:
        fail(f"CPU runtime probe loaded an external module: {selected_path}")
    selected = selected_path.name
    if selected not in receipt["runtime"]["cpuVariants"]:
        fail(f"CPU runtime probe selected an unreceipted variant: {selected}")
    vulkan_match = VULKAN_LOAD.search(cpu.stderr)
    if not vulkan_match:
        fail("runtime loader did not load the packaged Vulkan module")
    require_private_resolution(
        package, pathlib.Path(vulkan_match.group(1)), "runtime loader"
    )
    help_result = subprocess.run(
        [str(package / "whisper-cli"), "--no-gpu", "--help"],
        env=os.environ | {"LD_LIBRARY_PATH": str(package)},
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if help_result.returncode != 0 or "--no-gpu" not in help_result.stdout:
        fail("whisper-cli does not expose the managed CPU --no-gpu switch")
    if require_vulkan:
        vulkan = run_probe(package, False)
        if vulkan.returncode != 0:
            fail(f"Vulkan runtime probe failed: {vulkan.stderr.strip()}")
        validate_vulkan_receipt(vulkan.stderr)
    print(f"selectedCpuVariant={selected}")
    print("selectedBackend=cpu")
    print("gpuDisabled=true")
    print(f"artifactId={receipt['artifactId']}")
    print("portable=true")


def verify_package(args):
    package = args.package.resolve()
    if not package.is_dir():
        fail(f"package is not a directory: {package}")
    repo_root = pathlib.Path(__file__).resolve().parent.parent
    receipt = validate_receipt(package, repo_root)
    verify_elf_resolution(package, receipt)
    verify_runtime_loading(package, receipt, args.require_vulkan)


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.receipt = {
            "artifactId": "a" * 64,
            "cmake": {
                "cachePath": "cmake-cache.txt",
                "cacheSha256": "b" * 64,
                "options": dict(sorted(REQUIRED_OPTIONS.items())),
            },
            "compiler": {
                "c": {"path": "/usr/bin/cc", "sha256": "c" * 64, "version": "cc 1"},
                "cxx": {
                    "path": "/usr/bin/c++",
                    "sha256": "d" * 64,
                    "version": "c++ 1",
                },
            },
            "files": [],
            "patches": [],
            "privateElfDependencies": {},
            "runtime": {
                "cpuVariants": EXPECTED_CPU_VARIANTS.copy(),
                "portable": True,
                "vulkanModule": "libggml-vulkan.so",
            },
            "schemaVersion": 1,
            "source": {
                "commit": PINNED_REVISIONS["v1.9.2"][0],
                "revision": "v1.9.2",
            },
            "sourceDateEpoch": PINNED_REVISIONS["v1.9.2"][1],
        }
        self.cache = REQUIRED_OPTIONS.copy()
        self.temp = tempfile.TemporaryDirectory()
        self.package = pathlib.Path(self.temp.name)
        (self.package / "libggml-vulkan.so").touch()

    def tearDown(self):
        self.temp.cleanup()

    def assert_rejected(self, text):
        with self.assertRaisesRegex(VerificationError, text):
            validate_contract(self.receipt, self.cache, self.package)

    def test_rejects_native_build(self):
        self.cache["GGML_NATIVE"] = "ON"
        self.assert_rejected("GGML_NATIVE=OFF")

    def test_rejects_missing_baseline_variant(self):
        self.receipt["runtime"]["cpuVariants"].remove("libggml-cpu-x64.so")
        self.assert_rejected("libggml-cpu-x64.so")

    def test_rejects_missing_optimized_variant(self):
        self.receipt["runtime"]["cpuVariants"].remove("libggml-cpu-alderlake.so")
        self.assert_rejected("libggml-cpu-alderlake.so")

    def test_rejects_unpinned_revision(self):
        self.receipt["source"]["commit"] = "0" * 40
        self.assert_rejected("pinned revision")

    def test_rejects_external_private_library(self):
        outside = pathlib.Path("/usr/lib/libggml.so")
        with self.assertRaisesRegex(VerificationError, "outside the package"):
            require_private_resolution(self.package, outside, "fixture")

    def test_accepts_real_cmake_cache_shape(self):
        raw = self.package / "CMakeCache.txt"
        options = REQUIRED_OPTIONS | {
            "CMAKE_C_FLAGS": REPRODUCIBLE_FLAGS.replace(
                "<SCRATCH_DIR>", str(self.package)
            ),
            "CMAKE_CXX_FLAGS": REPRODUCIBLE_FLAGS.replace(
                "<SCRATCH_DIR>", str(self.package)
            ),
        }
        raw.write_text(
            "".join(
                [
                    *(f"{key}:BOOL={value}\n" for key, value in options.items()),
                    f"CMAKE_C_COMPILER:FILEPATH={sys.executable}\n",
                    f"CMAKE_CXX_COMPILER:FILEPATH={sys.executable}\n",
                    f"CMAKE_HOME_DIRECTORY:INTERNAL={self.package / 'source'}\n",
                ]
            ),
            encoding="utf-8",
        )
        contract = self.package / "cmake-cache.txt"
        write_cache_contract(raw, contract)
        cache = parse_cache(contract)
        self.assertEqual(cache["CMAKE_C_FLAGS"], REPRODUCIBLE_FLAGS)
        identity = compiler_identity(cache, "CMAKE_C_COMPILER")
        self.assertRegex(identity["sha256"], r"^[0-9a-f]{64}$")
        self.assertTrue(identity["version"])

    def test_rejects_truncated_patch_hunk(self):
        patch = self.package / "truncated.patch"
        patch.write_text(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1,1 @@\n-old\n+new\n+extra\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(VerificationError, "hunk count differs"):
            validate_patch_counts(patch)

    def test_rejects_duplicate_json_keys(self):
        with self.assertRaisesRegex(VerificationError, "duplicate JSON key"):
            strict_json_loads('{"schemaVersion":1,"schemaVersion":2}')

    def test_rejects_duplicate_cmake_cache_keys(self):
        cache = self.package / "duplicate-cache.txt"
        cache.write_text("GGML_NATIVE=OFF\nGGML_NATIVE=ON\n", encoding="utf-8")
        with self.assertRaisesRegex(VerificationError, "duplicate CMake cache key"):
            parse_cache(cache)

    def test_rejects_bundled_or_external_vulkan_loader(self):
        with self.assertRaisesRegex(VerificationError, "host-owned Vulkan loader"):
            validate_package_filenames(
                [{"path": "libvulkan.so.1", "type": "file", "mode": 0o755}]
            )
        with self.assertRaisesRegex(VerificationError, "approved system library root"):
            require_system_vulkan_loader(self.package / "libvulkan.so.1")


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
    subcommands.add_parser("self-test", add_help=False)
    return result


def main():
    arguments = sys.argv[1:]
    if arguments and arguments[0] in {"--create", "--verify", "--self-test"}:
        arguments[0] = arguments[0][2:]
    args = parser().parse_args(arguments)
    if args.command == "create":
        create_receipt(args)
    elif args.command == "verify":
        verify_package(args)
    else:
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
