#![allow(clippy::ptr_arg, clippy::too_many_arguments)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ashpd::zbus;
use ashpd::zbus::message::Header;
use ashpd::zbus::object_server::{ObjectServer, SignalEmitter};
use ashpd::zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Str, Value};

use super::*;

type Vardict = HashMap<String, OwnedValue>;
type WireShortcut = (String, Vardict);

struct TestBus {
    child: Child,
    address: String,
}

impl TestBus {
    fn spawn() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("start private session bus");
        let address = BufReader::new(child.stdout.take().expect("private bus stdout"))
            .lines()
            .next()
            .expect("private bus address line")
            .expect("read private bus address");
        Self { child, address }
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Default)]
struct Calls {
    stage: Arc<AtomicU8>,
    bound: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    session_path: Arc<Mutex<Option<OwnedObjectPath>>>,
}

struct Registry(Calls);

#[zbus::interface(name = "org.freedesktop.host.portal.Registry", crate = "ashpd::zbus")]
impl Registry {
    async fn register(&self, app_id: &str, _options: Vardict) -> zbus::fdo::Result<()> {
        if app_id != APP_ID {
            return Err(failed(format!("unexpected app ID {app_id}")));
        }
        self.0.stage.store(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Request;

#[zbus::interface(name = "org.freedesktop.portal.Request", crate = "ashpd::zbus")]
impl Request {
    async fn close(&self) {}

    #[zbus(signal)]
    async fn response(
        emitter: &SignalEmitter<'_>,
        response: u32,
        results: &Vardict,
    ) -> zbus::Result<()>;
}

struct Session(Calls);

#[zbus::interface(name = "org.freedesktop.portal.Session", crate = "ashpd::zbus")]
impl Session {
    async fn close(&self) {
        self.0.closed.store(true, Ordering::SeqCst);
    }

    #[zbus(signal)]
    async fn closed(emitter: &SignalEmitter<'_>, details: &Vardict) -> zbus::Result<()>;
}

struct Shortcuts(Calls);

#[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts", crate = "ashpd::zbus")]
impl Shortcuts {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }

    async fn create_session(
        &self,
        options: Vardict,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = sender_component(&header)?;
        let request = portal_path("request", &sender, option_string(&options, "handle_token")?)?;
        let session = portal_path(
            "session",
            &sender,
            option_string(&options, "session_handle_token")?,
        )?;
        server.at(request.clone(), Request).await.map_err(failed)?;
        server
            .at(session.clone(), Session(self.0.clone()))
            .await
            .map_err(failed)?;
        *self.0.session_path.lock().expect("session path lock") = Some(session.clone());

        let mut results = Vardict::new();
        results.insert("session_handle".into(), session.as_ref().into());
        let emitter = SignalEmitter::new(connection, request.clone()).map_err(failed)?;
        Request::response(&emitter, 0, &results)
            .await
            .map_err(failed)?;
        Ok(request)
    }

    async fn bind_shortcuts(
        &self,
        session_handle: OwnedObjectPath,
        shortcuts: Vec<WireShortcut>,
        _parent_window: &str,
        options: Vardict,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        if self.0.stage.load(Ordering::SeqCst) != 1 {
            return Err(failed("shortcuts bound before Registry.Register"));
        }
        if Some(&session_handle)
            != self
                .0
                .session_path
                .lock()
                .expect("session path lock")
                .as_ref()
        {
            return Err(failed("unexpected session path"));
        }
        if shortcuts.len() != 2 {
            return Err(failed("expected two shortcuts"));
        }

        let request = portal_path(
            "request",
            &sender_component(&header)?,
            option_string(&options, "handle_token")?,
        )?;
        server.at(request.clone(), Request).await.map_err(failed)?;
        let output = vec![
            wire_shortcut(TOGGLE_SHORTCUT_ID, "Start or stop recording", "Ctrl+Alt+T"),
            wire_shortcut(HOLD_SHORTCUT_ID, "Hold to record", "Control_R"),
        ];
        let mut results = Vardict::new();
        results.insert(
            "shortcuts".into(),
            Value::new(output).try_to_owned().map_err(failed)?,
        );
        let emitter = SignalEmitter::new(connection, request.clone()).map_err(failed)?;
        Request::response(&emitter, 0, &results)
            .await
            .map_err(failed)?;
        self.0.stage.store(2, Ordering::SeqCst);
        self.0.bound.store(true, Ordering::SeqCst);
        Ok(request)
    }

    #[zbus(signal)]
    async fn activated(
        emitter: &SignalEmitter<'_>,
        session_handle: &ObjectPath<'_>,
        shortcut_id: &str,
        timestamp: u64,
        options: &Vardict,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn deactivated(
        emitter: &SignalEmitter<'_>,
        session_handle: &ObjectPath<'_>,
        shortcut_id: &str,
        timestamp: u64,
        options: &Vardict,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn shortcuts_changed(
        emitter: &SignalEmitter<'_>,
        session_handle: &ObjectPath<'_>,
        shortcuts: &Vec<WireShortcut>,
    ) -> zbus::Result<()>;
}

fn failed(error: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

fn option_string(options: &Vardict, key: &str) -> zbus::fdo::Result<String> {
    options
        .get(key)
        .ok_or_else(|| failed(format!("missing {key}")))?
        .downcast_ref::<&str>()
        .map(str::to_owned)
        .map_err(failed)
}

fn sender_component(header: &Header<'_>) -> zbus::fdo::Result<String> {
    Ok(header
        .sender()
        .ok_or_else(|| failed("missing sender"))?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_"))
}

fn portal_path(kind: &str, sender: &str, token: String) -> zbus::fdo::Result<OwnedObjectPath> {
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/{kind}/{sender}/{token}"
    ))
    .map_err(failed)
}

fn wire_shortcut(id: &str, description: &str, trigger: &str) -> WireShortcut {
    let mut info = Vardict::new();
    info.insert("description".into(), Str::from(description).into());
    info.insert("trigger_description".into(), Str::from(trigger).into());
    (id.to_string(), info)
}

async fn receive_action(
    receiver: &std::sync::mpsc::Receiver<TestShortcutAction>,
) -> TestShortcutAction {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match receiver.try_recv() {
            Ok(edge) => return edge,
            Err(std::sync::mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(err) => panic!("timed out waiting for portal edge: {err}"),
        }
    }
}

#[test]
#[ignore = "needs dbus-daemon for a private portal bus"]
fn portal_runtime_registers_binds_routes_and_closes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("portal test runtime");
    runtime.block_on(async {
        let recording_env = ShortcutRecordingTestEnv::start("portal");
        let bus = TestBus::spawn();
        let calls = Calls::default();
        let service = zbus::connection::Builder::address(bus.address.as_str())
            .expect("private bus address")
            .name("org.freedesktop.portal.Desktop")
            .expect("portal bus name")
            .serve_at("/org/freedesktop/portal/desktop", Registry(calls.clone()))
            .expect("serve Registry")
            .serve_at("/org/freedesktop/portal/desktop", Shortcuts(calls.clone()))
            .expect("serve GlobalShortcuts")
            .build()
            .await
            .expect("build mock portal");
        let client = zbus::connection::Builder::address(bus.address.as_str())
            .expect("private bus address")
            .build()
            .await
            .expect("connect mock portal client");

        update_native_status(|status| *status = NativeShortcutStatus::default());
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        *TEST_SHORTCUT_ACTIONS
            .lock()
            .expect("test shortcut observer lock") = Some(action_sender);
        let cancel = echo::audio::CancellationToken::new();
        let listener_cancel = cancel.clone();
        let listener =
            run_portal_shortcuts_async("Super+Alt+Space", "RightCtrl", &listener_cancel, client);
        let exercise = async {
            let deadline = Instant::now() + Duration::from_secs(3);
            while (!calls.bound.load(Ordering::SeqCst) || !native_shortcut_status().healthy)
                && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(calls.stage.load(Ordering::SeqCst), 2);
            assert!(native_shortcut_status().healthy);
            assert_eq!(
                native_shortcut_status().effective_toggle.as_deref(),
                Some("Ctrl+Alt+T")
            );

            let session = calls
                .session_path
                .lock()
                .expect("session path lock")
                .clone()
                .expect("portal session path");
            let emitter = SignalEmitter::new(&service, "/org/freedesktop/portal/desktop")
                .expect("portal signal emitter");
            let empty = Vardict::new();
            Shortcuts::activated(&emitter, &session.as_ref(), TOGGLE_SHORTCUT_ID, 1, &empty)
                .await
                .expect("emit activation");
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Edge(
                    TOGGLE_SHORTCUT_ID.to_string(),
                    echo::hotkey::HotkeyEvent::Down
                )
            );
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Toggle
            );
            recording_env.assert_active();
            Shortcuts::deactivated(&emitter, &session.as_ref(), TOGGLE_SHORTCUT_ID, 2, &empty)
                .await
                .expect("emit deactivation");
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Edge(
                    TOGGLE_SHORTCUT_ID.to_string(),
                    echo::hotkey::HotkeyEvent::Up
                )
            );
            Shortcuts::activated(&emitter, &session.as_ref(), TOGGLE_SHORTCUT_ID, 3, &empty)
                .await
                .expect("emit second activation");
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Edge(
                    TOGGLE_SHORTCUT_ID.to_string(),
                    echo::hotkey::HotkeyEvent::Down
                )
            );
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Toggle
            );
            recording_env.wait_until_inactive();
            Shortcuts::activated(&emitter, &session.as_ref(), HOLD_SHORTCUT_ID, 4, &empty)
                .await
                .expect("emit hold activation");
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Edge(
                    HOLD_SHORTCUT_ID.to_string(),
                    echo::hotkey::HotkeyEvent::Down
                )
            );
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::HoldStart
            );
            recording_env.assert_active();
            Shortcuts::deactivated(&emitter, &session.as_ref(), HOLD_SHORTCUT_ID, 5, &empty)
                .await
                .expect("emit hold deactivation");
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Edge(
                    HOLD_SHORTCUT_ID.to_string(),
                    echo::hotkey::HotkeyEvent::Up
                )
            );
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::HoldStop
            );
            recording_env.wait_until_inactive();

            let replacement = vec![
                wire_shortcut(TOGGLE_SHORTCUT_ID, "Start or stop recording", "Alt+F8"),
                wire_shortcut(HOLD_SHORTCUT_ID, "Hold to record", "Control_R"),
            ];
            Shortcuts::shortcuts_changed(&emitter, &session.as_ref(), &replacement)
                .await
                .expect("emit shortcut change");
            let deadline = Instant::now() + Duration::from_secs(3);
            while native_shortcut_status().effective_toggle.as_deref() != Some("Alt+F8")
                && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(
                native_shortcut_status().effective_toggle.as_deref(),
                Some("Alt+F8")
            );
            Shortcuts::activated(&emitter, &session.as_ref(), HOLD_SHORTCUT_ID, 6, &empty)
                .await
                .expect("emit held activation before teardown");
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::Edge(
                    HOLD_SHORTCUT_ID.to_string(),
                    echo::hotkey::HotkeyEvent::Down
                )
            );
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::HoldStart
            );
            recording_env.assert_active();
            cancel.cancel();
            assert_eq!(
                receive_action(&action_receiver).await,
                TestShortcutAction::HoldStop
            );
        };

        let (result, ()) = tokio::join!(listener, exercise);
        result.expect("portal listener result");
        recording_env.wait_until_inactive();
        assert!(calls.closed.load(Ordering::SeqCst));
        *TEST_SHORTCUT_ACTIONS
            .lock()
            .expect("test shortcut observer lock") = None;
        drop(service);
        drop(bus);
    });
}
