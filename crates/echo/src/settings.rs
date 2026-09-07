use std::sync::{Mutex, MutexGuard};

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
        {
            let guard = recover_lock(snapshot(), "config snapshot");
            if let Some(cached) = guard.as_ref() {
                return cached.clone();
            }
        }
        let loaded = Config::load();
        let mut guard = recover_lock(snapshot(), "config snapshot");
        guard.get_or_insert(loaded).clone()
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
        let loaded = Config::load();
        *recover_lock(snapshot(), "config snapshot") = Some(loaded);
    }
}

fn recover_lock<'a, T>(mutex: &'a Mutex<T>, what: &str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("settings: recovering poisoned {what}");
            mutex.clear_poison();
            poisoned.into_inner()
        }
    }
}

#[cfg(not(test))]
fn snapshot() -> &'static Mutex<Option<Result<Config, String>>> {
    static FILE: Mutex<Option<Result<Config, String>>> = Mutex::new(None);
    &FILE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_lock_returns_inner_after_poison() {
        let mutex = Mutex::new(7);
        let _ = std::panic::catch_unwind(|| {
            let _guard = mutex.lock().expect("lock");
            panic!("poison config snapshot");
        });
        assert_eq!(*recover_lock(&mutex, "config snapshot"), 7);
    }
}
