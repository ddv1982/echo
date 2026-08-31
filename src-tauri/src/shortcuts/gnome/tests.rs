use super::super::*;
use super::*;
use crate::{Health, HEALTH};

fn custom_binding(path: &str, name: &str, command: &str, binding: &str) -> GnomeCustomBinding {
    GnomeCustomBinding {
        path: path.to_string(),
        name: name.to_string(),
        command: command.to_string(),
        binding: binding.to_string(),
    }
}

fn invoke_test_command(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
) -> serde_json::Value {
    tauri::test::get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: command.to_string(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .unwrap_or_else(|error| panic!("{command} IPC failed: {error}"))
    .deserialize()
    .unwrap()
}

#[test]
#[ignore = "manual 20-sample status IPC latency probe"]
fn status_ipc_latency_probe() {
    *HEALTH.lock().expect("health cache lock") = Some((
        Instant::now(),
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
        },
    ));
    let app = tauri::test::mock_builder()
        .invoke_handler(crate::shortcut_test_handler())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let _ = invoke_test_command(&webview, "get_app_status");
    let mut samples = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let _ = invoke_test_command(&webview, "get_app_status");
        samples.push(started.elapsed().as_micros());
    }
    eprintln!("STATUS_IPC_US {samples:?}");
}

#[test]
fn gnome_accelerators_and_commands_are_stable() {
    assert_eq!(FixedShortcut::DISPLAY, "Super+Alt+Space");
    assert_eq!(FixedShortcut::GNOME_ACCELERATOR, "<Super><Alt>space");
    assert_eq!(FixedShortcut::PORTAL_TRIGGER, "LOGO+ALT+space");
    assert_eq!(
        absolute_toggle_command("/usr/bin/echo-desktop").unwrap(),
        "/usr/bin/echo-desktop rec --toggle"
    );
    assert_eq!(
        absolute_toggle_command("/opt/Echo App/echo-desktop").unwrap(),
        "'/opt/Echo App/echo-desktop' rec --toggle"
    );
    assert_eq!(
        absolute_toggle_command(r"/opt/Echo\ App/echo-desktop").unwrap(),
        r"'/opt/Echo\ App/echo-desktop' rec --toggle"
    );
    assert_eq!(
        absolute_toggle_command("/opt/Echo's/echo-desktop").unwrap(),
        "'/opt/Echo'\\''s/echo-desktop' rec --toggle"
    );
    assert!(absolute_toggle_command("echo-desktop").is_err());
    assert_eq!(
        stable_shortcut_executable(
            "/tmp/.mount_echo/usr/bin/echo-desktop",
            Some(std::ffi::OsStr::new("/home/user/Echo.AppImage")),
        ),
        "/home/user/Echo.AppImage"
    );
    assert_eq!(
        stable_shortcut_executable(
            "/usr/bin/echo-desktop",
            Some(std::ffi::OsStr::new("relative.AppImage")),
        ),
        "/usr/bin/echo-desktop"
    );
}

#[test]
fn echo_command_ownership_requires_an_exact_safe_invocation() {
    let desired = "/usr/bin/echo-desktop rec --toggle";
    assert!(echo_toggle_command(desired, desired));
    assert!(echo_toggle_command(
        "/usr/bin/env PATH=/usr/bin ECHO_ENGINE=whisper /home/user/.local/bin/echo-app rec --toggle",
        desired
    ));
    assert!(echo_toggle_command(
        "'/opt/Echo App/echo-desktop' rec --toggle",
        desired
    ));
    assert!(echo_toggle_command(
        "'/opt/Echo'\\''s/echo-app' rec --toggle",
        desired
    ));
    assert!(!echo_toggle_command(
        "/tmp/not-echo-desktop rec --toggle",
        desired
    ));
    assert!(!echo_toggle_command(
        "/usr/bin/echo-desktop rec --toggle; rm -rf /",
        desired
    ));
    assert!(!echo_toggle_command(
        "/usr/bin/env LD_PRELOAD=/tmp/inject.so /usr/bin/echo-app rec --toggle",
        desired
    ));
    assert!(!echo_toggle_command(
        "'/opt/Echo App/echo-desktop rec --toggle",
        desired
    ));
}

#[test]
fn gnome_accelerator_comparison_is_semantic() {
    assert!(gnome_accelerators_match(
        "<Primary><Mod1>space",
        "<Ctrl><Alt>space"
    ));
    assert!(gnome_accelerators_match(
        "<mod4><alt>Return",
        "<Super><Alt>enter"
    ));
    assert!(gnome_accelerators_match(
        "<Alt><Super>space",
        FixedShortcut::GNOME_ACCELERATOR
    ));
    assert!(!gnome_accelerators_match(
        "<Super><Alt>space",
        "<Super><Alt>Return"
    ));
}

#[test]
fn gnome_shortcut_classifies_missing_stale_conflicting_and_ready() {
    let command = "/usr/bin/echo-desktop rec --toggle";
    let binding = "<Super><Alt>space";
    let empty_target = custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", "");
    let missing = GnomeShortcutSnapshot {
        paths: vec![],
        bindings: vec![empty_target.clone()],
    };
    assert_eq!(
        classify_gnome_shortcut(&missing, command, binding).state,
        LegacyShortcutState::Missing
    );

    let stale = GnomeShortcutSnapshot {
        paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
        bindings: vec![custom_binding(
            ECHO_CUSTOM_KEY_PATH,
            ECHO_CUSTOM_KEY_NAME,
            "/home/user/.local/bin/echo-app rec --toggle",
            binding,
        )],
    };
    assert_eq!(
        classify_gnome_shortcut(&stale, command, binding).state,
        LegacyShortcutState::Stale
    );

    let reserved = GnomeShortcutSnapshot {
        paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
        bindings: vec![custom_binding(
            ECHO_CUSTOM_KEY_PATH,
            "Unrelated action",
            "other-command",
            binding,
        )],
    };
    assert_eq!(
        classify_gnome_shortcut(&reserved, command, binding).state,
        LegacyShortcutState::Conflicting
    );
    let commandeered = GnomeShortcutSnapshot {
        paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
        bindings: vec![custom_binding(
            ECHO_CUSTOM_KEY_PATH,
            ECHO_CUSTOM_KEY_NAME,
            "unrelated-command --dangerous",
            binding,
        )],
    };
    assert_eq!(
        classify_gnome_shortcut(&commandeered, command, binding).state,
        LegacyShortcutState::Conflicting
    );

    let other_path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/";
    let collision = GnomeShortcutSnapshot {
        paths: vec![other_path.to_string()],
        bindings: vec![
            empty_target,
            custom_binding(other_path, "Other", "other-command", binding),
        ],
    };
    assert_eq!(
        classify_gnome_shortcut(&collision, command, binding).state,
        LegacyShortcutState::Conflicting
    );
    let semantic_collision = GnomeShortcutSnapshot {
        paths: vec![other_path.to_string()],
        bindings: vec![
            custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", ""),
            custom_binding(other_path, "Other", "other-command", "<Mod4><Mod1>space"),
        ],
    };
    assert_eq!(
        classify_gnome_shortcut(&semantic_collision, command, binding).state,
        LegacyShortcutState::Conflicting
    );

    let ready = GnomeShortcutSnapshot {
        paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
        bindings: vec![custom_binding(
            ECHO_CUSTOM_KEY_PATH,
            ECHO_CUSTOM_KEY_NAME,
            command,
            binding,
        )],
    };
    assert_eq!(
        classify_gnome_shortcut(&ready, command, binding).state,
        LegacyShortcutState::Ready
    );
}

#[test]
fn gnome_repair_is_explicit_idempotent_and_preserves_unrelated_paths() {
    let other_path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/";
    let snapshot = GnomeShortcutSnapshot {
        paths: vec![other_path.to_string()],
        bindings: vec![
            custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", ""),
            custom_binding(other_path, "Terminal", "kgx", "<Ctrl><Alt>t"),
        ],
    };
    let setup = classify_gnome_shortcut(
        &snapshot,
        "/usr/bin/echo-desktop rec --toggle",
        "<Super><Alt>space",
    );
    let writes = gnome_repair_writes(&setup, &snapshot.paths).unwrap();
    assert_eq!(writes.len(), 4);
    assert!(writes.iter().all(|write| {
        write.schema == GNOME_MEDIA_KEYS_SCHEMA || write.schema.ends_with(ECHO_CUSTOM_KEY_PATH)
    }));
    assert!(writes
        .last()
        .expect("path-list write")
        .value
        .contains(other_path));
    let keyfile = dconf_keyfile(&writes).unwrap();
    assert!(keyfile.contains("[/]\ncustom-keybindings="));
    assert!(keyfile.contains("[custom-keybindings/echo]\n"));
    let concurrent_path =
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/concurrent/";
    assert_eq!(
        with_echo_shortcut_path(vec![other_path.to_string(), concurrent_path.to_string()]),
        vec![
            other_path.to_string(),
            concurrent_path.to_string(),
            ECHO_CUSTOM_KEY_PATH.to_string()
        ]
    );
    assert_eq!(
        with_echo_shortcut_path(vec![ECHO_CUSTOM_KEY_PATH.to_string()]),
        vec![ECHO_CUSTOM_KEY_PATH.to_string()]
    );
    let mut changed = snapshot.clone();
    changed.paths.push(concurrent_path.to_string());
    assert!(gnome_repair_transaction(&snapshot, &changed, &setup).is_err());
    let mut commandeered = snapshot.clone();
    commandeered.bindings[0].command = "other-command".to_string();
    assert!(gnome_repair_transaction(&snapshot, &commandeered, &setup).is_err());
    assert_eq!(
        gnome_repair_transaction(&snapshot, &snapshot, &setup).unwrap(),
        writes
    );

    let ready = LegacyShortcutSetup {
        state: LegacyShortcutState::Ready,
        detail: String::new(),
        command: setup.command,
        binding: setup.binding,
    };
    assert!(gnome_repair_writes(&ready, &snapshot.paths)
        .unwrap()
        .is_empty());
    let conflict = LegacyShortcutSetup {
        state: LegacyShortcutState::Conflicting,
        detail: "occupied".to_string(),
        command: ready.command,
        binding: ready.binding,
    };
    assert!(gnome_repair_writes(&conflict, &snapshot.paths).is_err());
}

#[test]
fn gnome_repair_restores_an_empty_active_echo_slot() {
    let command = "/usr/bin/echo-desktop rec --toggle";
    let binding = "<Super><Alt>space";
    let snapshot = GnomeShortcutSnapshot {
        paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
        bindings: vec![custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", "")],
    };
    let setup = classify_gnome_shortcut(&snapshot, command, binding);
    assert_eq!(setup.state, LegacyShortcutState::Stale);

    let writes = gnome_repair_writes(&setup, &snapshot.paths).unwrap();
    assert_eq!(writes.len(), 3);
    assert!(writes.iter().any(|write| {
        write.key == "name" && write.value == gvariant_string(ECHO_CUSTOM_KEY_NAME)
    }));
    let repaired = GnomeShortcutSnapshot {
        paths: snapshot.paths,
        bindings: vec![custom_binding(
            ECHO_CUSTOM_KEY_PATH,
            ECHO_CUSTOM_KEY_NAME,
            command,
            binding,
        )],
    };
    assert_eq!(
        classify_gnome_shortcut(&repaired, command, binding).state,
        LegacyShortcutState::Ready
    );
}

#[test]
#[ignore = "explicitly repairs the current GNOME user's confirmed Echo shortcut"]
fn legacy_wayland_host_repairs_only_the_echo_owned_binding() {
    assert_eq!(
        echo::hotkey::DesktopSession::from_xdg_session_type(
            env::var("XDG_SESSION_TYPE").ok().as_deref()
        ),
        echo::hotkey::DesktopSession::Wayland
    );
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    assert!(desktop.to_ascii_lowercase().contains("gnome"));
    let command = "/usr/bin/echo-desktop rec --toggle";
    assert!(std::path::Path::new("/usr/bin/echo-desktop").is_file());
    let binding = FixedShortcut::GNOME_ACCELERATOR.to_string();
    set_native_shortcut_state(NativeShortcutState::Probing);
    let active = AtomicBool::new(false);
    run_portal_shortcuts(&echo::audio::CancellationToken::new(), &active).unwrap();
    assert!(matches!(
        native_shortcut_state(),
        NativeShortcutState::PortalAbsent { .. }
    ));

    let before = read_gnome_shortcuts().unwrap();
    let occupied = before
        .bindings
        .iter()
        .find(|entry| entry.path == ECHO_CUSTOM_KEY_PATH)
        .expect("Echo reserved shortcut slot");
    assert_eq!(occupied.name, ECHO_CUSTOM_KEY_NAME);
    assert!(echo_toggle_command(&occupied.command, command));
    let unrelated_before = before
        .bindings
        .iter()
        .filter(|entry| before.paths.contains(&entry.path) && entry.path != ECHO_CUSTOM_KEY_PATH)
        .cloned()
        .collect::<Vec<_>>();

    let schema = format!("{GNOME_CUSTOM_KEY_SCHEMA}:{ECHO_CUSTOM_KEY_PATH}");
    apply_gsettings_writes(&[GsettingsWrite {
        schema,
        key: "command",
        value: gvariant_string("/usr/bin/echo-app rec --toggle"),
    }])
    .unwrap();
    *LEGACY_SHORTCUT_CACHE
        .lock()
        .expect("legacy shortcut cache lock") = None;
    let health = Health {
        microphone_ready: false,
        engine_name: String::new(),
        engine_ready: false,
        injection_name: String::new(),
        injection_ready: false,
        current_exe: "/usr/bin/echo-desktop".to_string(),
        first_path_hit: None,
        stale_installs: Vec::new(),
        language_warning: None,
    };
    *HEALTH.lock().expect("health cache lock") = Some((Instant::now(), health));
    let app = tauri::test::mock_builder()
        .invoke_handler(crate::shortcut_test_handler())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let stale = invoke_test_command(&webview, "get_app_status");
    eprintln!(
        "observed GNOME Echo shortcut state: {}",
        stale["shortcut"]["setup"]["state"]
    );
    assert_eq!(stale["shortcut"]["setup"]["state"], "stale");

    let repaired = invoke_test_command(&webview, "repair_legacy_shortcut");
    assert_eq!(repaired["state"], "ready");
    let ready = invoke_test_command(&webview, "get_app_status");
    assert_eq!(ready["shortcut"]["kind"], "gnome-ready");
    assert_eq!(ready["shortcut"]["effective"], FixedShortcut::DISPLAY);
    assert_eq!(
        invoke_test_command(&webview, "repair_legacy_shortcut")["state"],
        "ready"
    );

    let after = read_gnome_shortcuts().unwrap();
    assert_eq!(
        classify_gnome_shortcut(&after, command, &binding).state,
        LegacyShortcutState::Ready
    );
    let unrelated_after = after
        .bindings
        .iter()
        .filter(|entry| after.paths.contains(&entry.path) && entry.path != ECHO_CUSTOM_KEY_PATH)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(unrelated_after, unrelated_before);
}

#[test]
fn only_confirmed_global_shortcuts_absence_enables_legacy_setup() {
    let registry_missing =
        ashpd::Error::PortalNotFound("org.freedesktop.host.portal.Registry".try_into().unwrap());
    let shortcuts_missing =
        ashpd::Error::PortalNotFound("org.freedesktop.portal.GlobalShortcuts".try_into().unwrap());
    assert!(is_legacy_registry_absence(&registry_missing));
    assert!(!is_legacy_registry_absence(&shortcuts_missing));
    assert!(!is_legacy_registry_absence(&ashpd::Error::InvalidAppID));
    assert!(!is_global_shortcuts_absence(&registry_missing));
    assert!(is_global_shortcuts_absence(&shortcuts_missing));
    assert!(!is_global_shortcuts_absence(&ashpd::Error::InvalidAppID));

    let unavailable = NativeShortcutState::Failed {
        detail: "registration failed".to_string(),
    };
    assert!(!needs_legacy_setup(
        &unavailable,
        echo::hotkey::DesktopSession::Wayland
    ));
    let absent = NativeShortcutState::PortalAbsent {
        detail: "portal absent".to_string(),
    };
    assert!(needs_legacy_setup(
        &absent,
        echo::hotkey::DesktopSession::Wayland
    ));
    assert!(!needs_legacy_setup(
        &absent,
        echo::hotkey::DesktopSession::X11
    ));
}
