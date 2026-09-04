use echo_core::Config;

pub fn preflight_paths() -> Result<(), String> {
    echo_core::try_data_dir()?;
    echo_core::try_config_dir()?;
    crate::stt::ModelCache::try_from_env()?;
    Ok(())
}

pub fn runtime_config() -> Result<Config, String> {
    #[cfg(test)]
    {
        Config::load()
    }
    #[cfg(not(test))]
    {
        let mut guard = snapshot().lock().expect("config snapshot lock");
        if guard.is_none() {
            *guard = Some(Config::load());
        }
        guard.as_ref().expect("config snapshot").clone()
    }
}

#[must_use]
pub fn config_for_display() -> (Config, Option<String>) {
    match runtime_config() {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error)),
    }
}

pub fn reload() {
    #[cfg(not(test))]
    {
        *snapshot().lock().expect("config snapshot lock") = Some(Config::load());
    }
}

#[cfg(not(test))]
fn snapshot() -> &'static std::sync::Mutex<Option<Result<Config, String>>> {
    static FILE: std::sync::Mutex<Option<Result<Config, String>>> = std::sync::Mutex::new(None);
    &FILE
}
