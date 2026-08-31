use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use echo_desktop::ipc::AppStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub(crate) enum StatusStage {
    StatusFile,
    RecordingLimit,
    Health,
    Shortcut,
    History,
    Presentation,
    Compose,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusStageSample {
    status_file_us: u64,
    recording_limit_us: u64,
    health_us: u64,
    shortcut_us: u64,
    history_us: u64,
    presentation_us: u64,
    compose_us: u64,
    total_us: u64,
}

pub(crate) struct StatusStageTimer {
    started: Instant,
    previous: Instant,
    sample: StatusStageSample,
}

impl StatusStageTimer {
    pub(crate) fn start() -> Self {
        let started = Instant::now();
        Self {
            started,
            previous: started,
            sample: StatusStageSample::default(),
        }
    }

    pub(crate) fn mark(&mut self, stage: StatusStage) {
        let now = Instant::now();
        let elapsed = micros(now.duration_since(self.previous));
        self.previous = now;
        match stage {
            StatusStage::StatusFile => self.sample.status_file_us = elapsed,
            StatusStage::RecordingLimit => self.sample.recording_limit_us = elapsed,
            StatusStage::Health => self.sample.health_us = elapsed,
            StatusStage::Shortcut => self.sample.shortcut_us = elapsed,
            StatusStage::History => self.sample.history_us = elapsed,
            StatusStage::Presentation => self.sample.presentation_us = elapsed,
            StatusStage::Compose => self.sample.compose_us = elapsed,
        }
    }

    pub(crate) fn finish(mut self) {
        self.sample.total_us = micros(self.started.elapsed());
        stage_samples()
            .lock()
            .expect("status performance samples lock")
            .push(self.sample);
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn stage_samples() -> &'static Mutex<Vec<StatusStageSample>> {
    static SAMPLES: OnceLock<Mutex<Vec<StatusStageSample>>> = OnceLock::new();
    SAMPLES.get_or_init(|| Mutex::new(Vec::new()))
}

fn take_status_stage_samples() -> Vec<StatusStageSample> {
    std::mem::take(
        &mut *stage_samples()
            .lock()
            .expect("status performance samples lock"),
    )
}

fn cold_status_stage() -> &'static Mutex<Option<StatusStageSample>> {
    static COLD: OnceLock<Mutex<Option<StatusStageSample>>> = OnceLock::new();
    COLD.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
pub(crate) fn perf_noop() -> u64 {
    1
}

#[tauri::command]
pub(crate) fn perf_fixed_status() -> AppStatus {
    static FIXED: OnceLock<AppStatus> = OnceLock::new();
    FIXED.get_or_init(crate::status::app_status).clone()
}

#[tauri::command]
pub(crate) fn perf_clear_status_stages() {
    take_status_stage_samples();
}

#[tauri::command]
pub(crate) fn perf_preserve_cold_status_stage() -> Result<(), String> {
    let mut samples = take_status_stage_samples();
    if samples.len() != 1 {
        return Err(format!(
            "expected one cold status stage sample, found {}",
            samples.len()
        ));
    }
    *cold_status_stage()
        .lock()
        .expect("cold status performance sample lock") = samples.pop();
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SampleSummary {
    count: usize,
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PerfLane {
    name: String,
    summary: SampleSummary,
    samples_ms: Vec<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PerfReport {
    schema_version: u8,
    app_version: String,
    user_agent: String,
    platform: String,
    lanes: Vec<PerfLane>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PerfOutput {
    commit: &'static str,
    report: PerfReport,
    cold_status_stage: StatusStageSample,
    status_stages: Vec<StatusStageSample>,
}

fn validate_report(report: &PerfReport) -> Result<(), String> {
    if report.schema_version != 1 || report.app_version != env!("CARGO_PKG_VERSION") {
        return Err(format!(
            "status performance report identity is invalid: schema {}, app {}, expected {}",
            report.schema_version,
            report.app_version,
            env!("CARGO_PKG_VERSION")
        ));
    }
    let expected = [("noop", 500), ("fixed-status", 500), ("current-status", 40)];
    if report.lanes.len() != expected.len() {
        return Err("status performance report lane count is invalid".to_string());
    }
    for (lane, (name, count)) in report.lanes.iter().zip(expected) {
        if lane.name != name
            || lane.samples_ms.len() != count
            || lane.summary.count != count
            || lane
                .samples_ms
                .iter()
                .any(|sample| !sample.is_finite() || *sample < 0.0)
        {
            return Err(format!("status performance lane {name} is invalid"));
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn perf_report_complete(
    app: tauri::AppHandle,
    report: PerfReport,
) -> Result<(), String> {
    validate_report(&report)?;
    let cold_status_stage = cold_status_stage()
        .lock()
        .expect("cold status performance sample lock")
        .take()
        .ok_or_else(|| "cold status performance sample is missing".to_string())?;
    let output = PerfOutput {
        commit: option_env!("ECHO_BUILD_SHA").unwrap_or("unknown"),
        report,
        cold_status_stage,
        status_stages: take_status_stage_samples(),
    };
    println!(
        "STATUS_PERF_JSON {}",
        serde_json::to_string(&output).map_err(|error| error.to_string())?
    );
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub(crate) fn perf_report_failed(app: tauri::AppHandle, message: String) {
    eprintln!("STATUS_PERF_ERROR {message}");
    app.exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_validation_rejects_non_finite_samples() {
        let report = PerfReport {
            schema_version: 1,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            user_agent: "test".to_string(),
            platform: "linux".to_string(),
            lanes: vec![
                lane("noop", 500, f64::NAN),
                lane("fixed-status", 500, 1.0),
                lane("current-status", 40, 1.0),
            ],
        };
        assert!(validate_report(&report).is_err());
    }

    fn lane(name: &str, count: usize, sample: f64) -> PerfLane {
        PerfLane {
            name: name.to_string(),
            summary: SampleSummary {
                count,
                min_ms: sample,
                p50_ms: sample,
                p95_ms: sample,
                max_ms: sample,
            },
            samples_ms: vec![sample; count],
        }
    }
}
