use std::sync::Mutex;

use echo_core::{History, RunDetail};
use echo_desktop::ipc::{
    AccelerationSkipReason, AppPhase, AppStatus, LastRun, LastRunPerformance, RecordingPolicy,
};

mod health;
#[cfg(test)]
mod tests;

pub(super) use health::health_invalidate;
#[cfg(test)]
pub(super) use health::{
    collect_and_publish_health, health_cache_state, health_clock, health_source_fingerprint,
    seed_health_for_test, Health, HealthCacheState,
};

use health::{file_identity, health_snapshot, same_history_contents, FileIdentity};

fn recover_cache_lock<'a, T>(cache: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> {
    match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("status: recovering poisoned {name} cache");
            cache.clear_poison();
            poisoned.into_inner()
        }
    }
}

fn app_phase(state: &str) -> AppPhase {
    match state {
        "Idle" => AppPhase::Idle,
        "Recording" => AppPhase::Recording,
        "Transcribing" => AppPhase::Transcribing,
        "Injecting" => AppPhase::Injecting,
        _ => AppPhase::Failed,
    }
}

pub(super) fn recording_snapshot(
    status: &echo::status::Status,
) -> echo_desktop::ipc::RecordingSnapshot {
    let capture_stop_requested = status.state == "Recording"
        && echo::rec::capture_stop_requested_for(status.session_id.as_deref());
    echo_desktop::ipc::RecordingSnapshot {
        session_id: status.session_id.clone(),
        phase: app_phase(&status.state),
        capture_stop_requested,
        revision: if capture_stop_requested {
            echo::rec::RecordingControlAck::after_revision(status.revision)
        } else {
            status.revision
        },
    }
}

fn recording_policy_dto() -> RecordingPolicy {
    RecordingPolicy {
        minimum_seconds: echo_core::RecordingLimit::MIN.seconds(),
        default_seconds: echo_core::RecordingLimit::DEFAULT.seconds(),
        maximum_seconds: echo_core::RecordingLimit::MAX.seconds(),
        presets_seconds: echo_core::RecordingLimit::PRESETS
            .map(echo_core::RecordingLimit::seconds)
            .to_vec(),
    }
}

fn project_acceleration_skip(
    whisper: &echo_core::WhisperRunTelemetry,
) -> Option<AccelerationSkipReason> {
    if let Some(skip) = whisper.skipped_acceleration {
        return Some(skip.into());
    }
    let recovery = whisper.recovery.as_ref()?;
    recovery.fallback_reason?;
    Some(if recovery.accelerated_attempted {
        AccelerationSkipReason::RecoveredToCpu
    } else {
        AccelerationSkipReason::DeviceQuarantined
    })
}

fn project_last_run_performance(detail: &RunDetail) -> Option<LastRunPerformance> {
    let whisper = detail.whisper.as_ref()?;
    Some(LastRunPerformance {
        mode: whisper.mode.into(),
        runtime_source: whisper.runtime.source.into(),
        backend: whisper.runtime.backend.into(),
        device: whisper.runtime.device.clone(),
        total_ms: whisper.total_ms,
        audio_encode_ms: whisper.audio_encode_ms,
        child_wall_ms: whisper
            .attempts
            .iter()
            .map(|attempt| attempt.child_wall_ms)
            .sum(),
        parse_ms: whisper.parse_ms,
        attempt_count: whisper.attempts.len(),
        tuning: whisper.tuning.into(),
        acceleration_skip: project_acceleration_skip(whisper),
        recovery: whisper.recovery.clone().map(Into::into),
    })
}

#[cfg(test)]
static LAST_RUN_PROJECTIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static LAST_RUN_LOAD_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[derive(Clone)]
struct CachedLastRun {
    history_id: Option<String>,
    history_file: FileIdentity,
    projection: Option<LastRun>,
}

static LAST_RUN: Mutex<Option<CachedLastRun>> = Mutex::new(None);

fn project_last_run(history: &History) -> Option<LastRun> {
    #[cfg(test)]
    LAST_RUN_PROJECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    history.rows().last().map(|row| LastRun {
        engine: row.engine.to_string(),
        binary: row.detail.binary.clone(),
        model_path: row.detail.model_path.clone(),
        multilingual: row.detail.multilingual,
        vad: row.detail.vad,
        infer_ms: row.infer_ms,
        language: row.detail.language.clone(),
        language_probability: row.detail.language_probability,
        performance: project_last_run_performance(&row.detail),
    })
}

fn last_run_for(history_id: Option<&str>) -> Option<LastRun> {
    let mut cached = recover_cache_lock(&LAST_RUN, "last-run");
    let path = echo_core::history_path();
    last_run_for_with_sources(
        history_id,
        &mut cached,
        || file_identity(&path),
        || {
            #[cfg(test)]
            LAST_RUN_LOAD_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            History::load_read_only()
                .ok()
                .and_then(|history| project_last_run(&history))
        },
    )
}

fn last_run_for_with_sources(
    history_id: Option<&str>,
    cached: &mut Option<CachedLastRun>,
    mut identity: impl FnMut() -> FileIdentity,
    mut load: impl FnMut() -> Option<LastRun>,
) -> Option<LastRun> {
    let mut before = identity();
    if let Some(cached) = cached.as_ref() {
        if cached.history_id.as_deref() == history_id
            && same_history_contents(&cached.history_file, &before)
        {
            return cached.projection.clone();
        }
    }
    *cached = None;

    let mut latest = None;
    // read_private repairs private permissions before reading. On Unix that
    // can change ctime, so treat the repaired identity as a new attempt rather
    // than attaching the projection to the pre-repair identity.
    for _ in 0..2 {
        let projection = load();
        let after = identity();
        if same_history_contents(&before, &after) {
            *cached = Some(CachedLastRun {
                history_id: history_id.map(str::to_string),
                history_file: after,
                projection: projection.clone(),
            });
            return projection;
        }
        before = after;
        latest = projection;
    }
    latest
}

#[must_use]
pub(super) fn last_run() -> Option<LastRun> {
    let status = echo::status::read();
    last_run_for(status.last_history_id.as_deref())
}

pub(super) fn last_run_invalidate() {
    *recover_cache_lock(&LAST_RUN, "last-run") = None;
}

pub(super) fn app_status() -> AppStatus {
    #[cfg(feature = "status-perf-probe")]
    let mut timer = crate::perf::StatusStageTimer::start();
    let status = echo::status::read();
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::StatusFile);
    let recording_limit =
        project_recording_limit(&status, echo::rec::recording_limit_from_process().limit);
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::RecordingLimit);
    let health = health_snapshot();
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::Health);
    let shortcut = crate::shortcuts::status(&health.current_exe);
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::Shortcut);
    let last_run = last_run_for(status.last_history_id.as_deref());
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::History);
    let recording_in_process = status.state == "Recording" && echo::rec::recording_in_process();
    let hud_enabled = echo::ui::hud::enabled();
    let settings_path = echo_core::config_path().to_string_lossy().into_owned();
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::Presentation);
    let recording = recording_snapshot(&status);
    let app_status = AppStatus {
        phase: recording.phase,
        last_transcript: status.last,
        last_history_id: status.last_history_id,
        microphone_ready: health.microphone_ready,
        engine_name: health.engine_name,
        engine_ready: health.engine_ready,
        injection_name: health.injection_name,
        injection_ready: health.injection_ready,
        shortcut,
        hud_enabled,
        recording_limit_seconds: recording_limit.map(echo_core::RecordingLimit::seconds),
        recording_policy: recording_policy_dto(),
        settings_path,
        version: env!("CARGO_PKG_VERSION").to_string(),
        last_error: status.error,
        last_run,
        language_warning: health.language_warning,
        recording_in_process,
        recording_session_id: recording.session_id,
        capture_stop_requested: recording.capture_stop_requested,
        recording_revision: recording.revision,
        current_exe: health.current_exe,
        first_path_hit: health.first_path_hit,
        stale_installs: health.stale_installs,
    };
    #[cfg(feature = "status-perf-probe")]
    {
        timer.mark(crate::perf::StatusStage::Compose);
        timer.finish();
    }
    app_status
}

fn project_recording_limit(
    status: &echo::status::Status,
    current: echo_core::RecordingLimit,
) -> Option<echo_core::RecordingLimit> {
    if status.state == "Recording" {
        status.recording_limit
    } else {
        Some(current)
    }
}

pub(super) fn current_exe_string() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}
