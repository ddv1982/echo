use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::num::NonZeroUsize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use echo_core::{WhisperRuntimeBackend, WhisperRuntimeSource, WhisperVulkanReceipt};
use fs2::FileExt;
use sha2::{Digest, Sha256};

use super::whisper_admission::{
    admission_state, AdmissionDeviceIdentity, AdmissionIdentity, AdmissionSet, AdmissionState,
    PackageEntry, PackageEntryKind,
};
use super::whisper_behavior::{RECEIPT_PROBE_TIMEOUT_SECS, VULKAN_RECEIPT_SCHEMA};
use super::{
    probe_vulkan_runtime_receipt, runtime_library_bindings, whisper_runtime_launch,
    WhisperExecutionPlan, WhisperPlanDecision, WhisperRuntimeCandidate, WhisperTuning,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileStamp {
    path: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

static FILE_DIGESTS: OnceLock<Mutex<HashMap<FileStamp, String>>> = OnceLock::new();
static PREFLIGHT_RECEIPTS: OnceLock<Mutex<HashMap<String, WhisperVulkanReceipt>>> = OnceLock::new();

pub(crate) struct ObservedWhisperHost {
    pub drm_vendor_id: u32,
    pub drm_device_id: u32,
    pub drm_driver: String,
}

pub(crate) struct PackageSelection<'a> {
    pub package_root: &'a Path,
    pub cache_root: &'a Path,
    pub echo_binary: &'a Path,
    pub echo_commit: &'a str,
    pub managed_cpu: WhisperExecutionPlan,
    pub host: ObservedWhisperHost,
    pub now: u64,
    pub require_package_ownership: bool,
    pub receipt_probe: fn(
        &Path,
        &super::WhisperRuntimeLaunch,
        std::time::Duration,
    ) -> Result<WhisperVulkanReceipt, String>,
}

pub(crate) fn select_qualified_package(
    selection: PackageSelection<'_>,
) -> Result<WhisperPlanDecision, String> {
    if selection.require_package_ownership {
        verify_package_ownership(selection.package_root)?;
    }
    let admission_path = selection.package_root.join("admission-set.json");
    let admission_bytes = fs::read(&admission_path).map_err(|error| error.to_string())?;
    let set = AdmissionSet::from_bytes(&admission_bytes)?;
    verify_inventory(selection.package_root, &set.inventory)?;
    let runtime = package_path(
        selection.package_root,
        &set.shared.runtime_relative_path,
        true,
    )?;
    let probe = package_path(
        selection.package_root,
        &set.shared.probe_relative_path,
        true,
    )?;
    if sha256_file(&probe)? != set.shared.probe_sha256 {
        return Err("Whisper runtime probe identity changed".to_string());
    }
    let model_sha256 = sha256_file(&selection.managed_cpu.model.path)?;
    let vad_sha256 = selection
        .managed_cpu
        .vad
        .as_deref()
        .map(sha256_file)
        .transpose()?;
    let runtime_launch = whisper_runtime_launch(&runtime);
    if runtime_library_bindings(&runtime).map_err(|error| error.to_string())?
        != set.shared.runtime_library_bindings
    {
        return Err("Whisper runtime library alias bindings changed".to_string());
    }
    for record in &set.records {
        let seed = package_path(
            selection.package_root,
            &record.cache_seed.relative_path,
            false,
        )?;
        if tree_sha256(&seed)? != record.cache_seed.sha256 {
            return Err("Whisper cache seed identity changed".to_string());
        }
    }
    let runtime_identity = runtime_launch
        .identity_sha256
        .clone()
        .ok_or_else(|| "Whisper Vulkan runtime has no composite identity".to_string())?;
    let echo_binary_sha256 = sha256_file(selection.echo_binary)?;
    let mut matches = Vec::new();
    for record in &set.records {
        if admission_state(record, &record.identity, None, selection.now) != AdmissionState::Passed
        {
            return Err("Whisper admission set contains an inactive record".to_string());
        }
        if record.identity.device.vendor_id != selection.host.drm_vendor_id
            || record.identity.device.device_id != selection.host.drm_device_id
            || record.identity.drm_driver != selection.host.drm_driver
        {
            continue;
        }
        let identity = AdmissionIdentity {
            schema_version: 1,
            echo_commit: selection.echo_commit.to_string(),
            echo_binary_sha256: echo_binary_sha256.clone(),
            runtime_identity_sha256: runtime_identity.clone(),
            model_sha256: model_sha256.clone(),
            vad_sha256: vad_sha256.clone(),
            protocol: "oneShotCli".to_string(),
            tuning: record.identity.tuning.clone(),
            language_policy: "pinned".to_string(),
            prompt_policy: "empty".to_string(),
            device: record.identity.device.clone(),
            drm_driver: selection.host.drm_driver.clone(),
            icd_manifest_sha256: sha256_file(Path::new(&record.icd_manifest_path))?,
            icd_library_sha256: sha256_file(Path::new(&record.icd_library_path))?,
            launch_contract_schema: 1,
        };
        if identity == record.identity {
            matches.push(record);
        }
    }
    let [record] = matches.as_slice() else {
        return Err("Whisper admission set requires exactly one full identity match".to_string());
    };
    let icd_manifest = PathBuf::from(&record.icd_manifest_path);
    let cache_seed = package_path(
        selection.package_root,
        &record.cache_seed.relative_path,
        false,
    )?;

    let cache = populate_cache_seed(
        selection.cache_root,
        record.identity_key.as_str(),
        &cache_seed,
        &record.cache_seed.sha256,
    )?;
    let tuning = WhisperTuning {
        threads: NonZeroUsize::new(usize::from(record.identity.tuning.threads)),
        beam_size: Some(record.identity.tuning.beam_size),
        best_of: Some(record.identity.tuning.best_of),
        no_fallback: Some(record.identity.tuning.no_fallback),
    };
    let mut primary = WhisperExecutionPlan::one_shot(
        WhisperRuntimeCandidate {
            source: WhisperRuntimeSource::Managed,
            backend: WhisperRuntimeBackend::Vulkan,
            cli: runtime,
            server: None,
            launch: runtime_launch,
        },
        selection.managed_cpu.model.clone(),
        selection.managed_cpu.vad.clone(),
    );
    primary.runtime.launch.vulkan_driver_files = Some(icd_manifest);
    primary.runtime.launch.mesa_shader_cache_dir = Some(cache);
    primary.tuning = tuning;
    primary.timeout = selection.managed_cpu.timeout;
    primary.allow_vad_retry = false;

    let expected_receipt = receipt(&record.identity.device);
    verify_live_receipt(
        record.identity_key.as_str(),
        &expected_receipt,
        &probe,
        &primary.runtime.launch,
        selection.receipt_probe,
    )?;

    let mut fallback = selection.managed_cpu;
    fallback.tuning = tuning;
    fallback.force_cpu = true;
    fallback.allow_vad_retry = false;
    WhisperPlanDecision::qualified(
        record.identity_key.clone(),
        primary,
        fallback,
        expected_receipt,
    )
}

fn verify_live_receipt(
    identity_key: &str,
    expected: &WhisperVulkanReceipt,
    probe: &Path,
    launch: &super::WhisperRuntimeLaunch,
    run_probe: fn(
        &Path,
        &super::WhisperRuntimeLaunch,
        std::time::Duration,
    ) -> Result<WhisperVulkanReceipt, String>,
) -> Result<(), String> {
    let receipts = PREFLIGHT_RECEIPTS.get_or_init(|| Mutex::new(HashMap::new()));
    if receipts
        .lock()
        .map_err(|_| "Whisper preflight receipt cache is unavailable".to_string())?
        .get(identity_key)
        == Some(expected)
    {
        return Ok(());
    }
    let observed = run_probe(
        probe,
        launch,
        std::time::Duration::from_secs(RECEIPT_PROBE_TIMEOUT_SECS),
    )?;
    if &observed != expected {
        return Err("live Vulkan receipt differs from admission".to_string());
    }
    receipts
        .lock()
        .map_err(|_| "Whisper preflight receipt cache is unavailable".to_string())?
        .insert(identity_key.to_string(), observed);
    Ok(())
}

pub(crate) fn production_whisper_decision(
    managed_cpu: WhisperExecutionPlan,
) -> Option<WhisperPlanDecision> {
    let echo_commit = crate::build_identity::qualified_commit()?;
    let echo_binary = std::env::current_exe().ok()?.canonicalize().ok()?;
    let package_root = package_root(&echo_binary)?;
    verify_package_ownership(&package_root).ok()?;
    let raw = fs::read(package_root.join("admission-set.json")).ok()?;
    let set = AdmissionSet::from_bytes(&raw).ok()?;
    let host = observed_host(&set)?;
    let cache_root = echo_core::data_dir().join("whisper-acceleration-cache");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    select_qualified_package(PackageSelection {
        package_root: &package_root,
        cache_root: &cache_root,
        echo_binary: &echo_binary,
        echo_commit,
        managed_cpu,
        host,
        now,
        require_package_ownership: false,
        receipt_probe: probe_vulkan_runtime_receipt,
    })
    .ok()
}

fn package_root(echo_binary: &Path) -> Option<PathBuf> {
    let parent = echo_binary.parent()?;
    let prefix = parent.parent()?;
    [
        parent.join("whisper-acceleration"),
        prefix.join("lib/echo/whisper-acceleration"),
        prefix.join("lib/io.github.ddv1982.echo/whisper-acceleration"),
    ]
    .into_iter()
    .find(|candidate| candidate.join("admission-set.json").is_file())
}

fn observed_host(set: &AdmissionSet) -> Option<ObservedWhisperHost> {
    for entry in fs::read_dir("/sys/class/drm").ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_name().to_string_lossy().starts_with("renderD") {
            continue;
        }
        let device = entry.path().join("device");
        let (Some(vendor_id), Some(device_id)) = (
            read_hex(&device.join("vendor")),
            read_hex(&device.join("device")),
        ) else {
            continue;
        };
        let Some(driver) = device.join("driver").canonicalize().ok().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        }) else {
            continue;
        };
        if set.records.iter().any(|record| {
            vendor_id == record.identity.device.vendor_id
                && device_id == record.identity.device.device_id
                && driver == record.identity.drm_driver
        }) {
            return Some(ObservedWhisperHost {
                drm_vendor_id: vendor_id,
                drm_device_id: device_id,
                drm_driver: driver,
            });
        }
    }
    None
}

fn read_hex(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    u32::from_str_radix(raw.trim().trim_start_matches("0x"), 16).ok()
}

fn receipt(value: &AdmissionDeviceIdentity) -> WhisperVulkanReceipt {
    WhisperVulkanReceipt {
        schema_version: VULKAN_RECEIPT_SCHEMA,
        backend: value.backend.clone(),
        selected_index: value.selected_index,
        vendor_id: value.vendor_id,
        device_id: value.device_id,
        api_version: value.api_version,
        driver_version: value.driver_version,
        device_uuid: value.device_uuid.clone(),
        driver_uuid: value.driver_uuid.clone(),
        pipeline_cache_uuid: value.pipeline_cache_uuid.clone(),
    }
}

fn package_path(root: &Path, relative: &str, file: bool) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.starts_with(&root) || (file && !path.is_file()) || (!file && !path.is_dir()) {
        return Err("Whisper package path escapes its trusted root".to_string());
    }
    Ok(path)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let before = file_stamp(path)?;
    let cache = FILE_DIGESTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(digest) = cache
        .lock()
        .map_err(|_| "Whisper digest cache lock is poisoned".to_string())?
        .get(&before)
        .cloned()
    {
        return Ok(digest);
    }
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
    let digest = format!("{:x}", hasher.finalize());
    if file_stamp(path)? != before {
        return Err("file changed while its Whisper identity was computed".to_string());
    }
    cache
        .lock()
        .map_err(|_| "Whisper digest cache lock is poisoned".to_string())?
        .insert(before, digest.clone());
    Ok(digest)
}

fn file_stamp(path: &Path) -> Result<FileStamp, String> {
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    Ok(FileStamp {
        path,
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    })
}

pub(crate) fn tree_sha256(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    if files.is_empty() {
        return Err("Whisper cache seed must contain files".to_string());
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    hasher.update(b"echo-whisper-tree-v1\0");
    for (relative, path) in files {
        let name = relative.as_bytes();
        hasher.update(u64::try_from(name.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(name);
        let size = path.metadata().map_err(|error| error.to_string())?.len();
        hasher.update(size.to_le_bytes());
        let mut file = File::open(path).map_err(|error| error.to_string())?;
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Whisper cache seed must not contain symlinks".to_string());
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            files.push((relative, path));
        } else {
            return Err("Whisper cache seed contains an unsupported entry".to_string());
        }
    }
    Ok(())
}

fn populate_cache_seed(
    cache_root: &Path,
    identity_key: &str,
    seed: &Path,
    expected_sha256: &str,
) -> Result<PathBuf, String> {
    fs::create_dir_all(cache_root).map_err(|error| error.to_string())?;
    let lock_path = cache_root.join("seed.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| error.to_string())?;
    lock.lock_exclusive().map_err(|error| error.to_string())?;
    let destination = cache_root.join(identity_key);
    let result = (|| {
        let marker = destination.join(".echo-seed-sha256");
        if fs::symlink_metadata(&destination)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("Whisper cache identity path is a symlink".to_string());
        }
        let marker_is_file =
            fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_file());
        if destination.is_dir()
            && marker_is_file
            && fs::read_to_string(&marker)
                .ok()
                .is_some_and(|value| value.trim() == expected_sha256)
        {
            return Ok(destination);
        }
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|error| error.to_string())?;
        }
        let stage = cache_root.join(format!(".{identity_key}.{}", std::process::id()));
        if stage.exists() {
            fs::remove_dir_all(&stage).map_err(|error| error.to_string())?;
        }
        copy_tree(seed, &stage)?;
        if tree_sha256(&stage)? != expected_sha256 {
            let _ = fs::remove_dir_all(&stage);
            return Err("copied Whisper cache seed changed identity".to_string());
        }
        fs::write(stage.join(".echo-seed-sha256"), expected_sha256)
            .map_err(|error| error.to_string())?;
        fs::rename(&stage, &destination).map_err(|error| error.to_string())?;
        Ok(destination)
    })();
    let unlock = FileExt::unlock(&lock).map_err(|error| error.to_string());
    match (result, unlock) {
        (Ok(path), Ok(())) => Ok(path),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|error| error.to_string())?;
        } else {
            return Err("Whisper cache seed contains an unsupported entry".to_string());
        }
    }
    Ok(())
}

fn verify_inventory(root: &Path, expected: &[PackageEntry]) -> Result<(), String> {
    let canonical_root = root.canonicalize().map_err(|error| error.to_string())?;
    let expected: BTreeMap<_, _> = expected
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut actual = BTreeMap::new();
    collect_inventory(&canonical_root, &canonical_root, &mut actual)?;
    if actual.len() != expected.len() {
        return Err("Whisper package inventory has missing or extra entries".to_string());
    }
    for (path, observed) in actual {
        let Some(entry) = expected.get(path.as_str()) else {
            return Err("Whisper package inventory contains an unlisted entry".to_string());
        };
        if &observed != *entry {
            return Err("Whisper package inventory identity changed".to_string());
        }
    }
    Ok(())
}

fn collect_inventory(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, PackageEntry>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if relative == "admission-set.json" {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            collect_inventory(root, &path, entries)?;
        } else if metadata.is_file() {
            entries.insert(
                relative.clone(),
                PackageEntry {
                    path: relative,
                    kind: PackageEntryKind::File,
                    bytes: metadata.len(),
                    sha256: Some(sha256_file(&path)?),
                    link_target: None,
                },
            );
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|error| error.to_string())?;
            let resolved = path
                .parent()
                .ok_or_else(|| "invalid package symlink".to_string())?
                .join(&target)
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !resolved.starts_with(root) {
                return Err("Whisper package inventory symlink escapes its root".to_string());
            }
            entries.insert(
                relative.clone(),
                PackageEntry {
                    path: relative,
                    kind: PackageEntryKind::Symlink,
                    bytes: metadata.len(),
                    sha256: None,
                    link_target: Some(
                        target
                            .to_string_lossy()
                            .replace(std::path::MAIN_SEPARATOR, "/"),
                    ),
                },
            );
        } else {
            return Err("Whisper package inventory contains an unsupported entry".to_string());
        }
    }
    Ok(())
}

fn verify_package_ownership(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    verify_owned_entry(&root, &root)?;
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        verify_owned_tree(&entry.map_err(|error| error.to_string())?.path(), &root)?;
    }
    Ok(())
}

fn verify_owned_tree(path: &Path, root: &Path) -> Result<(), String> {
    verify_owned_entry(path, root)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            verify_owned_tree(&entry.map_err(|error| error.to_string())?.path(), root)?;
        }
    }
    Ok(())
}

fn verify_owned_entry(path: &Path, root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.uid() != 0 || (!metadata.file_type().is_symlink() && metadata.mode() & 0o022 != 0) {
        return Err("Whisper acceleration package is not root-owned and read-only".to_string());
    }
    if metadata.file_type().is_symlink()
        && !path
            .canonicalize()
            .map_err(|error| error.to_string())?
            .starts_with(root)
    {
        return Err("Whisper acceleration package symlink escapes its root".to_string());
    }
    Ok(())
}

#[cfg(test)]
#[path = "whisper_acceleration_tests.rs"]
mod tests;
