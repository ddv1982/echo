use super::*;

#[test]
fn fixed_reconcile_is_idempotent() {
    #[derive(Default)]
    struct FakeAdapter {
        running: bool,
        calls: Vec<&'static str>,
    }

    fn reconcile(adapter: &mut FakeAdapter) {
        if adapter.running {
            return;
        }
        adapter.calls.push("register");
        adapter.running = true;
    }

    let mut adapter = FakeAdapter::default();
    reconcile(&mut adapter);
    reconcile(&mut adapter);
    assert_eq!(adapter.calls, ["register"]);
}

#[test]
fn portal_effective_trigger_is_distinct_from_fixed_policy() {
    let state = NativeShortcutState::Active {
        backend: ShortcutBackend::Portal,
        effective: "Alt+F8".to_string(),
    };
    let projected = project_shortcut_status(&state, "/usr/bin/echo-desktop");
    let serialized = serde_json::to_value(&projected).unwrap();
    assert_eq!(serialized["verificationIdentity"], "portal:Alt+F8");
    assert!(serialized.get("verification_identity").is_none());
    assert!(matches!(
        projected,
        ShortcutStatus::Active {
            desired,
            effective,
            verification_identity,
            ..
        } if desired == FixedShortcut::DISPLAY
            && effective == "Alt+F8"
            && verification_identity == "portal:Alt+F8"
    ));
}

#[test]
fn fixed_native_policy_has_one_backend_value_per_surface() {
    let hotkey = FixedShortcut::x11_hotkey();
    assert_eq!(hotkey.key, Code::Space);
    assert!(hotkey.mods.contains(Modifiers::SUPER));
    assert!(hotkey.mods.contains(Modifiers::ALT));
    assert_eq!(FixedShortcut::PORTAL_TRIGGER, "LOGO+ALT+space");
}

#[test]
fn native_retry_runs_after_delay_unless_cancelled() {
    assert!(!shortcut_retry_needed(&NativeShortcutState::Active {
        backend: ShortcutBackend::X11,
        effective: FixedShortcut::DISPLAY.to_string(),
    }));
    assert!(shortcut_retry_needed(&NativeShortcutState::Failed {
        detail: "listener stopped".to_string(),
    }));
    assert!(!should_retry_native_listener(false, false, false));
    assert!(should_retry_native_listener(true, false, false));
    assert!(should_retry_native_listener(false, true, false));
    assert!(!should_retry_native_listener(true, true, true));

    let (send, receive) = std::sync::mpsc::channel();
    schedule_native_retry(
        echo::audio::CancellationToken::new(),
        Duration::from_millis(20),
        move || send.send(()).unwrap(),
    )
    .unwrap();
    receive
        .recv_timeout(Duration::from_secs(1))
        .expect("native retry callback");

    let cancel = echo::audio::CancellationToken::new();
    cancel.cancel();
    let (send, receive) = std::sync::mpsc::channel();
    schedule_native_retry(cancel, Duration::from_millis(20), move || {
        send.send(()).unwrap()
    })
    .unwrap();
    assert!(receive.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
#[ignore = "needs an isolated X11 display"]
fn x11_runtime_registers_and_releases_the_fixed_grab() {
    set_native_shortcut_state(NativeShortcutState::Probing);
    let cancel = echo::audio::CancellationToken::new();
    let listener_cancel = cancel.clone();
    let active = Arc::new(AtomicBool::new(false));
    let listener_active = active.clone();
    let listener =
        std::thread::spawn(move || run_x11_shortcuts(&listener_cancel, &listener_active));

    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(native_shortcut_state(), NativeShortcutState::Active { .. })
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        matches!(native_shortcut_state(), NativeShortcutState::Active { .. }),
        "X11 listener did not become healthy"
    );
    assert!(active.load(Ordering::SeqCst));

    let competing = GlobalHotKeyManager::new().unwrap();
    assert!(
        competing.register(FixedShortcut::x11_hotkey()).is_err(),
        "a competing X11 grab should be rejected"
    );

    cancel.cancel();
    listener.join().unwrap().unwrap();
    let after = GlobalHotKeyManager::new().unwrap();
    let released = FixedShortcut::x11_hotkey();
    let deadline = Instant::now() + Duration::from_secs(1);
    while let Err(err) = after.register(released) {
        assert!(
            Instant::now() < deadline,
            "Echo's toggle grab was not released: {err}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    after.unregister(released).unwrap();
}

#[test]
#[ignore = "needs nested Xephyr, xmessage, xdotool, and ydotool"]
fn x11_runtime_routes_press_and_release_while_another_app_is_focused() {
    struct ChildGuard(std::process::Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let recording_env = ShortcutRecordingTestEnv::start("x11-routing");
    let title = format!("Echo X11 focus probe {}", std::process::id());
    let _other_app = ChildGuard(
        std::process::Command::new("xmessage")
            .args(["-title", &title, "Shortcut focus probe"])
            .spawn()
            .expect("start focus probe"),
    );
    let search = std::process::Command::new("xdotool")
        .args(["search", "--sync", "--name", &title])
        .output()
        .expect("find focus probe");
    assert!(search.status.success());
    let window = String::from_utf8(search.stdout)
        .unwrap()
        .lines()
        .next()
        .expect("focus probe window")
        .to_string();
    assert!(std::process::Command::new("xdotool")
        .args(["windowfocus", &window])
        .status()
        .expect("focus other app")
        .success());
    let focused = std::process::Command::new("xdotool")
        .arg("getwindowfocus")
        .output()
        .expect("read focused window");
    assert_eq!(String::from_utf8(focused.stdout).unwrap().trim(), window);

    set_native_shortcut_state(NativeShortcutState::Probing);
    let cancel = echo::audio::CancellationToken::new();
    let listener_cancel = cancel.clone();
    let (actions, received) = std::sync::mpsc::channel();
    *TEST_SHORTCUT_ACTIONS
        .lock()
        .expect("test shortcut observer lock") = Some(actions);
    let listener = std::thread::spawn(move || {
        let active = AtomicBool::new(false);
        run_x11_shortcuts(&listener_cancel, &active)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(native_shortcut_state(), NativeShortcutState::Active { .. })
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        matches!(native_shortcut_state(), NativeShortcutState::Active { .. }),
        "X11 listener not ready"
    );

    let send_shortcut = |shortcut: &str| {
        assert!(std::process::Command::new("ydotool")
            .args(["key", "--key-delay", "50", shortcut])
            .status()
            .expect("send hardware-level X11 shortcut")
            .success());
    };
    let receive_expected = |expected| {
        let event = received
            .recv_timeout(Duration::from_secs(2))
            .expect("routed X11 event");
        assert_eq!(event, expected);
    };

    send_shortcut("super+alt+space");
    for expected in [
        TestShortcutAction::Edge(
            FixedShortcut::ID.to_string(),
            echo::hotkey::HotkeyEvent::Down,
        ),
        TestShortcutAction::Toggle,
        TestShortcutAction::Edge(FixedShortcut::ID.to_string(), echo::hotkey::HotkeyEvent::Up),
    ] {
        receive_expected(expected);
    }
    recording_env.assert_active();

    send_shortcut("super+alt+space");
    for expected in [
        TestShortcutAction::Edge(
            FixedShortcut::ID.to_string(),
            echo::hotkey::HotkeyEvent::Down,
        ),
        TestShortcutAction::Toggle,
        TestShortcutAction::Edge(FixedShortcut::ID.to_string(), echo::hotkey::HotkeyEvent::Up),
    ] {
        receive_expected(expected);
    }
    recording_env.wait_until_inactive();

    cancel.cancel();
    listener.join().unwrap().unwrap();
    *TEST_SHORTCUT_ACTIONS
        .lock()
        .expect("test shortcut observer lock") = None;
}
