pub mod catalog;
mod download;
mod extract;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use catalog::{ComponentId, SetupPlanId};
pub use download::required_free_bytes;
pub use download::{
    DiskSpace, HttpRequest, HttpResponse, HttpTransport, SystemDisk, UreqTransport,
};

use catalog::{archive_component, component, ArtifactFormat, PayloadKind};
use download::{download_verified, forget_partial, DownloadSpec};
use extract::{extract_archive, ExtractFile, ExtractionPlan};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(String);

impl OperationId {
    #[must_use]
    pub fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        Self(format!(
            "{nanos:x}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    fn fixture(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallPhase {
    CheckingDisk,
    Downloading,
    Verifying,
    Extracting,
    Activating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub operation_id: OperationId,
    pub component: ComponentId,
    pub phase: InstallPhase,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub resumed_from_bytes: u64,
}

impl InstallProgress {
    fn new(
        operation_id: &OperationId,
        component: ComponentId,
        phase: InstallPhase,
        received_bytes: u64,
        total_bytes: u64,
        resumed_from_bytes: u64,
    ) -> Self {
        Self {
            operation_id: operation_id.clone(),
            component,
            phase,
            received_bytes,
            total_bytes,
            resumed_from_bytes,
        }
    }
}

#[derive(Debug)]
pub enum InstallError {
    Unsupported(String),
    Busy,
    InsufficientSpace { required: u64, available: u64 },
    Http(String),
    Range(String),
    Interrupted { received: u64, expected: u64 },
    Sha256Mismatch { expected: String, actual: String },
    UnsafeArchive(String),
    Payload(String),
    State(String),
    Io(std::io::Error),
    IoMessage(String),
    Cancelled,
    Probe(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message)
            | Self::Http(message)
            | Self::Range(message)
            | Self::UnsafeArchive(message)
            | Self::Payload(message)
            | Self::State(message)
            | Self::IoMessage(message)
            | Self::Probe(message) => formatter.write_str(message),
            Self::Busy => formatter.write_str("another managed component operation is active"),
            Self::InsufficientSpace {
                required,
                available,
            } => write!(
                formatter,
                "setup needs {required} bytes free, but {available} bytes are available"
            ),
            Self::Interrupted { received, expected } => write!(
                formatter,
                "download stopped at {received} of {expected} bytes and can be resumed"
            ),
            Self::Sha256Mismatch { expected, actual } => write!(
                formatter,
                "SHA-256 mismatch: expected {expected}, got {actual}"
            ),
            Self::Io(error) => write!(formatter, "disk error: {error}"),
            Self::Cancelled => formatter.write_str("setup cancelled; downloaded bytes were kept"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledFile {
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
    pub mode: u32,
    pub kind: PayloadKind,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationRecord {
    pub schema_version: u32,
    pub component: ComponentId,
    pub version: String,
    pub release: String,
    pub artifact_sha256: String,
    pub files: Vec<InstalledFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ManagedComponentState {
    Absent {
        resumable_bytes: u64,
    },
    Ready {
        version: String,
        bytes: u64,
        root: String,
    },
    NeedsRepair {
        reason: String,
        resumable_bytes: u64,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct ManagedStore {
    root: PathBuf,
}

impl ManagedStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn managed(&self) -> PathBuf {
        self.root.join("managed")
    }

    fn active_path(&self, id: ComponentId) -> PathBuf {
        self.managed()
            .join("active")
            .join(format!("{}.json", id.as_str()))
    }

    fn component_dir(&self, id: ComponentId) -> PathBuf {
        self.managed().join("components").join(id.as_str())
    }

    fn lock_path(&self, id: ComponentId) -> PathBuf {
        self.managed()
            .join("locks")
            .join(format!("{}.lock", id.as_str()))
    }

    fn operation_lock_path(&self) -> PathBuf {
        self.managed().join("locks").join("operations.lock")
    }

    fn repair_path(&self, id: ComponentId) -> PathBuf {
        self.managed()
            .join("repair")
            .join(format!("{}.txt", id.as_str()))
    }

    fn open_lock(&self, id: ComponentId) -> Result<fs::File, InstallError> {
        let path = self.lock_path(id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?)
    }

    fn open_operation_lock(&self) -> Result<fs::File, InstallError> {
        let path = self.operation_lock_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?)
    }

    fn operation_shared(&self) -> Result<ComponentLease, InstallError> {
        let file = self.open_operation_lock()?;
        FileExt::lock_shared(&file).map_err(InstallError::Io)?;
        Ok(ComponentLease { file })
    }

    fn mark_needs_repair(&self, id: ComponentId, error: &InstallError) {
        let path = self.repair_path(id);
        let _ = echo_core::write_atomic(&path, error.to_string().as_bytes());
    }

    fn clear_repair_marker(&self, id: ComponentId) -> Result<(), InstallError> {
        match fs::remove_file(self.repair_path(id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn repair_reason(&self, id: ComponentId) -> Option<String> {
        fs::read_to_string(self.repair_path(id)).ok()
    }

    pub fn lease_shared(&self, id: ComponentId) -> Result<ComponentLease, InstallError> {
        let file = self.open_lock(id)?;
        FileExt::lock_shared(&file).map_err(InstallError::Io)?;
        Ok(ComponentLease { file })
    }

    fn lock_exclusive(&self, id: ComponentId) -> Result<ComponentLease, InstallError> {
        let file = self.open_lock(id)?;
        FileExt::lock_exclusive(&file).map_err(InstallError::Io)?;
        Ok(ComponentLease { file })
    }

    fn read_active(&self, id: ComponentId) -> Result<Option<ActivationRecord>, InstallError> {
        let path = self.active_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(path)?;
        let record: ActivationRecord = serde_json::from_slice(&raw)
            .map_err(|error| InstallError::State(format!("invalid activation record: {error}")))?;
        if record.component != id || record.schema_version != 1 {
            return Err(InstallError::State(
                "activation record identity mismatch".to_string(),
            ));
        }
        Ok(Some(record))
    }

    #[cfg(test)]
    fn active_root(&self, id: ComponentId) -> Result<Option<PathBuf>, InstallError> {
        let Some(record) = self.read_active(id)? else {
            return Ok(None);
        };
        validate_release_name(id, &record.release)?;
        let root = self
            .component_dir(id)
            .join("releases")
            .join(&record.release);
        ensure_contained(&self.component_dir(id), &root)?;
        Ok(Some(root.join("payload")))
    }

    pub fn candidate_root(&self, id: ComponentId) -> Option<PathBuf> {
        if !matches!(self.status(id, false), ManagedComponentState::Ready { .. }) {
            return None;
        }
        self.read_active(id).ok().flatten().and_then(|record| {
            validate_release_name(id, &record.release).ok()?;
            Some(
                self.component_dir(id)
                    .join("releases")
                    .join(record.release)
                    .join("payload"),
            )
        })
    }

    pub fn active_root_leased(&self, id: ComponentId) -> Result<Option<ManagedPath>, InstallError> {
        let lease = self.lease_shared(id)?;
        let Some(record) = self.read_active(id)? else {
            return Ok(None);
        };
        validate_release_name(id, &record.release)?;
        let root = self
            .component_dir(id)
            .join("releases")
            .join(&record.release);
        ensure_contained(&self.component_dir(id), &root)?;
        if let Err(error) = verify_receipt(&root, &record)
            .and_then(|_| verify_payload_cached(&root.join("payload"), &expected_files(id), false))
        {
            self.mark_needs_repair(id, &error);
            return Err(error);
        }
        Ok(Some(ManagedPath {
            root: root.join("payload"),
            lease,
        }))
    }

    pub fn status(&self, id: ComponentId, full_verify: bool) -> ManagedComponentState {
        self.status_with(component(id), &expected_files(id), full_verify)
    }

    fn status_with(
        &self,
        spec: &catalog::ComponentSpec,
        expected: &[InstalledFile],
        full_verify: bool,
    ) -> ManagedComponentState {
        let id = spec.id;
        let resumable_bytes = resumable_bytes(&self.root, id);
        if let Some(reason) = self.repair_reason(id) {
            return ManagedComponentState::NeedsRepair {
                reason,
                resumable_bytes,
            };
        }
        let record = match self.read_active(id) {
            Ok(Some(record)) => record,
            Ok(None) => return ManagedComponentState::Absent { resumable_bytes },
            Err(error) => {
                return ManagedComponentState::NeedsRepair {
                    reason: error.to_string(),
                    resumable_bytes,
                }
            }
        };
        if record.artifact_sha256 != spec.artifact_sha256 || record.files != expected {
            return ManagedComponentState::NeedsRepair {
                reason: "activation record does not match the compiled catalogue".to_string(),
                resumable_bytes,
            };
        }
        if let Err(error) = validate_release_name_for(spec, &record.release) {
            return ManagedComponentState::NeedsRepair {
                reason: error.to_string(),
                resumable_bytes,
            };
        }
        let release = self
            .component_dir(id)
            .join("releases")
            .join(&record.release);
        if let Err(error) = verify_receipt(&release, &record) {
            return ManagedComponentState::NeedsRepair {
                reason: error.to_string(),
                resumable_bytes,
            };
        }
        if let Err(error) = verify_payload_cached(&release.join("payload"), expected, full_verify) {
            self.mark_needs_repair(id, &error);
            return ManagedComponentState::NeedsRepair {
                reason: error.to_string(),
                resumable_bytes,
            };
        }
        ManagedComponentState::Ready {
            version: record.version,
            bytes: expected.iter().map(|file| file.size).sum(),
            root: release.join("payload").to_string_lossy().into_owned(),
        }
    }

    fn activate_with(
        &self,
        spec: &catalog::ComponentSpec,
        files: Vec<InstalledFile>,
        stage: &Path,
        operation: &OperationId,
    ) -> Result<ActivationRecord, InstallError> {
        let id = spec.id;
        let _lease = self.lock_exclusive(id)?;
        let release_name = format!("{}-{}", spec.artifact_sha256, operation.as_str());
        validate_release_name_for(spec, &release_name)?;
        let releases = self.component_dir(id).join("releases");
        fs::create_dir_all(&releases)?;
        let release = releases.join(&release_name);
        ensure_contained(&self.component_dir(id), &release)?;
        if release.exists() {
            return Err(InstallError::State(format!(
                "generation {release_name} already exists"
            )));
        }
        fs::rename(stage, &release)?;
        remember_verified_payload(&release.join("payload"), &files)?;
        let record = ActivationRecord {
            schema_version: 1,
            component: id,
            version: spec.version.to_string(),
            release: release_name,
            artifact_sha256: spec.artifact_sha256.to_string(),
            files,
        };
        let receipt = serde_json::to_vec_pretty(&record)
            .map_err(|error| InstallError::State(error.to_string()))?;
        echo_core::write_atomic(&release.join("receipt.json"), &receipt)
            .map_err(InstallError::IoMessage)?;
        self.clear_repair_marker(id)?;
        echo_core::write_atomic(&self.active_path(id), &receipt)
            .map_err(InstallError::IoMessage)?;
        if let Err(error) =
            self.cleanup_releases_with(id, Some(&record.release), spec, &record.files)
        {
            eprintln!("managed release cleanup: {error}");
        }
        Ok(record)
    }

    pub fn verify(&self, id: ComponentId) -> Result<(), InstallError> {
        let _lease = self.lock_exclusive(id)?;
        let result = (|| {
            let record = self.read_active(id)?.ok_or_else(|| {
                InstallError::State("managed component is not installed".to_string())
            })?;
            validate_release_name(id, &record.release)?;
            let root = self
                .component_dir(id)
                .join("releases")
                .join(&record.release);
            verify_receipt(&root, &record)?;
            verify_payload_cached(&root.join("payload"), &expected_files(id), true)
        })();
        match result {
            Ok(()) => self.clear_repair_marker(id),
            Err(error) => {
                self.mark_needs_repair(id, &error);
                Err(error)
            }
        }
    }

    pub fn remove(&self, id: ComponentId) -> Result<(), InstallError> {
        let _operation = self.operation_shared()?;
        let _lease = self.lock_exclusive(id)?;
        if let Some(record) = self.read_active(id)? {
            validate_release_name(id, &record.release)?;
            if record.files != expected_files(id) {
                return Err(InstallError::State(
                    "refusing removal because the receipt differs from the compiled catalogue"
                        .to_string(),
                ));
            }
        }
        self.cleanup_releases(id, None)?;
        self.cleanup_staging(id)?;
        self.clear_repair_marker(id)?;
        let active = self.active_path(id);
        if active.exists() {
            fs::remove_file(&active)?;
        }
        forget_partial(&self.root, &DownloadSpec::from(component(id)));
        Ok(())
    }

    fn cleanup_releases(&self, id: ComponentId, keep: Option<&str>) -> Result<(), InstallError> {
        self.cleanup_releases_with(id, keep, component(id), &expected_files(id))
    }

    fn cleanup_releases_with(
        &self,
        id: ComponentId,
        keep: Option<&str>,
        spec: &catalog::ComponentSpec,
        expected: &[InstalledFile],
    ) -> Result<(), InstallError> {
        let releases = self.component_dir(id).join("releases");
        let entries = match fs::read_dir(&releases) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if keep == Some(name.as_str()) {
                continue;
            }
            validate_release_name_for(spec, &name)?;
            let release = entry.path();
            ensure_contained(&self.component_dir(id), &release)?;
            let receipt_path = release.join("receipt.json");
            if receipt_path.exists() {
                let record: ActivationRecord = serde_json::from_slice(&fs::read(&receipt_path)?)
                    .map_err(|error| InstallError::State(error.to_string()))?;
                if record.component != id || record.release != name || record.files != expected {
                    return Err(InstallError::State(format!(
                        "refusing unknown managed release {name}"
                    )));
                }
            }
            cleanup_payload_subset(&release.join("payload"), expected)?;
            if receipt_path.exists() {
                fs::remove_file(receipt_path)?;
            }
            remove_empty_tree(&release)?;
        }
        Ok(())
    }

    fn cleanup_staging(&self, id: ComponentId) -> Result<(), InstallError> {
        let staging = self.managed().join("staging");
        let operations = match fs::read_dir(&staging) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for operation in operations {
            let operation = operation?;
            let component_stage = operation.path().join(id.as_str());
            if !component_stage.exists() {
                continue;
            }
            cleanup_payload_subset(&component_stage.join("payload"), &expected_files(id))?;
            remove_empty_tree(&component_stage)?;
            let _ = fs::remove_dir(operation.path());
        }
        Ok(())
    }

    pub fn recover(&self) -> Vec<String> {
        let recovery = match self.open_operation_lock() {
            Ok(file) => file,
            Err(error) => return vec![error.to_string()],
        };
        match FileExt::try_lock_exclusive(&recovery) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Vec::new(),
            Err(error) => return vec![error.to_string()],
        }
        let mut problems = Vec::new();
        for spec in catalog::COMPONENTS {
            let keep = self
                .read_active(spec.id)
                .ok()
                .flatten()
                .map(|record| record.release);
            if let Err(error) = self.cleanup_releases(spec.id, keep.as_deref()) {
                problems.push(error.to_string());
            }
            if let Err(error) = self.cleanup_staging(spec.id) {
                problems.push(error.to_string());
            }
        }
        problems
    }
}

pub struct ComponentLease {
    file: fs::File,
}

pub struct ManagedPath {
    pub root: PathBuf,
    pub lease: ComponentLease,
}

impl Drop for ComponentLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn expected_files(id: ComponentId) -> Vec<InstalledFile> {
    expected_files_for(component(id))
}

fn expected_files_for(spec: &catalog::ComponentSpec) -> Vec<InstalledFile> {
    match spec.format {
        ArtifactFormat::Direct => vec![InstalledFile {
            relative_path: spec.artifact_name.to_string(),
            size: spec.artifact_size,
            sha256: spec.artifact_sha256.to_string(),
            mode: 0o644,
            kind: PayloadKind::File,
            link_target: None,
        }],
        ArtifactFormat::TarGzip | ArtifactFormat::TarBzip2 => archive_component(spec)
            .expect("archive component has inventory")
            .payload
            .iter()
            .map(|file| InstalledFile {
                relative_path: Path::new(&file.path)
                    .file_name()
                    .expect("catalogue member has a filename")
                    .to_string_lossy()
                    .into_owned(),
                size: file.size,
                sha256: file.sha256.clone(),
                mode: file.mode,
                kind: file.kind,
                link_target: file.link_target.clone(),
            })
            .collect(),
    }
}

fn extraction_plan(id: ComponentId) -> Option<ExtractionPlan> {
    let spec = component(id);
    let inventory = archive_component(spec)?;
    Some(ExtractionPlan {
        format: spec.format,
        files: inventory
            .payload
            .iter()
            .map(|file| ExtractFile {
                source: file.path.clone(),
                destination: Path::new(&file.path)
                    .file_name()
                    .expect("catalogue member has filename")
                    .to_string_lossy()
                    .into_owned(),
                kind: file.kind,
                link_target: file.link_target.clone(),
                size: file.size,
                mode: file.mode,
                sha256: file.sha256.clone(),
            })
            .collect(),
        max_entries: inventory.entries,
        max_expanded_bytes: inventory.expanded_bytes,
    })
}

fn verify_payload(root: &Path, files: &[InstalledFile], full: bool) -> Result<(), InstallError> {
    verify_payload_cancellable(root, files, full, None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PayloadFingerprint(Vec<(String, u64, u128, Option<String>)>);

fn verified_payloads() -> &'static Mutex<BTreeMap<PathBuf, PayloadFingerprint>> {
    static VERIFIED: OnceLock<Mutex<BTreeMap<PathBuf, PayloadFingerprint>>> = OnceLock::new();
    VERIFIED.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn payload_fingerprint(
    root: &Path,
    files: &[InstalledFile],
) -> Result<PayloadFingerprint, InstallError> {
    let mut values = Vec::with_capacity(files.len());
    for file in files {
        let path = root.join(&file.relative_path);
        let metadata = fs::symlink_metadata(&path)?;
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let target = if metadata.file_type().is_symlink() {
            Some(fs::read_link(&path)?.to_string_lossy().into_owned())
        } else {
            None
        };
        values.push((file.relative_path.clone(), metadata.len(), modified, target));
    }
    Ok(PayloadFingerprint(values))
}

fn remember_verified_payload(root: &Path, files: &[InstalledFile]) -> Result<(), InstallError> {
    let fingerprint = payload_fingerprint(root, files)?;
    verified_payloads()
        .lock()
        .expect("verified payload cache")
        .insert(root.to_path_buf(), fingerprint);
    Ok(())
}

#[cfg(test)]
pub(crate) fn trust_payload_fixture(root: &Path, files: &[InstalledFile]) {
    remember_verified_payload(root, files).unwrap();
}

fn verify_payload_cached(
    root: &Path,
    files: &[InstalledFile],
    force: bool,
) -> Result<(), InstallError> {
    verify_payload(root, files, false)?;
    let fingerprint = payload_fingerprint(root, files)?;
    if !force
        && verified_payloads()
            .lock()
            .expect("verified payload cache")
            .get(root)
            == Some(&fingerprint)
    {
        return Ok(());
    }
    verify_payload(root, files, true)?;
    verified_payloads()
        .lock()
        .expect("verified payload cache")
        .insert(root.to_path_buf(), fingerprint);
    Ok(())
}

fn verify_payload_cancellable(
    root: &Path,
    files: &[InstalledFile],
    full: bool,
    cancel: Option<&AtomicBool>,
) -> Result<(), InstallError> {
    for file in files {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            return Err(InstallError::Cancelled);
        }
        let path = root.join(&file.relative_path);
        ensure_contained(root, &path)?;
        let metadata = fs::symlink_metadata(&path)?;
        match file.kind {
            PayloadKind::File if !metadata.file_type().is_file() => {
                return Err(InstallError::Payload(format!(
                    "{} is not a regular file",
                    file.relative_path
                )));
            }
            PayloadKind::Symlink if !metadata.file_type().is_symlink() => {
                return Err(InstallError::Payload(format!(
                    "{} is not a symlink",
                    file.relative_path
                )));
            }
            _ => {}
        }
        if file.kind == PayloadKind::File && metadata.len() != file.size {
            return Err(InstallError::Payload(format!(
                "{} has the wrong size",
                file.relative_path
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file.kind == PayloadKind::File && metadata.permissions().mode() & 0o777 != file.mode
            {
                return Err(InstallError::Payload(format!(
                    "{} has the wrong mode",
                    file.relative_path
                )));
            }
        }
        if file.kind == PayloadKind::Symlink {
            let target = fs::read_link(&path)?;
            if target != Path::new(file.link_target.as_deref().unwrap_or("")) {
                return Err(InstallError::Payload(format!(
                    "{} has the wrong symlink target",
                    file.relative_path
                )));
            }
        }
        if full {
            let mut source = fs::File::open(&path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
                    return Err(InstallError::Cancelled);
                }
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
            }
            if format!("{:x}", hash.finalize()) != file.sha256 {
                return Err(InstallError::Payload(format!(
                    "{} is corrupt",
                    file.relative_path
                )));
            }
        }
    }
    Ok(())
}

fn copy_cancellable(
    source: &Path,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<(), InstallError> {
    let mut source = fs::File::open(source)?;
    let mut destination = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        destination.write_all(&buffer[..read])?;
    }
    destination.flush()?;
    Ok(())
}

fn ensure_contained(parent: &Path, child: &Path) -> Result<(), InstallError> {
    if !child.starts_with(parent) {
        return Err(InstallError::State(format!(
            "managed path escapes {}",
            parent.display()
        )));
    }
    Ok(())
}

fn verify_receipt(release: &Path, active: &ActivationRecord) -> Result<(), InstallError> {
    let raw = fs::read(release.join("receipt.json"))?;
    let receipt: ActivationRecord = serde_json::from_slice(&raw)
        .map_err(|error| InstallError::State(format!("invalid release receipt: {error}")))?;
    if receipt != *active {
        return Err(InstallError::State(
            "release receipt does not match its activation record".to_string(),
        ));
    }
    Ok(())
}

fn validate_release_name(id: ComponentId, release: &str) -> Result<(), InstallError> {
    validate_release_name_for(component(id), release)
}

fn validate_release_name_for(
    spec: &catalog::ComponentSpec,
    release: &str,
) -> Result<(), InstallError> {
    let prefix = format!("{}-", spec.artifact_sha256);
    let Some(operation) = release.strip_prefix(&prefix) else {
        return Err(InstallError::State(
            "activation generation does not match the pinned digest".to_string(),
        ));
    };
    if operation.is_empty()
        || operation.starts_with('-')
        || operation.ends_with('-')
        || !operation
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        || Path::new(release).components().count() != 1
    {
        return Err(InstallError::State(
            "activation generation is not one safe filename".to_string(),
        ));
    }
    Ok(())
}

fn remove_empty_tree(path: &Path) -> Result<(), InstallError> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
            Err(InstallError::State(format!(
                "refusing to delete unknown files under {}",
                path.display()
            )))
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_owned_tree(root: &Path, files: &[InstalledFile]) -> Result<(), InstallError> {
    let expected: std::collections::BTreeSet<_> = files
        .iter()
        .map(|file| file.relative_path.as_str())
        .collect();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !expected.contains(name.as_str()) {
            return Err(InstallError::State(format!(
                "refusing to delete unknown managed file {name}"
            )));
        }
    }
    Ok(())
}

fn cleanup_payload_subset(root: &Path, files: &[InstalledFile]) -> Result<(), InstallError> {
    validate_owned_tree(root, files)?;
    for file in files {
        let target = root.join(&file.relative_path);
        ensure_contained(root, &target)?;
        match fs::symlink_metadata(&target) {
            Ok(_) => fs::remove_file(target)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    remove_empty_tree(root)
}

fn resumable_bytes(root: &Path, id: ComponentId) -> u64 {
    let spec = DownloadSpec::from(component(id));
    let stem = format!("{}-{}", id.as_str(), &spec.sha256[..12]);
    fs::metadata(
        root.join("managed")
            .join("downloads")
            .join(format!("{stem}.part")),
    )
    .map(|value| value.len())
    .unwrap_or(0)
}

mod installer;
pub use installer::{CommandRuntimeProbe, Installer, RuntimeProbe};
#[cfg(test)]
mod tests;
