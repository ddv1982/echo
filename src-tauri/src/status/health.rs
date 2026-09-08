use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use echo::audio::AudioCapture;

use super::recover_cache_lock;

#[derive(Debug, Clone)]
pub struct Health {
    pub microphone_ready: bool,
    pub engine_name: String,
    pub engine_ready: bool,
    pub injection_name: String,
    pub injection_ready: bool,
    pub current_exe: String,
    pub first_path_hit: Option<String>,
    pub stale_installs: Vec<String>,
    pub language_warning: Option<String>,
}

const HEALTH_SOURCE_FRESHNESS: Duration = Duration::from_secs(1);
const HEALTH_TTL: Duration = Duration::from_secs(10);

pub(super) struct CachedHealth {
    pub(super) collected_at: Duration,
    pub(super) source_checked_at: Duration,
    pub(super) source_fingerprint: Option<u64>,
    pub(super) refresh_required: bool,
    pub(super) health: Health,
}

pub struct HealthCacheState {
    pub(super) source_freshness: Duration,
    pub(super) ttl: Duration,
    pub(super) cached: Option<CachedHealth>,
    pub(super) generation: u64,
    pub(super) probe_pending: Option<u64>,
    pub(super) refresh_pending: Option<u64>,
}

impl HealthCacheState {
    pub(super) fn new(source_freshness: Duration, ttl: Duration) -> Self {
        Self {
            source_freshness,
            ttl,
            cached: None,
            generation: 0,
            probe_pending: None,
            refresh_pending: None,
        }
    }

    pub(super) fn publish(
        &mut self,
        now: Duration,
        source_fingerprint: Option<u64>,
        health: Health,
    ) {
        self.cached = Some(CachedHealth {
            collected_at: now,
            source_checked_at: now,
            source_fingerprint,
            refresh_required: false,
            health,
        });
        self.refresh_pending = None;
    }

    pub(super) fn publish_if_current(
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

    pub(super) fn read(&mut self, now: Duration) -> HealthCacheDecision {
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

    pub(super) fn probe_completed(
        &mut self,
        generation: u64,
        now: Duration,
        source_fingerprint: u64,
    ) {
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

    pub(super) fn invalidate(&mut self) {
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

pub(super) struct HealthCacheDecision {
    pub(super) cached: Option<Health>,
    pub(super) probe_generation: Option<u64>,
    pub(super) refresh_generation: Option<u64>,
    pub(super) collection_generation: Option<u64>,
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

    pub(super) fn cached(&self) -> Option<&Health> {
        self.cached.as_ref()
    }

    #[cfg(test)]
    pub(super) fn starts_probe(&self) -> bool {
        self.probe_generation.is_some()
    }

    #[cfg(test)]
    pub(super) fn starts_refresh(&self) -> bool {
        self.refresh_generation.is_some()
    }

    pub(super) fn recollects(&self) -> bool {
        self.collection_generation.is_some()
    }
}

pub fn health_cache_state() -> &'static Mutex<HealthCacheState> {
    static STATE: OnceLock<Mutex<HealthCacheState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HealthCacheState::new(HEALTH_SOURCE_FRESHNESS, HEALTH_TTL)))
}

#[cfg(test)]
pub fn seed_health_for_test(health: Health) {
    let mut state = recover_cache_lock(health_cache_state(), "health state");
    state.invalidate();
    state.probe_pending = None;
    state.publish(health_clock(), None, health);
}

pub fn health_clock() -> Duration {
    static STARTED_AT: OnceLock<Instant> = OnceLock::new();
    STARTED_AT.get_or_init(Instant::now).elapsed()
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum FileIdentity {
    Missing,
    MetadataError {
        kind: std::io::ErrorKind,
        raw_os_error: Option<i32>,
    },
    Present(FileMetadataIdentity),
}

#[cfg(unix)]
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct FileMetadataIdentity {
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
pub(super) struct FileMetadataIdentity {
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

pub(super) fn file_identity(path: &Path) -> FileIdentity {
    match std::fs::metadata(path) {
        Ok(metadata) => FileIdentity::Present(file_metadata_identity(&metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileIdentity::Missing,
        Err(error) => FileIdentity::MetadataError {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
        },
    }
}

pub(super) fn same_history_contents(left: &FileIdentity, right: &FileIdentity) -> bool {
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

pub fn health_source_fingerprint(path_value: &OsStr, model_root: &Path) -> u64 {
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

pub fn collect_and_publish_health(
    state: &Mutex<HealthCacheState>,
    generation: u64,
    collect: impl FnOnce() -> Health,
) -> (Health, bool) {
    let health = collect();
    let published = recover_cache_lock(state, "health state").publish_if_current(
        generation,
        health_clock(),
        None,
        health.clone(),
    );
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
    collect_and_publish_health(health_cache_state(), generation, || health).0
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

pub(super) fn health_snapshot() -> Health {
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

pub fn health_invalidate() {
    let mut state = recover_cache_lock(health_cache_state(), "health state");
    state.invalidate();
}
