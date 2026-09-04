use super::*;

static NATIVE_RETRY_TEST_LOCK: Mutex<()> = Mutex::new(());

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
fn poisoned_shortcut_state_is_recovered() {
    let state = Arc::new(Mutex::new(NativeShortcutState::Probing));
    let poison = Arc::clone(&state);
    assert!(std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("poison shortcut state");
    })
    .join()
    .is_err());

    assert_eq!(
        *recover_shortcut_lock(&state, "test"),
        NativeShortcutState::Probing
    );
    assert!(!state.is_poisoned());
}

#[test]
fn native_retry_runs_after_delay_unless_cancelled() {
    let _serial = NATIVE_RETRY_TEST_LOCK.lock().unwrap();
    assert!(!shortcut_retry_needed(&NativeShortcutState::Active {
        backend: ShortcutBackend::X11,
        effective: FixedShortcut::DISPLAY.to_string(),
    }));
    assert!(shortcut_retry_needed(&NativeShortcutState::Failed {
        detail: "listener stopped".to_string(),
    }));
    let failed = NativeShortcutState::Failed {
        detail: "cold start failed".to_string(),
    };
    assert!(should_retry_native_listener(false, false, false, &failed));
    assert!(should_retry_native_listener(true, false, false, &failed));
    assert!(!should_retry_native_listener(false, true, false, &failed));
    assert!(!should_retry_native_listener(true, false, true, &failed));
    assert!(!should_retry_native_listener(
        false,
        false,
        false,
        &NativeShortcutState::Unsupported {
            detail: "headless".to_string(),
        },
    ));
    assert!(!should_retry_native_listener(
        false,
        false,
        false,
        &NativeShortcutState::PortalAbsent {
            detail: "portal unavailable".to_string(),
        },
    ));

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
fn stale_native_retry_cannot_act_after_listener_replacement() {
    let _serial = NATIVE_RETRY_TEST_LOCK.lock().unwrap();
    shutdown();

    fn idle_handle(generation: u64) -> NativeShortcutHandle {
        let cancel = echo::audio::CancellationToken::new();
        let thread_cancel = cancel.clone();
        let thread = std::thread::spawn(move || {
            while !thread_cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        NativeShortcutHandle {
            generation,
            cancel,
            thread,
        }
    }

    let stale_generation = NEXT_NATIVE_SHORTCUT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let stale = idle_handle(stale_generation);
    let stale_cancel = stale.cancel.clone();
    *NATIVE_SHORTCUT.lock().unwrap() = Some(stale);

    let (boundary_send, boundary_receive) = std::sync::mpsc::channel();
    let (release_send, release_receive) = std::sync::mpsc::channel();
    let (action_send, action_receive) = std::sync::mpsc::channel();
    let (done_send, done_receive) = std::sync::mpsc::channel();
    schedule_native_retry(stale_cancel, Duration::ZERO, move || {
        boundary_send.send(()).unwrap();
        release_receive.recv().unwrap();
        run_native_retry_if_owned(stale_generation, move || {
            action_send.send(()).unwrap();
        });
        done_send.send(()).unwrap();
    })
    .unwrap();
    boundary_receive
        .recv_timeout(Duration::from_secs(1))
        .expect("stale retry reached the post-cancellation-check boundary");

    let newer_generation = NEXT_NATIVE_SHORTCUT_GENERATION.fetch_add(1, Ordering::Relaxed);
    {
        let _reconcile = NATIVE_RECONCILE.lock().unwrap();
        let stale = NATIVE_SHORTCUT.lock().unwrap().take();
        stop_native_handle(stale);
        *NATIVE_SHORTCUT.lock().unwrap() = Some(idle_handle(newer_generation));
    }

    release_send.send(()).unwrap();
    done_receive
        .recv_timeout(Duration::from_secs(1))
        .expect("stale retry completed its ownership check");
    assert!(action_receive.try_recv().is_err());
    let current = NATIVE_SHORTCUT.lock().unwrap();
    assert!(current.as_ref().is_some_and(|running| {
        running.generation == newer_generation && !running.thread.is_finished()
    }));
    drop(current);
    shutdown();
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
