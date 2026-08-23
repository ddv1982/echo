pub mod catalog;
mod download;
mod extract;

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use catalog::{ComponentId, SetupPlanId};
pub use download::{DiskSpace, HttpRequest, HttpResponse, HttpTransport, SystemDisk, UreqTransport};

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
            | Self::IoMessage(message) => formatter.write_str(message),
            Self::Busy => formatter.write_str("another managed component operation is active"),
            Self::InsufficientSpace { required, available } => write!(
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
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ManagedComponentState {
    Absent { resumable_bytes: u64 },
    Ready { version: String, bytes: u64, root: String },
    NeedsRepair { reason: String, resumable_bytes: u64 },
    Unsupported { reason: String },
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
        self.managed().join("active").join(format!("{}.json", id.as_str()))
    }

    fn component_dir(&self, id: ComponentId) -> PathBuf {
        self.managed().join("components").join(id.as_str())
    }

    fn lock_path(&self, id: ComponentId) -> PathBuf {
        self.managed().join("locks").join(format!("{}.lock", id.as_str()))
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
            return Err(InstallError::State("activation record identity mismatch".to_string()));
        }
        Ok(Some(record))
    }

    #[cfg(test)]
    fn active_root(&self, id: ComponentId) -> Result<Option<PathBuf>, InstallError> {
        let Some(record) = self.read_active(id)? else {
            return Ok(None);
        };
        validate_release_name(id, &record.release)?;
        let root = self.component_dir(id).join("releases").join(&record.release);
        ensure_contained(&self.component_dir(id), &root)?;
        Ok(Some(root.join("payload")))
    }

    pub fn active_root_leased(&self, id: ComponentId) -> Result<Option<ManagedPath>, InstallError> {
        let lease = self.lease_shared(id)?;
        let Some(record) = self.read_active(id)? else {
            return Ok(None);
        };
        validate_release_name(id, &record.release)?;
        let root = self.component_dir(id).join("releases").join(&record.release);
        ensure_contained(&self.component_dir(id), &root)?;
        verify_payload(&root.join("payload"), &expected_files(id), false)?;
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
        let record = match self.read_active(id) {
            Ok(Some(record)) => record,
            Ok(None) => return ManagedComponentState::Absent { resumable_bytes },
            Err(error) => return ManagedComponentState::NeedsRepair {
                reason: error.to_string(),
                resumable_bytes,
            },
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
        let release = self.component_dir(id).join("releases").join(&record.release);
        if let Err(error) = verify_payload(&release.join("payload"), expected, full_verify) {
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
        echo_core::write_atomic(&self.active_path(id), &receipt).map_err(InstallError::IoMessage)?;
        self.cleanup_releases_with(id, Some(&record.release), spec, &record.files)?;
        Ok(record)
    }

    pub fn remove(&self, id: ComponentId) -> Result<(), InstallError> {
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
        let active = self.active_path(id);
        if active.exists() {
            fs::remove_file(&active)?;
        }
        self.cleanup_releases(id, None)?;
        self.cleanup_staging(id)?;
        forget_partial(&self.root, &DownloadSpec::from(component(id)));
        Ok(())
    }

    fn cleanup_releases(
        &self,
        id: ComponentId,
        keep: Option<&str>,
    ) -> Result<(), InstallError> {
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
                if record.component != id
                    || record.release != name
                    || record.files != expected
                {
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
            cleanup_payload_subset(
                &component_stage.join("payload"),
                &expected_files(id),
            )?;
            remove_empty_tree(&component_stage)?;
            let _ = fs::remove_dir(operation.path());
        }
        Ok(())
    }

    pub fn recover(&self) -> Vec<String> {
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
    for file in files {
        let path = root.join(&file.relative_path);
        ensure_contained(root, &path)?;
        let metadata = fs::symlink_metadata(&path)?;
        match file.kind {
            PayloadKind::File if !metadata.file_type().is_file() => {
                return Err(InstallError::Payload(format!("{} is not a regular file", file.relative_path)));
            }
            PayloadKind::Symlink if !metadata.file_type().is_symlink() => {
                return Err(InstallError::Payload(format!("{} is not a symlink", file.relative_path)));
            }
            _ => {}
        }
        if file.kind == PayloadKind::File && metadata.len() != file.size {
            return Err(InstallError::Payload(format!("{} has the wrong size", file.relative_path)));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file.kind == PayloadKind::File && metadata.permissions().mode() & 0o777 != file.mode {
                return Err(InstallError::Payload(format!("{} has the wrong mode", file.relative_path)));
            }
        }
        if file.kind == PayloadKind::Symlink {
            let target = fs::read_link(&path)?;
            if target != Path::new(file.link_target.as_deref().unwrap_or("")) {
                return Err(InstallError::Payload(format!("{} has the wrong symlink target", file.relative_path)));
            }
        }
        if full {
            let mut source = fs::File::open(&path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
            }
            if format!("{:x}", hash.finalize()) != file.sha256 {
                return Err(InstallError::Payload(format!("{} is corrupt", file.relative_path)));
            }
        }
    }
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

fn validate_release_name(id: ComponentId, release: &str) -> Result<(), InstallError> {
    validate_release_name_for(component(id), release)
}

fn validate_release_name_for(
    spec: &catalog::ComponentSpec,
    release: &str,
) -> Result<(), InstallError> {
    let prefix = format!("{}-", spec.artifact_sha256);
    let Some(operation) = release.strip_prefix(&prefix) else {
        return Err(InstallError::State("activation generation does not match the pinned digest".to_string()));
    };
    if operation.is_empty()
        || operation.starts_with('-')
        || operation.ends_with('-')
        || !operation
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        || Path::new(release).components().count() != 1
    {
        return Err(InstallError::State("activation generation is not one safe filename".to_string()));
    }
    Ok(())
}

fn remove_empty_tree(path: &Path) -> Result<(), InstallError> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Err(
            InstallError::State(format!("refusing to delete unknown files under {}", path.display())),
        ),
        Err(error) => Err(error.into()),
    }
}

fn validate_owned_tree(root: &Path, files: &[InstalledFile]) -> Result<(), InstallError> {
    let expected: std::collections::BTreeSet<_> =
        files.iter().map(|file| file.relative_path.as_str()).collect();
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
    fs::metadata(root.join("managed").join("downloads").join(format!("{stem}.part")))
        .map(|value| value.len())
        .unwrap_or(0)
}

pub struct Installer<'a> {
    pub store: ManagedStore,
    pub transport: &'a dyn HttpTransport,
    pub disk: &'a dyn DiskSpace,
}

impl Installer<'_> {
    pub fn ensure_plan(
        &self,
        components: &[ComponentId],
        repair: bool,
        operation: &OperationId,
        cancel: &AtomicBool,
        mut progress: impl FnMut(InstallProgress),
    ) -> Result<Vec<ActivationRecord>, InstallError> {
        let mut records = Vec::with_capacity(components.len());
        for id in components {
            records.push(self.ensure_component(
                *id,
                repair,
                operation,
                cancel,
                &mut progress,
            )?);
        }
        Ok(records)
    }

    pub fn ensure_component(
        &self,
        id: ComponentId,
        repair: bool,
        operation: &OperationId,
        cancel: &AtomicBool,
        progress: impl FnMut(InstallProgress),
    ) -> Result<ActivationRecord, InstallError> {
        self.ensure_spec(component(id), repair, operation, cancel, progress)
    }

    fn ensure_spec(
        &self,
        spec: &catalog::ComponentSpec,
        repair: bool,
        operation: &OperationId,
        cancel: &AtomicBool,
        mut progress: impl FnMut(InstallProgress),
    ) -> Result<ActivationRecord, InstallError> {
        let id = spec.id;
        let expected = expected_files_for(spec);
        if matches!(
            self.store.status_with(spec, &expected, repair),
            ManagedComponentState::Ready { .. }
        ) {
            return self.store.read_active(id)?.ok_or_else(|| InstallError::State("ready component lost its activation".to_string()));
        }
        let download = DownloadSpec::from(spec);
        let artifact = download_verified(
            self.store.root(),
            &download,
            self.transport,
            self.disk,
            operation,
            cancel,
            &mut progress,
        )?;
        let stage = self
            .store
            .managed()
            .join("staging")
            .join(operation.as_str())
            .join(id.as_str());
        if stage.exists() {
            fs::remove_dir_all(&stage)?;
        }
        let payload = stage.join("payload");
        fs::create_dir_all(&payload)?;
        progress(InstallProgress::new(operation, id, InstallPhase::Extracting, 0, spec.installed_bytes, 0));
        let outcome = match spec.format {
            ArtifactFormat::Direct => {
                let destination = payload.join(spec.artifact_name);
                fs::copy(&artifact, &destination)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&destination, fs::Permissions::from_mode(0o644))?;
                }
                Ok(())
            }
            ArtifactFormat::TarGzip | ArtifactFormat::TarBzip2 => extract_archive(
                &artifact,
                &payload,
                &extraction_plan(id).expect("archive has extraction plan"),
                cancel,
            ),
        };
        if let Err(error) = outcome {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
        verify_payload(&payload, &expected, true)?;
        progress(InstallProgress::new(operation, id, InstallPhase::Activating, spec.installed_bytes, spec.installed_bytes, 0));
        let record = self
            .store
            .activate_with(spec, expected, &stage, operation)?;
        forget_partial(self.store.root(), &download);
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::io::Cursor;
    use std::sync::Mutex;

    struct UnlimitedDisk;

    impl DiskSpace for UnlimitedDisk {
        fn available_bytes(&self, _: &Path) -> Result<Option<u64>, InstallError> {
            Ok(None)
        }
    }

    struct FixtureTransport(Mutex<VecDeque<Vec<u8>>>);

    impl HttpTransport for FixtureTransport {
        fn get(&self, _: &HttpRequest) -> Result<HttpResponse, InstallError> {
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: Box::new(Cursor::new(self.0.lock().unwrap().pop_front().unwrap())),
            })
        }
    }
    fn scratch(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("echo-managed-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn operation_ids_do_not_reuse_generation_names() {
        let first = OperationId::new();
        let second = OperationId::new();
        assert_ne!(first, second);
        assert!(first.as_str().contains(&std::process::id().to_string()));
    }

    #[test]
    fn direct_install_repairs_same_size_corruption_and_removes_only_managed_files() {
        let body = b"tiny verified model".to_vec();
        let digest: &'static str = Box::leak(format!("{:x}", Sha256::digest(&body)).into_boxed_str());
        let spec = catalog::ComponentSpec {
            id: ComponentId::SileroVad,
            label: "Fixture",
            version: "fixture",
            url: "https://fixture.invalid/model",
            artifact_name: "tiny.bin",
            artifact_size: body.len() as u64,
            artifact_sha256: digest,
            installed_bytes: body.len() as u64,
            format: ArtifactFormat::Direct,
            inventory_key: None,
        };
        let root = scratch("direct-install");
        let external = root.join("tiny.bin");
        fs::write(&external, b"manual sentinel").unwrap();
        let transport = FixtureTransport(Mutex::new(VecDeque::from([
            body.clone(),
            body.clone(),
        ])));
        let installer = Installer {
            store: ManagedStore::new(&root),
            transport: &transport,
            disk: &UnlimitedDisk,
        };
        let first = installer
            .ensure_spec(
                &spec,
                false,
                &OperationId::fixture("1"),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        assert!(matches!(
            installer
                .store
                .status_with(&spec, &expected_files_for(&spec), true),
            ManagedComponentState::Ready { .. }
        ));
        let active = installer.store.read_active(spec.id).unwrap().unwrap();
        let payload = installer
            .store
            .component_dir(spec.id)
            .join("releases")
            .join(&active.release)
            .join("payload/tiny.bin");
        fs::write(&payload, vec![b'x'; body.len()]).unwrap();
        assert_eq!(fs::metadata(&payload).unwrap().len(), body.len() as u64);
        assert!(matches!(
            installer
                .store
                .status_with(&spec, &expected_files_for(&spec), true),
            ManagedComponentState::NeedsRepair { .. }
        ));
        let second = installer
            .ensure_spec(
                &spec,
                true,
                &OperationId::fixture("2"),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        assert_ne!(first.release, second.release);
        assert!(!payload.exists());
        assert_eq!(fs::read(&external).unwrap(), b"manual sentinel");
    }

    #[test]
    fn removal_is_idempotent_and_external_files_survive() {
        let root = scratch("remove");
        let external = root.join("ggml-small.bin");
        fs::write(&external, b"manual sentinel").unwrap();
        let store = ManagedStore::new(&root);
        store.remove(ComponentId::WhisperSmall).unwrap();
        store.remove(ComponentId::WhisperSmall).unwrap();
        assert_eq!(fs::read(external).unwrap(), b"manual sentinel");
    }

    #[test]
    fn quick_status_finds_missing_payload_and_full_verify_finds_same_size_corruption() {
        let root = scratch("status");
        let id = ComponentId::SileroVad;
        let spec = component(id);
        let release_name = format!("{}-1", spec.artifact_sha256);
        let release = root
            .join("managed/components")
            .join(id.as_str())
            .join("releases")
            .join(&release_name);
        let payload = release.join("payload");
        fs::create_dir_all(&payload).unwrap();
        fs::write(payload.join(spec.artifact_name), vec![0u8; spec.artifact_size as usize]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                payload.join(spec.artifact_name),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        let record = ActivationRecord {
            schema_version: 1,
            component: id,
            version: spec.version.to_string(),
            release: release_name,
            artifact_sha256: spec.artifact_sha256.to_string(),
            files: expected_files(id),
        };
        let raw = serde_json::to_vec_pretty(&record).unwrap();
        echo_core::write_atomic(&release.join("receipt.json"), &raw).unwrap();
        let store = ManagedStore::new(&root);
        echo_core::write_atomic(&store.active_path(id), &raw).unwrap();
        assert!(matches!(store.status(id, false), ManagedComponentState::Ready { .. }));
        assert!(matches!(store.status(id, true), ManagedComponentState::NeedsRepair { .. }));
        fs::remove_file(payload.join(spec.artifact_name)).unwrap();
        assert!(matches!(store.status(id, false), ManagedComponentState::NeedsRepair { .. }));
        let external = root.join(spec.artifact_name);
        fs::write(&external, b"manual sentinel").unwrap();
        store.remove(id).unwrap();
        store.remove(id).unwrap();
        assert_eq!(fs::read(external).unwrap(), b"manual sentinel");
    }

    #[test]
    fn malicious_release_records_never_escape_and_repair_uses_a_fresh_generation() {
        let root = scratch("generation");
        let id = ComponentId::SileroVad;
        let spec = component(id);
        let store = ManagedStore::new(&root);
        let sentinel = root.join("sentinel");
        fs::write(&sentinel, b"external").unwrap();
        let malicious = ActivationRecord {
            schema_version: 1,
            component: id,
            version: spec.version.to_string(),
            release: format!("{}-../sentinel", spec.artifact_sha256),
            artifact_sha256: spec.artifact_sha256.to_string(),
            files: expected_files(id),
        };
        echo_core::write_atomic(
            &store.active_path(id),
            &serde_json::to_vec(&malicious).unwrap(),
        )
        .unwrap();
        assert!(store.active_root(id).is_err());
        assert!(store.remove(id).is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"external");

        let old_release = format!("{}-1", spec.artifact_sha256);
        let old = ActivationRecord {
            release: old_release.clone(),
            ..malicious
        };
        echo_core::write_atomic(&store.active_path(id), &serde_json::to_vec(&old).unwrap()).unwrap();
        let old_root = store.component_dir(id).join("releases").join(&old_release);
        fs::create_dir_all(old_root.join("payload")).unwrap();
        echo_core::write_atomic(&old_root.join("receipt.json"), &serde_json::to_vec(&old).unwrap()).unwrap();
        let stage = store.managed().join("staging/2").join(id.as_str());
        fs::create_dir_all(stage.join("payload")).unwrap();
        let fresh = store
            .activate_with(
                spec,
                expected_files(id),
                &stage,
                &OperationId::fixture("2"),
            )
            .unwrap();
        assert_ne!(fresh.release, old_release);
        assert!(!old_root.exists(), "the old generation is collected after pointer swap");
        assert_eq!(store.read_active(id).unwrap().unwrap().release, fresh.release);
    }

    #[test]
    fn recovery_discards_owned_staging_and_inactive_releases_but_keeps_partials() {
        let root = scratch("recover");
        let id = ComponentId::SileroVad;
        let spec = component(id);
        let store = ManagedStore::new(&root);
        let release_name = format!("{}-99", spec.artifact_sha256);
        let release = store
            .component_dir(id)
            .join("releases")
            .join(&release_name);
        let payload = release.join("payload");
        fs::create_dir_all(&payload).unwrap();
        let file = fs::File::create(payload.join(spec.artifact_name)).unwrap();
        file.set_len(spec.artifact_size).unwrap();
        let record = ActivationRecord {
            schema_version: 1,
            component: id,
            version: spec.version.to_string(),
            release: release_name,
            artifact_sha256: spec.artifact_sha256.to_string(),
            files: expected_files(id),
        };
        echo_core::write_atomic(
            &release.join("receipt.json"),
            &serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let stage = store.managed().join("staging/ab-1").join(id.as_str());
        fs::create_dir_all(stage.join("payload")).unwrap();
        fs::File::create(stage.join("payload").join(spec.artifact_name))
            .unwrap()
            .set_len(1)
            .unwrap();
        let partial = store.managed().join("downloads/keep.part");
        fs::create_dir_all(partial.parent().unwrap()).unwrap();
        fs::write(&partial, b"resume").unwrap();

        assert!(store.recover().is_empty());
        assert!(!release.exists());
        assert!(!stage.exists());
        assert_eq!(fs::read(partial).unwrap(), b"resume");
    }
}
