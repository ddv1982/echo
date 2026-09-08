use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use echo_core::PrivateDir;
use fs2::FileExt;
use sha2::{Digest, Sha256};

use crate::audio;
use crate::process_identity::{observe as read_process_observation, ProcessObservation};

use super::control::ControlIntent;

static RECORDING_IN_PROCESS: AtomicBool = AtomicBool::new(false);
const OWNER_REVISION_STEP: u64 = 2;

#[must_use]
pub fn recording_in_process() -> bool {
    RECORDING_IN_PROCESS.load(Ordering::Relaxed)
}

pub(super) struct InProcessSession;

impl InProcessSession {
    pub(super) fn start() -> Self {
        RECORDING_IN_PROCESS.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for InProcessSession {
    fn drop(&mut self) {
        RECORDING_IN_PROCESS.store(false, Ordering::Relaxed);
        audio::process_meter().publish(0.0);
    }
}

/// Idle/stale status (no session) can still stop a live lock; a different
/// observed session must not stop a replacement owner.
pub(super) fn observation_allows_toggle_stop(
    observed_session: Option<&str>,
    owner: &LockOwner,
) -> bool {
    observed_session.is_none() || owner.token.as_deref() == observed_session
}

pub(super) fn session_changed_before_stop() -> String {
    "recording session changed before stop was accepted".to_string()
}

pub(super) enum ToggleAction {
    Start(ToggleSession),
    Stop(LockOwner),
}

impl ToggleAction {
    pub(super) fn recording_token(&self) -> Option<&str> {
        match self {
            Self::Start(session) => Some(&session.token),
            Self::Stop(owner) => owner.token.as_deref(),
        }
    }
}

pub(super) enum LockAcquisition {
    Started(ToggleSession),
    Busy(LockOwner),
}

pub(super) struct ToggleSession {
    directory: PrivateDir,
    _gate: std::fs::File,
    pub(super) token: String,
    revision: AtomicU64,
}

pub struct RecordingSession(pub(super) ToggleSession);

impl RecordingSession {
    pub fn acquire() -> Result<Self, String> {
        Self::acquire_in(&echo_core::data_dir())
    }

    pub(super) fn acquire_in(dir: &Path) -> Result<Self, String> {
        match ToggleSession::acquire_in(dir)? {
            LockAcquisition::Started(session) => Ok(Self(session)),
            LockAcquisition::Busy(_) => Err("Another recording is already active.".to_string()),
        }
    }

    #[must_use]
    pub fn stop_requested(&self) -> bool {
        self.0.stop_requested()
    }

    pub fn clear_stop_request(&self) {
        self.0.clear_stop_request();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LockOwner {
    pub(super) pid: u32,
    pub(super) token: Option<String>,
    pub(super) start_time_ticks: Option<u64>,
    pub(super) scoped_intents: bool,
}

impl ToggleSession {
    pub(super) fn start_or_stop_in(
        dir: &Path,
        observed_session: Option<&str>,
    ) -> Result<ToggleAction, String> {
        match Self::acquire_in(dir)? {
            LockAcquisition::Started(session) => Ok(ToggleAction::Start(session)),
            LockAcquisition::Busy(owner) => {
                if !observation_allows_toggle_stop(observed_session, &owner) {
                    return Err(session_changed_before_stop());
                }
                write_stop_request(&intent_path(dir, "stop", &owner), &owner)?;
                Ok(ToggleAction::Stop(owner))
            }
        }
    }

    #[cfg(test)]
    pub(super) fn request_stop_if_active_in(dir: &Path) -> Result<bool, String> {
        let lock_path = dir.join("recording.lock");
        let Some(owner) = live_lock_owner(&lock_path) else {
            if let Ok(directory) = PrivateDir::open(dir) {
                let _ = directory.remove_file("recording.lock".as_ref());
                let _ = directory.remove_file("recording.stop".as_ref());
            }
            return Ok(false);
        };
        write_stop_request(&intent_path(dir, "stop", &owner), &owner)?;
        Ok(true)
    }

    pub(super) fn request_intent_for_token_in(
        dir: &Path,
        token: &str,
        intent: ControlIntent,
    ) -> Result<bool, String> {
        let Some(owner) = live_lock_owner(&dir.join("recording.lock")) else {
            return Ok(false);
        };
        if owner.token.as_deref() != Some(token) {
            return Ok(false);
        }
        write_stop_request(&intent_path(dir, intent.file_kind(), &owner), &owner)?;
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn try_start_in(dir: &Path) -> Result<Option<Self>, String> {
        match Self::acquire_in(dir)? {
            LockAcquisition::Started(session) => Ok(Some(session)),
            LockAcquisition::Busy(_) => Ok(None),
        }
    }

    pub(super) fn acquire_in(dir: &Path) -> Result<LockAcquisition, String> {
        let directory = PrivateDir::open(dir).map_err(|err| err.to_string())?;
        let gate = directory
            .open_or_create("recording.gate".as_ref())
            .map_err(|err| err.to_string())?;
        match gate.try_lock_exclusive() {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                for _ in 0..32 {
                    if let Some(owner) = live_lock_owner(&dir.join("recording.lock")) {
                        return Ok(LockAcquisition::Busy(owner));
                    }
                    std::thread::yield_now();
                }
                return Err("another recording is starting".to_string());
            }
            Err(err) => return Err(err.to_string()),
        }
        let process = read_process_observation(std::process::id())
            .ok_or_else(|| "cannot read recording owner process identity".to_string())?;
        if process.state == 'Z' {
            return Err("recording owner process is a zombie".to_string());
        }
        for _ in 0..2 {
            let token = new_session_token();
            let candidate = format!(".recording.lock.{token}");
            match directory.create_new(candidate.as_ref()) {
                Ok(mut lock) => {
                    writeln!(
                        lock,
                        "{}\n{token}\n{}\nscoped-intents-v1",
                        std::process::id(),
                        process.start_time_ticks
                    )
                    .map_err(|err| err.to_string())?;
                    drop(lock);
                    match directory.hard_link(candidate.as_ref(), "recording.lock".as_ref()) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                            let _ = directory.remove_file(candidate.as_ref());
                            if let Some(owner) = live_lock_owner(&dir.join("recording.lock")) {
                                return Ok(LockAcquisition::Busy(owner));
                            }
                            let _ = directory.remove_file("recording.lock".as_ref());
                            let _ = directory.remove_file("recording.stop".as_ref());
                            continue;
                        }
                        Err(err) => {
                            let _ = directory.remove_file(candidate.as_ref());
                            return Err(err.to_string());
                        }
                    }
                    let _ = directory.remove_file(candidate.as_ref());
                    return Ok(LockAcquisition::Started(Self {
                        directory,
                        _gate: gate,
                        token,
                        revision: AtomicU64::new(0),
                    }));
                }
                Err(err) => return Err(err.to_string()),
            }
        }
        Err("could not acquire recording lock".to_string())
    }

    pub(super) fn stop_requested(&self) -> bool {
        let scoped = self
            .directory
            .read_to_string(self.intent_name("stop").as_ref())
            .ok()
            .is_some_and(|request| stop_request_matches(Some(&self.token), &request));
        scoped
            || self
                .directory
                .read_to_string("recording.stop".as_ref())
                .ok()
                .is_some_and(|request| stop_request_matches(Some(&self.token), &request))
    }

    pub(super) fn cancel_requested(&self) -> bool {
        let scoped = self
            .directory
            .read_to_string(self.intent_name("cancel").as_ref())
            .ok()
            .is_some_and(|request| stop_request_matches(Some(&self.token), &request));
        scoped
            || self
                .directory
                .read_to_string("recording.cancel".as_ref())
                .ok()
                .is_some_and(|request| stop_request_matches(Some(&self.token), &request))
    }

    pub(super) fn next_revision(&self) -> u64 {
        self.revision
            .fetch_add(OWNER_REVISION_STEP, Ordering::SeqCst)
            + OWNER_REVISION_STEP
    }

    pub(super) fn intent_name(&self, kind: &str) -> String {
        self.directory
            .read_to_string("recording.lock".as_ref())
            .ok()
            .and_then(|raw| parse_lock_owner(&raw))
            .filter(|owner| {
                owner.token.as_deref() == Some(self.token.as_str()) || owner.token.is_none()
            })
            .map(|owner| intent_file_name(kind, &owner))
            .unwrap_or_else(|| scoped_intent_name(kind, &self.token))
    }

    pub(super) fn clear_stop_request(&self) {
        if self.stop_requested() {
            let _ = self
                .directory
                .remove_file(scoped_intent_name("stop", &self.token).as_ref());
            if self
                .directory
                .read_to_string("recording.stop".as_ref())
                .ok()
                .is_some_and(|request| stop_request_matches(Some(&self.token), &request))
            {
                let _ = self.directory.remove_file("recording.stop".as_ref());
            }
        }
    }
}

impl Drop for ToggleSession {
    fn drop(&mut self) {
        let still_owned = self
            .directory
            .read_to_string("recording.lock".as_ref())
            .ok()
            .and_then(|raw| parse_lock_owner(&raw))
            .is_some_and(|owner| owner.token.as_deref() == Some(self.token.as_str()));
        if still_owned {
            let _ = self.directory.remove_file("recording.stop".as_ref());
            let _ = self.directory.remove_file("recording.cancel".as_ref());
            let _ = self
                .directory
                .remove_file(scoped_intent_name("stop", &self.token).as_ref());
            let _ = self
                .directory
                .remove_file(scoped_intent_name("cancel", &self.token).as_ref());
            let _ = self.directory.remove_file("recording.lock".as_ref());
        }
    }
}

pub(super) fn new_session_token() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}-{}-{}-{}",
        std::process::id(),
        now.as_secs(),
        now.subsec_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) fn parse_lock_owner(raw: &str) -> Option<LockOwner> {
    let mut lines = raw.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let token = lines
        .next()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    let start_time_ticks = lines
        .next()
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
        .map(str::parse)
        .transpose()
        .ok()?;
    let scoped_intents = lines
        .next()
        .is_some_and(|marker| marker.trim() == "scoped-intents-v1");
    Some(LockOwner {
        pid,
        token,
        start_time_ticks,
        scoped_intents,
    })
}

pub(super) fn scoped_intent_name(kind: &str, token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let digest = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("recording.{kind}.{digest}")
}

pub(super) fn intent_path(dir: &Path, kind: &str, owner: &LockOwner) -> PathBuf {
    dir.join(intent_file_name(kind, owner))
}

pub(super) fn intent_file_name(kind: &str, owner: &LockOwner) -> String {
    match (owner.scoped_intents, owner.token.as_deref()) {
        (true, Some(token)) => scoped_intent_name(kind, token),
        _ => format!("recording.{kind}"),
    }
}

pub(super) fn live_lock_owner(path: &Path) -> Option<LockOwner> {
    let directory = PrivateDir::open(path.parent()?).ok()?;
    let raw = directory.read_to_string(path.file_name()?).ok()?;
    live_lock_owner_from_at(&raw, lock_timestamp(path), read_process_observation)
}

pub(super) fn lock_owner_is_alive(path: &Path) -> bool {
    live_lock_owner(path).is_some()
}

pub(super) fn write_stop_request(path: &Path, owner: &LockOwner) -> Result<(), String> {
    let contents = owner.token.as_deref().unwrap_or("stop");
    echo_core::write_atomic_private(path, format!("{contents}\n").as_bytes())
}

pub(super) fn stop_request_matches(token: Option<&str>, request: &str) -> bool {
    match token {
        Some(token) => request.trim() == token,
        None => request.trim() == "stop",
    }
}

pub(super) fn live_lock_owner_from_at(
    raw: &str,
    fallback_acquired_at: Option<u128>,
    observe: impl FnOnce(u32) -> Option<ProcessObservation>,
) -> Option<LockOwner> {
    let owner = parse_lock_owner(raw)?;
    let process = observe(owner.pid)?;
    if process.state == 'Z' {
        return None;
    }
    match owner.start_time_ticks {
        Some(recorded) if recorded == process.start_time_ticks => Some(owner),
        Some(_) => None,
        None => {
            let acquired_at = legacy_token_timestamp(&owner).or(fallback_acquired_at)?;
            let started_at = process.start_unix_nanos?;
            (started_at <= acquired_at).then_some(owner)
        }
    }
}

pub(super) fn lock_timestamp(path: &Path) -> Option<u128> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_nanos())
}

pub(super) fn legacy_token_timestamp(owner: &LockOwner) -> Option<u128> {
    let token = owner.token.as_deref()?;
    let mut fields = token.split('-');
    let token_pid = fields.next()?.parse::<u32>().ok()?;
    let seconds = fields.next()?.parse::<u128>().ok()?;
    let nanos = fields.next()?.parse::<u128>().ok()?;
    let _sequence = fields.next()?.parse::<u64>().ok()?;
    if token_pid != owner.pid || nanos >= 1_000_000_000 || fields.next().is_some() {
        return None;
    }
    seconds.checked_mul(1_000_000_000)?.checked_add(nanos)
}

/// True while any process holds an active recording session.
#[must_use]
pub fn session_active() -> bool {
    session_active_at(&echo_core::data_dir().join("recording.lock"))
}

pub(crate) fn session_active_at(path: &Path) -> bool {
    lock_owner_is_alive(path)
}

#[must_use]
pub(crate) fn session_matches_at(dir: &Path, session_id: &str) -> bool {
    live_lock_owner(&dir.join("recording.lock"))
        .is_some_and(|owner| owner.token.as_deref() == Some(session_id))
}
