use std::fs;
use std::path::{Component, Path};

use super::catalog::{self, component, ComponentId};
use super::download::DownloadSpec;
use super::types::{ActivationRecord, InstallError, InstalledFile};

pub(super) fn ensure_contained(parent: &Path, child: &Path) -> Result<(), InstallError> {
    // starts_with is a component prefix test, so `<parent>/../../elsewhere`
    // is "contained" by it while the kernel resolves it somewhere else
    // entirely. Everything below the parent has to be a plain relative path.
    let escapes = match child.strip_prefix(parent) {
        Ok(relative) => relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_))),
        Err(_) => true,
    };
    if escapes {
        return Err(InstallError::State(format!(
            "managed path escapes {}",
            parent.display()
        )));
    }
    Ok(())
}

/// A receipt names the files its release owns, and cleanup deletes exactly
/// that list. A damaged or edited receipt must not be able to aim that list
/// anywhere but inside its own payload directory.
fn validate_declared_paths(files: &[InstalledFile]) -> Result<(), InstallError> {
    for file in files {
        let path = Path::new(&file.relative_path);
        if file.relative_path.is_empty()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(InstallError::State(format!(
                "receipt names a file outside its payload: {}",
                file.relative_path
            )));
        }
    }
    Ok(())
}

pub(super) fn verify_receipt(
    release: &Path,
    active: &ActivationRecord,
) -> Result<(), InstallError> {
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

pub(super) fn validate_release_name(id: ComponentId, release: &str) -> Result<(), InstallError> {
    validate_release_name_for(component(id), release)
}

pub(super) fn validate_release_name_for(
    spec: &catalog::ComponentSpec,
    release: &str,
) -> Result<(), InstallError> {
    let prefix = format!("{}-", spec.artifact_sha256);
    let Some(operation) = release.strip_prefix(&prefix) else {
        return Err(InstallError::State(
            "activation generation does not match the pinned digest".to_string(),
        ));
    };
    validate_generation(release, operation)
}

/// A release directory this store could have written, whatever digest was
/// pinned when it did. Superseded generations keep their old digest in the
/// name, so binding collection to the current pin would strand them: they
/// could never be swept and would block removal of the whole component.
/// Confinement is what keeps deletion safe here, not the digest.
pub(super) fn validate_collectable_release_name(release: &str) -> Result<(), InstallError> {
    let Some((digest, operation)) = release.split_once('-') else {
        return Err(InstallError::State(
            "managed release name has no generation".to_string(),
        ));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(InstallError::State(
            "managed release name is not digest-prefixed".to_string(),
        ));
    }
    validate_generation(release, operation)
}

fn validate_generation(release: &str, operation: &str) -> Result<(), InstallError> {
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

pub(super) fn remove_empty_tree(path: &Path) -> Result<(), InstallError> {
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

pub(super) fn cleanup_release(
    component_dir: &Path,
    release: &Path,
    id: ComponentId,
    name: &str,
    expected: &[InstalledFile],
) -> Result<(), InstallError> {
    validate_collectable_release_name(name)?;
    ensure_contained(component_dir, release)?;
    let receipt_path = release.join("receipt.json");
    let owned_files = if receipt_path.exists() {
        let record: ActivationRecord = serde_json::from_slice(&fs::read(&receipt_path)?)
            .map_err(|error| InstallError::State(error.to_string()))?;
        // A superseded release is described by the receipt written
        // when it was installed, not by the catalogue that replaced
        // it. Comparing it against today's file list would refuse to
        // collect exactly the generations that need collecting.
        if record.component != id || record.release != name {
            return Err(InstallError::State(format!(
                "refusing unknown managed release {name}"
            )));
        }
        validate_declared_paths(&record.files)?;
        record.files
    } else {
        expected.to_vec()
    };
    cleanup_payload_subset(&release.join("payload"), &owned_files)?;
    let verification_path = release.join("verified.json");
    if verification_path.exists() {
        fs::remove_file(verification_path)?;
    }
    if receipt_path.exists() {
        fs::remove_file(receipt_path)?;
    }
    remove_empty_tree(release)
}

pub(super) fn cleanup_payload_subset(
    root: &Path,
    files: &[InstalledFile],
) -> Result<(), InstallError> {
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

pub(super) fn resumable_bytes(root: &Path, id: ComponentId) -> u64 {
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
