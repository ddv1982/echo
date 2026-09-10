use super::control::{
    apply_toggle_stop_intent_in, decide_toggle_intent, finish_toggle_stop_with,
    request_control_ack_with, ControlIntent,
};
use super::lease::{
    intent_path, live_lock_owner, live_lock_owner_from_at, new_session_token, parse_lock_owner,
    scoped_intent_name, stop_request_matches, write_stop_request, LockAcquisition, LockOwner,
    ToggleAction, ToggleSession,
};
use super::pipeline::{
    audio_fixture_path, capture_from, capture_with_started_at, dictionary_for_transcription,
    history_append_warning, new_history_id, play_fixture_capture, play_fixture_capture_with_player,
    transcription_failure, CaptureFailure, PublishedSession, StopWhen,
};
use super::upgrade::{attempt_upgrade_takeover_in, reserve_upgrade_takeover_in};
use super::*;
use echo_core::FailReason;
use std::cell::Cell;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, History, HistoryRow, InjectReport, Pcm16kMono, PrivateDir, RecordingLimit,
    SessionState, SAMPLE_RATE_HZ,
};

use crate::audio;
use crate::process_identity::{observe as read_process_observation, ProcessObservation};
use crate::status;

fn lock_owner(path: &Path) -> Option<LockOwner> {
    let directory = PrivateDir::open(path.parent()?).ok()?;
    let raw = directory.read_to_string(path.file_name()?).ok()?;
    parse_lock_owner(&raw)
}

fn live_lock_owner_from(
    raw: &str,
    observe: impl FnOnce(u32) -> Option<ProcessObservation>,
) -> Option<LockOwner> {
    live_lock_owner_from_at(raw, None, observe)
}

fn observed(state: char, start_time_ticks: u64, start_unix_nanos: u128) -> ProcessObservation {
    ProcessObservation {
        pid: 41,
        state,
        start_time_ticks,
        start_unix_nanos: Some(start_unix_nanos),
    }
}

#[test]
fn injecting_is_published_before_the_injection_effect_runs() {
    let dir = std::env::temp_dir().join(format!(
        "echo-published-injecting-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let session_id = session.token.clone();
    let status_path = dir.join("status");
    let mut published =
        PublishedSession::new_in(StopWhen::ToggleFile(session), status_path.clone());

    assert_eq!(
        published.start_recording(RecordingLimit::DEFAULT).unwrap(),
        2
    );
    published.finish_capture().unwrap();

    let (entered_send, entered_receive) = mpsc::sync_channel(0);
    let (release_send, release_receive) = mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        published
            .begin_injecting_then(|| {
                entered_send.send(()).unwrap();
                release_receive.recv().unwrap();
            })
            .unwrap();
    });

    entered_receive.recv().unwrap();
    let status = status::read_from(&status_path);
    assert_eq!(status.state, "Injecting");
    assert_eq!(status.session_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(status.revision, 6);
    assert_eq!(status.revision % 2, 0);
    let raw = fs::read_to_string(&status_path).unwrap();
    assert!(raw.contains(&format!("pid={}\n", std::process::id())));
    release_send.send(()).unwrap();
    worker.join().unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn injecting_publication_failure_prevents_the_injection_effect_and_releases_the_lease() {
    let dir = std::env::temp_dir().join(format!(
        "echo-injecting-publication-failure-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let session_id = session.token.clone();
    let status_path = dir.join("status");
    let mut published =
        PublishedSession::new_in(StopWhen::ToggleFile(session), status_path.clone());

    published.start_recording(RecordingLimit::DEFAULT).unwrap();
    published.finish_capture().unwrap();
    let status_backup = dir.join("status.backup");
    fs::rename(&status_path, &status_backup).unwrap();
    fs::create_dir(&status_path).unwrap();

    let effect_ran = Cell::new(false);
    let error = published
        .begin_injecting_then(|| effect_ran.set(true))
        .unwrap_err();
    assert!(!effect_ran.get());
    assert!(
        error.contains("Is a directory") || error.contains("directory"),
        "{error}"
    );
    fs::remove_dir(&status_path).unwrap();
    fs::rename(&status_backup, &status_path).unwrap();
    assert_eq!(status::read_from(&status_path).state, "Transcribing");
    assert!(dir.join("recording.lock").exists());

    drop(published);
    assert!(!dir.join("recording.lock").exists());
    assert!(!session_matches_at(&dir, &session_id));
    assert_eq!(status::read_from(&status_path).state, "Idle");
    let replacement = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let replacement_id = replacement.token.clone();
    let mut replacement =
        PublishedSession::new_in(StopWhen::ToggleFile(replacement), status_path.clone());
    replacement
        .start_recording(RecordingLimit::DEFAULT)
        .unwrap();
    let status = status::read_from(&status_path);
    assert_eq!(status.state, "Recording");
    assert_eq!(status.session_id.as_deref(), Some(replacement_id.as_str()));
    assert_ne!(replacement_id, session_id);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dictionary_read_failure_is_visible_without_discarding_the_transcript() {
    let (dictionary, warning) =
        dictionary_for_transcription(Err("dictionary permission denied".to_string()));
    let warning = warning.expect("dictionary failure should be reported");
    assert!(dictionary.entries().is_empty());

    let transcript = "Keep résumé text";
    let body = status::render(SessionState::Idle, Some(transcript), Some(&warning), None);
    assert!(body.contains("state=Idle\n"), "{body}");
    assert!(body.contains("last=Keep résumé text\n"), "{body}");
    assert!(body.contains("custom replacements were skipped"), "{body}");
    assert!(body.contains("Echo → Dictionary"), "{body}");
}

#[test]
fn malformed_dictionary_reaches_the_visible_warning_path() {
    let dir = std::env::temp_dir().join(format!(
        "echo-rec-malformed-dictionary-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dictionary.json");
    fs::write(&path, "{\"entries\": [").unwrap();

    let (dictionary, warning) = dictionary_for_transcription(Dictionary::load_from(&path));
    let warning = warning.expect("malformed dictionary should be reported");

    assert!(dictionary.entries().is_empty());
    assert!(
        warning.contains("custom replacements were skipped"),
        "{warning}"
    );
    assert!(warning.contains("invalid JSON"), "{warning}");
    assert!(warning.contains(path.to_str().unwrap()), "{warning}");
    assert_eq!(
        fs::read_to_string(dir.join("dictionary.json.corrupt")).unwrap(),
        "{\"entries\": ["
    );
    assert!(!path.exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn history_append_failure_is_visible_and_not_described_as_persisted() {
    let warning = history_append_warning(Err("read-only file system".to_string()))
        .expect("history failure should be reported");
    let body = status::render(
        SessionState::Idle,
        Some("still recoverable"),
        Some(&warning),
        None,
    );

    assert!(body.contains("last=still recoverable\n"), "{body}");
    assert!(
        body.contains("couldn't save the transcript to history"),
        "{body}"
    );
    assert!(body.contains("read-only file system"), "{body}");
    assert!(
        body.contains("check the data directory permissions"),
        "{body}"
    );
}

#[test]
fn empty_transcription_returns_to_idle_without_injecting() {
    let dir = std::env::temp_dir().join(format!(
        "echo-skip-insertion-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let status_path = dir.join("status");
    let mut published =
        PublishedSession::new_in(StopWhen::ToggleFile(session), status_path.clone());

    published.start_recording(RecordingLimit::DEFAULT).unwrap();
    published.finish_capture().unwrap();
    published
        .complete_without_insertion(None, None, None)
        .unwrap();

    let status = status::read_from(&status_path);
    assert_eq!(status.state, "Idle");
    assert_eq!(status.last.as_deref(), None);
    let raw = fs::read_to_string(&status_path).unwrap();
    assert!(!raw.contains("state=Injecting"), "{raw}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn history_started_at_persists_capture_time_across_calendar_boundary() {
    const BEFORE_MIDNIGHT: u64 = 86_399;
    const AFTER_MIDNIGHT: u64 = 86_401;
    let clock = Cell::new(BEFORE_MIDNIGHT);
    let (_, started_at) = capture_with_started_at(
        || UNIX_EPOCH + Duration::from_secs(clock.get()),
        || {
            clock.set(AFTER_MIDNIGHT);
            Ok::<(), ()>(())
        },
    )
    .unwrap();

    let dir = std::env::temp_dir().join(format!(
        "echo-rec-started-at-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let path = dir.join("history.json");
    let mut history = History::load_from(&path).unwrap();
    history
        .append(HistoryRow {
            id: "cross-midnight".to_string(),
            text: "captured before midnight".to_string(),
            raw: "captured before midnight".to_string(),
            engine: echo_core::EngineId::Whisper {
                model: "test".to_string(),
            },
            started_at,
            infer_ms: 1,
            inject: InjectReport::ClipboardOnly,
            detail: echo_core::RunDetail::default(),
        })
        .unwrap();

    let reloaded = History::load_from(&path).unwrap();
    assert_eq!(clock.get(), AFTER_MIDNIGHT);
    assert_eq!(reloaded.rows()[0].started_at, BEFORE_MIDNIGHT);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn history_ids_are_uuid_v4_and_unique() {
    let ids = (0..1_000)
        .map(|_| new_history_id())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 1_000);
    assert!(ids
        .iter()
        .all(|id| { uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.get_version_num() == 4) }));
}

#[test]
fn process_stat_parser_handles_spaces_and_parentheses_in_comm() {
    let mut trailing = vec!["0"; 18];
    trailing.push("424242");
    let raw = format!("77 (echo worker (old)) S {}", trailing.join(" "));
    assert_eq!(
        crate::process_identity::parse_stat(&raw),
        Some(('S', 424242))
    );
}

#[test]
fn new_lock_accepts_a_live_matching_process_identity() {
    let raw = "41\n41-200-123-0\n9001\n";
    assert!(live_lock_owner_from(raw, |_| Some(observed('S', 9001, 100))).is_some());
}

#[test]
fn new_lock_rejects_a_zombie_owner() {
    let raw = "41\n41-200-123-0\n9001\n";
    assert!(live_lock_owner_from(raw, |_| Some(observed('Z', 9001, 100))).is_none());
}

#[test]
fn new_lock_rejects_a_reused_pid_with_a_different_start_identity() {
    let raw = "41\n41-200-123-0\n9001\n";
    assert!(
        live_lock_owner_from(raw, |_| Some(observed('S', 9002, 300_000_000_000))).is_none(),
        "a reused pid has a different field-22 start identity"
    );
}

#[test]
fn legacy_two_line_lock_requires_process_to_predate_token() {
    let raw = "41\n41-200-123-0\n";
    let acquired = 200_000_000_123_u128;
    assert!(
        live_lock_owner_from(raw, |_| Some(observed('S', 9001, acquired - 1))).is_some(),
        "a currently-running legacy owner remains protected"
    );
    assert!(
        live_lock_owner_from(raw, |_| Some(observed('S', 9002, acquired + 1))).is_none(),
        "a process started after the token cannot inherit a legacy lock"
    );
    assert!(
        live_lock_owner_from(raw, |_| Some(ProcessObservation {
            pid: 41,
            state: 'S',
            start_time_ticks: 9001,
            start_unix_nanos: None,
        }))
        .is_none(),
        "legacy validation fails closed when start time cannot be resolved"
    );
}

#[test]
fn takeover_boundary_blocks_acquisition_and_spawn_failure_reopens_it() {
    let dir = std::env::temp_dir().join(format!(
        "echo-takeover-failure-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let outcome = attempt_upgrade_takeover_in(&dir, || {
        assert!(ToggleSession::try_start_in(&dir).unwrap().is_none());
        Err(std::io::Error::new(
            ErrorKind::NotFound,
            "replacement missing",
        ))
    });
    assert!(matches!(outcome, UpgradeTakeover::SpawnFailed(_)));
    assert!(ToggleSession::try_start_in(&dir).unwrap().is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn takeover_reservation_blocks_recording_until_released() {
    let dir = std::env::temp_dir().join(format!(
        "echo-takeover-success-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let reservation = reserve_upgrade_takeover_in(&dir).unwrap();
    assert!(ToggleSession::try_start_in(&dir).unwrap().is_none());
    drop(reservation);
    assert!(ToggleSession::try_start_in(&dir).unwrap().is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn active_acquisition_defers_takeover_without_spawning() {
    let dir = std::env::temp_dir().join(format!(
        "echo-takeover-busy-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let session = match ToggleSession::acquire_in(&dir).unwrap() {
        LockAcquisition::Started(session) => session,
        LockAcquisition::Busy(_) => panic!("test directory should be idle"),
    };
    let mut spawned = false;
    let outcome = attempt_upgrade_takeover_in(&dir, || {
        spawned = true;
        Ok(())
    });
    assert!(matches!(outcome, UpgradeTakeover::Deferred));
    assert!(!spawned);
    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn stale_reclaim_admits_exactly_one_concurrent_recording() {
    let dir = std::env::temp_dir().join(format!(
        "echo-stale-reclaim-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("recording.lock"), "99999999\nstale\n1\n").unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let contenders = (0..2)
        .map(|_| {
            let dir = dir.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                ToggleSession::try_start_in(&dir)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let sessions = contenders
        .into_iter()
        .map(|contender| contender.join().unwrap().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        sessions.iter().filter(|session| session.is_some()).count(),
        1
    );
    drop(sessions);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn stop_only_never_starts_a_recording() {
    let dir = std::env::temp_dir().join(format!("echo-stop-only-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);

    assert!(!ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(!dir.join("recording.lock").exists());
    assert!(!dir.join("recording.stop").exists());

    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let stop = dir.join(scoped_intent_name("stop", &session.token));
    assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(dir.join("recording.lock").exists());
    assert!(stop.exists());

    drop(session);
    assert!(!ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(!dir.join("recording.lock").exists());
    assert!(!stop.exists());
}

#[cfg(unix)]
#[test]
fn recording_lock_secures_its_directory_and_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!(
        "echo-recording-private-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(dir.join("recording.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let owner = lock_owner(&dir.join("recording.lock")).unwrap();
    assert_eq!(
        owner.start_time_ticks,
        Some(
            read_process_observation(std::process::id())
                .unwrap()
                .start_time_ticks
        ),
        "new on-disk locks include the owner's /proc start identity"
    );

    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn token_scoped_stop_ignores_an_unrelated_session() {
    let dir = std::env::temp_dir().join(format!("echo-scoped-stop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

    assert!(!ToggleSession::request_intent_for_token_in(
        &dir,
        "another-session",
        ControlIntent::CaptureStop
    )
    .unwrap());
    assert!(!dir.join("recording.stop").exists());
    assert!(ToggleSession::request_intent_for_token_in(
        &dir,
        &session.token,
        ControlIntent::CaptureStop
    )
    .unwrap());
    assert!(session.stop_requested());
}

#[test]
fn clearing_a_matching_legacy_stop_removes_the_flat_signal() {
    let dir = std::env::temp_dir().join(format!("echo-clear-flat-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    fs::write(dir.join("recording.stop"), format!("{}\n", session.token)).unwrap();
    assert!(session.stop_requested());
    session.clear_stop_request();
    assert!(!session.stop_requested());
    assert!(!dir.join("recording.stop").exists());
}

#[test]
fn live_pid_only_lock_receives_an_observable_legacy_stop_request() {
    let dir = std::env::temp_dir().join(format!(
        "echo-legacy-stop-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    fs::write(
        dir.join("recording.lock"),
        format!("{}\n", std::process::id()),
    )
    .unwrap();

    let owner = live_lock_owner(&dir.join("recording.lock")).expect("live legacy owner");
    assert_eq!(owner.token, None);
    assert!(matches!(
        ToggleSession::start_or_stop_in(&dir, None).unwrap(),
        ToggleAction::Stop(LockOwner { token: None, .. })
    ));
    let request = fs::read_to_string(dir.join("recording.stop")).unwrap();
    assert!(stop_request_matches(owner.token.as_deref(), &request));
    assert_eq!(request, "stop\n");

    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn stale_stop_request_cannot_cancel_a_replacement_session() {
    let dir = std::env::temp_dir().join(format!("echo-stop-token-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);

    let first = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let first_owner = lock_owner(&dir.join("recording.lock")).unwrap();
    drop(first);
    let second = ToggleSession::try_start_in(&dir).unwrap().unwrap();

    write_stop_request(&dir.join("recording.stop"), &first_owner).unwrap();
    assert!(!second.stop_requested());
    assert_ne!(first_owner.token.as_deref(), Some(second.token.as_str()));
}

#[test]
fn stale_cancel_request_cannot_cancel_a_replacement_session() {
    let dir = std::env::temp_dir().join(format!("echo-cancel-token-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let first = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let first_owner = lock_owner(&dir.join("recording.lock")).unwrap();
    drop(first);
    let second = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    write_stop_request(&dir.join("recording.cancel"), &first_owner).unwrap();
    assert!(!second.cancel_requested());
    assert!(!ToggleSession::request_intent_for_token_in(
        &dir,
        "old-token",
        ControlIntent::TranscriptionCancel
    )
    .unwrap());
}

#[test]
fn scoped_owner_accepts_only_its_token_in_a_legacy_flat_request() {
    let directory = tempfile::tempdir().unwrap();
    let session = ToggleSession::try_start_in(directory.path())
        .unwrap()
        .unwrap();
    let stop = directory.path().join("recording.stop");
    fs::write(&stop, "stop\n").unwrap();
    assert!(!session.stop_requested());
    fs::write(&stop, "replaced-session\n").unwrap();
    assert!(!session.stop_requested());
    fs::write(&stop, format!("{}\n", session.token)).unwrap();
    assert!(session.stop_requested());
    assert!(!session.cancel_requested());
    fs::write(
        directory.path().join("recording.cancel"),
        format!("{}\n", session.token),
    )
    .unwrap();
    assert!(session.cancel_requested());
}

#[test]
fn delayed_old_session_intent_cannot_replace_new_session_intent() {
    let dir = std::env::temp_dir().join(format!("echo-scoped-intent-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let first = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let old = lock_owner(&dir.join("recording.lock")).unwrap();
    drop(first);
    let second = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    assert!(ToggleSession::request_intent_for_token_in(
        &dir,
        &second.token,
        ControlIntent::CaptureStop
    )
    .unwrap());
    assert!(ToggleSession::request_intent_for_token_in(
        &dir,
        &second.token,
        ControlIntent::TranscriptionCancel
    )
    .unwrap());
    write_stop_request(&intent_path(&dir, "stop", &old), &old).unwrap();
    write_stop_request(&intent_path(&dir, "cancel", &old), &old).unwrap();
    assert!(second.stop_requested());
    assert!(second.cancel_requested());
}

#[test]
fn duplicate_capture_stop_cannot_become_a_transcription_cancel() {
    let dir = std::env::temp_dir().join(format!(
        "echo-transcription-stop-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

    assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(session.stop_requested());
    assert!(!session.cancel_requested());
    session.clear_stop_request();
    assert!(!session.stop_requested());
    assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(session.stop_requested());
    assert!(!session.cancel_requested());
    assert!(ToggleSession::request_intent_for_token_in(
        &dir,
        &session.token,
        ControlIntent::TranscriptionCancel
    )
    .unwrap());
    assert!(session.cancel_requested());

    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn capture_toggle_observed_before_transition_never_becomes_cancel() {
    let owner = LockOwner {
        pid: 1,
        token: Some("capture-a".to_string()),
        start_time_ticks: Some(1),
        scoped_intents: true,
    };
    let recording = status::Status {
        state: "Recording".to_string(),
        last: None,
        last_history_id: None,
        error: None,
        recording_limit: None,
        session_id: Some("capture-a".to_string()),
        revision: 2,
    };
    let read_happened = Cell::new(false);
    let (_, cancel) = decide_toggle_intent(
        || {
            read_happened.set(true);
            recording.clone()
        },
        |_| {
            assert!(
                read_happened.get(),
                "status must be observed before stop writes its signal"
            );
            // This callback represents capture ending and the owner
            // immediately publishing Transcribing.
            Ok(ToggleAction::Stop(owner.clone()))
        },
    )
    .unwrap();
    assert!(!cancel);
    let transcribing = status::Status {
        state: "Transcribing".to_string(),
        ..recording.clone()
    };
    let (_, cancel) =
        decide_toggle_intent(|| transcribing, |_| Ok(ToggleAction::Stop(owner.clone()))).unwrap();
    assert!(cancel);
    let transcribing_replacement = status::Status {
        state: "Transcribing".to_string(),
        session_id: Some("capture-b".to_string()),
        ..recording
    };
    let (_, cancel) = decide_toggle_intent(
        || transcribing_replacement,
        |_| Ok(ToggleAction::Stop(owner)),
    )
    .unwrap();
    assert!(!cancel);
}

#[test]
fn stale_observation_does_not_stop_a_replacement_session() {
    let dir = std::env::temp_dir().join(format!(
        "echo-stale-obs-stop-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let observed = status::Status {
        state: "Recording".to_string(),
        last: None,
        last_history_id: None,
        error: None,
        recording_limit: None,
        session_id: Some("observed-a".to_string()),
        revision: 2,
    };

    let result = decide_toggle_intent(
        || observed,
        |observed| ToggleSession::start_or_stop_in(&dir, observed),
    );
    let Err(err) = result else {
        panic!("stale observation must not stop a replacement session");
    };

    assert!(err.contains("session changed"));
    assert!(!session.stop_requested());
    assert!(!session.cancel_requested());

    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn toggle_does_not_succeed_unless_required_cancel_was_written() {
    let dir = std::env::temp_dir().join(format!(
        "echo-toggle-cancel-fail-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let live = lock_owner(&dir.join("recording.lock")).unwrap();

    let stale = LockOwner {
        token: Some("observed-a".to_string()),
        ..live.clone()
    };
    let changed = finish_toggle_stop_with(stale, true, |owner| {
        apply_toggle_stop_intent_in(&dir, owner)
    });
    assert!(changed.unwrap_err().contains("session changed"));
    assert!(!session.cancel_requested());

    let io_fail = finish_toggle_stop_with(live.clone(), true, |_| Err("disk full".into()));
    assert_eq!(io_fail.unwrap_err(), "disk full");
    assert!(!session.cancel_requested());

    let pid_only = LockOwner {
        token: None,
        ..live
    };
    let missing = finish_toggle_stop_with(pid_only, true, |owner| {
        apply_toggle_stop_intent_in(&dir, owner)
    });
    assert!(missing.is_err());
    assert!(!session.cancel_requested());

    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn matching_observation_still_stops_and_cancels() {
    let dir = std::env::temp_dir().join(format!(
        "echo-matching-obs-stop-{}-{}",
        std::process::id(),
        new_session_token()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let token = session.token.clone();
    let owner = lock_owner(&dir.join("recording.lock")).unwrap();
    let observed = status::Status {
        state: "Transcribing".to_string(),
        last: None,
        last_history_id: None,
        error: None,
        recording_limit: None,
        session_id: Some(token.clone()),
        revision: 2,
    };

    let (action, cancel) = decide_toggle_intent(
        || observed,
        |observed| ToggleSession::start_or_stop_in(&dir, observed),
    )
    .unwrap();
    assert!(cancel);
    let ToggleAction::Stop(stopped) = &action else {
        panic!("matching observation should stop");
    };
    assert_eq!(stopped.token.as_deref(), Some(token.as_str()));
    assert!(session.stop_requested());

    let result = finish_toggle_stop_with(owner, true, |owner| {
        apply_toggle_stop_intent_in(&dir, owner)
    });
    assert_eq!(result.unwrap().as_deref(), Some(token.as_str()));
    assert!(session.cancel_requested());

    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn fixture_returns_wav_without_opening_host() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
    let stop = StopWhen::Timer(None);
    let capture = capture_from(
        Some(path),
        &stop,
        RecordingLimit::DEFAULT,
        &audio::LevelMeter::new(),
    )
    .expect("fixture wav");
    assert!(capture.pcm.duration_ms() >= 300);
    assert!(capture.peak_rms > 0.05);
}

#[test]
fn fixture_publishes_its_loudness_to_the_meter() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
    let stop = StopWhen::Timer(None);
    let meter = audio::LevelMeter::new();
    let probe = meter.clone();
    let peak = std::thread::spawn(move || {
        let mut peak = 0.0f32;
        for _ in 0..400 {
            peak = peak.max(probe.level());
            std::thread::sleep(Duration::from_millis(5));
        }
        peak
    });
    let _ = capture_from(Some(path), &stop, RecordingLimit::DEFAULT, &meter).expect("fixture wav");
    let peak = peak.join().expect("probe thread");
    assert!(peak > 0.01, "fixture playback moved the meter: {peak}");
}

#[test]
fn fixture_obeys_the_snapped_limit() {
    let pcm = Pcm16kMono::from_samples(vec![i16::MAX / 4; SAMPLE_RATE_HZ as usize * 2]);
    let capture = audio::CaptureResult::from_pcm(pcm);
    let result = play_fixture_capture(
        capture,
        &StopWhen::Timer(None),
        RecordingLimit::MIN,
        &audio::LevelMeter::new(),
    )
    .unwrap();

    assert_eq!(result.pcm.len(), SAMPLE_RATE_HZ as usize);
    assert_eq!(result.duration, Duration::from_secs(1));
}

#[test]
fn fixture_obeys_a_token_scoped_toggle_stop() {
    let dir = std::env::temp_dir().join(format!("echo-fixture-stop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let pcm = Pcm16kMono::from_samples(vec![i16::MAX / 4; SAMPLE_RATE_HZ as usize * 2]);
    let capture = audio::CaptureResult::from_pcm(pcm);
    let full_duration = capture.duration;
    let meter = audio::LevelMeter::new();
    let (started_send, started_receive) = mpsc::sync_channel(0);
    let stopper = std::thread::spawn({
        let dir = dir.clone();
        move || {
            started_receive.recv().expect("fixture player start");
            ToggleSession::request_stop_if_active_in(&dir).unwrap()
        }
    });

    let result = play_fixture_capture_with_player(
        capture,
        &StopWhen::ToggleFile(session),
        RecordingLimit::MAX,
        &meter,
        |pcm, _meter, cancel| {
            let partial_len = (SAMPLE_RATE_HZ as usize / 33).min(pcm.len());
            std::thread::spawn(move || {
                started_send.send(()).expect("fixture start receiver");
                while !cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                partial_len
            })
        },
    )
    .unwrap();

    assert!(stopper.join().expect("stopper thread"));
    assert!(result.duration > Duration::ZERO);
    assert!(result.duration < full_duration);
}

#[test]
fn fixture_obeys_an_already_present_token_scoped_toggle_stop() {
    let dir =
        std::env::temp_dir().join(format!("echo-fixture-present-stop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
    let pcm = Pcm16kMono::from_samples(vec![i16::MAX / 4; SAMPLE_RATE_HZ as usize * 2]);
    let capture = audio::CaptureResult::from_pcm(pcm);
    assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());

    let result = play_fixture_capture(
        capture,
        &StopWhen::ToggleFile(session),
        RecordingLimit::MAX,
        &audio::LevelMeter::new(),
    )
    .unwrap();

    assert_eq!(result.duration, Duration::ZERO);
}

#[test]
fn toggle_starts_stops_and_can_restart() {
    let dir = std::env::temp_dir().join(format!("echo-toggle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);

    let first = match ToggleSession::start_or_stop_in(&dir, None).unwrap() {
        ToggleAction::Start(session) => session,
        ToggleAction::Stop(_) => panic!("first toggle should start"),
    };
    let stop = dir.join(scoped_intent_name("stop", &first.token));
    assert!(dir.join("recording.lock").is_file());
    assert!(matches!(
        ToggleSession::start_or_stop_in(&dir, None).unwrap(),
        ToggleAction::Stop(_)
    ));
    assert!(stop.is_file());

    drop(first);
    assert!(!dir.join("recording.lock").exists());
    assert!(!stop.exists());
    assert!(matches!(
        ToggleSession::start_or_stop_in(&dir, None).unwrap(),
        ToggleAction::Start(_)
    ));
}

#[test]
fn recording_session_serializes_with_toggle_recording() {
    let dir = std::env::temp_dir().join(format!(
        "echo-shared-recording-session-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let session = RecordingSession::acquire_in(&dir).unwrap();

    assert!(ToggleSession::try_start_in(&dir).unwrap().is_none());
    assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
    assert!(session.stop_requested());

    drop(session);
    assert!(ToggleSession::try_start_in(&dir).unwrap().is_some());
}

#[test]
fn cancel_ack_is_withheld_after_injecting_is_published() {
    let session_id = "session-a";
    let transcribing = status::Status {
        state: "Transcribing".to_string(),
        last: None,
        last_history_id: None,
        error: None,
        recording_limit: None,
        session_id: Some(session_id.to_string()),
        revision: 4,
    };
    let injecting = status::Status {
        state: "Injecting".to_string(),
        revision: 5,
        ..transcribing.clone()
    };
    let reads = Cell::new(0);
    let wrote = Cell::new(false);
    let ack = request_control_ack_with(
        session_id,
        ControlIntent::TranscriptionCancel,
        || {
            let n = reads.get();
            reads.set(n + 1);
            if n == 0 {
                transcribing.clone()
            } else {
                injecting.clone()
            }
        },
        |_, _| {
            wrote.set(true);
            Ok(true)
        },
    )
    .unwrap();
    assert!(wrote.get());
    assert!(ack.is_none());
}

#[test]
fn matching_cancel_ack_survives_a_stable_transcribing_phase() {
    let session_id = "session-a";
    let transcribing = status::Status {
        state: "Transcribing".to_string(),
        last: None,
        last_history_id: None,
        error: None,
        recording_limit: None,
        session_id: Some(session_id.to_string()),
        revision: 4,
    };
    let ack = request_control_ack_with(
        session_id,
        ControlIntent::TranscriptionCancel,
        || transcribing.clone(),
        |_, _| Ok(true),
    )
    .unwrap()
    .expect("stable transcribing cancel must ack");
    assert_eq!(ack.session_id, session_id);
    assert_eq!(ack.revision, 5);
}

#[test]
fn release_builds_ignore_audio_fixture_env() {
    let path = std::ffi::OsString::from("/tmp/echo-fixture.wav");
    assert_eq!(
        audio_fixture_path(true, Some(path.clone())),
        Some(PathBuf::from("/tmp/echo-fixture.wav"))
    );
    assert_eq!(audio_fixture_path(false, Some(path)), None);
    assert_eq!(audio_fixture_path(true, None), None);
}

#[test]
fn acknowledged_cancel_prevents_injection() {
    let dir = tempfile::tempdir().unwrap();
    let session = ToggleSession::try_start_in(dir.path()).unwrap().unwrap();
    let token = session.token.clone();
    let status_path = dir.path().join("status");
    let mut published =
        PublishedSession::new_in(StopWhen::ToggleFile(session), status_path.clone());
    published.start_recording(RecordingLimit::DEFAULT).unwrap();
    published.finish_capture().unwrap();
    let (release_send, release_receive) = mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        release_receive.recv().unwrap();
        assert!(published.cancel_requested());
        let injected = Cell::new(false);
        assert_eq!(
            published
                .begin_injecting_then(|| injected.set(true))
                .unwrap(),
            None
        );
        assert!(
            !injected.get(),
            "acknowledged cancellation must prevent injection"
        );
    });
    let ack = request_control_ack_with(
        &token,
        ControlIntent::TranscriptionCancel,
        || status::read_from(&status_path),
        |token, intent| ToggleSession::request_intent_for_token_in(dir.path(), token, intent),
    )
    .unwrap()
    .expect("cancellation must be acknowledged while transcribing");
    assert_eq!(ack.revision, 5);
    release_send.send(()).unwrap();
    worker.join().unwrap();
    let terminal = status::read_from(&status_path);
    assert_eq!(terminal.state, "Failed speech engine failed");
    assert_eq!(terminal.error.as_deref(), Some("Transcription canceled"));
    assert!(terminal.revision > ack.revision);
    assert!(ToggleSession::try_start_in(dir.path()).unwrap().is_some());
}

#[test]
fn cancel_requested_after_injection_commit_is_not_acknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let session = ToggleSession::try_start_in(dir.path()).unwrap().unwrap();
    let token = session.token.clone();
    let status_path = dir.path().join("status");
    let mut published =
        PublishedSession::new_in(StopWhen::ToggleFile(session), status_path.clone());
    published.start_recording(RecordingLimit::DEFAULT).unwrap();
    published.finish_capture().unwrap();
    let result = published
        .begin_injecting_then(|| {
            let ack = request_control_ack_with(
                &token,
                ControlIntent::TranscriptionCancel,
                || status::read_from(&status_path),
                |token, intent| {
                    ToggleSession::request_intent_for_token_in(dir.path(), token, intent)
                },
            )
            .unwrap();
            assert!(ack.is_none());
            "injected"
        })
        .unwrap();
    assert_eq!(result, Some("injected"));
    assert!(!published.cancel_requested());
}

#[test]
fn cancel_write_overlapping_injection_commit_is_not_acknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let session = ToggleSession::try_start_in(dir.path()).unwrap().unwrap();
    let token = session.token.clone();
    let status_path = dir.path().join("status");
    let mut published =
        PublishedSession::new_in(StopWhen::ToggleFile(session), status_path.clone());
    published.start_recording(RecordingLimit::DEFAULT).unwrap();
    published.finish_capture().unwrap();
    let injected = Cell::new(false);
    let ack = request_control_ack_with(
        &token,
        ControlIntent::TranscriptionCancel,
        || status::read_from(&status_path),
        |token, intent| {
            assert_eq!(
                published
                    .begin_injecting_then(|| injected.set(true))
                    .unwrap(),
                Some(())
            );
            ToggleSession::request_intent_for_token_in(dir.path(), token, intent)
        },
    )
    .unwrap();
    assert!(ack.is_none());
    assert!(injected.get());
    assert!(published.cancel_requested());
    published.complete_inject(None, None, None).unwrap();
    assert_eq!(status::read_from(&status_path).state, "Idle");
}

#[test]
fn capture_failure_preserves_category_and_audio_detail() {
    for (error, reason, detail) in [
        (
            audio::AudioError::NoDevice,
            FailReason::NoInputDevice,
            "no input device",
        ),
        (
            audio::AudioError::Stream("selected microphone disconnected".into()),
            FailReason::CaptureFailed,
            "selected microphone disconnected",
        ),
        (
            audio::AudioError::Permission("Microphone access denied".into()),
            FailReason::CaptureFailed,
            "Microphone access denied",
        ),
    ] {
        let failure = CaptureFailure::from_audio(error);
        assert_eq!(failure.reason, reason);
        assert_eq!(failure.detail.as_deref(), Some(detail));
        let body = crate::status::render(
            SessionState::Failed {
                reason: failure.reason,
            },
            None,
            failure.detail.as_deref(),
            None,
        );
        assert!(body.contains(&format!("error={detail}\n")));
    }
}

#[test]
fn missing_capture_fixture_preserves_loading_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.wav");
    let expected = audio::load_wav(&path).unwrap_err().to_string();
    let failure = capture_from(
        Some(path),
        &StopWhen::Timer(None),
        RecordingLimit::DEFAULT,
        &audio::LevelMeter::new(),
    )
    .unwrap_err();
    assert_eq!(failure.reason, FailReason::EngineError);
    assert_eq!(failure.detail.as_deref(), Some(expected.as_str()));
}

#[test]
fn requested_cancellation_uses_one_detail_and_preserves_unrelated_engine_errors() {
    let error = crate::transcribe::TranscriptionError::Engine(echo_core::EngineError::Missing);
    assert_eq!(
        transcription_failure(&error, true, &["History could not be loaded".to_string()]),
        (
            FailReason::EngineError,
            "Transcription canceled".to_string()
        )
    );
    assert_eq!(
        transcription_failure(&error, false, &[]),
        (FailReason::EngineMissing, error.to_string())
    );
    assert_eq!(
        transcription_failure(&error, false, &["History warning".to_string()]),
        (
            FailReason::EngineMissing,
            format!("History warning {error}")
        )
    );
}
