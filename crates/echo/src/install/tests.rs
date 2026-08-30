use super::*;
use filetime::{set_file_mtime, FileTime};
use std::collections::{BTreeMap, VecDeque};
use std::io::Cursor;
use std::sync::Mutex;

struct UnlimitedDisk;

impl DiskSpace for UnlimitedDisk {
    fn available_bytes(&self, _: &Path) -> Result<Option<u64>, InstallError> {
        Ok(None)
    }
}

struct AcceptProbe;

impl RuntimeProbe for AcceptProbe {
    fn probe(&self, _: ComponentId, _: &Path, _: &AtomicBool) -> Result<(), InstallError> {
        Ok(())
    }
}

struct RejectProbe;

impl RuntimeProbe for RejectProbe {
    fn probe(&self, _: ComponentId, _: &Path, _: &AtomicBool) -> Result<(), InstallError> {
        Err(InstallError::Probe(
            "fixture runtime is incompatible".to_string(),
        ))
    }
}

struct MutatingProbe;

impl RuntimeProbe for MutatingProbe {
    fn probe(&self, _: ComponentId, binary: &Path, _: &AtomicBool) -> Result<(), InstallError> {
        let size = fs::metadata(binary)?.len();
        fs::write(binary, vec![b'x'; size as usize])?;
        Ok(())
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

fn installed_direct_fixture(label: &str) -> (ManagedStore, catalog::ComponentSpec, PathBuf) {
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
    let root = scratch(label);
    let transport = FixtureTransport(Mutex::new(VecDeque::from([body])));
    let installer = Installer {
        store: ManagedStore::new(&root),
        transport: &transport,
        disk: &UnlimitedDisk,
        probe: &AcceptProbe,
    };
    let record = installer
        .ensure_spec(
            &spec,
            false,
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
    let release = installer
        .store
        .component_dir(spec.id)
        .join("releases")
        .join(record.release);
    (installer.store, spec, release)
}

fn write_payload_fixture(root: &Path, relative_path: &str, contents: &[u8]) -> InstalledFile {
    let path = root.join(relative_path);
    fs::write(&path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    InstalledFile {
        relative_path: relative_path.to_string(),
        size: contents.len() as u64,
        sha256: format!("{:x}", Sha256::digest(contents)),
        mode: 0o644,
        kind: PayloadKind::File,
        link_target: None,
    }
}

#[test]
fn operation_ids_do_not_reuse_generation_names() {
    let first = OperationId::new();
    let second = OperationId::new();
    assert_ne!(first, second);
    assert!(first.as_str().contains(&std::process::id().to_string()));
}

#[cfg(unix)]
#[test]
#[ignore = "needs the pinned Vulkan runtime archive"]
fn pinned_vulkan_runtime_archive_installs() {
    let archive = std::env::var_os("ECHO_PINNED_VULKAN_ARCHIVE")
        .map(PathBuf::from)
        .expect("ECHO_PINNED_VULKAN_ARCHIVE");
    let body = fs::read(&archive).unwrap();
    let spec = component(ComponentId::WhisperVulkanRuntime);
    assert_eq!(body.len() as u64, spec.artifact_size);
    assert_eq!(format!("{:x}", Sha256::digest(&body)), spec.artifact_sha256);

    let root = scratch("pinned-vulkan-runtime");
    let transport = FixtureTransport(Mutex::new(VecDeque::from([body])));
    // The real probe, matching the CPU archive test. Accepting any payload here
    // is what let a runtime whose whisper-cli cannot resolve its own libraries
    // pass every test while failing every real install.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let probe = CommandRuntimeProbe;
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let probe = AcceptProbe;
    let installer = Installer {
        store: ManagedStore::new(&root),
        transport: &transport,
        disk: &UnlimitedDisk,
        probe: &probe,
    };
    let record = installer
        .ensure_component(
            ComponentId::WhisperVulkanRuntime,
            false,
            &OperationId::fixture("92"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
    assert_eq!(record.artifact_sha256, spec.artifact_sha256);

    let payload = installer
        .store
        .active_root(ComponentId::WhisperVulkanRuntime)
        .unwrap()
        .unwrap();
    // Extraction flattens payload members to their basename, so the archive's
    // runtime/ prefix does not survive into the installed tree.
    assert!(payload.join("libggml-vulkan.so").is_file());
    assert!(payload.join("echo-whisper-runtime-probe").is_file());
    assert!(payload.join("whisper-cli").is_file());
    assert_eq!(
        fs::read_link(payload.join("libwhisper.so")).unwrap(),
        Path::new("libwhisper.so.1")
    );
    installer
        .store
        .verify(ComponentId::WhisperVulkanRuntime)
        .unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "downloads the pinned Whisper runtime archive"]
fn pinned_whisper_runtime_archive_installs() {
    let archive = std::env::var_os("ECHO_PINNED_WHISPER_ARCHIVE")
        .map(PathBuf::from)
        .expect("ECHO_PINNED_WHISPER_ARCHIVE");
    let body = fs::read(&archive).unwrap();
    let spec = component(ComponentId::WhisperRuntime);
    assert_eq!(body.len() as u64, spec.artifact_size);
    assert_eq!(format!("{:x}", Sha256::digest(&body)), spec.artifact_sha256);

    let root = scratch("pinned-whisper-runtime");
    let transport = FixtureTransport(Mutex::new(VecDeque::from([body])));
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let probe = CommandRuntimeProbe;
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let probe = AcceptProbe;
    let installer = Installer {
        store: ManagedStore::new(&root),
        transport: &transport,
        disk: &UnlimitedDisk,
        probe: &probe,
    };
    let record = installer
        .ensure_component(
            ComponentId::WhisperRuntime,
            false,
            &OperationId::fixture("91"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
    assert_eq!(record.version, spec.version);
    assert_eq!(record.artifact_sha256, spec.artifact_sha256);

    let payload = installer
        .store
        .active_root(ComponentId::WhisperRuntime)
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read_link(payload.join("libwhisper.so")).unwrap(),
        Path::new("libwhisper.so.1")
    );
    assert_eq!(
        fs::read_link(payload.join("libwhisper.so.1")).unwrap(),
        Path::new("libwhisper.so.1.9.2")
    );
    assert!(payload.join("whisper-server").is_file());
    installer.store.verify(ComponentId::WhisperRuntime).unwrap();
}

#[cfg(unix)]
#[test]
fn legacy_whisper_runtime_without_server_stays_usable_and_removable() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let root = scratch("legacy-whisper-runtime");
    let store = ManagedStore::new(&root);
    let id = ComponentId::WhisperRuntime;
    let spec = component(id);
    let release_name = format!("{}-1e9ac", spec.artifact_sha256);
    let release = root
        .join("managed/components")
        .join(id.as_str())
        .join("releases")
        .join(&release_name);
    let payload = release.join("payload");
    fs::create_dir_all(&payload).unwrap();
    let files = expected_files(id)
        .into_iter()
        .filter(|file| file.relative_path != "whisper-server")
        .collect::<Vec<_>>();
    for file in &files {
        let path = payload.join(&file.relative_path);
        match file.kind {
            PayloadKind::File => {
                fs::File::create(&path).unwrap().set_len(file.size).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(file.mode)).unwrap();
            }
            PayloadKind::Symlink => {
                symlink(file.link_target.as_deref().unwrap(), &path).unwrap();
            }
        }
    }
    let record = ActivationRecord {
        schema_version: 1,
        component: id,
        version: spec.version.to_string(),
        release: release_name,
        artifact_sha256: spec.artifact_sha256.to_string(),
        files,
    };
    let raw = serde_json::to_vec_pretty(&record).unwrap();
    echo_core::write_atomic(&release.join("receipt.json"), &raw).unwrap();
    echo_core::write_atomic(&store.active_path(id), &raw).unwrap();
    trust_payload_fixture(&payload, &record.files);

    assert!(matches!(
        store.status(id, false),
        ManagedComponentState::Ready { .. }
    ));
    assert!(store.candidate_root(id).is_some());
    assert!(store.active_root_leased(id).unwrap().is_some());
    store.remove(id).unwrap();
    assert!(store.candidate_root(id).is_none());
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
    let transport = FixtureTransport(Mutex::new(VecDeque::from([body.clone(), body.clone()])));
    let installer = Installer {
        store: ManagedStore::new(&root),
        transport: &transport,
        disk: &UnlimitedDisk,
        probe: &AcceptProbe,
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
    let release = installer
        .store
        .component_dir(spec.id)
        .join("releases")
        .join(&active.release);
    assert!(!release.join("verified.json").exists());
    verified_payloads()
        .lock()
        .unwrap()
        .remove(&release.join("payload"));
    assert!(matches!(
        installer
            .store
            .status_with(&spec, &expected_files_for(&spec), false),
        ManagedComponentState::Ready { .. }
    ));
    let payload = release.join("payload/tiny.bin");
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
fn cached_status_detects_same_size_mutation_with_restored_mtime() {
    let (store, spec, release) = installed_direct_fixture("restored-mtime");
    let payload_root = release.join("payload");
    let payload = payload_root.join(spec.artifact_name);
    let original_mtime = FileTime::from_last_modification_time(&fs::metadata(&payload).unwrap());

    fs::write(&payload, vec![b'x'; spec.artifact_size as usize]).unwrap();
    set_file_mtime(&payload, original_mtime).unwrap();
    assert_eq!(
        FileTime::from_last_modification_time(&fs::metadata(&payload).unwrap()),
        original_mtime
    );

    assert!(matches!(
        store.status_with(&spec, &expected_files_for(&spec), false),
        ManagedComponentState::NeedsRepair { .. }
    ));
}

#[test]
fn forged_legacy_verification_stamp_cannot_bypass_cold_verification() {
    let (store, spec, release) = installed_direct_fixture("forged-verification-stamp");
    let payload_root = release.join("payload");
    let payload = payload_root.join(spec.artifact_name);
    let original_mtime = FileTime::from_last_modification_time(&fs::metadata(&payload).unwrap());

    fs::write(&payload, vec![b'x'; spec.artifact_size as usize]).unwrap();
    set_file_mtime(&payload, original_mtime).unwrap();
    let modified = fs::metadata(&payload)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let forged = serde_json::json!({
        "schemaVersion": 1,
        "fingerprint": [[spec.artifact_name, spec.artifact_size, modified, null]]
    });
    echo_core::write_atomic(
        &release.join("verified.json"),
        &serde_json::to_vec_pretty(&forged).unwrap(),
    )
    .unwrap();
    verified_payloads().lock().unwrap().remove(&payload_root);

    assert!(matches!(
        store.status_with(&spec, &expected_files_for(&spec), false),
        ManagedComponentState::NeedsRepair { .. }
    ));
}

#[test]
#[ignore = "reads the managed component root named by ECHO_MANAGED_ROOT"]
fn managed_status_timing() {
    let root = std::env::var_os("ECHO_MANAGED_ROOT")
        .map(PathBuf::from)
        .expect("ECHO_MANAGED_ROOT must name the directory that contains managed/");
    let store = ManagedStore::new(root);
    verified_payloads().lock().unwrap().clear();
    let mut measured = 0;

    for spec in catalog::COMPONENTS {
        if !store.active_path(spec.id).exists() {
            continue;
        }
        let cold_started = Instant::now();
        let cold_state = store.status(spec.id, false);
        let cold = cold_started.elapsed();
        if !matches!(cold_state, ManagedComponentState::Ready { .. }) {
            println!(
                "managed_status component={} skipped={cold_state:?}",
                spec.id.as_str()
            );
            continue;
        }

        let warm_started = Instant::now();
        let warm_state = store.status(spec.id, false);
        let warm = warm_started.elapsed();
        assert!(
            matches!(warm_state, ManagedComponentState::Ready { .. }),
            "{} warm status was {warm_state:?}",
            spec.id.as_str()
        );
        println!(
            "managed_status component={} cold_us={} warm_us={}",
            spec.id.as_str(),
            cold.as_micros(),
            warm.as_micros()
        );
        measured += 1;
    }

    assert!(measured > 0, "ECHO_MANAGED_ROOT has no active components");
}

#[test]
fn runtime_probe_failure_never_activates_staging() {
    let body = b"fake executable".to_vec();
    let digest: &'static str = Box::leak(format!("{:x}", Sha256::digest(&body)).into_boxed_str());
    let spec = catalog::ComponentSpec {
        id: ComponentId::WhisperRuntime,
        label: "Fixture runtime",
        version: "fixture",
        url: "https://fixture.invalid/runtime",
        artifact_name: "whisper-cli",
        artifact_size: body.len() as u64,
        artifact_sha256: digest,
        installed_bytes: body.len() as u64,
        format: ArtifactFormat::Direct,
        inventory_key: None,
    };
    let root = scratch("runtime-probe");
    let transport = FixtureTransport(Mutex::new(VecDeque::from([body])));
    let installer = Installer {
        store: ManagedStore::new(&root),
        transport: &transport,
        disk: &UnlimitedDisk,
        probe: &RejectProbe,
    };
    let error = installer
        .ensure_spec(
            &spec,
            false,
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
    assert!(matches!(error, InstallError::Probe(_)));
    assert!(installer.store.read_active(spec.id).unwrap().is_none());
}

#[test]
fn runtime_probe_mutation_never_activates_payload() {
    let body = b"fake executable".to_vec();
    let digest: &'static str = Box::leak(format!("{:x}", Sha256::digest(&body)).into_boxed_str());
    let spec = catalog::ComponentSpec {
        id: ComponentId::WhisperRuntime,
        label: "Fixture runtime",
        version: "fixture",
        url: "https://fixture.invalid/runtime",
        artifact_name: "whisper-cli",
        artifact_size: body.len() as u64,
        artifact_sha256: digest,
        installed_bytes: body.len() as u64,
        format: ArtifactFormat::Direct,
        inventory_key: None,
    };
    let root = scratch("mutating-runtime-probe");
    let transport = FixtureTransport(Mutex::new(VecDeque::from([body])));
    let installer = Installer {
        store: ManagedStore::new(&root),
        transport: &transport,
        disk: &UnlimitedDisk,
        probe: &MutatingProbe,
    };

    let error = installer
        .ensure_spec(
            &spec,
            false,
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();

    assert!(matches!(error, InstallError::Payload(ref reason) if reason.contains("corrupt")));
    assert!(installer.store.read_active(spec.id).unwrap().is_none());
    assert!(!installer
        .store
        .component_dir(spec.id)
        .join("releases")
        .exists());
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
fn cached_status_detects_same_size_corruption_and_persists_repair_state() {
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
    fs::write(
        payload.join(spec.artifact_name),
        vec![0u8; spec.artifact_size as usize],
    )
    .unwrap();
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
    assert!(matches!(
        store.status(id, false),
        ManagedComponentState::NeedsRepair { .. }
    ));
    assert!(store.verify(id).is_err());
    assert!(matches!(
        store.status(id, false),
        ManagedComponentState::NeedsRepair { .. }
    ));
    assert!(store.candidate_root(id).is_none());
    fs::remove_file(payload.join(spec.artifact_name)).unwrap();
    assert!(matches!(
        store.status(id, false),
        ManagedComponentState::NeedsRepair { .. }
    ));
    let external = root.join(spec.artifact_name);
    fs::write(&external, b"manual sentinel").unwrap();
    store.remove(id).unwrap();
    store.remove(id).unwrap();
    assert_eq!(fs::read(external).unwrap(), b"manual sentinel");
}

#[test]
fn cancellation_during_copy_or_before_activation_never_activates() {
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
    for phase in [InstallPhase::Extracting, InstallPhase::Activating] {
        let root = scratch(match phase {
            InstallPhase::Extracting => "cancel-copy",
            InstallPhase::Activating => "cancel-activate",
            _ => unreachable!(),
        });
        let transport = FixtureTransport(Mutex::new(VecDeque::from([body.clone()])));
        let installer = Installer {
            store: ManagedStore::new(&root),
            transport: &transport,
            disk: &UnlimitedDisk,
            probe: &AcceptProbe,
        };
        let cancel = AtomicBool::new(false);
        let error = installer
            .ensure_spec(
                &spec,
                false,
                &OperationId::fixture("1"),
                &cancel,
                |progress| {
                    if progress.phase == phase {
                        cancel.store(true, Ordering::Relaxed);
                    }
                },
            )
            .unwrap_err();
        assert!(matches!(error, InstallError::Cancelled));
        assert!(installer.store.read_active(spec.id).unwrap().is_none());
    }
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
    echo_core::write_atomic(
        &old_root.join("receipt.json"),
        &serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    let stage = store.managed().join("staging/2").join(id.as_str());
    fs::create_dir_all(stage.join("payload")).unwrap();
    let files = vec![write_payload_fixture(
        &stage.join("payload"),
        spec.artifact_name,
        b"fresh payload",
    )];
    let fresh = store
        .activate_with(spec, files, &stage, &OperationId::fixture("2"))
        .unwrap();
    assert_ne!(fresh.release, old_release);
    assert!(
        !old_root.exists(),
        "the old generation is collected after pointer swap"
    );
    assert_eq!(
        store.read_active(id).unwrap().unwrap().release,
        fresh.release
    );
}

#[test]
fn recovery_discards_owned_staging_and_inactive_releases_but_keeps_partials() {
    let root = scratch("recover");
    let id = ComponentId::SileroVad;
    let spec = component(id);
    let store = ManagedStore::new(&root);
    let release_name = format!("{}-99", spec.artifact_sha256);
    let release = store.component_dir(id).join("releases").join(&release_name);
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

    let operation = store.operation_shared().unwrap();
    assert!(store.recover().is_empty());
    assert!(release.exists());
    assert!(stage.exists());
    drop(operation);
    assert!(store.recover().is_empty());
    assert!(!release.exists());
    assert!(!stage.exists());
    assert_eq!(fs::read(partial).unwrap(), b"resume");
}

#[test]
fn a_superseded_digest_is_collected_and_never_blocks_removal() {
    // Rotating a catalogue digest leaves the previous generation on disk under
    // its old name. It used to be uncollectable and it poisoned remove(), so a
    // runtime version bump permanently broke the component's Remove button and
    // leaked its payload.
    let root = scratch("superseded-digest");
    let id = ComponentId::SileroVad;
    let spec = component(id);
    let store = ManagedStore::new(&root);

    let stale_release = format!("{}-7", "0".repeat(64));
    let stale_root = store
        .component_dir(id)
        .join("releases")
        .join(&stale_release);
    let stale_payload = stale_root.join("payload");
    fs::create_dir_all(&stale_payload).unwrap();
    let stale_file = InstalledFile {
        relative_path: "retired.bin".to_string(),
        size: 3,
        sha256: format!("{:x}", Sha256::digest(b"old")),
        mode: 0o644,
        kind: PayloadKind::File,
        link_target: None,
    };
    fs::write(stale_payload.join(&stale_file.relative_path), b"old").unwrap();
    let stale_record = ActivationRecord {
        schema_version: 1,
        component: id,
        version: "retired".to_string(),
        release: stale_release.clone(),
        artifact_sha256: "0".repeat(64),
        // The retired payload is nothing like today's catalogue, which is the
        // case a version bump produces.
        files: vec![stale_file],
    };
    let raw = serde_json::to_vec(&stale_record).unwrap();
    echo_core::write_atomic(&stale_root.join("receipt.json"), &raw).unwrap();
    echo_core::write_atomic(&store.active_path(id), &raw).unwrap();

    // Installing the current digest collects the generation it supersedes.
    let stage = store.managed().join("staging/8").join(id.as_str());
    fs::create_dir_all(stage.join("payload")).unwrap();
    let files = vec![write_payload_fixture(
        &stage.join("payload"),
        spec.artifact_name,
        b"current payload",
    )];
    let fresh = store
        .activate_with(spec, files, &stage, &OperationId::fixture("8"))
        .unwrap();
    assert!(
        !stale_root.exists(),
        "the superseded generation survived activation"
    );

    // And removal works rather than reporting a digest that no longer applies.
    store.remove(id).unwrap();
    assert!(!store
        .component_dir(id)
        .join("releases")
        .join(&fresh.release)
        .exists());
    assert!(!store.active_path(id).exists());
}

#[test]
fn removal_still_refuses_an_active_record_pointing_outside_the_store() {
    let root = scratch("superseded-escape");
    let id = ComponentId::SileroVad;
    let store = ManagedStore::new(&root);
    for release in [
        format!("{}-../sentinel", "0".repeat(64)),
        "not-a-digest-1".to_string(),
        format!("{}-", "0".repeat(64)),
    ] {
        let record = ActivationRecord {
            schema_version: 1,
            component: id,
            version: "x".to_string(),
            release: release.clone(),
            artifact_sha256: "0".repeat(64),
            files: expected_files(id),
        };
        echo_core::write_atomic(
            &store.active_path(id),
            &serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(store.remove(id).is_err(), "accepted {release}");
    }
}

#[test]
fn a_receipt_can_never_aim_deletion_outside_its_payload() {
    // Cleanup deletes exactly the files a release receipt names, so the
    // receipt is an input to rm. ensure_contained alone does not settle this:
    // starts_with is a component prefix test, and `payload/../../x` passes it.
    let root = scratch("receipt-escape");
    let id = ComponentId::SileroVad;
    let store = ManagedStore::new(&root);
    let victim = root.join("victim.txt");

    for declared in [
        "../../../../../../victim.txt",
        "/etc/passwd",
        "nested/../../../../../../victim.txt",
        "",
    ] {
        fs::write(&victim, b"a file outside the store").unwrap();
        let release_name = format!("{}-3", "0".repeat(64));
        let release = store.component_dir(id).join("releases").join(&release_name);
        fs::create_dir_all(release.join("payload")).unwrap();
        let record = ActivationRecord {
            schema_version: 1,
            component: id,
            version: "tampered".to_string(),
            release: release_name.clone(),
            artifact_sha256: "0".repeat(64),
            files: vec![InstalledFile {
                relative_path: declared.to_string(),
                size: 1,
                sha256: "0".repeat(64),
                mode: 0o644,
                kind: PayloadKind::File,
                link_target: None,
            }],
        };
        let raw = serde_json::to_vec(&record).unwrap();
        echo_core::write_atomic(&release.join("receipt.json"), &raw).unwrap();
        echo_core::write_atomic(&store.active_path(id), &raw).unwrap();

        assert!(store.remove(id).is_err(), "removal accepted {declared:?}");
        // The sweep leaves the active release alone, so it reports nothing
        // here. What matters either way is that nothing outside was touched.
        store.recover();
        assert!(
            victim.exists(),
            "{declared:?} deleted a file outside the store"
        );
        fs::remove_dir_all(store.component_dir(id)).unwrap();
        fs::remove_file(store.active_path(id)).unwrap();
    }
}
