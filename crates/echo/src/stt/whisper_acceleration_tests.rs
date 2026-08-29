use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::stt::{
    AdmissionGates, AdmissionIdentityKey, AdmissionTuning, AdmissionVerdict, CacheSeedArtifact,
    ModelAdmission, SharedRuntimeArtifacts, WhisperModelAsset, WhisperProtocol,
    WhisperRuntimeLaunch,
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
        stability_success: true,
        memory_evidence: true,
        memory_floor: true,
        swap_stable: true,
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

fn live_receipt(device_id: u32) -> WhisperVulkanReceipt {
    let mut value = device();
    value.device_id = device_id;
    receipt(&value)
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
    let seed = package.join("cache-seeds/pending/mesa_shader_cache");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::create_dir_all(&seed).unwrap();
    let runtime = runtime_dir.join("whisper-cli");
    let probe = runtime_dir.join("echo-whisper-runtime-probe");
    let cpu = root.join("whisper-cpu");
    let echo = root.join("echo-desktop");
    let model = root.join("ggml-small.bin");
    let vad = root.join("ggml-silero.bin");
    let icd_manifest = root.join("intel_icd.json");
    let icd_library = root.join("libvulkan_intel.so");
    executable(&runtime, b"vulkan runtime");
    executable(&probe, b"runtime probe");
    fs::write(runtime_dir.join("libwhisper.so.1.9.2"), b"whisper").unwrap();
    fs::write(runtime_dir.join("libwhisper.so.1"), b"whisper").unwrap();
    fs::write(runtime_dir.join("libggml.so.0.18.1"), b"ggml").unwrap();
    executable(&cpu, b"cpu runtime");
    executable(&echo, b"echo binary");
    fs::write(&model, format!("small model {label}")).unwrap();
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
        runtime_identity_sha256: whisper_runtime_launch(&runtime).identity_sha256.unwrap(),
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
    let mut record = ModelAdmission {
        identity,
        identity_key,
        evidence_sha256: "a".repeat(64),
        gates: gates(),
        verdict: AdmissionVerdict::Passed,
        accepted_at: NOW - 60,
        expires_at: NOW + 60,
        icd_manifest_path: icd_manifest.to_string_lossy().into_owned(),
        icd_library_path: icd_library.to_string_lossy().into_owned(),
        cache_seed: CacheSeedArtifact {
            relative_path: "cache-seeds/pending".to_string(),
            sha256: tree_sha256(&package.join("cache-seeds/pending")).unwrap(),
        },
    };
    let keyed_seed = format!("cache-seeds/{}", record.identity_key.as_str());
    fs::rename(
        package.join("cache-seeds/pending"),
        package.join(&keyed_seed),
    )
    .unwrap();
    record.cache_seed.relative_path = keyed_seed;
    let mut inventory = BTreeMap::new();
    collect_inventory(&package, &package, &mut inventory).unwrap();
    let set = AdmissionSet {
        schema_version: 2,
        shared: SharedRuntimeArtifacts {
            runtime_relative_path: "runtime/whisper-cli".to_string(),
            runtime_library_bindings: runtime_library_bindings(&runtime).unwrap(),
            probe_relative_path: "runtime/echo-whisper-runtime-probe".to_string(),
            probe_sha256: sha256_file(&probe).unwrap(),
        },
        records: vec![record],
        inventory: inventory.into_values().collect(),
    };
    fs::write(
        package.join("admission-set.json"),
        serde_json::to_vec_pretty(&set).unwrap(),
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
        allow_vad_retry: true,
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
    fn test_probe(
        _: &Path,
        _: &WhisperRuntimeLaunch,
        _: std::time::Duration,
    ) -> Result<WhisperVulkanReceipt, String> {
        Ok(live_receipt(0x46a6))
    }
    PackageSelection {
        package_root: &fixture.package,
        cache_root: &fixture.cache,
        echo_binary: &fixture.echo,
        echo_commit: "4444444444444444444444444444444444444444",
        managed_cpu: fixture.cpu_plan.clone(),
        host,
        now: NOW,
        require_package_ownership: false,
        receipt_probe: test_probe,
    }
}

fn read_set(fixture: &Fixture) -> AdmissionSet {
    AdmissionSet::from_bytes(&fs::read(fixture.package.join("admission-set.json")).unwrap())
        .unwrap()
}

fn write_set(fixture: &Fixture, mut set: AdmissionSet) {
    let mut inventory = BTreeMap::new();
    collect_inventory(&fixture.package, &fixture.package, &mut inventory).unwrap();
    set.inventory = inventory.into_values().collect();
    fs::write(
        fixture.package.join("admission-set.json"),
        serde_json::to_vec_pretty(&set).unwrap(),
    )
    .unwrap();
}

fn add_large_record(fixture: &mut Fixture) {
    let mut set = read_set(fixture);
    let large_model = fixture
        .cpu_plan
        .model
        .path
        .with_file_name("ggml-large-v3-turbo.bin");
    fs::write(&large_model, b"large turbo model").unwrap();
    let mut record = set.records[0].clone();
    record.identity.model_sha256 = sha256_file(&large_model).unwrap();
    record.identity.tuning.beam_size = 1;
    record.identity.tuning.best_of = 2;
    record.identity.tuning.no_fallback = true;
    record.identity_key = AdmissionIdentityKey::for_identity(&record.identity);
    let seed_path = format!("cache-seeds/{}", record.identity_key.as_str());
    copy_tree(
        &fixture
            .package
            .join(&set.records[0].cache_seed.relative_path),
        &fixture.package.join(&seed_path),
    )
    .unwrap();
    record.cache_seed.relative_path = seed_path;
    record.cache_seed.sha256 =
        tree_sha256(&fixture.package.join(&record.cache_seed.relative_path)).unwrap();
    set.records.push(record);
    write_set(fixture, set);
    fixture.cpu_plan.model.path = large_model;
    fixture.cpu_plan.model.name = "large-v3-turbo-q5_0".to_string();
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
fn production_decision_p95_is_bounded() {
    let plan = fixture("production-decision-p95").cpu_plan;
    let mut timings = (0..100)
        .map(|_| {
            let started = std::time::Instant::now();
            for _ in 0..100 {
                assert!(std::hint::black_box(production_whisper_decision(plan.clone())).is_none());
            }
            started.elapsed().as_nanos() / 100
        })
        .collect::<Vec<_>>();
    timings.sort();
    println!("production_decision_p95_ns={}", timings[94]);
}

#[test]
fn cache_identity_path_cannot_escape_through_a_symlink() {
    let fixture = fixture("cache-symlink");
    let record = &read_set(&fixture).records[0];
    let destination = fixture.cache.join(record.identity_key.as_str());
    let outside = scratch("cache-symlink-outside");
    fs::create_dir_all(&fixture.cache).unwrap();
    fs::write(outside.join(".echo-seed-sha256"), &record.cache_seed.sha256).unwrap();
    std::os::unix::fs::symlink(&outside, &destination).unwrap();

    assert!(select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .is_err());
}

#[test]
fn small_and_large_records_select_and_seed_independently() {
    let mut fixture = fixture("dual-model");
    let small_plan = fixture.cpu_plan.clone();
    add_large_record(&mut fixture);
    let large_key = read_set(&fixture).records[1].identity_key.clone();
    let large = select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .unwrap();
    assert!(matches!(
        large,
        WhisperPlanDecision::QualifiedAccelerator(_)
    ));
    fixture.cpu_plan = small_plan;
    let small_key = read_set(&fixture).records[0].identity_key.clone();
    let small = select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .unwrap();
    assert!(matches!(
        small,
        WhisperPlanDecision::QualifiedAccelerator(_)
    ));
    assert_ne!(small_key, large_key);
    assert!(fixture.cache.join(small_key.as_str()).is_dir());
    assert!(fixture.cache.join(large_key.as_str()).is_dir());
}

#[test]
fn unrelated_cache_seed_drift_disables_the_complete_set() {
    let mut fixture = fixture("unrelated-seed-drift");
    let small_plan = fixture.cpu_plan.clone();
    add_large_record(&mut fixture);
    fixture.cpu_plan = small_plan;
    let set = read_set(&fixture);
    let unrelated_seed = fixture
        .package
        .join(&set.records[1].cache_seed.relative_path);
    fs::write(
        unrelated_seed.join("mesa_shader_cache/index"),
        b"changed unrelated seed",
    )
    .unwrap();
    write_set(&fixture, set);
    assert!(select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .is_err());
}

#[test]
fn same_model_on_two_hardware_identities_selects_the_exact_host() {
    let fixture = fixture("multi-hardware");
    let mut set = read_set(&fixture);
    let mut other = set.records[0].clone();
    other.identity.device.device_id = 0x9999;
    other.identity_key = AdmissionIdentityKey::for_identity(&other.identity);
    other.cache_seed.relative_path = format!("cache-seeds/{}", other.identity_key.as_str());
    copy_tree(
        &fixture
            .package
            .join(&set.records[0].cache_seed.relative_path),
        &fixture.package.join(&other.cache_seed.relative_path),
    )
    .unwrap();
    set.records.push(other);
    write_set(&fixture, set);
    assert!(select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .is_ok());
}

#[test]
fn two_live_tuning_identities_are_ambiguous() {
    let fixture = fixture("ambiguous-tuning");
    let mut set = read_set(&fixture);
    let mut other = set.records[0].clone();
    other.identity.tuning.beam_size = 1;
    other.identity.tuning.best_of = 2;
    other.identity.tuning.no_fallback = true;
    other.identity_key = AdmissionIdentityKey::for_identity(&other.identity);
    other.cache_seed.relative_path = format!("cache-seeds/{}", other.identity_key.as_str());
    copy_tree(
        &fixture
            .package
            .join(&set.records[0].cache_seed.relative_path),
        &fixture.package.join(&other.cache_seed.relative_path),
    )
    .unwrap();
    set.records.push(other);
    write_set(&fixture, set);
    assert!(select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .is_err());
}

#[test]
fn inactive_unrelated_record_disables_the_complete_set() {
    for (label, mutate) in [("expired", 0_u8), ("stopped", 1), ("false-gate", 2)] {
        let mut fixture = fixture(label);
        add_large_record(&mut fixture);
        fixture.cpu_plan.model.path = fixture
            .cpu_plan
            .model
            .path
            .with_file_name(format!("ggml-small-{label}.bin"));
        fs::write(&fixture.cpu_plan.model.path, format!("small model {label}")).unwrap();
        let mut set = read_set(&fixture);
        set.records[0].identity.model_sha256 = sha256_file(&fixture.cpu_plan.model.path).unwrap();
        set.records[0].identity_key = AdmissionIdentityKey::for_identity(&set.records[0].identity);
        match mutate {
            0 => set.records[1].expires_at = NOW,
            1 => set.records[1].verdict = AdmissionVerdict::Stopped,
            _ => set.records[1].gates.memory_evidence = false,
        }
        write_set(&fixture, set);
        assert!(
            select_qualified_package(selection(
                &fixture,
                ObservedWhisperHost {
                    drm_vendor_id: 0x8086,
                    drm_device_id: 0x46a6,
                    drm_driver: "i915".to_string(),
                }
            ))
            .is_err(),
            "{label}"
        );
    }
}

#[test]
fn legacy_single_record_file_is_never_read() {
    let fixture = fixture("legacy-refused");
    fs::rename(
        fixture.package.join("admission-set.json"),
        fixture.package.join("admission.json"),
    )
    .unwrap();
    assert!(select_qualified_package(selection(
        &fixture,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        }
    ))
    .is_err());
}

#[test]
fn complete_inventory_rejects_extra_missing_and_type_drift() {
    let extra = fixture("inventory-extra");
    fs::write(extra.package.join("unlisted"), b"extra").unwrap();
    assert!(select_qualified_package(selection(
        &extra,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        }
    ))
    .is_err());

    let missing = fixture("inventory-missing");
    fs::remove_file(missing.package.join("runtime/libggml.so.0.18.1")).unwrap();
    assert!(select_qualified_package(selection(
        &missing,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        }
    ))
    .is_err());

    let drift = fixture("inventory-type");
    let path = drift.package.join("runtime/libggml.so.0.18.1");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink("libwhisper.so.1", &path).unwrap();
    assert!(select_qualified_package(selection(
        &drift,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        }
    ))
    .is_err());

    let link_drift = fixture("inventory-link");
    let link = link_drift.package.join("runtime/libggml.so.0.18.1");
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("libwhisper.so.1", &link).unwrap();
    write_set(&link_drift, read_set(&link_drift));
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("libwhisper.so.1.9.2", &link).unwrap();
    assert!(select_qualified_package(selection(
        &link_drift,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .is_err());

    let escape = fixture("inventory-escape");
    let link = escape.package.join("runtime/libggml.so.0.18.1");
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("libwhisper.so.1", &link).unwrap();
    write_set(&escape, read_set(&escape));
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("../../../echo-desktop", &link).unwrap();
    assert!(select_qualified_package(selection(
        &escape,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    ))
    .is_err());
}

#[test]
fn strict_set_parser_rejects_unknown_duplicates_and_empty_sets() {
    let fixture = fixture("strict-parser");
    let set = read_set(&fixture);

    let mut unknown = serde_json::to_value(&set).unwrap();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_string(), true.into());
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&unknown).unwrap()).is_err());

    let mut empty = set.clone();
    empty.records.clear();
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&empty).unwrap()).is_err());

    let mut duplicate_identity = set.clone();
    duplicate_identity
        .records
        .push(duplicate_identity.records[0].clone());
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&duplicate_identity).unwrap()).is_err());

    let (alias, digest) = set
        .shared
        .runtime_library_bindings
        .first_key_value()
        .unwrap();
    let binding = format!("\"{alias}\":\"{digest}\"");
    let duplicate_binding =
        serde_json::to_string(&set)
            .unwrap()
            .replacen(&binding, &format!("{binding},{binding}"), 1);
    assert!(AdmissionSet::from_bytes(duplicate_binding.as_bytes()).is_err());

    assert!(AdmissionSet::from_bytes(&vec![b' '; 1024 * 1024 + 1]).is_err());
    let mut too_many_records = set.clone();
    too_many_records.records = vec![set.records[0].clone(); 129];
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&too_many_records).unwrap()).is_err());
    let mut too_many_entries = set.clone();
    too_many_entries.inventory = vec![set.inventory[0].clone(); 4097];
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&too_many_entries).unwrap()).is_err());
    let mut oversized_entry = set.clone();
    oversized_entry.inventory[0].bytes = 1024 * 1024 * 1024 + 1;
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&oversized_entry).unwrap()).is_err());

    let mut aggregate_oversized = set.clone();
    let aggregate_entry_bytes = crate::stt::whisper_admission::MAX_PACKAGE_BYTES / 5 + 1;
    aggregate_oversized.inventory = (0..5)
        .map(|index| PackageEntry {
            path: format!("aggregate/{index}"),
            kind: PackageEntryKind::File,
            bytes: aggregate_entry_bytes,
            sha256: Some("a".repeat(64)),
            link_target: None,
        })
        .collect();
    assert!(aggregate_oversized
        .inventory
        .iter()
        .all(|entry| entry.bytes <= 1024 * 1024 * 1024));
    assert!(AdmissionSet::from_bytes(&serde_json::to_vec(&aggregate_oversized).unwrap()).is_err());

    let mut distinct_duplicate_cache = set.clone();
    let mut other = distinct_duplicate_cache.records[0].clone();
    other.identity.model_sha256 = "9".repeat(64);
    other.identity_key = AdmissionIdentityKey::for_identity(&other.identity);
    distinct_duplicate_cache.records.push(other);
    assert!(
        AdmissionSet::from_bytes(&serde_json::to_vec(&distinct_duplicate_cache).unwrap()).is_err()
    );
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

    let changed_alias = fixture("alias-change");
    fs::write(
        changed_alias.package.join("runtime/libwhisper.so.1"),
        b"ggml",
    )
    .unwrap();
    assert!(select_qualified_package(selection(
        &changed_alias,
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

    fn wrong_receipt(
        _: &Path,
        _: &WhisperRuntimeLaunch,
        _: std::time::Duration,
    ) -> Result<WhisperVulkanReceipt, String> {
        Ok(live_receipt(0x9999))
    }
    let changed_receipt = fixture("receipt-change");
    let mut changed_receipt_selection = selection(
        &changed_receipt,
        ObservedWhisperHost {
            drm_vendor_id: 0x8086,
            drm_device_id: 0x46a6,
            drm_driver: "i915".to_string(),
        },
    );
    changed_receipt_selection.receipt_probe = wrong_receipt;
    assert!(select_qualified_package(changed_receipt_selection).is_err());
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
