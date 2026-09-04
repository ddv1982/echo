use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::catalog::{ArtifactFormat, PayloadKind};
use super::sha256_file_cancellable;
use super::types::InstallError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractFile {
    pub source: String,
    pub destination: String,
    pub kind: PayloadKind,
    pub link_target: Option<String>,
    pub size: u64,
    pub mode: u32,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct ExtractionPlan {
    pub format: ArtifactFormat,
    pub files: Vec<ExtractFile>,
    pub max_entries: usize,
    pub max_expanded_bytes: u64,
}

#[derive(Debug)]
struct PendingSymlink {
    output_relative: PathBuf,
    target: PathBuf,
    expected_sha256: String,
}

fn safe_relative(raw: &Path) -> Result<PathBuf, InstallError> {
    if raw.as_os_str().is_empty() || raw.is_absolute() {
        return Err(InstallError::UnsafeArchive(format!(
            "absolute or empty archive path {}",
            raw.display()
        )));
    }
    let mut safe = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(InstallError::UnsafeArchive(format!(
                    "archive path escapes its root: {}",
                    raw.display()
                )));
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err(InstallError::UnsafeArchive(
            "empty archive path".to_string(),
        ));
    }
    Ok(safe)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), InstallError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> Result<(), InstallError> {
    Ok(())
}

fn validate_symlink_graph(
    selected: &BTreeMap<PathBuf, &ExtractFile>,
    regular_files: &BTreeSet<PathBuf>,
    symlinks: &BTreeMap<PathBuf, PendingSymlink>,
    cancel: &AtomicBool,
) -> Result<(), InstallError> {
    for source in symlinks.keys() {
        let mut current = source.clone();
        let mut chain = BTreeSet::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallError::Cancelled);
            }
            if !chain.insert(current.clone()) {
                return Err(InstallError::UnsafeArchive(format!(
                    "symlink cycle at {}",
                    source.display()
                )));
            }
            let link = symlinks.get(&current).ok_or_else(|| {
                InstallError::UnsafeArchive(format!(
                    "symlink target was not extracted for {}",
                    source.display()
                ))
            })?;
            let target_source = safe_relative(
                &current
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join(&link.target),
            )?;
            let target_spec = selected.get(&target_source).ok_or_else(|| {
                InstallError::UnsafeArchive(format!(
                    "symlink target is not selected for {}",
                    source.display()
                ))
            })?;
            let output_target = safe_relative(
                &link
                    .output_relative
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join(&link.target),
            )?;
            let selected_target = safe_relative(Path::new(&target_spec.destination))?;
            if output_target != selected_target {
                return Err(InstallError::UnsafeArchive(format!(
                    "symlink target destination changed for {}",
                    source.display()
                )));
            }
            match target_spec.kind {
                PayloadKind::File => {
                    if !regular_files.contains(&target_source) {
                        return Err(InstallError::UnsafeArchive(format!(
                            "symlink target was not extracted for {}",
                            source.display()
                        )));
                    }
                    break;
                }
                PayloadKind::Symlink => current = target_source,
            }
        }
    }
    Ok(())
}

fn extract_tar<R: Read>(
    reader: R,
    destination: &Path,
    plan: &ExtractionPlan,
    cancel: &AtomicBool,
) -> Result<(), InstallError> {
    let selected: BTreeMap<_, _> = plan
        .files
        .iter()
        .map(|file| (safe_relative(Path::new(&file.source)), file))
        .map(|(path, file)| Ok((path?, file)))
        .collect::<Result<_, InstallError>>()?;
    let mut archive = tar::Archive::new(reader);
    let mut seen = BTreeSet::new();
    let mut written = BTreeSet::new();
    let mut symlinks = BTreeMap::new();
    let mut entries = 0usize;
    let mut expanded = 0u64;
    for entry in archive
        .entries()
        .map_err(|error| InstallError::UnsafeArchive(error.to_string()))?
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let mut entry = entry.map_err(|error| InstallError::UnsafeArchive(error.to_string()))?;
        entries += 1;
        if entries > plan.max_entries {
            return Err(InstallError::UnsafeArchive(
                "archive entry limit exceeded".to_string(),
            ));
        }
        expanded = expanded.saturating_add(entry.size());
        if expanded > plan.max_expanded_bytes {
            return Err(InstallError::UnsafeArchive(
                "archive expanded-byte limit exceeded".to_string(),
            ));
        }
        let source = safe_relative(
            &entry
                .path()
                .map_err(|error| InstallError::UnsafeArchive(error.to_string()))?,
        )?;
        if !seen.insert(source.clone()) {
            return Err(InstallError::UnsafeArchive(format!(
                "duplicate archive path {}",
                source.display()
            )));
        }
        let kind = entry.header().entry_type();
        if kind.is_hard_link()
            || kind.is_block_special()
            || kind.is_character_special()
            || kind.is_fifo()
        {
            return Err(InstallError::UnsafeArchive(format!(
                "special archive member {}",
                source.display()
            )));
        }
        let Some(file) = selected.get(&source) else {
            if kind.is_file() || kind.is_dir() || kind.is_symlink() {
                continue;
            }
            return Err(InstallError::UnsafeArchive(format!(
                "unsupported archive member {}",
                source.display()
            )));
        };
        let output_relative = safe_relative(Path::new(&file.destination))?;
        let output = destination.join(&output_relative);
        match file.kind {
            PayloadKind::File if kind.is_file() => {
                if entry.size() != file.size {
                    return Err(InstallError::Payload(format!(
                        "{} has size {}, expected {}",
                        source.display(),
                        entry.size(),
                        file.size
                    )));
                }
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut target = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&output)?;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(InstallError::Cancelled);
                    }
                    let read = entry.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    target.write_all(&buffer[..read])?;
                }
                target.flush()?;
                set_mode(&output, file.mode)?;
                if sha256_file_cancellable(&output, Some(cancel))? != file.sha256 {
                    return Err(InstallError::Payload(format!(
                        "{} failed its payload SHA-256",
                        source.display()
                    )));
                }
                written.insert(source);
            }
            PayloadKind::Symlink if kind.is_symlink() => {
                let link = entry
                    .link_name()
                    .map_err(|error| InstallError::UnsafeArchive(error.to_string()))?
                    .ok_or_else(|| {
                        InstallError::UnsafeArchive("symlink has no target".to_string())
                    })?;
                let expected = file.link_target.as_deref().ok_or_else(|| {
                    InstallError::UnsafeArchive("catalogue symlink has no target".to_string())
                })?;
                if link != Path::new(expected)
                    || link.is_absolute()
                    || link
                        .components()
                        .any(|part| matches!(part, Component::ParentDir))
                {
                    return Err(InstallError::UnsafeArchive(format!(
                        "symlink {} escapes or changed target",
                        source.display()
                    )));
                }
                symlinks.insert(
                    source,
                    PendingSymlink {
                        output_relative,
                        target: PathBuf::from(expected),
                        expected_sha256: file.sha256.clone(),
                    },
                );
            }
            _ => {
                return Err(InstallError::UnsafeArchive(format!(
                    "archive member type changed for {}",
                    source.display()
                )));
            }
        }
    }
    let missing: Vec<_> = selected
        .keys()
        .filter(|path| !seen.contains(*path))
        .collect();
    if !missing.is_empty() {
        return Err(InstallError::Payload(format!(
            "archive is missing {} required members",
            missing.len()
        )));
    }
    validate_symlink_graph(&selected, &written, &symlinks, cancel)?;
    for link in symlinks.values() {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let output = destination.join(&link.output_relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&link.target, &output)?;
        #[cfg(not(unix))]
        return Err(InstallError::Unsupported(
            "managed archive symlinks require Unix".to_string(),
        ));
    }
    for (source, link) in &symlinks {
        let output = destination.join(&link.output_relative);
        if sha256_file_cancellable(&output, Some(cancel))? != link.expected_sha256 {
            return Err(InstallError::Payload(format!(
                "materialized symlink {} failed SHA-256",
                source.display()
            )));
        }
        written.insert(source.clone());
    }
    let unwritten: Vec<_> = selected
        .keys()
        .filter(|path| !written.contains(*path))
        .collect();
    if !unwritten.is_empty() {
        return Err(InstallError::Payload(format!(
            "archive is missing {} required members",
            unwritten.len()
        )));
    }
    Ok(())
}

pub fn extract_archive(
    artifact: &Path,
    destination: &Path,
    plan: &ExtractionPlan,
    cancel: &AtomicBool,
) -> Result<(), InstallError> {
    fs::create_dir_all(destination)?;
    let file = fs::File::open(artifact)?;
    match plan.format {
        ArtifactFormat::TarGzip => extract_tar(
            flate2::read::GzDecoder::new(file),
            destination,
            plan,
            cancel,
        ),
        ArtifactFormat::TarBzip2 => {
            extract_tar(bzip2::read::BzDecoder::new(file), destination, plan, cancel)
        }
        ArtifactFormat::Direct => Err(InstallError::State(
            "direct files do not use archive extraction".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn payload(body: &[u8]) -> ExtractFile {
        ExtractFile {
            source: "root/bin/tool".to_string(),
            destination: "tool".to_string(),
            kind: PayloadKind::File,
            link_target: None,
            size: body.len() as u64,
            mode: 0o755,
            sha256: format!("{:x}", Sha256::digest(body)),
        }
    }

    fn planned_file(source: &str, destination: &str, body: &[u8]) -> ExtractFile {
        ExtractFile {
            source: source.to_string(),
            destination: destination.to_string(),
            kind: PayloadKind::File,
            link_target: None,
            size: body.len() as u64,
            mode: 0o755,
            sha256: format!("{:x}", Sha256::digest(body)),
        }
    }

    fn planned_symlink(source: &str, destination: &str, target: &str, sha256: &str) -> ExtractFile {
        ExtractFile {
            source: source.to_string(),
            destination: destination.to_string(),
            kind: PayloadKind::Symlink,
            link_target: Some(target.to_string()),
            size: 0,
            mode: 0o777,
            sha256: sha256.to_string(),
        }
    }

    fn tar_bytes(path: &str, body: &[u8], entry_type: tar::EntryType) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(entry_type);
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, path, Cursor::new(body))
                .unwrap();
            builder.finish().unwrap();
        }
        bytes
    }

    fn tar_with_links(links: &[(&str, &str)], files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            for (path, target) in links {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_mode(0o777);
                header.set_link_name(target).unwrap();
                header.set_cksum();
                builder
                    .append_data(&mut header, path, std::io::empty())
                    .unwrap();
            }
            for (path, body) in files {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, Cursor::new(body))
                    .unwrap();
            }
            builder.finish().unwrap();
        }
        bytes
    }

    fn reversed_symlink_chain_tar(body: &[u8]) -> Vec<u8> {
        tar_with_links(
            &[
                ("root/libtool.so", "libtool.so.1"),
                ("root/libtool.so.1", "libtool.so.1.2"),
            ],
            &[("root/libtool.so.1.2", body)],
        )
    }

    fn reversed_symlink_chain_plan(body: &[u8]) -> ExtractionPlan {
        let sha256 = format!("{:x}", Sha256::digest(body));
        ExtractionPlan {
            format: ArtifactFormat::TarGzip,
            files: vec![
                planned_symlink("root/libtool.so", "libtool.so", "libtool.so.1", &sha256),
                planned_symlink(
                    "root/libtool.so.1",
                    "libtool.so.1",
                    "libtool.so.1.2",
                    &sha256,
                ),
                planned_file("root/libtool.so.1.2", "libtool.so.1.2", body),
            ],
            max_entries: 3,
            max_expanded_bytes: body.len() as u64,
        }
    }

    fn plan(body: &[u8]) -> ExtractionPlan {
        ExtractionPlan {
            format: ArtifactFormat::TarGzip,
            files: vec![payload(body)],
            max_entries: 4,
            max_expanded_bytes: 1024,
        }
    }

    fn archive_path(label: &str, extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "echo-extract-{label}-{}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn selected_file_extracts_and_verifies() {
        let body = b"tiny runner";
        let root = std::env::temp_dir().join(format!("echo-extract-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        extract_tar(
            Cursor::new(tar_bytes("root/bin/tool", body, tar::EntryType::Regular)),
            &root,
            &plan(body),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(root.join("tool")).unwrap(), body);
    }

    #[cfg(unix)]
    #[test]
    fn chained_symlinks_do_not_depend_on_archive_order() {
        let body = b"shared library";
        let root = std::env::temp_dir().join(format!("echo-extract-chain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        extract_tar(
            Cursor::new(reversed_symlink_chain_tar(body)),
            &root,
            &reversed_symlink_chain_plan(body),
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(
            fs::read_link(root.join("libtool.so")).unwrap(),
            Path::new("libtool.so.1")
        );
        assert_eq!(
            fs::read_link(root.join("libtool.so.1")).unwrap(),
            Path::new("libtool.so.1.2")
        );
        assert_eq!(fs::read(root.join("libtool.so")).unwrap(), body);
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_symlink_graphs_are_rejected() {
        let root =
            std::env::temp_dir().join(format!("echo-extract-link-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let sha256 = "0".repeat(64);
        let cycle = ExtractionPlan {
            format: ArtifactFormat::TarGzip,
            files: vec![
                planned_symlink("root/a", "a", "b", &sha256),
                planned_symlink("root/b", "b", "a", &sha256),
            ],
            max_entries: 2,
            max_expanded_bytes: 0,
        };
        let error = extract_tar(
            Cursor::new(tar_with_links(&[("root/a", "b"), ("root/b", "a")], &[])),
            &root,
            &cycle,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(error, InstallError::UnsafeArchive(message) if message.contains("cycle")));

        let unselected = ExtractionPlan {
            files: vec![planned_symlink("root/a", "a", "missing", &sha256)],
            max_entries: 1,
            ..cycle
        };
        let error = extract_tar(
            Cursor::new(tar_with_links(&[("root/a", "missing")], &[])),
            &root,
            &unselected,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            matches!(error, InstallError::UnsafeArchive(message) if message.contains("not selected"))
        );

        let body = b"shared library";
        let sha256 = format!("{:x}", Sha256::digest(body));
        let mut target_plan = ExtractionPlan {
            format: ArtifactFormat::TarGzip,
            files: vec![
                planned_symlink("root/a", "a", "file", &sha256),
                planned_file("root/file", "file", body),
            ],
            max_entries: 2,
            max_expanded_bytes: body.len() as u64,
        };
        let _ = fs::remove_dir_all(&root);
        let changed = extract_tar(
            Cursor::new(tar_with_links(
                &[("root/a", "other")],
                &[("root/file", body)],
            )),
            &root,
            &target_plan,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            matches!(changed, InstallError::UnsafeArchive(message) if message.contains("changed target"))
        );

        target_plan.files[0].link_target = Some("../file".to_string());
        let _ = fs::remove_dir_all(&root);
        let escaping = extract_tar(
            Cursor::new(tar_with_links(
                &[("root/a", "../file")],
                &[("root/file", body)],
            )),
            &root,
            &target_plan,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(escaping, InstallError::UnsafeArchive(_)));
    }

    #[cfg(unix)]
    #[test]
    fn flattened_symlink_target_must_match_selected_destination() {
        let body = b"shared library";
        let sha256 = format!("{:x}", Sha256::digest(body));
        let plan = ExtractionPlan {
            format: ArtifactFormat::TarGzip,
            files: vec![
                planned_symlink(
                    "root/libtool.so",
                    "aliases/libtool.so",
                    "libtool.so.1",
                    &sha256,
                ),
                planned_file("root/libtool.so.1", "libtool.so.1", body),
            ],
            max_entries: 2,
            max_expanded_bytes: body.len() as u64,
        };
        let root = std::env::temp_dir().join(format!(
            "echo-extract-link-destination-{}",
            std::process::id()
        ));
        let error = extract_tar(
            Cursor::new(tar_with_links(
                &[("root/libtool.so", "libtool.so.1")],
                &[("root/libtool.so.1", body)],
            )),
            &root,
            &plan,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            matches!(error, InstallError::UnsafeArchive(message) if message.contains("destination changed"))
        );
    }

    #[test]
    fn gzip_and_bzip2_decoder_boundaries_extract_the_same_payload() {
        let body = b"compressed runner";
        let tar = tar_bytes("root/bin/tool", body, tar::EntryType::Regular);
        let gzip = archive_path("gzip", "tar.gz");
        let mut encoder = flate2::write::GzEncoder::new(
            fs::File::create(&gzip).unwrap(),
            flate2::Compression::default(),
        );
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap();
        let gzip_root = archive_path("gzip-root", "dir");
        let _ = fs::remove_dir_all(&gzip_root);
        let gzip_plan = plan(body);
        extract_archive(&gzip, &gzip_root, &gzip_plan, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(gzip_root.join("tool")).unwrap(), body);

        let bzip = archive_path("bzip", "tar.bz2");
        let mut encoder = bzip2::write::BzEncoder::new(
            fs::File::create(&bzip).unwrap(),
            bzip2::Compression::best(),
        );
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap();
        let bzip_root = archive_path("bzip-root", "dir");
        let _ = fs::remove_dir_all(&bzip_root);
        let bzip_plan = ExtractionPlan {
            format: ArtifactFormat::TarBzip2,
            ..plan(body)
        };
        extract_archive(&bzip, &bzip_root, &bzip_plan, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(bzip_root.join("tool")).unwrap(), body);
    }

    #[test]
    fn traversal_special_files_and_limits_are_rejected() {
        assert!(safe_relative(Path::new("../escape")).is_err());
        assert!(safe_relative(Path::new("/absolute")).is_err());
        let body = b"x";
        let root = std::env::temp_dir().join(format!("echo-extract-bad-{}", std::process::id()));
        let special = tar_bytes("root/bin/tool", &[], tar::EntryType::Fifo);
        assert!(matches!(
            extract_tar(
                Cursor::new(special),
                &root,
                &plan(body),
                &AtomicBool::new(false)
            ),
            Err(InstallError::UnsafeArchive(_))
        ));
        let limited = ExtractionPlan {
            max_entries: 0,
            ..plan(body)
        };
        assert!(matches!(
            extract_tar(
                Cursor::new(tar_bytes("root/bin/tool", body, tar::EntryType::Regular)),
                &root,
                &limited,
                &AtomicBool::new(false)
            ),
            Err(InstallError::UnsafeArchive(_))
        ));
        let limited = ExtractionPlan {
            max_expanded_bytes: 0,
            ..plan(body)
        };
        assert!(matches!(
            extract_tar(
                Cursor::new(tar_bytes("root/bin/tool", body, tar::EntryType::Regular)),
                &root,
                &limited,
                &AtomicBool::new(false)
            ),
            Err(InstallError::UnsafeArchive(_))
        ));
    }

    #[test]
    fn wrong_payload_hash_and_missing_required_file_fail() {
        let body = b"tiny runner";
        let root = std::env::temp_dir().join(format!("echo-extract-hash-{}", std::process::id()));
        let mut wrong = plan(body);
        wrong.files[0].sha256 = "0".repeat(64);
        assert!(matches!(
            extract_tar(
                Cursor::new(tar_bytes("root/bin/tool", body, tar::EntryType::Regular)),
                &root,
                &wrong,
                &AtomicBool::new(false)
            ),
            Err(InstallError::Payload(_))
        ));
        let empty = tar_bytes("root/other", b"ignored", tar::EntryType::Regular);
        assert!(matches!(
            extract_tar(
                Cursor::new(empty),
                &root,
                &plan(body),
                &AtomicBool::new(false)
            ),
            Err(InstallError::Payload(_))
        ));
    }
}
