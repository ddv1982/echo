use std::collections::BTreeMap;
use std::env;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use echo_desktop::ipc::{LegacyShortcutSetup, LegacyShortcutState};

use super::{FixedShortcut, NativeShortcutState};

const GNOME_MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GNOME_CUSTOM_KEY_SCHEMA: &str =
    "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const ECHO_CUSTOM_KEY_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/echo/";
const ECHO_CUSTOM_KEY_NAME: &str = "Echo Dictation";

#[derive(Debug, Clone, PartialEq, Eq)]
struct GnomeCustomBinding {
    path: String,
    name: String,
    command: String,
    binding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GnomeShortcutSnapshot {
    paths: Vec<String>,
    bindings: Vec<GnomeCustomBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GsettingsWrite {
    schema: String,
    key: &'static str,
    value: String,
}

static LEGACY_SHORTCUT_CACHE: Mutex<Option<(Instant, String, String, LegacyShortcutSetup)>> =
    Mutex::new(None);

pub(super) fn legacy_shortcut_setup(
    native: &NativeShortcutState,
    current_exe: &str,
) -> Option<LegacyShortcutSetup> {
    let session = echo::hotkey::DesktopSession::from_xdg_session_type(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    if !needs_legacy_setup(native, session) {
        return None;
    }

    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let executable = stable_shortcut_executable(current_exe, env::var_os("APPIMAGE").as_deref());
    let command = absolute_toggle_command(&executable).unwrap_or_default();
    let binding = FixedShortcut::GNOME_ACCELERATOR.to_string();
    if !desktop
        .split(':')
        .any(|part| matches!(part.to_ascii_lowercase().as_str(), "gnome" | "zorin"))
    {
        return Some(LegacyShortcutSetup {
            state: LegacyShortcutState::Unsupported,
            detail: "This Wayland compositor has no GlobalShortcuts portal; add the command in its keyboard settings."
                .to_string(),
            command,
            binding,
        });
    }
    if command.is_empty() || binding.is_empty() {
        return Some(LegacyShortcutSetup {
            state: LegacyShortcutState::Unsupported,
            detail: "Echo could not derive a stable executable command for GNOME settings."
                .to_string(),
            command,
            binding,
        });
    }

    const TTL: Duration = Duration::from_secs(2);
    let mut cache = LEGACY_SHORTCUT_CACHE
        .lock()
        .expect("legacy shortcut cache lock");
    if let Some((at, cached_command, cached_binding, status)) = cache.as_ref() {
        if at.elapsed() < TTL && cached_command == &command && cached_binding == &binding {
            return Some(status.clone());
        }
    }
    let setup = match read_gnome_shortcuts() {
        Ok(snapshot) => classify_gnome_shortcut(&snapshot, &command, &binding),
        Err(err) => LegacyShortcutSetup {
            state: LegacyShortcutState::Unsupported,
            detail: format!("Cannot inspect GNOME custom shortcuts: {err}"),
            command: command.clone(),
            binding: binding.clone(),
        },
    };
    *cache = Some((Instant::now(), command, binding, setup.clone()));
    Some(setup)
}

fn needs_legacy_setup(native: &NativeShortcutState, session: echo::hotkey::DesktopSession) -> bool {
    matches!(native, NativeShortcutState::PortalAbsent { .. })
        && session == echo::hotkey::DesktopSession::Wayland
}

fn absolute_toggle_command(current_exe: &str) -> Result<String, String> {
    let path = std::path::Path::new(current_exe);
    if !path.is_absolute() {
        return Err("the running executable path is not absolute".to_string());
    }
    let raw = path.to_string_lossy();
    let quoted = if raw
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
    {
        raw.into_owned()
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    };
    Ok(format!("{quoted} rec --toggle"))
}

fn stable_shortcut_executable(current_exe: &str, appimage: Option<&std::ffi::OsStr>) -> String {
    appimage
        .map(std::path::Path::new)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| std::path::Path::new(current_exe))
        .to_string_lossy()
        .into_owned()
}

fn classify_gnome_shortcut(
    snapshot: &GnomeShortcutSnapshot,
    command: &str,
    binding: &str,
) -> LegacyShortcutSetup {
    let target = snapshot
        .bindings
        .iter()
        .find(|entry| entry.path == ECHO_CUSTOM_KEY_PATH);
    let collision = snapshot.bindings.iter().find(|entry| {
        entry.path != ECHO_CUSTOM_KEY_PATH
            && snapshot.paths.contains(&entry.path)
            && gnome_accelerators_match(&entry.binding, binding)
    });
    let setup = |state, detail: String| LegacyShortcutSetup {
        state,
        detail,
        command: command.to_string(),
        binding: binding.to_string(),
    };

    if let Some(entry) = target {
        let occupied =
            !entry.name.is_empty() || !entry.command.is_empty() || !entry.binding.is_empty();
        let echo_command = entry.command.is_empty() || echo_toggle_command(&entry.command, command);
        if occupied && (entry.name != ECHO_CUSTOM_KEY_NAME || !echo_command) {
            return setup(
                LegacyShortcutState::Conflicting,
                "The reserved Echo shortcut slot belongs to another custom shortcut; Echo will not overwrite it."
                    .to_string(),
            );
        }
    }
    if let Some(entry) = collision {
        return setup(
            LegacyShortcutState::Conflicting,
            format!(
                "{} already uses {}; change that shortcut in GNOME Settings first.",
                if entry.name.is_empty() {
                    "Another custom shortcut"
                } else {
                    &entry.name
                },
                binding
            ),
        );
    }
    if !snapshot
        .paths
        .iter()
        .any(|path| path == ECHO_CUSTOM_KEY_PATH)
    {
        return setup(
            LegacyShortcutState::Missing,
            "GNOME has no Echo custom shortcut yet. Close GNOME Settings before setup.".to_string(),
        );
    }
    let Some(entry) = target else {
        return setup(
            LegacyShortcutState::Missing,
            "GNOME has no Echo custom shortcut yet. Close GNOME Settings before setup.".to_string(),
        );
    };
    if entry.name == ECHO_CUSTOM_KEY_NAME
        && entry.command == command
        && gnome_accelerators_match(&entry.binding, binding)
    {
        setup(
            LegacyShortcutState::Ready,
            "GNOME owns this Echo shortcut and its command is current.".to_string(),
        )
    } else {
        setup(
            LegacyShortcutState::Stale,
            "The Echo-owned GNOME shortcut uses an old command or key binding. Close GNOME Settings before repair."
                .to_string(),
        )
    }
}

fn echo_toggle_command(command: &str, desired: &str) -> bool {
    if command == desired {
        return true;
    }
    let Some(parts) = safe_shell_words(command) else {
        return false;
    };
    let mut index = 0;
    if parts.first().map(String::as_str) == Some("/usr/bin/env") {
        index = 1;
        while let Some(part) = parts.get(index) {
            let Some((name, _)) = part.split_once('=') else {
                break;
            };
            if name != "PATH"
                && !(name.starts_with("ECHO_")
                    && name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    }))
            {
                return false;
            }
            index += 1;
        }
    }
    let Some(executable) = parts.get(index) else {
        return false;
    };
    let executable = std::path::Path::new(executable);
    executable.is_absolute()
        && matches!(
            executable.file_name().and_then(|name| name.to_str()),
            Some("echo-desktop" | "echo-app")
        )
        && parts.get(index + 1).map(String::as_str) == Some("rec")
        && parts.get(index + 2).map(String::as_str) == Some("--toggle")
        && parts.len() == index + 3
}

fn safe_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut started = false;
    for character in command.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            started = true;
        } else if quoted {
            if character == '\'' {
                quoted = false;
            } else {
                word.push(character);
            }
        } else if character == '\'' {
            quoted = true;
            started = true;
        } else if character == '\\' {
            escaped = true;
            started = true;
        } else if character.is_ascii_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else if matches!(
            character,
            ';' | '|' | '&' | '$' | '`' | '(' | ')' | '<' | '>' | '"'
        ) || character.is_control()
        {
            return None;
        } else {
            word.push(character);
            started = true;
        }
    }
    if quoted || escaped {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

fn gnome_accelerators_match(left: &str, right: &str) -> bool {
    canonical_gnome_accelerator(left)
        .zip(canonical_gnome_accelerator(right))
        .is_some_and(|(left, right)| left == right)
}

fn canonical_gnome_accelerator(raw: &str) -> Option<String> {
    let mut rest = raw.trim();
    let mut modifiers = [false; 4];
    while let Some(after_open) = rest.strip_prefix('<') {
        let close = after_open.find('>')?;
        let index = match after_open[..close].to_ascii_lowercase().as_str() {
            "super" | "mod4" => 0,
            "ctrl" | "control" | "primary" => 1,
            "alt" | "mod1" => 2,
            "shift" => 3,
            _ => return None,
        };
        if modifiers[index] {
            return None;
        }
        modifiers[index] = true;
        rest = &after_open[close + 1..];
    }
    let terminal = match rest.to_ascii_lowercase().as_str() {
        "enter" | "return" => "Enter".to_string(),
        "up" => "ArrowUp".to_string(),
        "down" => "ArrowDown".to_string(),
        "left" => "ArrowLeft".to_string(),
        "right" => "ArrowRight".to_string(),
        key => key.to_string(),
    };
    if terminal.is_empty() {
        return None;
    }
    let names = ["Super", "Ctrl", "Alt", "Shift"];
    let mut parts = modifiers
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .map(|(index, _)| names[index].to_string())
        .collect::<Vec<_>>();
    parts.push(terminal);
    Some(parts.join("+"))
}

fn read_gnome_shortcuts() -> Result<GnomeShortcutSnapshot, String> {
    let paths = parse_gsettings_strings(&gsettings_get(
        GNOME_MEDIA_KEYS_SCHEMA,
        "custom-keybindings",
    )?);
    let mut inspected = paths.clone();
    if !inspected.iter().any(|path| path == ECHO_CUSTOM_KEY_PATH) {
        inspected.push(ECHO_CUSTOM_KEY_PATH.to_string());
    }
    let mut bindings = Vec::with_capacity(inspected.len());
    for path in inspected {
        let schema = format!("{GNOME_CUSTOM_KEY_SCHEMA}:{path}");
        bindings.push(GnomeCustomBinding {
            path,
            name: gsettings_string(&gsettings_get(&schema, "name")?)?,
            command: gsettings_string(&gsettings_get(&schema, "command")?)?,
            binding: gsettings_string(&gsettings_get(&schema, "binding")?)?,
        });
    }
    Ok(GnomeShortcutSnapshot { paths, bindings })
}

fn gsettings_get(schema: &str, key: &str) -> Result<String, String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_gsettings_strings(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in raw.chars() {
        if !quoted {
            if character == '\'' {
                quoted = true;
                current.clear();
            }
            continue;
        }
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '\'' {
            quoted = false;
            values.push(current.clone());
        } else {
            current.push(character);
        }
    }
    values
}

fn gsettings_string(raw: &str) -> Result<String, String> {
    parse_gsettings_strings(raw)
        .into_iter()
        .next()
        .ok_or_else(|| format!("invalid gsettings string: {raw}"))
}

fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn gvariant_strv(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| gvariant_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn gnome_repair_writes(
    setup: &LegacyShortcutSetup,
    current_paths: &[String],
) -> Result<Vec<GsettingsWrite>, String> {
    let schema = format!("{GNOME_CUSTOM_KEY_SCHEMA}:{ECHO_CUSTOM_KEY_PATH}");
    match setup.state {
        LegacyShortcutState::Ready => Ok(Vec::new()),
        LegacyShortcutState::Missing => Ok(vec![
            GsettingsWrite {
                schema: schema.clone(),
                key: "name",
                value: gvariant_string(ECHO_CUSTOM_KEY_NAME),
            },
            GsettingsWrite {
                schema: schema.clone(),
                key: "command",
                value: gvariant_string(&setup.command),
            },
            GsettingsWrite {
                schema,
                key: "binding",
                value: gvariant_string(&setup.binding),
            },
            GsettingsWrite {
                schema: GNOME_MEDIA_KEYS_SCHEMA.to_string(),
                key: "custom-keybindings",
                value: gvariant_strv(&with_echo_shortcut_path(current_paths.to_vec())),
            },
        ]),
        LegacyShortcutState::Stale => Ok(vec![
            GsettingsWrite {
                schema: schema.clone(),
                key: "name",
                value: gvariant_string(ECHO_CUSTOM_KEY_NAME),
            },
            GsettingsWrite {
                schema: schema.clone(),
                key: "command",
                value: gvariant_string(&setup.command),
            },
            GsettingsWrite {
                schema,
                key: "binding",
                value: gvariant_string(&setup.binding),
            },
        ]),
        LegacyShortcutState::Conflicting | LegacyShortcutState::Unsupported => {
            Err(setup.detail.clone())
        }
    }
}

fn gnome_repair_transaction(
    expected: &GnomeShortcutSnapshot,
    current: &GnomeShortcutSnapshot,
    setup: &LegacyShortcutSetup,
) -> Result<Vec<GsettingsWrite>, String> {
    if current != expected {
        return Err(
            "GNOME shortcuts changed while Echo was preparing the repair; review them and try again."
                .to_string(),
        );
    }
    gnome_repair_writes(setup, &current.paths)
}

fn dconf_keyfile(writes: &[GsettingsWrite]) -> Result<String, String> {
    let mut groups = BTreeMap::<&str, Vec<&GsettingsWrite>>::new();
    for write in writes {
        let group = if write.schema == GNOME_MEDIA_KEYS_SCHEMA {
            "/"
        } else if write.schema == format!("{GNOME_CUSTOM_KEY_SCHEMA}:{ECHO_CUSTOM_KEY_PATH}") {
            "custom-keybindings/echo"
        } else {
            return Err(format!("refusing unexpected dconf schema {}", write.schema));
        };
        groups.entry(group).or_default().push(write);
    }
    let mut keyfile = String::new();
    for (group, writes) in groups {
        keyfile.push_str(&format!("[{group}]\n"));
        for write in writes {
            keyfile.push_str(&format!("{}={}\n", write.key, write.value));
        }
        keyfile.push('\n');
    }
    Ok(keyfile)
}

fn apply_dconf_transaction(writes: &[GsettingsWrite]) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if writes.is_empty() {
        return Ok(());
    }
    let mut child = Command::new("dconf")
        .args(["load", "/org/gnome/settings-daemon/plugins/media-keys/"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot start atomic dconf repair: {err}"))?;
    child
        .stdin
        .take()
        .ok_or("cannot open dconf repair input")?
        .write_all(dconf_keyfile(writes)?.as_bytes())
        .map_err(|err| format!("cannot write dconf repair input: {err}"))?;
    let output = child
        .wait_with_output()
        .map_err(|err| format!("cannot finish atomic dconf repair: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn with_echo_shortcut_path(mut paths: Vec<String>) -> Vec<String> {
    if !paths.iter().any(|path| path == ECHO_CUSTOM_KEY_PATH) {
        paths.push(ECHO_CUSTOM_KEY_PATH.to_string());
    }
    paths
}

#[cfg(test)]
fn apply_gsettings_writes(writes: &[GsettingsWrite]) -> Result<(), String> {
    for write in writes {
        let output = std::process::Command::new("gsettings")
            .args(["set", &write.schema, write.key, &write.value])
            .output()
            .map_err(|err| err.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    }
    Ok(())
}

pub(super) fn repair(
    native: &NativeShortcutState,
    current_exe: &str,
) -> Result<LegacyShortcutSetup, String> {
    let advertised = legacy_shortcut_setup(native, current_exe)
        .ok_or("this session does not need a legacy compositor shortcut")?;
    if advertised.state == LegacyShortcutState::Unsupported {
        return Err(advertised.detail);
    }

    let snapshot = read_gnome_shortcuts()?;
    let setup = classify_gnome_shortcut(&snapshot, &advertised.command, &advertised.binding);
    let current = read_gnome_shortcuts()?;
    let writes = gnome_repair_transaction(&snapshot, &current, &setup)?;
    apply_dconf_transaction(&writes)?;
    *LEGACY_SHORTCUT_CACHE
        .lock()
        .expect("legacy shortcut cache lock") = None;

    let repaired = classify_gnome_shortcut(
        &read_gnome_shortcuts()?,
        &advertised.command,
        &advertised.binding,
    );
    if repaired.state != LegacyShortcutState::Ready {
        return Err(format!(
            "GNOME shortcut repair did not become ready: {}",
            repaired.detail
        ));
    }
    Ok(repaired)
}

#[cfg(test)]
mod tests;
