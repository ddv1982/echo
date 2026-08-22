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
        "Name={{name}}",
        "Categories={{categories}}",
        "Comment={{comment}}",
        "Type=Application",
    ] {
        assert!(template.contains(field), "template missing {field}");
    }
}

#[test]
fn deb_and_rpm_bundles_use_the_template() {
    let config = include_str!("../tauri.conf.json");
    assert_eq!(
        config.matches("\"desktopTemplate\": \"templates/Echo.desktop\"").count(),
        2,
        "deb and rpm both reference templates/Echo.desktop"
    );
}
