use std::fs;

use echo_core::{status_path, SessionState};

pub fn write_status(state: SessionState, last: Option<&str>) -> Result<(), String> {
    let path = status_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let name = match state {
        SessionState::Idle => "Idle",
        SessionState::Recording { .. } => "Recording",
        SessionState::Transcribing => "Transcribing",
        SessionState::Cleaning => "Cleaning",
        SessionState::Injecting => "Injecting",
        SessionState::Failed { reason } => {
            return write(&format!("Failed {}", reason.as_str()), last)
        }
    };
    write(name, last)
}

fn write(state: &str, last: Option<&str>) -> Result<(), String> {
    let mut body = format!("state={state}\n");
    if let Some(text) = last {
        body.push_str("last=");
        body.push_str(text);
        body.push('\n');
    }
    fs::write(status_path(), body).map_err(|err| err.to_string())
}

pub fn read_status() -> Result<String, String> {
    fs::read_to_string(status_path()).map_err(|err| err.to_string())
}
