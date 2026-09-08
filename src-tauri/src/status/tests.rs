use super::*;
use echo_core::{
    EngineId, HistoryRow, InjectBackend, InjectReport, WhisperAccelerationSkip, WhisperRunMode,
    WhisperRuntimeBackend, WhisperRuntimeSource, WhisperTuningTelemetry,
};
use std::collections::VecDeque;
use std::time::Duration;

const LAST_RUN_CACHE_CHILD: &str = "ECHO_LAST_RUN_CACHE_CHILD";

fn history_row(id: &str, infer_ms: u64) -> HistoryRow {
    HistoryRow {
        id: id.to_string(),
        text: id.to_string(),
        raw: id.to_string(),
        engine: EngineId::Whisper {
            model: "test".to_string(),
        },
        started_at: 1,
        infer_ms,
        inject: InjectReport::Typed {
            backend: InjectBackend::Xdotool,
        },
        detail: RunDetail::default(),
    }
}

fn last_run_projection(infer_ms: u64) -> LastRun {
    LastRun {
        engine: "test".to_string(),
        binary: None,
        model_path: None,
        multilingual: None,
        vad: None,
        infer_ms,
        language: None,
        language_probability: None,
        performance: None,
    }
}

fn fake_health(label: &str) -> Health {
    Health {
        microphone_ready: true,
        engine_name: label.to_string(),
        engine_ready: true,
        injection_name: "fake injection".to_string(),
        injection_ready: true,
        current_exe: "/fake/echo".to_string(),
        first_path_hit: None,
        stale_installs: Vec::new(),
        language_warning: None,
    }
}

#[test]
fn poisoned_reconstructible_cache_remains_available() {
    let cache = std::sync::Arc::new(Mutex::new(Some("cached".to_string())));
    let poison = std::sync::Arc::clone(&cache);
    assert!(std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("poison cache");
    })
    .join()
    .is_err());

    let cached = recover_cache_lock(&cache, "test");
    assert_eq!(cached.as_deref(), Some("cached"));
    drop(cached);
    assert!(!cache.is_poisoned());
}

#[test]
fn runtime_responsiveness_last_run_retries_a_changed_post_read_identity() {
    let old_identity = FileIdentity::Missing;
    let replacement_identity = FileIdentity::MetadataError {
        kind: std::io::ErrorKind::Other,
        raw_os_error: Some(17),
    };
    let mut identities = VecDeque::from([
        old_identity,
        replacement_identity.clone(),
        replacement_identity.clone(),
        replacement_identity.clone(),
    ]);
    let mut projections =
        VecDeque::from([Some(last_run_projection(10)), Some(last_run_projection(20))]);
    let mut reads = 0;
    let mut cache = None;

    let projected = last_run_for_with_sources(
        Some("replacement"),
        &mut cache,
        || identities.pop_front().expect("identity observation"),
        || {
            reads += 1;
            projections.pop_front().expect("history projection")
        },
    );

    assert_eq!(reads, 2, "one unstable read is retried exactly once");
    assert_eq!(projected.as_ref().map(|run| run.infer_ms), Some(20));
    let cached = cache.as_ref().expect("stable replacement cached");
    assert!(cached.history_file == replacement_identity);
    assert_eq!(
        cached.projection.as_ref().map(|run| run.infer_ms),
        Some(20),
        "the old projection must never be associated with the replacement identity"
    );

    let reused = last_run_for_with_sources(
        Some("replacement"),
        &mut cache,
        || identities.pop_front().expect("cached identity observation"),
        || panic!("the stable replacement projection must be reused"),
    );
    assert_eq!(reused.as_ref().map(|run| run.infer_ms), Some(20));

    let mut unstable_identities = VecDeque::from([
        FileIdentity::Missing,
        FileIdentity::MetadataError {
            kind: std::io::ErrorKind::Other,
            raw_os_error: Some(1),
        },
        FileIdentity::MetadataError {
            kind: std::io::ErrorKind::Other,
            raw_os_error: Some(2),
        },
    ]);
    let mut unstable_projections =
        VecDeque::from([Some(last_run_projection(30)), Some(last_run_projection(40))]);
    let mut unstable_cache = None;
    let latest = last_run_for_with_sources(
        Some("continuously-changing"),
        &mut unstable_cache,
        || {
            unstable_identities
                .pop_front()
                .expect("unstable identity observation")
        },
        || {
            unstable_projections
                .pop_front()
                .expect("unstable history projection")
        },
    );
    assert_eq!(latest.as_ref().map(|run| run.infer_ms), Some(40));
    assert!(
        unstable_cache.is_none(),
        "two unstable reads return the latest projection without caching it"
    );
}

#[test]
fn runtime_responsiveness_health_reuses_cache_while_one_probe_is_pending() {
    const SOURCE_FRESHNESS: Duration = Duration::from_secs(1);
    const TTL: Duration = Duration::from_secs(10);
    let mut state = HealthCacheState::new(SOURCE_FRESHNESS, TTL);
    state.publish(Duration::ZERO, None, fake_health("cached"));

    let starts_probe = state.read(Duration::from_secs(2));
    assert_eq!(
        starts_probe
            .cached()
            .map(|health| health.engine_name.as_str()),
        Some("cached")
    );
    assert!(starts_probe.starts_probe());
    assert!(!starts_probe.recollects());

    let while_pending = state.read(Duration::from_secs(3));
    assert_eq!(
        while_pending
            .cached()
            .map(|health| health.engine_name.as_str()),
        Some("cached"),
        "a pending source probe cannot hold up a cached health read"
    );
    assert!(
        !while_pending.starts_probe(),
        "only one source probe may be in flight"
    );
    assert!(!while_pending.recollects());

    state.probe_completed(0, Duration::from_secs(3), 7);
    let established_baseline = state.read(Duration::from_secs(3));
    assert_eq!(
        established_baseline
            .cached()
            .map(|health| health.engine_name.as_str()),
        Some("cached")
    );
    assert!(!established_baseline.starts_probe());
    assert!(
        !established_baseline.recollects(),
        "the first probe establishes an unknown baseline without invalidating fresh health"
    );

    assert!(state.read(Duration::from_secs(5)).starts_probe());
    state.probe_completed(0, Duration::from_secs(6), 7);
    let unchanged = state.read(Duration::from_secs(6));
    assert_eq!(
        unchanged.cached().map(|health| health.engine_name.as_str()),
        Some("cached")
    );
    assert!(!unchanged.starts_probe());
    assert!(
        !unchanged.recollects(),
        "an unchanged established fingerprint reuses cached health"
    );
}

#[test]
fn runtime_responsiveness_health_refreshes_in_background_and_honors_invalidation() {
    const SOURCE_FRESHNESS: Duration = Duration::from_secs(1);
    const TTL: Duration = Duration::from_secs(10);
    let mut changed = HealthCacheState::new(SOURCE_FRESHNESS, TTL);
    changed.publish(Duration::ZERO, Some(7), fake_health("old"));
    assert!(changed.read(Duration::from_secs(2)).starts_probe());
    changed.probe_completed(0, Duration::from_secs(3), 8);
    let changed_fingerprint = changed.read(Duration::from_secs(3));
    assert!(
        changed_fingerprint.starts_refresh(),
        "a changed completed fingerprint starts a background health refresh"
    );
    assert_eq!(
        changed_fingerprint
            .cached()
            .map(|health| health.engine_name.as_str()),
        Some("old"),
        "source refresh work cannot hold up a cached health read"
    );
    assert!(!changed_fingerprint.recollects());
    assert!(changed.publish_if_current(
        changed_fingerprint.refresh_generation.unwrap(),
        Duration::from_secs(3),
        None,
        fake_health("replacement"),
    ));
    assert_eq!(
        changed
            .read(Duration::from_secs(3))
            .cached()
            .map(|health| health.engine_name.as_str()),
        Some("replacement")
    );

    changed.invalidate();
    let explicitly_invalidated = changed.read(Duration::from_secs(4));
    assert!(
        explicitly_invalidated.recollects(),
        "explicit invalidation must force health recollection"
    );
    assert!(explicitly_invalidated.cached().is_none());

    let mut stalled = HealthCacheState::new(SOURCE_FRESHNESS, TTL);
    stalled.publish(Duration::ZERO, None, fake_health("expired"));
    assert!(stalled.read(Duration::from_secs(2)).starts_probe());
    let expired = stalled.read(Duration::from_secs(11));
    assert_eq!(
        expired.cached().map(|health| health.engine_name.as_str()),
        Some("expired")
    );
    assert!(
        expired.starts_refresh(),
        "TTL expiry must refresh in the background even when a source probe stalls"
    );
    stalled.publish(Duration::from_secs(11), None, fake_health("recollected"));
    let after_ttl = stalled.read(Duration::from_secs(13));
    assert_eq!(
        after_ttl.cached().map(|health| health.engine_name.as_str()),
        Some("recollected")
    );
    assert!(
        !after_ttl.starts_probe(),
        "TTL recollection cannot spawn a second probe while the first is stalled"
    );
}

#[test]
fn runtime_responsiveness_invalidation_wins_over_in_flight_collection() {
    let state = std::sync::Arc::new(Mutex::new(HealthCacheState::new(
        Duration::from_secs(1),
        Duration::from_secs(10),
    )));
    let generation = state
        .lock()
        .unwrap()
        .read(Duration::ZERO)
        .collection_generation
        .unwrap();
    let (started_send, started_receive) = std::sync::mpsc::sync_channel(0);
    let (release_send, release_receive) = std::sync::mpsc::sync_channel(0);
    let collection_state = std::sync::Arc::clone(&state);
    let collection = std::thread::spawn(move || {
        collect_and_publish_health(&collection_state, generation, || {
            started_send.send(()).unwrap();
            release_receive.recv().unwrap();
            fake_health("stale")
        })
        .1
    });

    started_receive.recv().unwrap();
    let concurrent = state.lock().unwrap().read(Duration::ZERO);
    assert_eq!(
        concurrent
            .cached()
            .map(|health| health.engine_name.as_str()),
        Some(""),
        "a concurrent cache-empty read returns immediately without another collection"
    );
    assert!(!concurrent.recollects());
    state.lock().unwrap().invalidate();
    let after_invalidation_while_pending = state.lock().unwrap().read(Duration::from_secs(1));
    assert!(after_invalidation_while_pending.cached().is_some());
    assert!(
        !after_invalidation_while_pending.recollects(),
        "invalidation cannot multiply a still-running full-health collection"
    );
    release_send.send(()).unwrap();
    assert!(
        !collection.join().unwrap(),
        "a collection from the invalidated generation must not publish"
    );
    let after_invalidation = state.lock().unwrap().read(Duration::from_secs(1));
    assert!(after_invalidation.cached().is_none());
    assert_eq!(
        after_invalidation.collection_generation,
        Some(generation + 1)
    );
}

#[test]
fn health_fixture_reseeds_initialized_cache_and_rejects_pending_work() {
    seed_health_for_test(fake_health("first"));
    assert_eq!(health_snapshot().engine_name, "first");
    let generation = {
        let mut state = health_cache_state().lock().unwrap();
        let generation = state.generation;
        state.probe_pending = Some(generation);
        state.refresh_pending = Some(generation);
        generation
    };

    seed_health_for_test(fake_health("replacement"));
    {
        let mut state = health_cache_state().lock().unwrap();
        state.probe_completed(generation, health_clock(), 42);
        assert!(!state.publish_if_current(generation, health_clock(), None, fake_health("stale"),));
        assert!(state.probe_pending.is_none());
        assert!(state.refresh_pending.is_none());
        assert!(state.cached.as_ref().unwrap().source_fingerprint.is_none());
    }
    assert_eq!(health_snapshot().engine_name, "replacement");
    health_invalidate();
}

fn health_test_root(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "echo-health-source-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn health_source_fingerprint_ignores_unrelated_path_entries() {
    let root = health_test_root("unrelated-path");
    let path_a = root.join("path-a");
    let path_b = root.join("path-b");
    let model_root = root.join("models");
    std::fs::create_dir_all(&path_a).unwrap();
    std::fs::create_dir_all(&path_b).unwrap();
    std::fs::create_dir_all(&model_root).unwrap();

    let path_a_only = std::env::join_paths([&path_a]).unwrap();
    let baseline = health_source_fingerprint(&path_a_only, &model_root);
    let with_unrelated_path_directory = std::env::join_paths([&path_a, &path_b]).unwrap();
    assert_eq!(
        health_source_fingerprint(&with_unrelated_path_directory, &model_root),
        baseline,
        "an empty unrelated PATH directory cannot change readiness"
    );

    let unrelated_path_entry = path_a.join("unrelated-tool");
    std::fs::write(&unrelated_path_entry, b"unrelated").unwrap();
    assert_eq!(
        health_source_fingerprint(&path_a_only, &model_root),
        baseline,
        "unrelated files in a PATH directory cannot change readiness"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn health_source_fingerprint_tracks_xdotool_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let root = health_test_root("xdotool-mode");
    let path = root.join("path");
    let model_root = root.join("models");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::create_dir_all(&model_root).unwrap();
    let path_value = std::env::join_paths([&path]).unwrap();
    let xdotool = path.join("xdotool");
    std::fs::write(&xdotool, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&xdotool, std::fs::Permissions::from_mode(0o644)).unwrap();
    let non_executable = health_source_fingerprint(&path_value, &model_root);

    std::fs::set_permissions(&xdotool, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(
        health_source_fingerprint(&path_value, &model_root),
        non_executable,
        "making the readiness-relevant xdotool candidate executable must invalidate health"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn health_source_fingerprint_tracks_model_root_without_enumerating_entries() {
    let root = health_test_root("model-root");
    let path = root.join("path");
    let model_root = root.join("models");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::create_dir_all(&model_root).unwrap();
    let path_value = std::env::join_paths([&path]).unwrap();

    #[cfg(unix)]
    let changed_model_root = {
        use std::os::unix::fs::PermissionsExt;

        let before = health_source_fingerprint(&path_value, &model_root);
        let original_mode = std::fs::metadata(&model_root).unwrap().permissions().mode();
        std::fs::set_permissions(
            &model_root,
            std::fs::Permissions::from_mode(original_mode ^ 0o020),
        )
        .unwrap();
        (before, health_source_fingerprint(&path_value, &model_root))
    };
    #[cfg(not(unix))]
    let changed_model_root = {
        let before = health_source_fingerprint(&path_value, &model_root);
        let replacement = root.join("replacement-models");
        std::fs::create_dir_all(&replacement).unwrap();
        (before, health_source_fingerprint(&path_value, &replacement))
    };
    assert_ne!(
        changed_model_root.0, changed_model_root.1,
        "the managed model root identity must remain part of the fingerprint"
    );

    let _ = std::fs::remove_dir_all(root);
}

fn run_isolated_last_run_test(test_name: &str) -> bool {
    if std::env::var_os(LAST_RUN_CACHE_CHILD).is_some() {
        return false;
    }
    let dir = std::env::temp_dir().join(format!(
        "echo-last-run-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(LAST_RUN_CACHE_CHILD, "1")
        .env("ECHO_DATA_DIR", &dir)
        .status()
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(status.success(), "isolated last-run cache test failed");
    true
}

#[tokio::test(flavor = "current_thread")]
async fn last_run_projection_is_cached_and_history_mutations_invalidate_it() {
    if run_isolated_last_run_test(
        "status::tests::last_run_projection_is_cached_and_history_mutations_invalidate_it",
    ) {
        return;
    }

    History::append_default(history_row("first", 10)).unwrap();
    echo::status::write_status(echo_core::SessionState::Idle, None, None, Some("first")).unwrap();
    last_run_invalidate();
    LAST_RUN_PROJECTIONS.store(0, std::sync::atomic::Ordering::Relaxed);

    assert_eq!(last_run().map(|run| run.infer_ms), Some(10));

    History::append_default(history_row("second", 20)).unwrap();
    echo::status::write_status(echo_core::SessionState::Idle, None, None, Some("second")).unwrap();
    assert_eq!(last_run().map(|run| run.infer_ms), Some(20));
    assert_eq!(last_run().map(|run| run.infer_ms), Some(20));

    assert!(crate::commands::delete_history_item("second".to_string())
        .await
        .unwrap());
    assert_eq!(last_run().map(|run| run.infer_ms), Some(10));

    assert_eq!(crate::commands::clear_history().await.unwrap(), 1);
    assert_eq!(last_run(), None);
    assert_eq!(
        LAST_RUN_PROJECTIONS.load(std::sync::atomic::Ordering::Relaxed),
        4,
        "append identity changes, delete, and clear must project once each, while unchanged identity must reuse its projection"
    );
}

#[test]
fn corrupt_unchanged_history_is_loaded_at_most_once() {
    if run_isolated_last_run_test("status::tests::corrupt_unchanged_history_is_loaded_at_most_once")
    {
        return;
    }

    std::fs::create_dir_all(echo_core::data_dir()).unwrap();
    std::fs::write(echo_core::history_path(), b"not valid history json").unwrap();
    echo::status::write_status(echo_core::SessionState::Idle, None, None, Some("unchanged"))
        .unwrap();
    last_run_invalidate();
    LAST_RUN_LOAD_ATTEMPTS.store(0, std::sync::atomic::Ordering::Relaxed);

    assert_eq!(last_run(), None);
    assert_eq!(last_run(), None);
    assert_eq!(
        LAST_RUN_LOAD_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "a failed load must be cached until the history file identity changes"
    );
}

#[test]
fn direct_history_deletion_and_replacement_refresh_the_same_status_id() {
    if run_isolated_last_run_test(
        "status::tests::direct_history_deletion_and_replacement_refresh_the_same_status_id",
    ) {
        return;
    }

    History::append_default(history_row("first", 10)).unwrap();
    echo::status::write_status(
        echo_core::SessionState::Idle,
        None,
        None,
        Some("stable-status-id"),
    )
    .unwrap();
    last_run_invalidate();
    assert_eq!(last_run().map(|run| run.infer_ms), Some(10));

    std::fs::remove_file(echo_core::history_path()).unwrap();
    assert_eq!(
        last_run(),
        None,
        "direct history deletion must invalidate the cached projection"
    );

    let replacement = serde_json::json!({"rows": [history_row("replacement", 20)]});
    std::fs::write(
        echo_core::history_path(),
        serde_json::to_vec(&replacement).unwrap(),
    )
    .unwrap();
    assert_eq!(
        last_run().map(|run| run.infer_ms),
        Some(20),
        "direct history replacement must invalidate the cached missing projection"
    );
}

#[test]
fn legacy_status_refreshes_when_history_file_identity_changes() {
    if run_isolated_last_run_test(
        "status::tests::legacy_status_refreshes_when_history_file_identity_changes",
    ) {
        return;
    }

    History::append_default(history_row("first", 10)).unwrap();
    echo::status::write_status(echo_core::SessionState::Idle, None, None, None).unwrap();
    last_run_invalidate();
    assert_eq!(last_run().map(|run| run.infer_ms), Some(10));

    std::fs::remove_file(echo_core::history_path()).unwrap();
    let replacement = serde_json::json!({"rows": [history_row("replacement", 20)]});
    std::fs::write(
        echo_core::history_path(),
        serde_json::to_vec(&replacement).unwrap(),
    )
    .unwrap();
    assert_eq!(
        last_run().map(|run| run.infer_ms),
        Some(20),
        "legacy status without a history ID must use history file identity"
    );
}

#[test]
fn recording_policy_projects_defaults_presets_and_compatibility_values() {
    let policy = recording_policy_dto();
    let serialized = serde_json::to_value(&policy).unwrap();
    assert_eq!(serialized["minimumSeconds"], 1);
    assert_eq!(serialized["defaultSeconds"], 600);
    assert_eq!(serialized["maximumSeconds"], 600);
    assert_eq!(
        serialized["presetsSeconds"],
        serde_json::json!([30, 60, 120, 300, 600])
    );
}

#[test]
fn active_recording_limit_snapshot_wins_over_current_settings() {
    let active = echo::status::Status {
        state: "Recording".to_string(),
        last: None,
        last_history_id: None,
        error: None,
        recording_limit: echo_core::RecordingLimit::new(120),
        session_id: None,
        revision: 0,
    };
    assert_eq!(
        project_recording_limit(&active, echo_core::RecordingLimit::MAX)
            .map(echo_core::RecordingLimit::seconds),
        Some(120)
    );

    let legacy = echo::status::Status {
        recording_limit: None,
        ..active.clone()
    };
    assert_eq!(
        project_recording_limit(&legacy, echo_core::RecordingLimit::MAX),
        None
    );

    let idle = echo::status::Status {
        state: "Idle".to_string(),
        ..active
    };
    assert_eq!(
        project_recording_limit(&idle, echo_core::RecordingLimit::MAX)
            .map(echo_core::RecordingLimit::seconds),
        Some(600)
    );
}

#[test]
fn last_run_performance_projects_split_whisper_detail() {
    let detail = RunDetail {
        whisper: Some(echo_core::WhisperRunTelemetry {
            mode: WhisperRunMode::ColdFallback,
            total_ms: 1_230,
            audio_encode_ms: 10,
            parse_ms: 4,
            runtime: echo_core::WhisperRuntimeTelemetry {
                binary: "/usr/bin/whisper-cli".to_string(),
                source: WhisperRuntimeSource::System,
                backend: WhisperRuntimeBackend::Cpu,
                device: Some("Test CPU".to_string()),
                library_path: None,
                vulkan_driver_files: None,
                mesa_shader_cache_dir: None,
                identity_sha256: None,
                vulkan_receipt: None,
            },
            tuning: WhisperTuningTelemetry {
                threads: Some(4),
                beam_size: Some(5),
                best_of: Some(5),
                no_fallback: Some(false),
            },
            attempts: vec![
                echo_core::WhisperAttemptTelemetry {
                    vad: true,
                    process_start_ms: 1,
                    child_wall_ms: 500,
                    success: false,
                    exit_code: Some(1),
                    retry_reason: Some(echo_core::WhisperRetryReason::VadRejected),
                },
                echo_core::WhisperAttemptTelemetry {
                    vad: false,
                    process_start_ms: 1,
                    child_wall_ms: 710,
                    success: true,
                    exit_code: Some(0),
                    retry_reason: None,
                },
            ],
            recovery: None,
            skipped_acceleration: None,
        }),
        ..RunDetail::default()
    };
    let projected = project_last_run_performance(&detail).unwrap();
    assert_eq!(projected.mode, WhisperRunMode::ColdFallback.into());
    assert_eq!(projected.child_wall_ms, 1_210);
    assert_eq!(projected.attempt_count, 2);
    assert_eq!(projected.tuning.threads, Some(4));
    assert_eq!(projected.device.as_deref(), Some("Test CPU"));
    assert_eq!(projected.acceleration_skip, None);
}

fn cpu_telemetry() -> echo_core::WhisperRunTelemetry {
    echo_core::WhisperRunTelemetry {
        mode: WhisperRunMode::ColdCli,
        total_ms: 100,
        audio_encode_ms: 1,
        parse_ms: 1,
        runtime: echo_core::WhisperRuntimeTelemetry {
            binary: "/usr/bin/whisper-cli".to_string(),
            source: WhisperRuntimeSource::Managed,
            backend: WhisperRuntimeBackend::Cpu,
            device: None,
            library_path: None,
            vulkan_driver_files: None,
            mesa_shader_cache_dir: None,
            identity_sha256: None,
            vulkan_receipt: None,
        },
        tuning: WhisperTuningTelemetry {
            threads: None,
            beam_size: Some(3),
            best_of: Some(5),
            no_fallback: Some(false),
        },
        attempts: Vec::new(),
        recovery: None,
        skipped_acceleration: None,
    }
}

#[test]
fn every_gate_refusal_reaches_the_readout() {
    for (skip, expected) in [
        (
            WhisperAccelerationSkip::RuntimeMissing,
            AccelerationSkipReason::RuntimeMissing,
        ),
        (
            WhisperAccelerationSkip::NoDeviceEnumerated,
            AccelerationSkipReason::NoDeviceEnumerated,
        ),
        (
            WhisperAccelerationSkip::PinnedDeviceAbsent,
            AccelerationSkipReason::PinnedDeviceAbsent,
        ),
        (
            WhisperAccelerationSkip::DeviceQuarantined,
            AccelerationSkipReason::DeviceQuarantined,
        ),
        (
            WhisperAccelerationSkip::CpuFallbackMissing,
            AccelerationSkipReason::CpuFallbackMissing,
        ),
        (
            WhisperAccelerationSkip::DeviceNotReady,
            AccelerationSkipReason::DeviceNotReady,
        ),
    ] {
        let mut whisper = cpu_telemetry();
        whisper.skipped_acceleration = Some(skip);
        assert_eq!(
            project_acceleration_skip(&whisper),
            Some(expected),
            "{skip:?}"
        );
    }
}

#[test]
fn a_failed_accelerated_run_reports_the_retreat_not_its_diagnosis() {
    let mut whisper = cpu_telemetry();
    whisper.recovery = Some(echo_core::WhisperRecoveryTelemetry {
        identity_key: "accelerator".to_string(),
        accelerated_attempted: true,
        fallback_reason: Some(echo_core::WhisperRecoveryReason::Timeout),
    });
    assert_eq!(
        project_acceleration_skip(&whisper),
        Some(AccelerationSkipReason::RecoveredToCpu),
    );
}

#[test]
fn a_quarantine_hit_is_not_reported_as_a_failed_gpu_run() {
    for reason in [
        echo_core::WhisperRecoveryReason::Quarantined,
        echo_core::WhisperRecoveryReason::QuarantineUnreadable,
    ] {
        let mut whisper = cpu_telemetry();
        whisper.recovery = Some(echo_core::WhisperRecoveryTelemetry {
            identity_key: "accelerator".to_string(),
            accelerated_attempted: false,
            fallback_reason: Some(reason),
        });
        assert_eq!(
            project_acceleration_skip(&whisper),
            Some(AccelerationSkipReason::DeviceQuarantined),
            "{reason:?}"
        );
    }
}

#[test]
fn an_accelerated_run_that_kept_the_gpu_reports_no_skip() {
    let mut whisper = cpu_telemetry();
    whisper.runtime.backend = WhisperRuntimeBackend::Vulkan;
    whisper.recovery = Some(echo_core::WhisperRecoveryTelemetry {
        identity_key: "accelerator".to_string(),
        accelerated_attempted: true,
        fallback_reason: None,
    });
    assert_eq!(project_acceleration_skip(&whisper), None);
}
