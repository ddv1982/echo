import json
import pathlib
import sys
import tempfile
import unittest

import whisper_runtime_verifier as verifier
from whisper_runtime_verifier import (
    EXPECTED_CPU_VARIANTS,
    FIXED_PACKAGE_FILES,
    PINNED_REVISIONS,
    REPRODUCIBLE_FLAGS,
    REQUIRED_OPTIONS,
    SCHEMA_VERSION,
    SYSTEM_VULKAN_ROOTS,
    VerificationError,
    compiler_identity,
    elf_files,
    parse_cache,
    parse_elf_header,
    receipt_cache_path,
    require_private_resolution,
    require_system_vulkan_loader,
    strict_json_loads,
    validate_contract,
    validate_cpu_probe,
    validate_package_filenames,
    validate_patch_counts,
    validate_receipt,
    validate_vulkan_receipt,
    verify_builder_contract,
    write_cache_contract,
)


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
                "c": {
                    "path": "/usr/bin/cc",
                    "reportedVersionLine": "cc 1",
                    "sha256": "c" * 64,
                },
                "cxx": {
                    "path": "/usr/bin/c++",
                    "reportedVersionLine": "c++ 1",
                    "sha256": "d" * 64,
                },
            },
            "files": [],
            "patches": [],
            "platformAbi": {
                "architecture": "x86_64",
                "elfClass": "ELF64",
                "machine": "Advanced Micro Devices X86-64",
                "minimumSymbolVersions": {},
            },
            "privateElfDependencies": {},
            "reproducibility": {
                "pathMapping": "/usr/src/echo-whisper-runtime",
                "scope": "sameToolchain",
            },
            "runtime": {
                "cpuVariants": EXPECTED_CPU_VARIANTS.copy(),
                "portable": True,
                "vulkanModule": "libggml-vulkan.so",
            },
            "schemaVersion": SCHEMA_VERSION,
            "source": {
                "commit": PINNED_REVISIONS["v1.9.2"][0],
                "revision": "v1.9.2",
            },
            "sourceDateEpoch": PINNED_REVISIONS["v1.9.2"][1],
            "trustBoundary": "buildObservation",
        }
        self.cache = REQUIRED_OPTIONS.copy()
        self.temp = tempfile.TemporaryDirectory()
        self.package = pathlib.Path(self.temp.name)
        (self.package / "libggml-vulkan.so").touch()

    def tearDown(self):
        self.temp.cleanup()

    def assert_rejected(self, text):
        with self.assertRaisesRegex(VerificationError, text):
            validate_contract(self.receipt, self.cache)

    def required_file_entries(self):
        return [
            {
                "mode": 0o644 if name == "cmake-cache.txt" else 0o755,
                "path": name,
                "sha256": "e" * 64,
                "size": 1,
                "type": "file",
            }
            for name in sorted(FIXED_PACKAGE_FILES)
        ]

    def test_rejects_native_build(self):
        self.cache["GGML_NATIVE"] = "ON"
        self.assert_rejected("GGML_NATIVE=OFF")

    def test_rejects_missing_baseline_variant(self):
        self.receipt["runtime"]["cpuVariants"].remove("libggml-cpu-x64.so")
        self.assert_rejected("libggml-cpu-x64.so")

    def test_rejects_missing_optimized_variant(self):
        self.receipt["runtime"]["cpuVariants"].remove("libggml-cpu-alderlake.so")
        self.assert_rejected("libggml-cpu-alderlake.so")

    def test_rejects_missing_required_runtime_file(self):
        self.receipt["files"] = [
            entry
            for entry in self.required_file_entries()
            if entry["path"] != "libggml-cpu-x64.so"
        ]
        self.assert_rejected("missing required regular file")

    def test_rejects_required_runtime_symlink(self):
        entries = self.required_file_entries()
        entries = [
            {
                "path": entry["path"],
                "target": "libggml-cpu-x64.so",
                "type": "symlink",
            }
            if entry["path"] == "libggml-vulkan.so"
            else entry
            for entry in entries
        ]
        self.receipt["files"] = entries
        self.assert_rejected("required runtime file is not regular")

    def test_rejects_duplicate_inventory_path(self):
        entries = self.required_file_entries()
        self.receipt["files"] = entries + [entries[-1].copy()]
        self.assert_rejected("duplicate package inventory path")

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
        self.assertTrue(identity["reportedVersionLine"])

    def test_rejects_truncated_patch_hunk(self):
        patch = self.package / "truncated.patch"
        patch.write_text(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1,1 @@\n-old\n+new\n+extra\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(VerificationError, "hunk count differs"):
            validate_patch_counts(patch)

    def test_rejects_patch_without_a_hunk(self):
        patch = self.package / "not-a-patch.patch"
        patch.write_text("this is not a patch\n", encoding="utf-8")
        with self.assertRaisesRegex(VerificationError, "no unified-diff hunk"):
            validate_patch_counts(patch)

    def test_rejects_duplicate_json_keys(self):
        with self.assertRaisesRegex(VerificationError, "duplicate JSON key"):
            strict_json_loads('{"schemaVersion":1,"schemaVersion":2}')

    def test_rejects_invalid_cmake_cache_path_type(self):
        with self.assertRaisesRegex(VerificationError, "CMake cache path"):
            receipt_cache_path(self.package, {"cmake": {"cachePath": []}})

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
        root = next(value for value in SYSTEM_VULKAN_ROOTS if value.exists())
        require_system_vulkan_loader(root / "libvulkan.so.1")

    def test_builder_declares_the_portable_contract(self):
        verify_builder_contract(
            pathlib.Path(__file__).with_name("build-whisper-vulkan-receipt.sh")
        )

    def test_builder_requires_vulkan_verification(self):
        builder = (
            pathlib.Path(__file__)
            .with_name("build-whisper-vulkan-receipt.sh")
            .read_text(encoding="utf-8")
        )
        self.assertIn(
            '"${runtime_verifier}" --verify --require-vulkan "${stage_dir}"',
            builder,
        )

    def test_builder_accepts_tagless_pinned_source(self):
        builder = (
            pathlib.Path(__file__)
            .with_name("build-whisper-vulkan-receipt.sh")
            .read_text(encoding="utf-8")
        )
        self.assertNotIn('rev-parse "${revision}^{commit}"', builder)

    def test_revision_info_is_authoritative(self):
        self.assertEqual(
            PINNED_REVISIONS["v1.9.2"],
            ("306c88f4d1286aec1bf96e544632897886af5501", 1785851811),
        )

    def test_cpu_probe_does_not_require_vulkan(self):
        cpu = self.package / "libggml-cpu-x64.so"
        cpu.touch()
        stderr = f"load_backend: loaded CPU backend from {cpu}\n"
        self.assertEqual(
            validate_cpu_probe(stderr, self.package, self.receipt, False),
            cpu.name,
        )
        with self.assertRaisesRegex(VerificationError, "did not load"):
            validate_cpu_probe(stderr, self.package, self.receipt, True)

    def test_vulkan_receipt_rejects_boolean_integers(self):
        receipt = {
            "apiVersion": True,
            "backend": "vulkan",
            "deviceId": True,
            "deviceUUID": "1" * 32,
            "driverUUID": "2" * 32,
            "driverVersion": True,
            "pipelineCacheUUID": "3" * 32,
            "schemaVersion": 1,
            "selectedIndex": False,
            "vendorId": True,
        }
        stderr = "echo_whisper_runtime_receipt: " + json.dumps(receipt)
        with self.assertRaisesRegex(VerificationError, "integer fields"):
            validate_vulkan_receipt(stderr)

    def test_vulkan_receipt_rejects_out_of_range_integers(self):
        receipt = {
            "apiVersion": 1,
            "backend": "vulkan",
            "deviceId": 1,
            "deviceUUID": "1" * 32,
            "driverUUID": "2" * 32,
            "driverVersion": 1,
            "pipelineCacheUUID": "3" * 32,
            "schemaVersion": 1,
            "selectedIndex": 2**32,
            "vendorId": 1,
        }
        stderr = "echo_whisper_runtime_receipt: " + json.dumps(receipt)
        with self.assertRaisesRegex(VerificationError, "unsigned 32-bit"):
            validate_vulkan_receipt(stderr)

    def test_rejects_symlinked_build_receipt(self):
        outside = self.package / "outside.json"
        outside.write_text("{}", encoding="utf-8")
        (self.package / "build-receipt.json").symlink_to(outside.name)
        with self.assertRaisesRegex(VerificationError, "regular file"):
            validate_receipt(self.package, pathlib.Path(__file__).parent.parent)

    def test_create_rejects_preexisting_output_symlinks(self):
        outside = self.package / "outside.txt"
        outside.write_text("unchanged\n", encoding="utf-8")
        (self.package / "cmake-cache.txt").symlink_to(outside.name)
        with self.assertRaisesRegex(VerificationError, "already exists"):
            verifier.require_fresh_receipt_outputs(self.package)
        self.assertEqual(outside.read_text(encoding="utf-8"), "unchanged\n")

    def test_runtime_environment_strips_loader_injection(self):
        environment = verifier.runtime_environment(
            self.package,
            {
                "LD_AUDIT": "audit.so",
                "LD_LIBRARY_PATH": "/outside",
                "LD_PRELOAD": "preload.so",
                "VK_DRIVER_FILES": "/driver.json",
                "MESA_SHADER_CACHE_DIR": "/shader-cache",
                "GGML_VK_VISIBLE_DEVICES": "1",
                "ECHO_WHISPER_VULKAN_DEVICE_UUID": "1" * 32,
                "ECHO_WHISPER_VULKAN_DRIVER_UUID": "2" * 32,
            },
        )
        self.assertEqual(environment["LD_LIBRARY_PATH"], str(self.package))
        for name in (
            "ECHO_WHISPER_VULKAN_DEVICE_UUID",
            "ECHO_WHISPER_VULKAN_DRIVER_UUID",
            "GGML_VK_VISIBLE_DEVICES",
            "LD_AUDIT",
            "LD_PRELOAD",
            "MESA_SHADER_CACHE_DIR",
            "VK_DRIVER_FILES",
        ):
            self.assertNotIn(name, environment)

    def test_rejects_invalid_platform_abi(self):
        self.receipt["platformAbi"]["architecture"] = "aarch64"
        self.assert_rejected("architecture")

    def test_parses_x86_64_elf_header(self):
        identity = parse_elf_header(
            "Class:                             ELF64\n"
            "Data:                              2's complement, little endian\n"
            "Machine:                           Advanced Micro Devices X86-64\n"
        )
        self.assertEqual(
            identity,
            {
                "architecture": "x86_64",
                "elfClass": "ELF64",
                "machine": "Advanced Micro Devices X86-64",
            },
        )

    def test_rejects_non_x86_64_elf_header(self):
        with self.assertRaisesRegex(VerificationError, "ELF identity"):
            parse_elf_header(
                "Class:                             ELF64\n"
                "Data:                              2's complement, little endian\n"
                "Machine:                           AArch64\n"
            )

    def test_rejects_non_elf_runtime_file(self):
        runtime = self.package / "libggml-cpu-x64.so"
        runtime.touch()
        with self.assertRaisesRegex(VerificationError, "not an ELF"):
            elf_files(
                self.package,
                {"files": [{"path": runtime.name, "type": "file"}]},
            )
