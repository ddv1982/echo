use std::env;

#[tauri::command]
pub(crate) fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub(crate) async fn remove_stale_installs() -> Result<Vec<String>, String> {
    crate::blocking::run_blocking("stale installation removal", || {
        let current = std::env::current_exe()
            .ok()
            .and_then(|path| path.canonicalize().ok())
            .ok_or("cannot resolve the running executable")?;
        let path_var = env::var("PATH").unwrap_or_default();
        let home = env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let report = echo::upgrade::remove_stale_installs(&current, &path_var, &home);
        crate::status::health_invalidate();
        if report.remaining.is_empty() {
            Ok(report
                .removed
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect())
        } else {
            let removed = report
                .removed
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let remaining = report
                .remaining
                .iter()
                .map(|(path, err)| format!("{}: {err}", path.display()))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!("removed {removed}; still present: {remaining}"))
        }
    })
    .await?
}
