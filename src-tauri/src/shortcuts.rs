use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, GlobalShortcuts, NewShortcut, Shortcut,
};
use ashpd::desktop::CreateSessionOptions;
use echo_desktop::ipc::{
    LegacyShortcutSetup, LegacyShortcutState, ShortcutBackend, ShortcutStatus,
};
use futures_util::StreamExt;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::APP_ID;

mod gnome;

struct FixedShortcut;

impl FixedShortcut {
    const ID: &'static str = "toggle-recording";
    const DISPLAY: &'static str = "Super+Alt+Space";
    const PORTAL_TRIGGER: &'static str = "LOGO+ALT+space";
    const GNOME_ACCELERATOR: &'static str = "<Super><Alt>space";

    fn x11_hotkey() -> HotKey {
        HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::Space)
    }
}

fn project_shortcut_status(native: &NativeShortcutState, current_exe: &str) -> ShortcutStatus {
    let desired = FixedShortcut::DISPLAY.to_string();
    let activation = echo::status::shortcut_activation();
    match native {
        NativeShortcutState::Probing => ShortcutStatus::Probing { desired },
        NativeShortcutState::Active { backend, effective } => ShortcutStatus::Active {
            desired,
            effective: effective.clone(),
            backend: *backend,
            activation,
            verification_identity: format!("{}:{effective}", backend.as_str()),
        },
        NativeShortcutState::PortalAbsent { detail } => {
            let Some(setup) = gnome::legacy_shortcut_setup(native, current_exe) else {
                return ShortcutStatus::Unsupported {
                    desired,
                    detail: detail.clone(),
                };
            };
            let is_gnome = env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .split(':')
                .any(|part| matches!(part.to_ascii_lowercase().as_str(), "gnome" | "zorin"));
            if is_gnome {
                if setup.state == LegacyShortcutState::Ready {
                    ShortcutStatus::GnomeReady {
                        desired: desired.clone(),
                        effective: desired,
                        detail: setup.detail,
                        verification_identity: format!("gnome:{}:{}", setup.binding, setup.command),
                        command: setup.command,
                        binding: setup.binding,
                        activation,
                    }
                } else {
                    ShortcutStatus::GnomeSetup {
                        desired,
                        setup: setup
                            .as_gnome_setup()
                            .expect("ready GNOME shortcut handled above"),
                    }
                }
            } else {
                if setup.command.is_empty() || setup.binding.is_empty() {
                    return ShortcutStatus::Unsupported {
                        desired,
                        detail: setup.detail,
                    };
                }
                ShortcutStatus::Manual {
                    desired,
                    command: setup.command,
                    detail: setup.detail,
                }
            }
        }
        NativeShortcutState::Failed { detail } => ShortcutStatus::Failed {
            desired,
            detail: detail.clone(),
        },
        NativeShortcutState::Unsupported { detail } => ShortcutStatus::Unsupported {
            desired,
            detail: detail.clone(),
        },
    }
}

pub(crate) fn status(current_exe: &str) -> ShortcutStatus {
    project_shortcut_status(&native_shortcut_state(), current_exe)
}

pub(crate) fn repair(current_exe: &str) -> Result<LegacyShortcutSetup, String> {
    gnome::repair(&native_shortcut_state(), current_exe)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NativeShortcutState {
    Probing,
    Active {
        backend: ShortcutBackend,
        effective: String,
    },
    PortalAbsent {
        detail: String,
    },
    Failed {
        detail: String,
    },
    Unsupported {
        detail: String,
    },
}

static NATIVE_SHORTCUT_STATE: OnceLock<Arc<Mutex<NativeShortcutState>>> = OnceLock::new();

fn recover_shortcut_lock<'a, T>(state: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("native shortcuts: recovering poisoned {name} state");
            state.clear_poison();
            poisoned.into_inner()
        }
    }
}

fn native_state_cell() -> &'static Arc<Mutex<NativeShortcutState>> {
    NATIVE_SHORTCUT_STATE.get_or_init(|| Arc::new(Mutex::new(NativeShortcutState::Probing)))
}

fn native_shortcut_state() -> NativeShortcutState {
    recover_shortcut_lock(native_state_cell(), "status").clone()
}

fn set_native_shortcut_state(state: NativeShortcutState) {
    *recover_shortcut_lock(native_state_cell(), "status") = state;
}

fn is_legacy_registry_absence(error: &ashpd::Error) -> bool {
    matches!(
        error,
        ashpd::Error::PortalNotFound(interface)
            if interface.as_str() == "org.freedesktop.host.portal.Registry"
    )
}

fn is_global_shortcuts_absence(error: &ashpd::Error) -> bool {
    matches!(
        error,
        ashpd::Error::PortalNotFound(interface)
            if interface.as_str() == "org.freedesktop.portal.GlobalShortcuts"
    )
}

struct NativeShortcutHandle {
    generation: u64,
    cancel: echo::audio::CancellationToken,
    thread: JoinHandle<()>,
}

static NATIVE_SHORTCUT: Mutex<Option<NativeShortcutHandle>> = Mutex::new(None);
static NEXT_NATIVE_SHORTCUT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum TestShortcutAction {
    Edge(String, echo::hotkey::HotkeyEvent),
    Toggle,
}

#[cfg(test)]
static TEST_SHORTCUT_ACTIONS: Mutex<Option<std::sync::mpsc::Sender<TestShortcutAction>>> =
    Mutex::new(None);

#[cfg(test)]
struct ShortcutRecordingTestEnv {
    dir: std::path::PathBuf,
    old: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

#[cfg(test)]
impl ShortcutRecordingTestEnv {
    fn start(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("echo-shortcut-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/echo/tests/fixtures/claude_code.wav")
            .into_os_string();
        let values = [
            ("ECHO_DATA_DIR", dir.clone().into_os_string()),
            ("ECHO_CONFIG_DIR", dir.join("config").into_os_string()),
            ("ECHO_MODEL_DIR", dir.join("models").into_os_string()),
            ("ECHO_AUDIO_FIXTURE", fixture),
            ("ECHO_ENGINE", "fake".into()),
            ("ECHO_SKIP_INJECT", "1".into()),
            ("ECHO_HUD", "0".into()),
        ];
        let old = values
            .into_iter()
            .map(|(key, value)| {
                let old = std::env::var_os(key);
                std::env::set_var(key, value);
                (key, old)
            })
            .collect();
        Self { dir, old }
    }

    fn assert_active(&self) {
        assert!(
            echo::rec::session_active(),
            "recording lock was not acquired"
        );
    }

    fn wait_until_inactive(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while echo::rec::session_active() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !echo::rec::session_active(),
            "recording lock was not released"
        );
    }
}

#[cfg(test)]
impl Drop for ShortcutRecordingTestEnv {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while echo::rec::session_active() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        for (key, old) in self.old.drain(..) {
            if let Some(old) = old {
                std::env::set_var(key, old);
            } else {
                std::env::remove_var(key);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
fn report_test_shortcut(action: TestShortcutAction) -> bool {
    let observer = TEST_SHORTCUT_ACTIONS
        .lock()
        .expect("test shortcut observer lock");
    if let Some(observer) = observer.as_ref() {
        let _ = observer.send(action);
        true
    } else {
        false
    }
}
static NATIVE_RECONCILE: Mutex<()> = Mutex::new(());

/// Reconcile serializes teardown and startup. The old worker is cancelled and
/// joined (which closes a portal session or unregisters X11 grabs) before the
/// replacement can register, while the runtime/status mutexes stay unlocked.
pub(crate) fn reconcile() {
    reconcile_native_shortcuts_with_recovery(false, false);
}

fn retry_native_shortcuts_after_failure(generation: u64) {
    run_native_retry_if_owned(generation, || reconcile_native_shortcuts_locked(true, true));
}

pub(crate) fn retry() -> ShortcutStatus {
    if shortcut_retry_needed(&native_shortcut_state()) {
        reconcile_native_shortcuts_with_recovery(false, true);
    }
    status(&crate::status::current_exe_string())
}

fn shortcut_retry_needed(state: &NativeShortcutState) -> bool {
    !matches!(state, NativeShortcutState::Active { .. })
}

fn reconcile_native_shortcuts_with_recovery(recovering: bool, force: bool) {
    let _reconcile = NATIVE_RECONCILE
        .lock()
        .expect("native shortcut reconcile lock");
    reconcile_native_shortcuts_locked(recovering, force);
}

fn reconcile_native_shortcuts_locked(recovering: bool, force: bool) {
    let old = {
        let mut guard = NATIVE_SHORTCUT.lock().expect("native shortcut lock");
        if !force
            && guard
                .as_ref()
                .is_some_and(|running| !running.thread.is_finished())
        {
            return;
        }
        guard.take()
    };
    stop_native_handle(old);

    let session = echo::hotkey::DesktopSession::from_xdg_session_type(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    set_native_shortcut_state(NativeShortcutState::Probing);

    if session == echo::hotkey::DesktopSession::Unknown {
        set_native_shortcut_state(NativeShortcutState::Unsupported {
            detail: "unknown or headless desktop session".to_string(),
        });
        return;
    }

    let cancel = echo::audio::CancellationToken::new();
    let thread_cancel = cancel.clone();
    let generation = NEXT_NATIVE_SHORTCUT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name(
            match session {
                echo::hotkey::DesktopSession::Wayland => "echo-shortcuts-portal",
                _ => "echo-shortcuts-x11",
            }
            .to_string(),
        )
        .spawn(move || {
            let active = AtomicBool::new(false);
            let result = panic::catch_unwind(AssertUnwindSafe(|| match session {
                echo::hotkey::DesktopSession::Wayland => {
                    run_portal_shortcuts(&thread_cancel, &active)
                }
                echo::hotkey::DesktopSession::X11 => run_x11_shortcuts(&thread_cancel, &active),
                echo::hotkey::DesktopSession::Unknown => Ok(()),
            }));
            let failure = match result {
                Ok(Ok(())) => None,
                Ok(Err(err)) => Some(err),
                Err(_) => Some("native shortcut listener panicked".to_string()),
            };
            let active = active.load(Ordering::SeqCst);
            if let Some(error) = failure {
                if thread_cancel.is_cancelled() {
                    eprintln!("native shortcuts: listener teardown failed: {error}");
                    return;
                }
                mark_native_failure(error);
                if should_retry_native_listener(
                    active,
                    recovering,
                    thread_cancel.is_cancelled(),
                    &native_shortcut_state(),
                ) {
                    if let Err(err) =
                        schedule_native_retry(thread_cancel, Duration::from_secs(1), move || {
                            retry_native_shortcuts_after_failure(generation)
                        })
                    {
                        eprintln!("native shortcuts: {err}");
                    }
                }
            } else if should_retry_native_listener(
                active,
                recovering,
                thread_cancel.is_cancelled(),
                &native_shortcut_state(),
            ) {
                if let Err(err) =
                    schedule_native_retry(thread_cancel, Duration::from_secs(1), move || {
                        retry_native_shortcuts_after_failure(generation)
                    })
                {
                    eprintln!("native shortcuts: {err}");
                }
            }
        });
    match spawned {
        Ok(thread) => {
            *NATIVE_SHORTCUT.lock().expect("native shortcut lock") = Some(NativeShortcutHandle {
                generation,
                cancel,
                thread,
            });
        }
        Err(err) => mark_native_failure(format!("cannot spawn native shortcut listener: {err}")),
    }
}

fn stop_native_handle(handle: Option<NativeShortcutHandle>) {
    if let Some(handle) = handle {
        handle.cancel.cancel();
        if handle.thread.join().is_err() {
            mark_native_failure("native shortcut listener panicked during teardown".to_string());
        }
    }
}

pub(crate) fn shutdown() {
    let _reconcile = NATIVE_RECONCILE
        .lock()
        .expect("native shortcut reconcile lock");
    let old = NATIVE_SHORTCUT.lock().expect("native shortcut lock").take();
    stop_native_handle(old);
}

fn mark_native_failure(error: String) {
    eprintln!("native shortcuts: {error}");
    set_native_shortcut_state(NativeShortcutState::Failed { detail: error });
}

fn schedule_native_retry(
    cancel: echo::audio::CancellationToken,
    delay: Duration,
    retry: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("echo-shortcuts-retry".to_string())
        .spawn(move || {
            let deadline = Instant::now() + delay;
            while !cancel.is_cancelled() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    if !cancel.is_cancelled() {
                        retry();
                    }
                    break;
                }
                std::thread::sleep(remaining.min(Duration::from_millis(50)));
            }
        })
        .map(|_| ())
        .map_err(|err| format!("cannot schedule native shortcut retry: {err}"))
}

fn run_native_retry_if_owned(generation: u64, retry: impl FnOnce()) {
    let _reconcile = NATIVE_RECONCILE
        .lock()
        .expect("native shortcut reconcile lock");
    let owned = NATIVE_SHORTCUT
        .lock()
        .expect("native shortcut lock")
        .as_ref()
        .is_some_and(|running| running.generation == generation && !running.cancel.is_cancelled());
    if owned {
        retry();
    }
}

fn should_retry_native_listener(
    active: bool,
    recovering: bool,
    cancelled: bool,
    state: &NativeShortcutState,
) -> bool {
    !cancelled
        && !recovering
        && (active
            || matches!(
                state,
                NativeShortcutState::Probing | NativeShortcutState::Failed { .. }
            ))
}

fn dispatch_shortcut_edge(
    id: &str,
    edge: echo::hotkey::HotkeyEvent,
    toggle: &mut echo::hotkey::ToggleDriver,
) {
    #[cfg(test)]
    report_test_shortcut(TestShortcutAction::Edge(id.to_string(), edge));
    match id {
        FixedShortcut::ID if toggle.on_edge(edge) => {
            match crate::commands::start_recording_thread() {
                Ok(recording_token) => {
                    if let Err(err) = echo::status::mark_shortcut_activation(
                        "native-toggle",
                        recording_token.as_deref(),
                    ) {
                        eprintln!("toggle shortcut: cannot record provenance: {err}");
                    }
                    #[cfg(test)]
                    report_test_shortcut(TestShortcutAction::Toggle);
                }
                Err(err) => eprintln!("toggle shortcut: cannot change recording: {err}"),
            }
        }
        _ => {}
    }
}

fn run_x11_shortcuts(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
) -> Result<(), String> {
    let mut toggle = echo::hotkey::ToggleDriver::new();
    let result = run_x11_event_loop(cancel, active, |id, edge| {
        dispatch_shortcut_edge(id, edge, &mut toggle);
    });
    toggle.terminate();
    result
}

fn run_x11_event_loop(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
    mut on_edge: impl FnMut(&'static str, echo::hotkey::HotkeyEvent),
) -> Result<(), String> {
    let decision = echo::hotkey::select_native_backend(echo::hotkey::DesktopSession::X11, None);
    debug_assert_eq!(decision.backend, echo::hotkey::NativeBackend::X11);
    let toggle_key = FixedShortcut::x11_hotkey();

    let manager = GlobalHotKeyManager::new()
        .map_err(|err| format!("cannot create X11 global-hotkey manager: {err}"))?;
    while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
    manager
        .register(toggle_key)
        .map_err(|err| format!("X11 toggle shortcut conflict: {err}"))?;
    set_native_shortcut_state(NativeShortcutState::Active {
        backend: ShortcutBackend::X11,
        effective: FixedShortcut::DISPLAY.to_string(),
    });
    active.store(true, Ordering::SeqCst);

    while !cancel.is_cancelled() {
        match GlobalHotKeyEvent::receiver().recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                if event.id != toggle_key.id() {
                    continue;
                }
                let edge = match event.state {
                    HotKeyState::Pressed => echo::hotkey::HotkeyEvent::Down,
                    HotKeyState::Released => echo::hotkey::HotkeyEvent::Up,
                };
                on_edge(FixedShortcut::ID, edge);
            }
            Err(err) if err.is_timeout() => {}
            Err(err) => {
                let _ = manager.unregister(toggle_key);
                return Err(format!("X11 shortcut listener terminated: {err}"));
            }
        }
    }
    manager
        .unregister(toggle_key)
        .map_err(|err| format!("cannot unregister X11 shortcut: {err}"))
}

fn run_portal_shortcuts(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("cannot start portal runtime: {err}"))?;
    runtime.block_on(async {
        let connection = ashpd::zbus::Connection::session()
            .await
            .map_err(|err| format!("cannot connect to the session bus: {err}"))?;
        run_portal_shortcuts_async(cancel, active, connection).await
    })
}

async fn run_portal_shortcuts_async(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
    connection: ashpd::zbus::Connection,
) -> Result<(), String> {
    let app_id = APP_ID
        .parse::<ashpd::AppID>()
        .map_err(|err| format!("invalid portal application id: {err}"))?;
    if let Err(err) = ashpd::register_host_app_with_connection(connection.clone(), app_id).await {
        if is_legacy_registry_absence(&err) {
            eprintln!("native shortcuts: host portal Registry is unavailable; probing legacy GlobalShortcuts support");
        } else {
            return Err(format!("portal host registry handshake failed: {err}"));
        }
    }

    // The Registry attempt above intentionally precedes every portal proxy,
    // session and bind operation. New stacks attribute permissions to APP_ID;
    // legacy stacks without Registry can still expose GlobalShortcuts.
    let portal = match GlobalShortcuts::with_connection(connection).await {
        Ok(portal) => portal,
        Err(err) => {
            let detail = format!("Wayland GlobalShortcuts interface is unavailable: {err}");
            if is_global_shortcuts_absence(&err) {
                set_native_shortcut_state(NativeShortcutState::PortalAbsent { detail });
                return Ok(());
            }
            return Err(detail);
        }
    };
    let decision = echo::hotkey::select_native_backend(
        echo::hotkey::DesktopSession::Wayland,
        Some(portal.version()),
    );
    if decision.backend != echo::hotkey::NativeBackend::Portal {
        set_native_shortcut_state(NativeShortcutState::Unsupported {
            detail: decision
                .reason
                .unwrap_or_else(|| "Wayland GlobalShortcuts interface is unavailable".to_string()),
        });
        return Ok(());
    }
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(|err| format!("cannot create GlobalShortcuts session: {err}"))?;
    let session_path = serde_json::to_value(&session)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or("cannot read GlobalShortcuts session path");
    let session_path = match session_path {
        Ok(path) => path,
        Err(err) => return Err(close_portal_after_failure(&session, err.to_string()).await),
    };
    let mut activated = Box::pin(match portal.receive_activated().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal activations: {err}"),
            )
            .await)
        }
    });
    let mut deactivated = Box::pin(match portal.receive_deactivated().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal deactivations: {err}"),
            )
            .await)
        }
    });
    let mut changed = Box::pin(match portal.receive_shortcuts_changed().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal shortcut changes: {err}"),
            )
            .await)
        }
    });
    let mut closed = Box::pin(match session.receive_closed().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal session closure: {err}"),
            )
            .await)
        }
    });

    let shortcuts = [
        NewShortcut::new(FixedShortcut::ID, "Start or stop recording")
            .preferred_trigger(FixedShortcut::PORTAL_TRIGGER),
    ];
    let request = tokio::select! {
        result = portal.bind_shortcuts(
            &session,
            &shortcuts,
            None,
            BindShortcutsOptions::default(),
        ) => match result {
            Ok(request) => request,
            Err(err) => return Err(close_portal_after_failure(
                &session,
                format!("cannot bind portal shortcuts: {err}"),
            ).await),
        },
        () = wait_for_native_cancel(cancel) => {
            session.close().await
                .map_err(|err| format!("cannot close cancelled portal shortcut session: {err}"))?;
            return Ok(());
        }
    };
    let response = match request.response() {
        Ok(response) => response,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("portal shortcut registration was rejected: {err}"),
            )
            .await)
        }
    };
    let effective = match effective_portal_shortcut(response.shortcuts()) {
        Ok(effective) => effective,
        Err(err) => return Err(close_portal_after_failure(&session, err).await),
    };
    set_native_shortcut_state(NativeShortcutState::Active {
        backend: ShortcutBackend::Portal,
        effective,
    });
    active.store(true, Ordering::SeqCst);

    let mut toggle = echo::hotkey::ToggleDriver::new();
    let listener_error = loop {
        if cancel.is_cancelled() {
            break None;
        }
        tokio::select! {
            event = activated.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    dispatch_shortcut_edge(
                        event.shortcut_id(),
                        echo::hotkey::HotkeyEvent::Down,
                        &mut toggle,
                    );
                }
                Some(_) => {}
                None => break Some("portal Activated listener terminated".to_string()),
            },
            event = deactivated.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    dispatch_shortcut_edge(
                        event.shortcut_id(),
                        echo::hotkey::HotkeyEvent::Up,
                        &mut toggle,
                    );
                }
                Some(_) => {}
                None => break Some("portal Deactivated listener terminated".to_string()),
            },
            event = changed.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    match effective_portal_shortcut(event.shortcuts()) {
                        Ok(effective) => set_native_shortcut_state(NativeShortcutState::Active {
                            backend: ShortcutBackend::Portal,
                            effective,
                        }),
                        Err(err) => break Some(format!("invalid ShortcutsChanged signal: {err}")),
                    }
                }
                Some(_) => {}
                None => break Some("portal ShortcutsChanged listener terminated".to_string()),
            },
            event = closed.next() => match event {
                Some(_) => break Some("portal shortcut session terminated".to_string()),
                None => break Some("portal session listener terminated".to_string()),
            },
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    };
    toggle.terminate();
    let close_result = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
    match close_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(format!("cannot close portal shortcut session: {err}")),
        Err(_) => return Err("timed out closing portal shortcut session".to_string()),
    }
    if let Some(error) = listener_error {
        return Err(error);
    }
    Ok(())
}

async fn wait_for_native_cancel(cancel: &echo::audio::CancellationToken) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn close_portal_after_failure<T>(
    session: &ashpd::desktop::Session<T>,
    primary: String,
) -> String
where
    T: ashpd::desktop::SessionPortal,
{
    match tokio::time::timeout(Duration::from_secs(2), session.close()).await {
        Ok(Ok(())) => primary,
        Ok(Err(err)) => format!("{primary}; portal session cleanup failed: {err}"),
        Err(_) => format!("{primary}; portal session cleanup timed out"),
    }
}

fn effective_portal_shortcut(shortcuts: &[Shortcut]) -> Result<String, String> {
    shortcuts
        .iter()
        .find(|shortcut| shortcut.id() == FixedShortcut::ID)
        .map(Shortcut::trigger_description)
        .filter(|trigger| !trigger.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "portal did not assign an effective trigger for {}",
                FixedShortcut::ID
            )
        })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod portal_runtime_tests;
