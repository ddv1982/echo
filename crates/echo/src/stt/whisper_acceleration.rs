use std::collections::HashMap;
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
    admission_state_from_bytes, AdmissionDeviceIdentity, AdmissionIdentity, AdmissionRecord,
    AdmissionState,
};
use super::{
    whisper_runtime_launch, WhisperExecutionPlan, WhisperPlanDecision, WhisperRuntimeCandidate,
    WhisperTuning,
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
}

pub(crate) fn select_qualified_package(
    selection: PackageSelection<'_>,
) -> Result<WhisperPlanDecision, String> {
    if selection.require_package_ownership {
        verify_package_ownership(selection.package_root)?;
    }
    let admission_path = selection.package_root.join("admission.json");
    let admission_bytes = fs::read(&admission_path).map_err(|error| error.to_string())?;
    if admission_bytes.len() > 1024 * 1024 {
        return Err("Whisper admission record exceeds 1 MiB".to_string());
    }
    let record: AdmissionRecord =
        serde_json::from_slice(&admission_bytes).map_err(|error| error.to_string())?;
    let runtime = package_path(
        selection.package_root,
        &record.artifacts.runtime_relative_path,
        true,
    )?;
    let cache_seed = package_path(
        selection.package_root,
        &record.artifacts.cache_seed_relative_path,
        false,
    )?;
    if tree_sha256(&cache_seed)? != record.artifacts.cache_seed_sha256 {
        return Err("Whisper cache seed identity changed".to_string());
    }
    let icd_manifest = PathBuf::from(&record.artifacts.icd_manifest_path);
    let icd_library = PathBuf::from(&record.artifacts.icd_library_path);
    let model_sha256 = sha256_file(&selection.managed_cpu.model.path)?;
    let vad_sha256 = selection
        .managed_cpu
        .vad
        .as_deref()
        .map(sha256_file)
        .transpose()?;
    let runtime_launch = whisper_runtime_launch(&runtime);
    let runtime_identity = runtime_launch
        .identity_sha256
        .clone()
        .ok_or_else(|| "Whisper Vulkan runtime has no composite identity".to_string())?;
    let identity = AdmissionIdentity {
        schema_version: 1,
        echo_commit: selection.echo_commit.to_string(),
        echo_binary_sha256: sha256_file(selection.echo_binary)?,
        runtime_identity_sha256: runtime_identity,
        model_sha256,
        vad_sha256,
        protocol: "oneShotCli".to_string(),
        tuning: record.identity.tuning.clone(),
        language_policy: "pinned".to_string(),
        prompt_policy: "empty".to_string(),
        device: record.identity.device.clone(),
        drm_driver: selection.host.drm_driver,
        icd_manifest_sha256: sha256_file(&icd_manifest)?,
        icd_library_sha256: sha256_file(&icd_library)?,
        launch_contract_schema: 1,
    };
    if identity.device.vendor_id != selection.host.drm_vendor_id
        || identity.device.device_id != selection.host.drm_device_id
    {
        return Err("Whisper admission does not match the active DRM device".to_string());
    }
    if admission_state_from_bytes(
        &identity,
        Some(&admission_bytes),
        None,
        selection.now,
    ) != AdmissionState::Passed
    {
        return Err("Whisper admission record did not pass exact selection".to_string());
    }

    let cache = populate_cache_seed(
        selection.cache_root,
        record.identity_key.as_str(),
        &cache_seed,
        &record.artifacts.cache_seed_sha256,
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

    let mut fallback = selection.managed_cpu;
    fallback.tuning = tuning;
    fallback.force_cpu = true;
    WhisperPlanDecision::qualified(
        record.identity_key,
        primary,
        fallback,
        receipt(&record.identity.device),
    )
}

pub(crate) fn production_whisper_decision(
    managed_cpu: WhisperExecutionPlan,
) -> Option<WhisperPlanDecision> {
    let echo_commit = option_env!("ECHO_BUILD_COMMIT")?;
    let echo_binary = std::env::current_exe().ok()?.canonicalize().ok()?;
    let package_root = package_root(&echo_binary)?;
    verify_package_ownership(&package_root).ok()?;
    let raw = fs::read(package_root.join("admission.json")).ok()?;
    let record: AdmissionRecord = serde_json::from_slice(&raw).ok()?;
    let host = observed_host(&record)?;
    let cache_root = echo_core::data_dir().join("whisper-acceleration-cache");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    select_qualified_package(PackageSelection {
        package_root: &package_root,
        cache_root: &cache_root,
        echo_binary: &echo_binary,
        echo_commit,
        managed_cpu,
        host,
        now,
        require_package_ownership: true,
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
    .find(|candidate| candidate.join("admission.json").is_file())
}

fn observed_host(record: &AdmissionRecord) -> Option<ObservedWhisperHost> {
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
        let Some(driver) = device
            .join("driver")
            .canonicalize()
            .ok()
            .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        else {
            continue;
        };
        if vendor_id == record.identity.device.vendor_id
            && device_id == record.identity.device.device_id
            && driver == record.identity.drm_driver
        {
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
        schema_version: 1,
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
        if destination.is_dir() && tree_sha256(&destination)? == expected_sha256 {
            return Ok(destination.clone());
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

fn verify_package_ownership(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    verify_owned_entry(&root, &root)?;
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        verify_owned_tree(
            &entry.map_err(|error| error.to_string())?.path(),
            &root,
        )?;
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
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::stt::{
        AdmissionArtifacts, AdmissionGates, AdmissionIdentityKey, AdmissionTuning,
        AdmissionVerdict, WhisperModelAsset, WhisperProtocol,
    };

    const NOW: u64 = 2_000_000_000;

    fn scratch(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "echo-whisper-package-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn executable(path: &Path, body: &[u8]) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn gates() -> AdmissionGates {
        AdmissionGates {
            complete_pairs: true,
            pair_integrity: true,
            sample_size: true,
            backend_truth: true,
            identity_match: true,
            hardware_device: true,
            median_reduction: true,
            median_speedup: true,
            p95_improved: true,
            per_language_quality: true,
            no_new_hallucinations: true,
            receipt_consistency: true,
            coverage_complete: true,
            cache_evidence: true,
            reset_evidence: true,
            driver_icd_identity: true,
            clean_child_environment: true,
            exact_runtime: true,
        }
    }

    fn device() -> AdmissionDeviceIdentity {
        AdmissionDeviceIdentity {
            backend: "vulkan".to_string(),
            selected_index: 0,
            vendor_id: 0x8086,
            device_id: 0x46a6,
            api_version: 4_211_006,
            driver_version: 104_865_800,
            device_uuid: "8680a6460c0000000002000000000000".to_string(),
            driver_uuid: "ee99561e45e1e718c6121d36d8345582".to_string(),
            pipeline_cache_uuid: "35e9eb9761bf7afc9291ffc449ddf849".to_string(),
        }
    }

    struct Fixture {
        package: PathBuf,
        cache: PathBuf,
        echo: PathBuf,
        runtime: PathBuf,
        cpu_plan: WhisperExecutionPlan,
    }

    fn fixture(label: &str) -> Fixture {
        let root = scratch(label);
        let package = root.join("package");
        let runtime_dir = package.join("runtime");
        let seed = package.join("cache-seed/mesa_shader_cache");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::create_dir_all(&seed).unwrap();
        let runtime = runtime_dir.join("whisper-cli");
        let cpu = root.join("whisper-cpu");
        let echo = root.join("echo-desktop");
        let model = root.join("ggml-small.bin");
        let vad = root.join("ggml-silero.bin");
        let icd_manifest = root.join("intel_icd.json");
        let icd_library = root.join("libvulkan_intel.so");
        executable(&runtime, b"vulkan runtime");
        executable(&cpu, b"cpu runtime");
        executable(&echo, b"echo binary");
        fs::write(&model, b"small model").unwrap();
        fs::write(&vad, b"vad model").unwrap();
        fs::write(&icd_manifest, b"icd manifest").unwrap();
        fs::write(&icd_library, b"icd library").unwrap();
        fs::write(seed.join("index"), b"shader cache").unwrap();
        let tuning = AdmissionTuning {
            threads: 4,
            beam_size: 3,
            best_of: 5,
            no_fallback: false,
        };
        let identity = AdmissionIdentity {
            schema_version: 1,
            echo_commit: "4".repeat(40),
            echo_binary_sha256: sha256_file(&echo).unwrap(),
            runtime_identity_sha256: whisper_runtime_launch(&runtime)
                .identity_sha256
                .unwrap(),
            model_sha256: sha256_file(&model).unwrap(),
            vad_sha256: Some(sha256_file(&vad).unwrap()),
            protocol: "oneShotCli".to_string(),
            tuning,
            language_policy: "pinned".to_string(),
            prompt_policy: "empty".to_string(),
            device: device(),
            drm_driver: "i915".to_string(),
            icd_manifest_sha256: sha256_file(&icd_manifest).unwrap(),
            icd_library_sha256: sha256_file(&icd_library).unwrap(),
            launch_contract_schema: 1,
        };
        let identity_key = AdmissionIdentityKey::for_identity(&identity);
        let record = AdmissionRecord {
            schema_version: 1,
            identity,
            identity_key,
            evidence_sha256: "a".repeat(64),
            gates: gates(),
            verdict: AdmissionVerdict::Passed,
            accepted_at: NOW - 60,
            expires_at: NOW + 60,
            artifacts: AdmissionArtifacts {
                runtime_relative_path: "runtime/whisper-cli".to_string(),
                icd_manifest_path: icd_manifest.to_string_lossy().into_owned(),
                icd_library_path: icd_library.to_string_lossy().into_owned(),
                cache_seed_relative_path: "cache-seed".to_string(),
                cache_seed_sha256: tree_sha256(&package.join("cache-seed")).unwrap(),
            },
        };
        fs::write(
            package.join("admission.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        let cpu_plan = WhisperExecutionPlan {
            runtime: WhisperRuntimeCandidate {
                source: WhisperRuntimeSource::Managed,
                backend: WhisperRuntimeBackend::Cpu,
                launch: whisper_runtime_launch(&cpu),
                cli: cpu,
                server: None,
            },
            model: WhisperModelAsset {
                name: "small".to_string(),
                path: model.clone(),
                multilingual: true,
            },
            vad: Some(vad),
            tuning: WhisperTuning::runtime_defaults(),
            protocol: WhisperProtocol::OneShotCli,
            force_cpu: false,
            timeout: std::time::Duration::from_secs(60),
        };
        Fixture {
            package,
            cache: root.join("cache"),
            echo,
            runtime,
            cpu_plan,
        }
    }

    fn selection<'a>(fixture: &'a Fixture, host: ObservedWhisperHost) -> PackageSelection<'a> {
        PackageSelection {
            package_root: &fixture.package,
            cache_root: &fixture.cache,
            echo_binary: &fixture.echo,
            echo_commit: "4444444444444444444444444444444444444444",
            managed_cpu: fixture.cpu_plan.clone(),
            host,
            now: NOW,
            require_package_ownership: false,
        }
    }

    #[test]
    fn exact_package_selects_vulkan_and_seeds_its_identity_cache() {
        let fixture = fixture("pass");
        let decision = select_qualified_package(selection(
            &fixture,
            ObservedWhisperHost {
                drm_vendor_id: 0x8086,
                drm_device_id: 0x46a6,
                drm_driver: "i915".to_string(),
            },
        ))
        .unwrap();
        assert!(matches!(
            decision,
            WhisperPlanDecision::QualifiedAccelerator(_)
        ));
        assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 2);
    }

    #[test]
    fn changed_runtime_hardware_and_untrusted_package_fail_closed() {
        let changed_runtime = fixture("runtime-change");
        fs::write(&changed_runtime.runtime, b"changed runtime").unwrap();
        assert!(select_qualified_package(selection(
            &changed_runtime,
            ObservedWhisperHost {
                drm_vendor_id: 0x8086,
                drm_device_id: 0x46a6,
                drm_driver: "i915".to_string(),
            },
        ))
        .is_err());

        let changed_host = fixture("host-change");
        assert!(select_qualified_package(selection(
            &changed_host,
            ObservedWhisperHost {
                drm_vendor_id: 0x1002,
                drm_device_id: 0x46a6,
                drm_driver: "amdgpu".to_string(),
            },
        ))
        .is_err());

        let untrusted = fixture("untrusted");
        let mut untrusted_selection = selection(
            &untrusted,
            ObservedWhisperHost {
                drm_vendor_id: 0x8086,
                drm_device_id: 0x46a6,
                drm_driver: "i915".to_string(),
            },
        );
        untrusted_selection.require_package_ownership = true;
        assert!(select_qualified_package(untrusted_selection).is_err());
    }

    #[test]
    fn tree_identity_is_order_stable_and_content_bound() {
        let root = scratch("tree");
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("b/two"), b"two").unwrap();
        fs::write(root.join("one"), b"one").unwrap();
        let first = tree_sha256(&root).unwrap();
        let second = tree_sha256(&root).unwrap();
        assert_eq!(first, second);
        fs::write(root.join("one"), b"changed").unwrap();
        assert_ne!(first, tree_sha256(&root).unwrap());
    }
}
