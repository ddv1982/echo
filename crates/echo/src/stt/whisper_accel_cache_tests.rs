use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::*;

static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

fn digest(value: char) -> Sha256Digest {
    Sha256Digest::parse(value.to_string().repeat(64)).unwrap()
}

fn uuid(value: char) -> UuidDigest {
    UuidDigest::parse(value.to_string().repeat(32)).unwrap()
}

fn receipt() -> StableVulkanReceipt {
    StableVulkanReceipt {
        backend: "vulkan".to_string(),
        vendor_id: 0x8086,
        device_id: 0x46a6,
        api_version: 1,
        driver_version: 2,
        device_uuid: uuid('1'),
        driver_uuid: uuid('2'),
        pipeline_cache_uuid: uuid('3'),
    }
}

fn fingerprint() -> DriverIcdFingerprint {
    DriverIcdFingerprint {
        drm_driver: "i915".to_string(),
        icd_manifest_sha256: digest('4'),
        icd_library_sha256: digest('5'),
    }
}

fn key() -> LocalSelectionKey {
    LocalSelectionKey::derive(
        &ExecutionArtifactId::parse("6".repeat(64)).unwrap(),
        &InferenceContractId::parse("7".repeat(64)).unwrap(),
        &receipt(),
        &fingerprint(),
    )
    .unwrap()
}

fn scratch(label: &str) -> PathBuf {
    let count = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "echo-whisper-local-{label}-{}-{count}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn eligible(key: LocalSelectionKey, observed_at: u64) -> NewCalibrationObservation {
    let observed = VulkanReceiptObservation {
        stable: receipt(),
        selected_index: 4,
    };
    NewCalibrationObservation {
        key,
        verdict: CalibrationVerdict::GpuEligible,
        cpu_infer_ms: 200,
        gpu_infer_ms: Some(100),
        transcript_parity: Some(true),
        ready_receipt: Some(observed.clone()),
        result_receipt: Some(observed),
        observed_at,
    }
}

fn route(root: &Path, observed_at: u64) -> NewLocalRouteObservation {
    for (name, contents) in [
        ("driver.json", b"manifest".as_slice()),
        ("driver.so", b"library".as_slice()),
    ] {
        let path = root.join(name);
        if !path.exists() {
            fs::write(path, contents).unwrap();
        }
    }
    let execution_artifact_id = ExecutionArtifactId::parse("6".repeat(64)).unwrap();
    let inference_contract_id = InferenceContractId::parse("7".repeat(64)).unwrap();
    let receipt = receipt();
    let fingerprint = fingerprint();
    let key = LocalSelectionKey::derive(
        &execution_artifact_id,
        &inference_contract_id,
        &receipt,
        &fingerprint,
    )
    .unwrap();
    NewLocalRouteObservation {
        execution_artifact_id,
        inference_contract_id,
        key,
        stable_receipt: receipt.clone(),
        ready_receipt: VulkanReceiptObservation {
            stable: receipt,
            selected_index: 0,
        },
        fingerprint,
        manifest_path: root.join("driver.json"),
        library_path: root.join("driver.so"),
        observed_at,
    }
}

#[test]
fn key_changes_for_identity_but_not_selected_index() {
    let base = key();
    let first = VulkanReceiptObservation {
        stable: receipt(),
        selected_index: 0,
    };
    let second = VulkanReceiptObservation {
        stable: receipt(),
        selected_index: 9,
    };
    assert_eq!(first.stable, second.stable);
    assert_eq!(base, key());

    let mut changed = receipt();
    changed.driver_uuid = uuid('8');
    assert_ne!(
        base,
        LocalSelectionKey::derive(
            &ExecutionArtifactId::parse("6".repeat(64)).unwrap(),
            &InferenceContractId::parse("7".repeat(64)).unwrap(),
            &changed,
            &fingerprint(),
        )
        .unwrap()
    );
}

#[test]
fn immutable_records_fold_deterministically_and_quarantine_expires() {
    let root = scratch("fold");
    let store = LocalSelectionStore::at(root);
    let key = key();
    let older = store
        .append_calibration(eligible(key.clone(), 100))
        .unwrap();
    let newer = store
        .append_calibration(eligible(key.clone(), 200))
        .unwrap();
    let quarantine = store
        .append_quarantine(key.clone(), QuarantineReason::ReceiptMismatch, 250)
        .unwrap();

    let active = store.snapshot(&key, 251).unwrap();
    assert_eq!(active.latest_calibration, Some(newer));
    assert_eq!(active.active_quarantine, Some(quarantine));
    assert_ne!(active.latest_calibration, Some(older));
    assert!(store
        .snapshot(&key, 250 + MAX_QUARANTINE_LIFETIME_SECS)
        .unwrap()
        .active_quarantine
        .is_none());
}

#[test]
fn corrupt_record_is_preserved_and_fails_closed() {
    let root = scratch("corrupt");
    let store = LocalSelectionStore::at(root.clone());
    let key = key();
    store
        .append_calibration(eligible(key.clone(), 100))
        .unwrap();
    let corrupt = root
        .join("keys")
        .join(key.as_str())
        .join("calibration")
        .join("ffffffffffffffffffffffffffffffff.json");
    fs::write(&corrupt, b"{not-json\n").unwrap();

    assert!(store.snapshot(&key, 101).is_err());
    assert_eq!(fs::read(&corrupt).unwrap(), b"{not-json\n");
}

#[test]
fn unpublished_temporary_record_is_not_visible() {
    let root = scratch("temporary");
    let store = LocalSelectionStore::at(root.clone());
    let key = key();
    let directory = root.join("keys").join(key.as_str()).join("calibration");
    fs::create_dir_all(&directory).unwrap();
    let temporary = directory.join(".interrupted.1.tmp");
    fs::write(&temporary, b"{partial").unwrap();

    assert_eq!(
        store.snapshot(&key, 100).unwrap(),
        LocalSelectionSnapshot {
            latest_calibration: None,
            active_quarantine: None,
        }
    );
    assert_eq!(fs::read(&temporary).unwrap(), b"{partial");
}

#[test]
fn process_writer_entry() {
    let Some(root) = std::env::var_os("ECHO_ACCEL_STORE_PROCESS_ROOT") else {
        return;
    };
    let observed_at = std::env::var("ECHO_ACCEL_STORE_PROCESS_AT")
        .unwrap()
        .parse()
        .unwrap();
    LocalSelectionStore::at(PathBuf::from(root))
        .append_calibration(eligible(key(), observed_at))
        .unwrap();
}

#[test]
fn two_processes_publish_separate_complete_records() {
    let root = scratch("processes");
    let test_binary = std::env::current_exe().unwrap();
    let mut first = Command::new(&test_binary)
        .args([
            "--exact",
            "stt::whisper_accel_cache::tests::process_writer_entry",
        ])
        .env("ECHO_ACCEL_STORE_PROCESS_ROOT", &root)
        .env("ECHO_ACCEL_STORE_PROCESS_AT", "100")
        .spawn()
        .unwrap();
    let mut second = Command::new(test_binary)
        .args([
            "--exact",
            "stt::whisper_accel_cache::tests::process_writer_entry",
        ])
        .env("ECHO_ACCEL_STORE_PROCESS_ROOT", &root)
        .env("ECHO_ACCEL_STORE_PROCESS_AT", "200")
        .spawn()
        .unwrap();
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());

    let snapshot = LocalSelectionStore::at(root.clone())
        .snapshot(&key(), 201)
        .unwrap();
    assert_eq!(
        snapshot.latest_calibration.map(|record| record.observed_at),
        Some(200)
    );
    let records = fs::read_dir(root.join("keys").join(key().as_str()).join("calibration"))
        .unwrap()
        .count();
    assert_eq!(records, 2);
}

#[test]
fn routes_are_immutable_and_scope_lease_is_exclusive() {
    let root = scratch("routes");
    let store = LocalSelectionStore::at(root.clone());
    let older = route(&root, 100);
    let execution = older.execution_artifact_id.clone();
    let inference = older.inference_contract_id.clone();
    store.append_route(older).unwrap();
    let newer = store.append_route(route(&root, 200)).unwrap();
    assert_eq!(
        store.latest_route(&execution, &inference).unwrap(),
        Some(newer)
    );

    let first = store.try_claim(&execution, &inference).unwrap().unwrap();
    assert!(store.try_claim(&execution, &inference).unwrap().is_none());
    drop(first);
    assert!(store.try_claim(&execution, &inference).unwrap().is_some());
}

#[test]
fn model_view_is_disposable_and_rotates_on_file_change() {
    let root = scratch("model-view");
    let model = root.join("model.bin");
    let vad = root.join("vad.bin");
    fs::write(&model, b"model-a").unwrap();
    fs::write(&vad, b"vad-a").unwrap();
    let store = LocalSelectionStore::at(root.join("state"));
    let route = route(&root, 100);
    let execution = route.execution_artifact_id.clone();
    let inference = route.inference_contract_id.clone();
    let key = route.key.clone();
    store.append_route(route).unwrap();
    store
        .append_calibration(eligible(key.clone(), 100))
        .unwrap();
    store
        .write_model_view(
            &model,
            Some(&vad),
            execution.clone(),
            inference.clone(),
            key.clone(),
        )
        .unwrap();
    assert_eq!(
        store
            .model_view(&model, Some(&vad), Some(&execution))
            .unwrap()
            .unwrap()
            .key,
        key
    );
    let mut timings = (0..100)
        .map(|_| {
            let started = std::time::Instant::now();
            assert!(store
                .model_view(&model, Some(&vad), Some(&execution))
                .unwrap()
                .is_some());
            started.elapsed()
        })
        .collect::<Vec<_>>();
    timings.sort();
    println!("cached_model_view_p95_us={}", timings[94].as_micros());
    assert!(timings[94] <= Duration::from_millis(25));
    std::thread::sleep(Duration::from_millis(2));
    fs::write(&vad, b"vad-b").unwrap();
    assert!(store
        .model_view(&model, Some(&vad), Some(&execution))
        .unwrap()
        .is_none());
    store
        .write_model_view(
            &model,
            Some(&vad),
            execution.clone(),
            inference,
            key.clone(),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(2));
    fs::write(&model, b"model-b").unwrap();
    assert!(store
        .model_view(&model, Some(&vad), Some(&execution))
        .unwrap()
        .is_none());
}

#[test]
fn changed_driver_rotates_route_without_deleting_history() {
    let root = scratch("driver-rotation");
    let store = LocalSelectionStore::at(root.join("state"));
    let old = route(&root, 100);
    let execution = old.execution_artifact_id.clone();
    let inference = old.inference_contract_id.clone();
    store.append_route(old).unwrap();
    fs::write(root.join("driver.so"), b"changed library").unwrap();
    assert!(store
        .latest_route(&execution, &inference)
        .unwrap()
        .is_none());

    let mut new = route(&root, 200);
    new.fingerprint.icd_library_sha256 = digest('9');
    new.key = LocalSelectionKey::derive(
        &new.execution_artifact_id,
        &new.inference_contract_id,
        &new.stable_receipt,
        &new.fingerprint,
    )
    .unwrap();
    let selected = store.append_route(new).unwrap();
    assert_eq!(
        store.latest_route(&execution, &inference).unwrap(),
        Some(selected)
    );
    assert_eq!(
        fs::read_dir(
            store
                .root()
                .join("scopes")
                .join(execution.as_str())
                .join(inference.as_str())
                .join("routes")
        )
        .unwrap()
        .count(),
        2
    );
}

#[test]
fn completed_job_history_does_not_exhaust_pending_queue() {
    let root = scratch("completed-jobs");
    let store = LocalSelectionStore::at(root.join("state"));
    for index in 0..257_u128 {
        let job_id = format!("{index:032x}");
        store
            .publish_job(&job_id, &serde_json::json!({"jobId": job_id}))
            .unwrap();
        store
            .publish_job_result(&job_id, &serde_json::json!({"jobId": job_id}))
            .unwrap();
    }
    let pending = "ffffffffffffffffffffffffffffffff";
    store
        .publish_job(pending, &serde_json::json!({"jobId": pending}))
        .unwrap();
    assert_eq!(store.job_paths().unwrap().len(), 1);
}
