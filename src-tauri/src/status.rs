use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use echo::audio::AudioCapture;
use echo_core::{History, RunDetail};
use echo_desktop::ipc::{
    AccelerationSkipReason, AppPhase, AppStatus, LastRun, LastRunPerformance, RecordingPolicy,
};

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
        revision: status.revision + u64::from(capture_stop_requested),
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
    pub(super) language_warning: Option<String>,
}

pub(super) static HEALTH: Mutex<Option<(Instant, Health)>> = Mutex::new(None);

const HEALTH_SOURCE_FRESHNESS: Duration = Duration::from_secs(1);
const HEALTH_TTL: Duration = Duration::from_secs(10);

struct CachedHealth {
    collected_at: Duration,
    source_checked_at: Duration,
    source_fingerprint: Option<u64>,
    refresh_required: bool,
    health: Health,
}

struct HealthCacheState {
    source_freshness: Duration,
    ttl: Duration,
    cached: Option<CachedHealth>,
    generation: u64,
    probe_pending: Option<u64>,
    refresh_pending: Option<u64>,
}

impl HealthCacheState {
    fn new(source_freshness: Duration, ttl: Duration) -> Self {
        Self {
            source_freshness,
            ttl,
            cached: None,
            generation: 0,
            probe_pending: None,
            refresh_pending: None,
        }
    }

    fn publish(&mut self, now: Duration, source_fingerprint: Option<u64>, health: Health) {
        self.cached = Some(CachedHealth {
            collected_at: now,
            source_checked_at: now,
            source_fingerprint,
            refresh_required: false,
            health,
        });
        self.refresh_pending = None;
    }

    fn publish_if_current(
        &mut self,
        generation: u64,
        now: Duration,
        source_fingerprint: Option<u64>,
        health: Health,
    ) -> bool {
        if self.refresh_pending == Some(generation) {
            self.refresh_pending = None;
        }
        if generation != self.generation {
            return false;
        }
        self.publish(now, source_fingerprint, health);
        true
    }

    fn read(&mut self, now: Duration) -> HealthCacheDecision {
        let Some(cached) = self.cached.as_ref() else {
            if self.refresh_pending.is_some() {
                return HealthCacheDecision::with_cached(health_pending(), None, None);
            }
            self.refresh_pending = Some(self.generation);
            return HealthCacheDecision::recollect(self.generation);
        };
        let health = cached.health.clone();
        if cached.refresh_required || now.saturating_sub(cached.collected_at) >= self.ttl {
            let refresh_generation = self.refresh_pending.is_none().then_some(self.generation);
            self.refresh_pending = self.refresh_pending.or(refresh_generation);
            return HealthCacheDecision::with_cached(health, None, refresh_generation);
        }
        if now.saturating_sub(cached.source_checked_at) < self.source_freshness {
            return HealthCacheDecision::with_cached(health, None, None);
        }
        if self.probe_pending.is_some() {
            return HealthCacheDecision::with_cached(health, None, None);
        }
        self.probe_pending = Some(self.generation);
        HealthCacheDecision::with_cached(health, Some(self.generation), None)
    }

    fn probe_completed(&mut self, generation: u64, now: Duration, source_fingerprint: u64) {
        if self.probe_pending != Some(generation) {
            return;
        }
        self.probe_pending = None;
        if self.generation != generation {
            return;
        }
        let Some(cached) = self.cached.as_mut() else {
            return;
        };
        match cached.source_fingerprint {
            None => {
                cached.source_fingerprint = Some(source_fingerprint);
                cached.source_checked_at = now;
            }
            Some(cached_fingerprint) if cached_fingerprint == source_fingerprint => {
                cached.source_checked_at = now;
            }
            Some(_) => {
                cached.source_checked_at = now;
                cached.refresh_required = true;
            }
        }
    }

    fn probe_failed(&mut self, generation: u64) {
        if self.probe_pending == Some(generation) {
            self.probe_pending = None;
        }
    }

    fn refresh_failed(&mut self, generation: u64) {
        if self.refresh_pending == Some(generation) {
            self.refresh_pending = None;
        }
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cached = None;
    }
}

fn health_pending() -> Health {
    Health {
        microphone_ready: false,
        engine_name: String::new(),
        engine_ready: false,
        injection_name: String::new(),
        injection_ready: false,
        current_exe: String::new(),
        first_path_hit: None,
        stale_installs: Vec::new(),
        language_warning: None,
    }
}

struct HealthCacheDecision {
    cached: Option<Health>,
    probe_generation: Option<u64>,
    refresh_generation: Option<u64>,
    collection_generation: Option<u64>,
}

impl HealthCacheDecision {
    fn with_cached(
        health: Health,
        probe_generation: Option<u64>,
        refresh_generation: Option<u64>,
    ) -> Self {
        Self {
            cached: Some(health),
            probe_generation,
            refresh_generation,
            collection_generation: None,
        }
    }

    fn recollect(generation: u64) -> Self {
        Self {
            cached: None,
            probe_generation: None,
            refresh_generation: None,
            collection_generation: Some(generation),
        }
    }

    fn cached(&self) -> Option<&Health> {
        self.cached.as_ref()
    }

    #[cfg(test)]
    fn starts_probe(&self) -> bool {
        self.probe_generation.is_some()
    }

    #[cfg(test)]
    fn starts_refresh(&self) -> bool {
        self.refresh_generation.is_some()
    }

    fn recollects(&self) -> bool {
        self.collection_generation.is_some()
    }
}

fn health_cache_state() -> &'static Mutex<HealthCacheState> {
    static STATE: OnceLock<Mutex<HealthCacheState>> = OnceLock::new();
    STATE.get_or_init(|| {
        let legacy_seed = {
            let mirror = recover_cache_lock(&HEALTH, "health mirror");
            mirror
                .as_ref()
                .filter(|(at, _)| at.elapsed() < HEALTH_TTL)
                .map(|(_, health)| health.clone())
        };
        let mut state = HealthCacheState::new(HEALTH_SOURCE_FRESHNESS, HEALTH_TTL);
        if let Some(health) = legacy_seed {
            state.publish(health_clock(), None, health);
        }
        Mutex::new(state)
    })
}

fn health_clock() -> Duration {
    static STARTED_AT: OnceLock<Instant> = OnceLock::new();
    STARTED_AT.get_or_init(Instant::now).elapsed()
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum FileIdentity {
    Missing,
    MetadataError {
        kind: std::io::ErrorKind,
        raw_os_error: Option<i32>,
    },
    Present(FileMetadataIdentity),
}

#[cfg(unix)]
#[derive(Clone, PartialEq, Eq, Hash)]
struct FileMetadataIdentity {
    device: u64,
    inode: u64,
    size: u64,
    mode: u32,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[cfg(not(unix))]
#[derive(Clone, PartialEq, Eq, Hash)]
struct FileMetadataIdentity {
    is_file: bool,
    is_directory: bool,
    size: u64,
    readonly: bool,
    modified: Option<std::time::SystemTime>,
    created: Option<std::time::SystemTime>,
}

fn file_metadata_identity(metadata: &std::fs::Metadata) -> FileMetadataIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        FileMetadataIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            mode: metadata.mode(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
    #[cfg(not(unix))]
    {
        FileMetadataIdentity {
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
        }
    }
}

fn file_identity(path: &Path) -> FileIdentity {
    match std::fs::metadata(path) {
        Ok(metadata) => FileIdentity::Present(file_metadata_identity(&metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileIdentity::Missing,
        Err(error) => FileIdentity::MetadataError {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
        },
    }
}

fn same_history_contents(left: &FileIdentity, right: &FileIdentity) -> bool {
    match (left, right) {
        (FileIdentity::Missing, FileIdentity::Missing) => true,
        (
            FileIdentity::MetadataError {
                kind: left_kind,
                raw_os_error: left_raw,
            },
            FileIdentity::MetadataError {
                kind: right_kind,
                raw_os_error: right_raw,
            },
        ) => left_kind == right_kind && left_raw == right_raw,
        (FileIdentity::Present(left), FileIdentity::Present(right)) => {
            #[cfg(unix)]
            {
                left.device == right.device
                    && left.inode == right.inode
                    && left.size == right.size
                    && left.modified_seconds == right.modified_seconds
                    && left.modified_nanoseconds == right.modified_nanoseconds
            }
            #[cfg(not(unix))]
            {
                left.is_file == right.is_file
                    && left.is_directory == right.is_directory
                    && left.size == right.size
                    && left.modified == right.modified
                    && left.created == right.created
            }
        }
        _ => false,
    }
}

#[cfg(unix)]
fn executable_file(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.is_file() && metadata.mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable_file(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file()
}

const READINESS_EXECUTABLES: [&str; 10] = [
    "ydotool",
    "wtype",
    "xdotool",
    "xclip",
    "wl-copy",
    "whisper-cli",
    "whisper-cpp",
    "whisper",
    "sherpa-onnx-offline",
    "sherpa-onnx",
];

// Bound one-second source checks under pathological PATH values. Candidates
// beyond this cap are still discovered by the ten-second full health refresh.
const READINESS_PATH_DIRECTORY_LIMIT: usize = 64;

fn health_source_fingerprint(path_value: &OsStr, model_root: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    let directories = env::split_paths(path_value)
        .take(READINESS_PATH_DIRECTORY_LIMIT)
        .collect::<Vec<_>>();
    for executable in READINESS_EXECUTABLES {
        executable.hash(&mut hasher);
        let resolved = directories.iter().find_map(|directory| {
            let candidate = directory.join(executable);
            let metadata = std::fs::metadata(&candidate).ok()?;
            executable_file(&metadata).then(|| (candidate, file_metadata_identity(&metadata)))
        });
        resolved.hash(&mut hasher);
    }
    file_identity(model_root).hash(&mut hasher);
    hasher.finish()
}

fn current_health_source_fingerprint() -> u64 {
    let path = env::var_os("PATH").unwrap_or_default();
    let models = echo::stt::ModelCache::from_env();
    health_source_fingerprint(&path, models.dir())
}

fn collect_health() -> Health {
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
    Health {
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
        language_warning: echo::stt::language_warning(),
    }
}

fn start_health_source_probe(generation: u64) -> bool {
    std::thread::Builder::new()
        .name("echo-health-source-probe".to_string())
        .spawn(move || {
            let result = std::panic::catch_unwind(current_health_source_fingerprint);
            let mut state = recover_cache_lock(health_cache_state(), "health state");
            match result {
                Ok(source_fingerprint) => {
                    state.probe_completed(generation, health_clock(), source_fingerprint);
                }
                Err(_) => state.probe_failed(generation),
            }
        })
        .is_ok()
}

fn collect_and_publish_health<C, P>(
    state: &Mutex<HealthCacheState>,
    generation: u64,
    collect: C,
    on_publish: P,
) -> (Health, bool)
where
    C: FnOnce() -> Health,
    P: FnOnce(&Health),
{
    let health = collect();
    let published = {
        let mut state = recover_cache_lock(state, "health state");
        let published = state.publish_if_current(generation, health_clock(), None, health.clone());
        if published {
            on_publish(&health);
        }
        published
    };
    (health, published)
}

fn publish_health_for_generation(generation: u64) -> Health {
    let health = match std::panic::catch_unwind(collect_health) {
        Ok(health) => health,
        Err(payload) => {
            recover_cache_lock(health_cache_state(), "health state").refresh_failed(generation);
            std::panic::resume_unwind(payload);
        }
    };
    collect_and_publish_health(
        health_cache_state(),
        generation,
        || health,
        |health| {
            *recover_cache_lock(&HEALTH, "health mirror") = Some((Instant::now(), health.clone()));
        },
    )
    .0
}

fn start_health_refresh(generation: u64) -> bool {
    std::thread::Builder::new()
        .name("echo-health-refresh".to_string())
        .spawn(move || {
            let result = std::panic::catch_unwind(|| publish_health_for_generation(generation));
            if result.is_err() {
                recover_cache_lock(health_cache_state(), "health state").refresh_failed(generation);
            }
        })
        .is_ok()
}

fn health_snapshot() -> Health {
    let now = health_clock();
    let decision = recover_cache_lock(health_cache_state(), "health state").read(now);
    if let Some(generation) = decision.probe_generation {
        if !start_health_source_probe(generation) {
            recover_cache_lock(health_cache_state(), "health state").probe_failed(generation);
        }
    }
    if let Some(generation) = decision.refresh_generation {
        if !start_health_refresh(generation) {
            recover_cache_lock(health_cache_state(), "health state").refresh_failed(generation);
        }
    }
    if let Some(health) = decision.cached() {
        return health.clone();
    }
    debug_assert!(decision.recollects());
    publish_health_for_generation(
        decision
            .collection_generation
            .expect("recollection generation"),
    )
}

pub(super) fn health_invalidate() {
    let mut state = recover_cache_lock(health_cache_state(), "health state");
    state.invalidate();
    *recover_cache_lock(&HEALTH, "health mirror") = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use echo_core::{
        EngineId, HistoryRow, InjectBackend, InjectReport, WhisperAccelerationSkip, WhisperRunMode,
        WhisperRuntimeBackend, WhisperRuntimeSource, WhisperTuningTelemetry,
    };
    use std::collections::VecDeque;

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
            collect_and_publish_health(
                &collection_state,
                generation,
                || {
                    started_send.send(()).unwrap();
                    release_receive.recv().unwrap();
                    fake_health("stale")
                },
                |_| {},
            )
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
        echo::status::write_status(echo_core::SessionState::Idle, None, None, Some("first"))
            .unwrap();
        last_run_invalidate();
        LAST_RUN_PROJECTIONS.store(0, std::sync::atomic::Ordering::Relaxed);

        assert_eq!(last_run().map(|run| run.infer_ms), Some(10));

        History::append_default(history_row("second", 20)).unwrap();
        echo::status::write_status(echo_core::SessionState::Idle, None, None, Some("second"))
            .unwrap();
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
        if run_isolated_last_run_test(
            "status::tests::corrupt_unchanged_history_is_loaded_at_most_once",
        ) {
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
}
