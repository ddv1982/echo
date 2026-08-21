use std::io::Write;
use std::process::{Command, Stdio};

use echo_core::{Cleanup, CleanupError, Dictionary, Rewrite};

pub struct LocalCleanup {
    pub bin: String,
}

impl Cleanup for LocalCleanup {
    fn apply(&self, raw: &str, dict: &Dictionary) -> Result<Rewrite, CleanupError> {
        let mut child = Command::new(&self.bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| CleanupError::Local(err.to_string()))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(raw.as_bytes())
                .map_err(|err| CleanupError::Local(err.to_string()))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|err| CleanupError::Local(err.to_string()))?;
        if !out.status.success() {
            return Err(CleanupError::Local(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        let cleaned = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(dict.rewrite(&cleaned))
    }
}
