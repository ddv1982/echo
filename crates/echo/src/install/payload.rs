use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::catalog::{
    self, archive_component, component, ArtifactFormat, ComponentId, PayloadKind,
};
use super::extract::{ExtractFile, ExtractionPlan};
use super::filesystem::ensure_contained;
use super::sha256_file_cancellable;
use super::types::{InstallError, InstalledFile};

pub(super) fn expected_files(id: ComponentId) -> Vec<InstalledFile> {
    expected_files_for(component(id))
}

pub(super) fn receipt_files_compatible(
    id: ComponentId,
    actual: &[InstalledFile],
    expected: &[InstalledFile],
) -> bool {
    if actual == expected {
        return true;
    }
    if id != ComponentId::WhisperRuntime {
        return false;
    }
    expected
        .iter()
        .filter(|file| file.relative_path != "whisper-server")
        .eq(actual.iter())
}

pub(super) fn expected_files_for(spec: &catalog::ComponentSpec) -> Vec<InstalledFile> {
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

pub(super) fn extraction_plan(id: ComponentId) -> Option<ExtractionPlan> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FingerprintFileType {
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    relative_path: String,
    file_type: FingerprintFileType,
    mode: u32,
    size: u64,
    device: u64,
    inode: u64,
    ctime: (i64, i64),
    mtime: (i64, i64),
    symlink_target: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PayloadFingerprint(Vec<FileFingerprint>);

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
        let file_type = if metadata.file_type().is_file() {
            FingerprintFileType::File
        } else if metadata.file_type().is_symlink() {
            FingerprintFileType::Symlink
        } else {
            FingerprintFileType::Other
        };
        let symlink_target = if file_type == FingerprintFileType::Symlink {
            Some(fs::read_link(&path)?)
        } else {
            None
        };
        values.push(FileFingerprint {
            relative_path: file.relative_path.clone(),
            file_type,
            mode: metadata.mode(),
            size: metadata.len(),
            device: metadata.dev(),
            inode: metadata.ino(),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            symlink_target,
        });
    }
    Ok(PayloadFingerprint(values))
}

pub(super) fn remember_verified_payload(
    root: &Path,
    files: &[InstalledFile],
) -> Result<(), InstallError> {
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

#[cfg(test)]
pub(super) fn forget_verified_payload_fixture(root: &Path) {
    verified_payloads()
        .lock()
        .expect("verified payload cache")
        .remove(root);
}

#[cfg(test)]
pub(super) fn clear_verified_payload_fixtures() {
    verified_payloads()
        .lock()
        .expect("verified payload cache")
        .clear();
}

pub(super) fn verify_payload_cached(
    root: &Path,
    files: &[InstalledFile],
    force: bool,
) -> Result<(), InstallError> {
    verify_payload(root, files, false)?;
    let fingerprint = payload_fingerprint(root, files)?;
    if !force {
        let cache_matches = verified_payloads()
            .lock()
            .expect("verified payload cache")
            .get(root)
            == Some(&fingerprint);
        if cache_matches {
            return Ok(());
        }
    }
    verify_payload(root, files, true)?;
    remember_verified_payload(root, files)
}

pub(super) fn verify_payload_cancellable(
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
        if full && sha256_file_cancellable(&path, cancel)? != file.sha256 {
            return Err(InstallError::Payload(format!(
                "{} is corrupt",
                file.relative_path
            )));
        }
    }
    Ok(())
}

pub(super) fn copy_cancellable(
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
