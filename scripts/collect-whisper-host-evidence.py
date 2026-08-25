#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import platform
import shutil
import subprocess
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
TRUSTED_SYSTEM_PATH = "/usr/sbin:/usr/bin:/sbin:/bin"


def now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def portable_path(path: Path) -> str:
    resolved = path.resolve()
    for label, root in (("$REPO", REPO_ROOT), ("$HOME", Path.home())):
        try:
            return str(Path(label) / resolved.relative_to(root.resolve()))
        except ValueError:
            continue
    return str(resolved)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact(path: Path) -> dict[str, object]:
    resolved = path.resolve(strict=True)
    if not resolved.is_file():
        raise ValueError(f"artifact is not a regular file: {path}")
    return {
        "path": portable_path(resolved),
        "bytes": resolved.stat().st_size,
        "sha256": sha256(resolved),
    }


def write_atomic(path: Path, value: str) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        temporary.write_text(value, encoding="utf-8")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def read_optional(path: Path) -> str | None:
    try:
        value = path.read_text(encoding="utf-8").strip()
    except OSError:
        return None
    return value or None


def strict_json_object(path: Path) -> dict[str, object]:
    def reject_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
        value: dict[str, object] = {}
        for key, item in pairs:
            if key in value:
                raise ValueError(f"ICD manifest has duplicate key {key!r}: {path}")
            value[key] = item
        return value

    try:
        value = json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_pairs
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"could not parse ICD manifest {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"ICD manifest must be a JSON object: {path}")
    return value


def library_from_manifest(manifest: Path) -> tuple[str, Path, str]:
    value = strict_json_object(manifest)
    icd = value.get("ICD")
    if not isinstance(icd, dict):
        raise ValueError(f"ICD manifest has no ICD object: {manifest}")
    library_path = icd.get("library_path")
    if not isinstance(library_path, str) or not library_path:
        raise ValueError(f"ICD manifest has no library_path: {manifest}")
    candidate = Path(library_path)
    if candidate.is_absolute():
        try:
            return library_path, candidate.resolve(strict=True), "absolute"
        except OSError as error:
            raise ValueError(f"ICD library is missing: {library_path}") from error
    if "/" in library_path:
        try:
            return (
                library_path,
                (manifest.parent / candidate).resolve(strict=True),
                "manifest-relative",
            )
        except OSError as error:
            raise ValueError(
                f"manifest-relative ICD library is missing: {library_path}"
            ) from error
    ldconfig = shutil.which("ldconfig", path=TRUSTED_SYSTEM_PATH)
    if ldconfig is None:
        raise ValueError("could not find ldconfig in the trusted system PATH")
    completed = subprocess.run(
        [ldconfig, "-p"],
        check=False,
        capture_output=True,
        text=True,
        env={"LC_ALL": "C", "PATH": TRUSTED_SYSTEM_PATH},
    )
    if completed.returncode != 0:
        raise ValueError("ldconfig -p failed while resolving the ICD library")
    resolved_paths = set()
    for line in completed.stdout.splitlines():
        name, marker, target = line.strip().partition(" => ")
        if marker and name.split(" ", 1)[0] == library_path:
            try:
                resolved_paths.add(Path(target).resolve(strict=True))
            except OSError:
                continue
    if len(resolved_paths) != 1:
        raise ValueError(
            f"ICD library {library_path!r} did not resolve uniquely through ldconfig"
        )
    return library_path, resolved_paths.pop(), "ldconfig"


def default_icd_roots() -> list[Path]:
    config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    config_dirs = os.environ.get("XDG_CONFIG_DIRS", "/etc/xdg").split(os.pathsep)
    data_home = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    data_dirs = os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share").split(
        os.pathsep
    )
    roots = [
        config_home / "vulkan/icd.d",
        Path("/etc/vulkan/icd.d"),
        Path("/usr/local/etc/vulkan/icd.d"),
    ]
    roots.extend(
        Path(directory) / "vulkan/icd.d" for directory in config_dirs if directory
    )
    roots.append(data_home / "vulkan/icd.d")
    roots.extend(
        Path(directory) / "vulkan/icd.d" for directory in data_dirs if directory
    )
    unique: list[Path] = []
    for root in roots:
        if root not in unique:
            unique.append(root)
    return unique


def default_icd_enumeration() -> dict[str, object]:
    roots = default_icd_roots()
    manifests: list[dict[str, object]] = []
    seen: set[Path] = set()
    for root in roots:
        try:
            paths = sorted(root.glob("*.json"))
        except OSError:
            paths = []
        for path in paths:
            try:
                resolved = path.resolve(strict=True)
            except OSError:
                continue
            if resolved in seen or not resolved.is_file():
                continue
            seen.add(resolved)
            manifests.append(artifact(resolved))
    return {
        "searchRoots": [portable_path(root) for root in roots],
        "manifests": manifests,
    }


def drm_devices() -> list[dict[str, object]]:
    devices: list[dict[str, object]] = []
    for render in sorted(Path("/sys/class/drm").glob("renderD*")):
        device = render / "device"
        driver_link = device / "driver"
        try:
            driver = driver_link.resolve().name
        except OSError:
            driver = None
        devices.append(
            {
                "renderNode": f"/dev/dri/{render.name}",
                "vendor": read_optional(device / "vendor"),
                "device": read_optional(device / "device"),
                "revision": read_optional(device / "revision"),
                "driver": driver,
                "driverModuleVersion": read_optional(driver_link / "module/version"),
            }
        )
    return devices


def power_evidence() -> dict[str, object]:
    governors = []
    for path in sorted(
        Path("/sys/devices/system/cpu").glob("cpu*/cpufreq/scaling_governor")
    ):
        value = read_optional(path)
        if value is not None:
            governors.append({"cpu": path.parent.parent.name, "governor": value})
    supplies = []
    for supply in (
        sorted(Path("/sys/class/power_supply").iterdir())
        if Path("/sys/class/power_supply").is_dir()
        else []
    ):
        supplies.append(
            {
                "name": supply.name,
                "type": read_optional(supply / "type"),
                "online": read_optional(supply / "online"),
                "status": read_optional(supply / "status"),
                "capacity": read_optional(supply / "capacity"),
            }
        )
    return {
        "platformProfile": read_optional(Path("/sys/firmware/acpi/platform_profile")),
        "cpuGovernors": governors,
        "supplies": supplies,
    }


def memory_evidence() -> dict[str, str]:
    selected = {"MemTotal", "MemAvailable", "SwapTotal", "SwapFree"}
    values: dict[str, str] = {}
    try:
        lines = Path("/proc/meminfo").read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise ValueError(f"could not read /proc/meminfo: {error}") from error
    for line in lines:
        key, _, value = line.partition(":")
        if key in selected:
            values[key] = value.strip()
    if values.keys() != selected:
        raise ValueError("/proc/meminfo did not contain the required memory fields")
    return values


def selected_icd(vk_driver_files: Path | None) -> dict[str, object] | None:
    if vk_driver_files is None:
        return None
    raw = str(vk_driver_files)
    if os.pathsep in raw:
        raise ValueError("VK_DRIVER_FILES must name exactly one ICD manifest")
    try:
        manifest = vk_driver_files.resolve(strict=True)
    except OSError as error:
        raise ValueError(
            f"selected VK_DRIVER_FILES manifest is missing: {vk_driver_files}"
        ) from error
    if not manifest.is_file():
        raise ValueError(f"selected VK_DRIVER_FILES manifest is not a file: {manifest}")
    library_path, library, resolution = library_from_manifest(manifest)
    if not library.is_file():
        raise ValueError(f"resolved ICD library is not a file: {library}")
    return {
        "environmentVariable": "VK_DRIVER_FILES",
        "value": portable_path(manifest),
        "manifest": artifact(manifest),
        "libraryPath": library_path,
        "libraryResolution": resolution,
        "library": artifact(library),
    }


def collect_host_evidence(vk_driver_files: Path | None) -> dict[str, object]:
    if platform.system() != "Linux":
        raise ValueError("Whisper cache host evidence is supported only on Linux")
    boot_id = read_optional(Path("/proc/sys/kernel/random/boot_id"))
    if boot_id is None:
        raise ValueError("could not read /proc/sys/kernel/random/boot_id")
    uname = platform.uname()
    return {
        "schemaVersion": 1,
        "capturedAt": now(),
        "bootId": boot_id,
        "kernel": {
            "system": uname.system,
            "release": uname.release,
            "version": uname.version,
            "machine": uname.machine,
        },
        "drmDevices": drm_devices(),
        "power": power_evidence(),
        "memory": memory_evidence(),
        "loader": {
            "defaultIcdEnumeration": default_icd_enumeration(),
            "selectedIcd": selected_icd(vk_driver_files),
        },
    }


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="echo-host-evidence-") as temporary:
        root = Path(temporary)
        library = root / "libfake-vulkan.so"
        library.write_bytes(b"fake driver")
        manifest = root / "fake_icd.json"
        manifest.write_text(
            json.dumps({"ICD": {"library_path": str(library)}}), encoding="utf-8"
        )
        selected = selected_icd(manifest)
        assert selected is not None
        assert selected["environmentVariable"] == "VK_DRIVER_FILES"
        assert selected["library"] == artifact(library)
        try:
            selected_icd(Path(f"{manifest}{os.pathsep}{manifest}"))
        except ValueError as error:
            assert "exactly one" in str(error)
        else:
            raise AssertionError(
                "multiple VK_DRIVER_FILES manifests should be rejected"
            )
    default = collect_host_evidence(None)
    assert default["loader"]["selectedIcd"] is None
    assert isinstance(default["loader"]["defaultIcdEnumeration"]["manifests"], list)
    roots = {str(root) for root in default_icd_roots()}
    assert "/etc/vulkan/icd.d" in roots
    assert "/usr/local/etc/vulkan/icd.d" in roots
    trusted_ldconfig = shutil.which("ldconfig", path=TRUSTED_SYSTEM_PATH)
    assert trusted_ldconfig is not None
    assert Path(trusted_ldconfig).parent in {
        Path("/usr/sbin"),
        Path("/usr/bin"),
        Path("/sbin"),
        Path("/bin"),
    }
    print("whisper host evidence self-test passed")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--self-test", action="store_true")
    result.add_argument("--output", type=Path)
    result.add_argument(
        "--vk-driver-files",
        type=Path,
        help="the one explicitly selected ICD manifest to place in VK_DRIVER_FILES",
    )
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.output is None:
        raise ValueError("--output is required")
    if args.output.exists():
        raise ValueError(f"host evidence output already exists: {args.output}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    evidence = collect_host_evidence(args.vk_driver_files)
    write_atomic(args.output, json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(args.output)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"collect-whisper-host-evidence: {error}", file=os.sys.stderr)
        raise SystemExit(2) from error
