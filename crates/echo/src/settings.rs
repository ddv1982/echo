use echo_core::Config;

#[must_use]
pub fn file_config() -> Config {
    #[cfg(test)]
    {
        Config::load().unwrap_or_default()
    }
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static FILE: OnceLock<Config> = OnceLock::new();
        FILE.get_or_init(|| Config::load().unwrap_or_default())
            .clone()
    }
}
