use echo_core::Config;

#[must_use]
pub fn file_config() -> Config {
    #[cfg(test)]
    {
        Config::load().unwrap_or_default()
    }
    #[cfg(not(test))]
    {
        let mut guard = snapshot().lock().expect("config snapshot lock");
        if guard.is_none() {
            *guard = Some(Config::load().unwrap_or_default());
        }
        guard.as_ref().expect("config snapshot").clone()
    }
}

/// Replace the in-process snapshot so later `file_config()` calls see disk.
pub fn reload() {
    #[cfg(not(test))]
    {
        *snapshot().lock().expect("config snapshot lock") =
            Some(Config::load().unwrap_or_default());
    }
}

#[cfg(not(test))]
fn snapshot() -> &'static std::sync::Mutex<Option<Config>> {
    static FILE: std::sync::Mutex<Option<Config>> = std::sync::Mutex::new(None);
    &FILE
}
