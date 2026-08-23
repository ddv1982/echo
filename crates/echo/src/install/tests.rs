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
    assert!(release.join("verified.json").is_file());
    verified_payloads().lock().unwrap().clear();
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
    let staged_payload = stage.join("payload").join(spec.artifact_name);
    fs::File::create(&staged_payload)
        .unwrap()
        .set_len(spec.artifact_size)
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_payload, fs::Permissions::from_mode(0o644)).unwrap();
    }
    let fresh = store
        .activate_with(spec, expected_files(id), &stage, &OperationId::fixture("2"))
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
