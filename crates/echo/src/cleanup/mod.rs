mod local;

pub use local::LocalCleanup;

use echo_core::{Cleanup, CleanupMode, Dictionary, OffCleanup, Rewrite, RulesCleanup};

pub fn from_mode(mode: CleanupMode) -> Box<dyn Cleanup> {
    match mode {
        CleanupMode::Off => Box::new(OffCleanup),
        CleanupMode::Rules => Box::new(RulesCleanup),
        CleanupMode::LocalModel { model } => Box::new(LocalCleanup { bin: model }),
    }
}

pub fn from_env() -> Box<dyn Cleanup> {
    let raw = std::env::var("ECHO_CLEANUP").unwrap_or_else(|_| "rules".to_string());
    match CleanupMode::parse(&raw) {
        Ok(mode) => from_mode(mode),
        Err(err) => {
            eprintln!("cleanup: {err}; using rules");
            Box::new(RulesCleanup)
        }
    }
}

pub fn apply(raw: &str, dict: &Dictionary) -> Rewrite {
    from_env()
        .apply(raw, dict)
        .unwrap_or_else(|_| dict.rewrite(raw))
}
