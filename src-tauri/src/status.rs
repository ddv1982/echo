use std::env;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use echo::audio::AudioCapture;
use echo_core::{History, RunDetail, WhisperAccelerationSkip};
use echo_desktop::ipc::{
    AccelerationSkipReason, AppStatus, LastRun, LastRunPerformance, RecordingPolicy,
};

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
        return Some(match skip {
            WhisperAccelerationSkip::RuntimeMissing => AccelerationSkipReason::RuntimeMissing,
            WhisperAccelerationSkip::NoDeviceEnumerated => {
                AccelerationSkipReason::NoDeviceEnumerated
            }
            WhisperAccelerationSkip::PinnedDeviceAbsent => {
                AccelerationSkipReason::PinnedDeviceAbsent
            }
            WhisperAccelerationSkip::DeviceQuarantined => AccelerationSkipReason::DeviceQuarantined,
            WhisperAccelerationSkip::CpuFallbackMissing => {
                AccelerationSkipReason::CpuFallbackMissing
            }
            WhisperAccelerationSkip::DeviceNotReady => AccelerationSkipReason::DeviceNotReady,
        });
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

#[derive(Debug, Clone)]
pub(super) struct Health {
    pub(super) microphone_ready: bool,
    pub(super) engine_name: String,
    pub(super) engine_ready: bool,
    pub(super) injection_name: String,
    pub(super) injection_ready: bool,
    pub(super) current_exe: String,
    pub(super) first_path_hit: Option<String>,
    pub(super) stale_installs: Vec<String>,
}

pub(super) static HEALTH: Mutex<Option<(Instant, Health)>> = Mutex::new(None);

fn health_snapshot() -> Health {
    const TTL: Duration = Duration::from_secs(10);
    let mut cache = HEALTH.lock().expect("health cache lock");
    if let Some((at, health)) = cache.as_ref() {
        if at.elapsed() < TTL {
            return health.clone();
        }
    }
    let (engine_name, engine_ready) = echo::stt::engine_summary();
    let (injection_name, injection_ready) = echo::inject::detection_summary();
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let installs = echo::upgrade::path_installs(&env::var("PATH").unwrap_or_default());
    let first_path_hit = installs
        .first()
        .map(|(path, _)| path.to_string_lossy().into_owned());
    let stale_installs = current_exe
        .as_ref()
        .and_then(|path| echo::upgrade::file_identity(path).ok())
        .map(|current| {
            echo::upgrade::stale_installs(&installs, current)
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let health = Health {
        microphone_ready: AudioCapture::default_input_ready().is_ok(),
        engine_name,
        engine_ready,
        injection_name,
        injection_ready,
        current_exe: current_exe
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        first_path_hit,
        stale_installs,
    };
    *cache = Some((Instant::now(), health.clone()));
    health
}

pub(super) fn health_invalidate() {
    *HEALTH.lock().expect("health cache lock") = None;
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
    let last_run = History::load().ok().and_then(|history| {
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
    });
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::History);
    let recording_in_process = status.state == "Recording" && echo::rec::recording_in_process();
    let cleanup_name = echo::cleanup::mode_name();
    let hud_enabled = echo::ui::hud::enabled();
    let settings_path = echo_core::config_path().to_string_lossy().into_owned();
    let language_warning = echo::stt::language_warning();
    #[cfg(feature = "status-perf-probe")]
    timer.mark(crate::perf::StatusStage::Presentation);
    let app_status = AppStatus {
        recording: status.state == "Recording",
        phase: status.state,
        last_transcript: status.last,
        microphone_ready: health.microphone_ready,
        engine_name: health.engine_name,
        engine_ready: health.engine_ready,
        injection_name: health.injection_name,
        injection_ready: health.injection_ready,
        shortcut,
        cleanup_name,
        hud_enabled,
        recording_limit_seconds: recording_limit.map(echo_core::RecordingLimit::seconds),
        recording_policy: recording_policy_dto(),
        settings_path,
        version: env!("CARGO_PKG_VERSION").to_string(),
        last_error: status.error,
        last_run,
        language_warning,
        recording_in_process,
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

#[cfg(test)]
mod tests {
    use super::*;
    use echo_core::{
        WhisperRunMode, WhisperRuntimeBackend, WhisperRuntimeSource, WhisperTuningTelemetry,
    };

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
            error: None,
            recording_limit: echo_core::RecordingLimit::new(120),
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
}
