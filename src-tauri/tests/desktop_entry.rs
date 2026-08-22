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
