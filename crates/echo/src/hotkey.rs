#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

/// Desktop session capability used by the GUI's native shortcut runtime.
/// Detection deliberately consults only `XDG_SESSION_TYPE`: a stale DISPLAY
/// variable in a Wayland session is not evidence that X11 grabs are usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSession {
    Wayland,
    X11,
    Unknown,
}

impl DesktopSession {
    /// Detect the current desktop session from `XDG_SESSION_TYPE` only.
    #[must_use]
    pub fn current() -> Self {
        Self::from_xdg_session_type(std::env::var("XDG_SESSION_TYPE").ok().as_deref())
    }

    #[must_use]
    pub fn from_xdg_session_type(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("wayland") => Self::Wayland,
            Some("x11") => Self::X11,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBackend {
    Portal,
    X11,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDecision {
    pub backend: NativeBackend,
    pub reason: Option<String>,
}

/// Select a backend from observed capabilities. `portal_version` must come
/// from the live GlobalShortcuts proxy, never from a desktop name/version.
#[must_use]
pub fn select_native_backend(
    session: DesktopSession,
    portal_version: Option<u32>,
) -> BackendDecision {
    match session {
        DesktopSession::Wayland if portal_version.unwrap_or(0) >= 1 => BackendDecision {
            backend: NativeBackend::Portal,
            reason: None,
        },
        DesktopSession::Wayland => BackendDecision {
            backend: NativeBackend::Unsupported,
            reason: Some(
                "Wayland session has no org.freedesktop.portal.GlobalShortcuts interface"
                    .to_string(),
            ),
        },
        DesktopSession::X11 => BackendDecision {
            backend: NativeBackend::X11,
            reason: None,
        },
        DesktopSession::Unknown => BackendDecision {
            backend: NativeBackend::Unsupported,
            reason: Some("unknown or headless desktop session".to_string()),
        },
    }
}

/// Debounces a toggle shortcut into one action per physical press.
#[derive(Debug)]
pub struct ToggleDriver {
    down: bool,
    live: bool,
}

impl Default for ToggleDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ToggleDriver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            down: false,
            live: true,
        }
    }

    #[must_use]
    pub fn on_edge(&mut self, edge: HotkeyEvent) -> bool {
        if !self.live {
            return false;
        }
        match edge {
            HotkeyEvent::Down if !self.down => {
                self.down = true;
                true
            }
            HotkeyEvent::Up => {
                self.down = false;
                false
            }
            HotkeyEvent::Down => false,
        }
    }

    /// Permanently disarm the driver when its listener/session terminates.
    pub fn terminate(&mut self) {
        self.live = false;
        self.down = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_session_uses_session_type_only() {
        let cases = [
            (Some("wayland"), DesktopSession::Wayland),
            (Some("  WaYlAnD\t"), DesktopSession::Wayland),
            (Some(" X11 "), DesktopSession::X11),
            (Some("mir"), DesktopSession::Unknown),
            (Some(""), DesktopSession::Unknown),
            (None, DesktopSession::Unknown),
        ];
        for (value, expected) in cases {
            assert_eq!(DesktopSession::from_xdg_session_type(value), expected);
        }
    }

    #[test]
    fn backend_selection_requires_observed_portal_support() {
        assert_eq!(
            select_native_backend(DesktopSession::Wayland, Some(1)).backend,
            NativeBackend::Portal
        );
        assert_eq!(
            select_native_backend(DesktopSession::Wayland, None).backend,
            NativeBackend::Unsupported
        );
        assert_eq!(
            select_native_backend(DesktopSession::X11, None).backend,
            NativeBackend::X11
        );
    }

    #[test]
    fn toggle_driver_fires_once_per_press_and_disarms() {
        let mut driver = ToggleDriver::new();
        assert!(driver.on_edge(HotkeyEvent::Down));
        assert!(!driver.on_edge(HotkeyEvent::Down));
        assert!(!driver.on_edge(HotkeyEvent::Up));
        assert!(driver.on_edge(HotkeyEvent::Down));
        driver.terminate();
        assert!(!driver.on_edge(HotkeyEvent::Down));
        assert!(!driver.on_edge(HotkeyEvent::Up));
    }
}
