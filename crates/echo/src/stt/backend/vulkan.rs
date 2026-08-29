use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use echo_core::WhisperVulkanReceipt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::super::whisper::enumerate_vulkan_runtime_receipts;
use super::super::whisper_accel_cache::{DriverIcdFingerprint, StableVulkanReceipt};
use super::super::whisper_identity::{Sha256Digest, UuidDigest};
use super::super::{probe_vulkan_runtime_receipt, VulkanRuntimeSelector, WhisperRuntimeLaunch};

const MAX_ICD_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
struct IcdManifest {
    #[serde(rename = "ICD")]
    icd: IcdRecord,
}

#[derive(Debug, Deserialize)]
struct IcdRecord {
    library_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalVulkanRoute {
    pub receipt: StableVulkanReceipt,
    pub selected_index: u32,
    pub fingerprint: DriverIcdFingerprint,
    pub manifest_path: PathBuf,
    pub library_path: PathBuf,
    pub selector: VulkanRuntimeSelector,
}

pub(crate) struct VulkanBackend {
    probe: PathBuf,
    base_launch: WhisperRuntimeLaunch,
    icd_directories: Vec<PathBuf>,
    drm_root: PathBuf,
    timeout: Duration,
}

impl VulkanBackend {
    pub(crate) fn system(
        probe: PathBuf,
        base_launch: WhisperRuntimeLaunch,
        timeout: Duration,
    ) -> Self {
        let mut icd_directories = Vec::new();
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            icd_directories.push(PathBuf::from(data_home).join("vulkan/icd.d"));
        }
        if let Some(data_dirs) = std::env::var_os("XDG_DATA_DIRS") {
            icd_directories
                .extend(std::env::split_paths(&data_dirs).map(|path| path.join("vulkan/icd.d")));
        }
        icd_directories.extend([
            PathBuf::from("/etc/vulkan/icd.d"),
            PathBuf::from("/usr/local/share/vulkan/icd.d"),
            PathBuf::from("/usr/share/vulkan/icd.d"),
        ]);
        Self {
            probe,
            base_launch,
            icd_directories,
            drm_root: PathBuf::from("/sys/class/drm"),
            timeout,
        }
    }

    pub(crate) fn enumerate(&self) -> Result<Vec<LocalVulkanRoute>, String> {
        let libraries = loader_libraries()?;
        let mut routes = Vec::new();
        for manifest_path in discover_manifests(&self.icd_directories)? {
            let Ok(library_path) = resolve_icd_library(&manifest_path, &libraries) else {
                continue;
            };
            let fingerprint = DriverIcdFingerprint {
                drm_driver: String::new(),
                icd_manifest_sha256: sha256(&manifest_path)?,
                icd_library_sha256: sha256(&library_path)?,
            };
            let mut launch = self.base_launch.clone();
            launch.vulkan_driver_files = Some(manifest_path.clone());
            launch.vulkan_selector = None;
            launch.mesa_shader_cache_dir = None;
            let observed =
                match enumerate_vulkan_runtime_receipts(&self.probe, &launch, self.timeout) {
                    Ok(observed) => observed,
                    Err(_) => continue,
                };
            for receipt in observed {
                let Ok(drm_driver) = drm_driver(&self.drm_root, &receipt) else {
                    continue;
                };
                let fingerprint = DriverIcdFingerprint {
                    drm_driver,
                    ..fingerprint.clone()
                };
                let stable = stable_receipt(&receipt)?;
                let selector =
                    VulkanRuntimeSelector::parse(receipt.device_uuid, receipt.driver_uuid)?;
                routes.push(LocalVulkanRoute {
                    receipt: stable,
                    selected_index: receipt.selected_index,
                    fingerprint,
                    manifest_path: manifest_path.clone(),
                    library_path: library_path.clone(),
                    selector,
                });
            }
        }
        routes.sort_by(|left, right| {
            (
                left.receipt.device_uuid.as_str(),
                left.receipt.driver_uuid.as_str(),
                left.receipt.pipeline_cache_uuid.as_str(),
                left.fingerprint.icd_manifest_sha256.as_str(),
                left.fingerprint.icd_library_sha256.as_str(),
            )
                .cmp(&(
                    right.receipt.device_uuid.as_str(),
                    right.receipt.driver_uuid.as_str(),
                    right.receipt.pipeline_cache_uuid.as_str(),
                    right.fingerprint.icd_manifest_sha256.as_str(),
                    right.fingerprint.icd_library_sha256.as_str(),
                ))
        });
        routes.dedup_by(|left, right| {
            left.receipt == right.receipt && left.fingerprint == right.fingerprint
        });
        if routes.is_empty() {
            return Err("no receipt-capable local Vulkan GPU was found".to_string());
        }
        Ok(routes)
    }

    pub(crate) fn ready(&self, route: &LocalVulkanRoute) -> Result<WhisperVulkanReceipt, String> {
        let mut launch = self.base_launch.clone();
        launch.vulkan_driver_files = Some(route.manifest_path.clone());
        launch.vulkan_selector = Some(route.selector.clone());
        launch.mesa_shader_cache_dir = None;
        let receipt = probe_vulkan_runtime_receipt(&self.probe, &launch, self.timeout)?;
        if stable_receipt(&receipt)? != route.receipt || receipt.selected_index != 0 {
            return Err("Vulkan ready receipt differs from the selected stable UUID".to_string());
        }
        Ok(receipt)
    }
}

fn discover_manifests(directories: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut manifests = BTreeSet::new();
    for directory in directories {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            let metadata = path.metadata().map_err(|error| error.to_string())?;
            if metadata.is_file() && metadata.len() <= MAX_ICD_MANIFEST_BYTES {
                manifests.insert(path);
            }
        }
    }
    Ok(manifests.into_iter().collect())
}

fn resolve_icd_library(
    manifest_path: &Path,
    loader_libraries: &BTreeMap<String, PathBuf>,
) -> Result<PathBuf, String> {
    let raw = fs::read(manifest_path).map_err(|error| error.to_string())?;
    let manifest: IcdManifest = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    if manifest.icd.library_path.is_empty() || manifest.icd.library_path.contains('\0') {
        return Err("Vulkan ICD manifest has an invalid library path".to_string());
    }
    let declared = Path::new(&manifest.icd.library_path);
    let path = if declared.is_absolute() {
        declared.to_path_buf()
    } else if declared.components().count() > 1 {
        manifest_path
            .parent()
            .ok_or_else(|| "Vulkan ICD manifest has no parent".to_string())?
            .join(declared)
    } else {
        loader_libraries
            .get(&manifest.icd.library_path)
            .cloned()
            .ok_or_else(|| "Vulkan ICD library is not in the loader cache".to_string())?
    };
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    if !path.is_file() {
        return Err("Vulkan ICD library is not a regular file".to_string());
    }
    Ok(path)
}

fn loader_libraries() -> Result<BTreeMap<String, PathBuf>, String> {
    let output = Command::new("ldconfig")
        .arg("-p")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("ldconfig could not list Vulkan ICD libraries".to_string());
    }
    let mut libraries = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((description, path)) = line.trim().split_once(" => ") else {
            continue;
        };
        let Some(name) = description.split_whitespace().next() else {
            continue;
        };
        let path = PathBuf::from(path);
        if name.starts_with("libvulkan_") && path.is_absolute() && path.is_file() {
            libraries.entry(name.to_string()).or_insert(path);
        }
    }
    Ok(libraries)
}

fn drm_driver(root: &Path, receipt: &WhisperVulkanReceipt) -> Result<String, String> {
    let mut drivers = BTreeSet::new();
    let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.file_name().to_string_lossy().starts_with("renderD") {
            continue;
        }
        let device = entry.path().join("device");
        if read_hex(&device.join("vendor")) != Some(receipt.vendor_id)
            || read_hex(&device.join("device")) != Some(receipt.device_id)
        {
            continue;
        }
        let driver = device
            .join("driver")
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let name = driver
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "DRM driver name is invalid".to_string())?;
        drivers.insert(name.to_string());
    }
    let drivers = drivers.into_iter().collect::<Vec<_>>();
    let [driver] = drivers.as_slice() else {
        return Err("Vulkan receipt does not map to one DRM driver".to_string());
    };
    Ok(driver.clone())
}

fn read_hex(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    u32::from_str_radix(raw.trim().trim_start_matches("0x"), 16).ok()
}

fn stable_receipt(receipt: &WhisperVulkanReceipt) -> Result<StableVulkanReceipt, String> {
    Ok(StableVulkanReceipt {
        backend: receipt.backend.clone(),
        vendor_id: receipt.vendor_id,
        device_id: receipt.device_id,
        api_version: receipt.api_version,
        driver_version: receipt.driver_version,
        device_uuid: UuidDigest::parse(receipt.device_uuid.clone())
            .map_err(|error| error.to_string())?,
        driver_uuid: UuidDigest::parse(receipt.driver_uuid.clone())
            .map_err(|error| error.to_string())?,
        pipeline_cache_uuid: UuidDigest::parse(receipt.pipeline_cache_uuid.clone())
            .map_err(|error| error.to_string())?,
    })
}

fn sha256(path: &Path) -> Result<Sha256Digest, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Sha256Digest::parse(format!("{:x}", hasher.finalize())).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    const DEVICE_UUID: &str = "8680a6460c0000000002000000000000";
    const DRIVER_UUID: &str = "ee99561e45e1e718c6121d36d8345582";
    const PIPELINE_UUID: &str = "35e9eb9761bf7afc9291ffc449ddf849";

    fn scratch() -> PathBuf {
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "echo-vulkan-backend-{}-{count}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn receipt(index: u32) -> String {
        format!(
            "{{\"schemaVersion\":1,\"backend\":\"vulkan\",\"selectedIndex\":{index},\"vendorId\":32902,\"deviceId\":18086,\"apiVersion\":4211006,\"driverVersion\":104865800,\"deviceUUID\":\"{DEVICE_UUID}\",\"driverUUID\":\"{DRIVER_UUID}\",\"pipelineCacheUUID\":\"{PIPELINE_UUID}\"}}"
        )
    }

    #[test]
    fn local_discovery_and_ready_follow_uuid() {
        let root = scratch();
        let icd = root.join("icd");
        fs::create_dir_all(&icd).unwrap();
        fs::write(icd.join("libfake.so"), b"library").unwrap();
        fs::write(
            icd.join("fake.json"),
            r#"{"ICD":{"library_path":"./libfake.so","api_version":"1.3.0"},"file_format_version":"1.0.1"}"#,
        )
        .unwrap();
        let drm = root.join("drm");
        let render = drm.join("renderD128/device");
        fs::create_dir_all(&render).unwrap();
        fs::write(render.join("vendor"), "0x8086\n").unwrap();
        fs::write(render.join("device"), "0x46a6\n").unwrap();
        let drivers = root.join("drivers/i915");
        fs::create_dir_all(&drivers).unwrap();
        symlink(&drivers, render.join("driver")).unwrap();

        let probe = root.join("probe");
        fs::write(
            &probe,
            format!(
                "#!/bin/sh\nif [ \"$1\" = --list-vulkan-json ]; then printf '%s\\n' 'echo_whisper_vulkan_device: {}'; exit 0; fi\n[ \"$ECHO_WHISPER_VULKAN_DEVICE_UUID\" = {DEVICE_UUID} ] || exit 6\n[ \"$ECHO_WHISPER_VULKAN_DRIVER_UUID\" = {DRIVER_UUID} ] || exit 7\nprintf '%s\\n' 'echo_whisper_runtime_receipt: {}' >&2\n",
                receipt(0),
                receipt(0),
            ),
        )
        .unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

        let backend = VulkanBackend {
            probe,
            base_launch: WhisperRuntimeLaunch::default(),
            icd_directories: vec![icd],
            drm_root: drm,
            timeout: Duration::from_secs(2),
        };
        let routes = backend.enumerate().unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].selected_index, 0);
        assert_eq!(routes[0].fingerprint.drm_driver, "i915");
        assert_eq!(backend.ready(&routes[0]).unwrap().selected_index, 0);
    }

    #[test]
    fn stable_receipt_ignores_diagnostic_index() {
        let first: WhisperVulkanReceipt = serde_json::from_str(&receipt(0)).unwrap();
        let second: WhisperVulkanReceipt = serde_json::from_str(&receipt(9)).unwrap();
        assert_eq!(
            stable_receipt(&first).unwrap(),
            stable_receipt(&second).unwrap()
        );
    }

    #[test]
    fn live_uuid_selector_when_probe_is_supplied() {
        let Some(probe) = std::env::var_os("ECHO_TEST_VULKAN_PROBE") else {
            return;
        };
        let probe = PathBuf::from(probe);
        let backend = VulkanBackend::system(
            probe.clone(),
            WhisperRuntimeLaunch {
                library_dir: probe.parent().map(Path::to_path_buf),
                ..WhisperRuntimeLaunch::default()
            },
            Duration::from_secs(15),
        );
        let routes = backend.enumerate().unwrap();
        assert!(!routes.is_empty());
        for route in routes {
            let ready = backend.ready(&route).unwrap();
            assert_eq!(ready.device_uuid, route.receipt.device_uuid.as_str());
            assert_eq!(ready.driver_uuid, route.receipt.driver_uuid.as_str());
            assert_eq!(ready.selected_index, 0);
        }
    }
}
