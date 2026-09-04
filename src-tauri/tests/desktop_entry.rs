//! The packaged desktop entry must launch the packaged binary by absolute
//! path. A PATH-based Exec lets a stale source build in ~/.local/bin shadow
//! every upgrade, which is the incident this hardening exists to kill.

#[test]
fn packaged_desktop_entry_uses_the_absolute_path() {
    let template = include_str!("../templates/Echo.desktop");
    assert!(template.contains("Exec=/usr/bin/{{exec}}"));
    for field in [
        "StartupWMClass={{exec}}",
        "Icon={{icon}}",
        "Name=Echo",
        "Categories={{categories}}",
        "Comment={{comment}}",
        "Type=Application",
    ] {
        assert!(template.contains(field), "template missing {field}");
    }
}

#[test]
fn appimage_desktop_entry_uses_the_bundled_binary() {
    let template = include_str!("../templates/Echo.AppImage.desktop");
    assert!(template.contains("Exec={{exec}}"));
    assert!(!template.contains("/usr/bin"));

    let config = include_str!("../tauri.appimage.conf.json");
    assert_eq!(
        config
            .matches("\"desktopTemplate\": \"templates/Echo.AppImage.desktop\"")
            .count(),
        2,
        "the AppImage override must replace both Linux package templates"
    );
}

#[test]
fn packaged_desktop_basename_matches_the_portal_app_id() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid Tauri config");
    let desktop_runtime = include_str!("../src/main.rs");

    assert_eq!(config["productName"], config["identifier"]);
    assert_eq!(config["identifier"], "io.github.ddv1982.echo");
    assert!(desktop_runtime.contains("const APP_ID: &str = \"io.github.ddv1982.echo\";"));
}

#[test]
fn tauri_frontend_hooks_have_an_explicit_working_directory() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid Tauri config");

    let build = &config["build"];
    assert_eq!(build["beforeDevCommand"]["script"], "npm run dev");
    assert_eq!(build["beforeDevCommand"]["cwd"], "../frontend");
    assert_eq!(build["beforeBuildCommand"]["script"], "npm run build");
    assert_eq!(build["beforeBuildCommand"]["cwd"], "../frontend");
}

#[test]
fn deb_and_rpm_bundles_use_the_template() {
    let config = include_str!("../tauri.conf.json");
    assert_eq!(
        config
            .matches("\"desktopTemplate\": \"templates/Echo.desktop\"")
            .count(),
        2,
        "deb and rpm both reference templates/Echo.desktop"
    );
}

#[test]
fn tray_menu_is_retained_in_managed_state() {
    let desktop_runtime = include_str!("../src/main.rs");
    let tray_runtime = include_str!("../src/tray.rs");

    assert!(desktop_runtime.contains("app.manage(tray_menu);"));
    assert!(tray_runtime.contains("pub(crate) struct TrayMenu"));
    assert!(tray_runtime.contains("_menu: Menu<Wry>"));
}

#[test]
fn settings_changes_publish_an_ordered_tray_snapshot() {
    let settings_command = include_str!("../src/commands/settings.rs");
    let setup_runtime = include_str!("../src/setup.rs");
    let tray_runtime = include_str!("../src/tray.rs");

    assert!(settings_command.contains("crate::settings::snapshot_with_revision"));
    let request = settings_command
        .find("let tray_request = crate::tray::request();")
        .expect("Settings reserves a tray update before detached work");
    let detached_work = settings_command
        .find("crate::blocking::run_blocking(\"settings change\"")
        .expect("Settings change uses detached work");
    assert!(request < detached_work);
    assert!(
        settings_command.contains("crate::tray::sync(&app, tray_request, revision, &snapshot);")
    );
    let setup_completion = setup_runtime
        .find("let mut active = lock_active_operation(&worker_state);")
        .expect("setup completion clears its active operation");
    let setup_completion = &setup_runtime[setup_completion..];
    let setup_request = setup_completion
        .find("let tray_request = crate::tray::request();")
        .expect("setup reserves its completion refresh");
    let setup_unlock = setup_completion
        .find("drop(active);")
        .expect("setup releases its active-operation lock");
    assert!(setup_request < setup_unlock);
    assert!(setup_completion.contains("crate::tray::refresh_requested(&app, tray_request);"));
    assert!(tray_runtime.contains("app.run_on_main_thread(move ||"));
    assert!(tray_runtime.contains("LanguageWriteQueue"));
}

#[test]
fn tray_language_changes_notify_the_settings_ui() {
    let tray_runtime = include_str!("../src/tray.rs");
    let settings_controller = include_str!("../../frontend/src/settings/useSettingsController.ts");

    assert!(tray_runtime.contains("app.emit(\"settings-event\", ())"));
    assert!(settings_controller.contains("onSettingsEvent"));
}

#[cfg(feature = "status-perf-probe")]
#[test]
#[ignore = "needs a live Linux session bus and StatusNotifierWatcher"]
fn tray_menu_stays_exported_after_setup() {
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    struct TempRoot(PathBuf);
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let root = TempRoot(std::env::temp_dir().join(format!(
        "echo-tray-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    std::fs::create_dir_all(&root.0).unwrap();
    let config_dir = root.0.join("config");
    let data_dir = root.0.join("data");
    let log_path = root.0.join("echo-desktop.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env_remove("ECHO_ENGINE")
        .env_remove("ECHO_LANGUAGE")
        .env_remove("ECHO_WHISPER_MODEL")
        .env("ECHO_TRAY_TEST_SETTINGS_LANGUAGE", "fr")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .expect("start the feature-isolated desktop process");
    let pid = child.id();
    let mut child = ChildGuard(child);
    let path = format!("/org/ayatana/NotificationItem/tray_icon_tray_app_{pid}_1");
    let deadline = Instant::now() + Duration::from_secs(10);
    let service = loop {
        assert_eq!(
            child.0.try_wait().unwrap(),
            None,
            "desktop exited before registering its tray"
        );
        let output = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.kde.StatusNotifierWatcher",
                "--object-path",
                "/StatusNotifierWatcher",
                "--method",
                "org.freedesktop.DBus.Properties.Get",
                "org.kde.StatusNotifierWatcher",
                "RegisteredStatusNotifierItems",
            ])
            .output()
            .expect("query StatusNotifierWatcher");
        assert!(output.status.success());
        let registered = String::from_utf8(output.stdout).unwrap();
        let marker = format!("@{path}");
        if let Some(index) = registered.find(&marker) {
            let service = registered[..index]
                .rsplit('\'')
                .next()
                .expect("registered item includes a service name");
            break service.to_string();
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            panic!("tray did not register: {registered}\n{log}");
        }
        thread::sleep(Duration::from_millis(100));
    };

    let menu_path = format!("{path}/Menu");
    let layout = || {
        let output = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                &service,
                "--object-path",
                &menu_path,
                "--method",
                "com.canonical.dbusmenu.GetLayout",
                "--",
                "0",
                "-1",
                "[]",
            ])
            .output()
            .expect("query exported tray menu");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };
    let assert_menu = |layout: &str| {
        for label in ["Open Echo", "Start / stop recording", "Language", "Quit"] {
            assert!(layout.contains(label), "menu is missing {label}: {layout}");
        }
    };

    let initial_layout = layout();
    assert_menu(&initial_layout);
    let deadline = Instant::now() + Duration::from_secs(30);
    let ready_layout = loop {
        let current = layout();
        if current
            .contains("'toggle-state': <1>, 'toggle-type': <'checkmark'>, 'label': <'French'>")
        {
            break current;
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            panic!("tray language state did not load: {current}\n{log}");
        }
        thread::sleep(Duration::from_millis(100));
    };
    let config_path = config_dir.join("config.json");
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(config["language"], "fr");
    let language_marker = "'label': <'Language'>";
    let language_label_index = ready_layout
        .find(language_marker)
        .expect("Language submenu");
    let language_id_start = ready_layout[..language_label_index]
        .rfind("<(")
        .expect("Language submenu tuple")
        + 2;
    let language_id = ready_layout[language_id_start..]
        .split_once(',')
        .expect("Language submenu id")
        .0;
    let about_to_show = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            &service,
            "--object-path",
            &menu_path,
            "--method",
            "com.canonical.dbusmenu.AboutToShow",
            language_id,
        ])
        .output()
        .expect("open the exported Language submenu");
    assert!(about_to_show.status.success());
    let marker = "'label': <'German'>";
    let label_index = ready_layout
        .find(marker)
        .unwrap_or_else(|| panic!("German tray item missing: {ready_layout}"));
    let id_start = ready_layout[..label_index]
        .rfind("<(")
        .expect("German item tuple")
        + 2;
    let german_id = ready_layout[id_start..]
        .split_once(',')
        .expect("German item id")
        .0;
    let event = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            &service,
            "--object-path",
            &menu_path,
            "--method",
            "com.canonical.dbusmenu.Event",
            german_id,
            "clicked",
            "<int32 0>",
            "0",
        ])
        .output()
        .expect("select German through the exported tray menu");
    assert!(
        event.status.success(),
        "{}",
        String::from_utf8_lossy(&event.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(raw) = std::fs::read(&config_path) {
            let config: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            if config["language"] == "de" {
                break;
            }
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            panic!("tray selection was not persisted\n{log}");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let german_checked = "'toggle-state': <1>, 'toggle-type': <'checkmark'>, 'label': <'German'>";
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let updated_layout = layout();
        assert_menu(&updated_layout);
        if updated_layout.contains(german_checked) {
            break;
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            panic!("tray did not apply its post-write Settings snapshot: {updated_layout}\n{log}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}
