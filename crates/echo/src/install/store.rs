use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use super::catalog::{self, component, ComponentId};
use super::download::{forget_partial, DownloadSpec};
use super::filesystem::{
    cleanup_payload_subset, cleanup_release, ensure_contained, remove_empty_tree, resumable_bytes,
    validate_collectable_release_name, validate_release_name, validate_release_name_for,
    verify_receipt,
};
use super::payload::{
    expected_files, receipt_files_compatible, remember_verified_payload, verify_payload_cached,
};
use super::types::{
    ActivationRecord, ComponentLease, InstallError, InstalledFile, ManagedComponentState,
    ManagedPath, OperationId,
};

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

    pub(super) fn managed(&self) -> PathBuf {
        self.root.join("managed")
    }

    pub(super) fn active_path(&self, id: ComponentId) -> PathBuf {
        self.managed()
            .join("active")
            .join(format!("{}.json", id.as_str()))
    }

    pub(super) fn component_dir(&self, id: ComponentId) -> PathBuf {
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

    pub(super) fn operation_shared(&self) -> Result<ComponentLease, InstallError> {
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

    pub(super) fn read_active(
        &self,
        id: ComponentId,
    ) -> Result<Option<ActivationRecord>, InstallError> {
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
    pub(super) fn active_root(&self, id: ComponentId) -> Result<Option<PathBuf>, InstallError> {
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
        let expected = expected_files(id);
        if !receipt_files_compatible(id, &record.files, &expected) {
            return Err(InstallError::State(
                "activation record does not match the compiled catalogue".to_string(),
            ));
        }
        if let Err(error) = verify_receipt(&root, &record)
            .and_then(|_| verify_payload_cached(&root.join("payload"), &record.files, false))
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

    pub(super) fn status_with(
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
        if record.artifact_sha256 != spec.artifact_sha256
            || !receipt_files_compatible(id, &record.files, expected)
        {
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
        if let Err(error) =
            verify_payload_cached(&release.join("payload"), &record.files, full_verify)
        {
            self.mark_needs_repair(id, &error);
            return ManagedComponentState::NeedsRepair {
                reason: error.to_string(),
                resumable_bytes,
            };
        }
        ManagedComponentState::Ready {
            version: record.version,
            bytes: record.files.iter().map(|file| file.size).sum(),
            root: release.join("payload").to_string_lossy().into_owned(),
        }
    }

    pub(super) fn activate_with(
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
        if let Err(error) = self.cleanup_releases_with(id, Some(&record.release), &record.files) {
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
            let expected = expected_files(id);
            if !receipt_files_compatible(id, &record.files, &expected) {
                return Err(InstallError::State(
                    "activation record does not match the compiled catalogue".to_string(),
                ));
            }
            verify_payload_cached(&root.join("payload"), &record.files, true)
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
        // Removal must survive a digest rotation, so the active record only
        // has to name a release this store could have written. What each
        // release owns is settled per directory from its own receipt.
        if let Some(record) = self.read_active(id)? {
            validate_collectable_release_name(&record.release)?;
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
        self.cleanup_releases_with(id, keep, &expected_files(id))
    }

    fn cleanup_releases_with(
        &self,
        id: ComponentId,
        keep: Option<&str>,
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
            cleanup_release(&self.component_dir(id), &entry.path(), id, &name, expected)?;
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
