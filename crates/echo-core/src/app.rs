#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommand {
    RecordOnce,
    OpenHistory,
    OpenDictionary,
    Quit,
}

impl AppCommand {
    pub const TRAY_MENU: [Self; 4] = [
        Self::RecordOnce,
        Self::OpenHistory,
        Self::OpenDictionary,
        Self::Quit,
    ];

    #[must_use]
    pub fn tray_label(self) -> &'static str {
        match self {
            Self::RecordOnce => "Record",
            Self::OpenHistory => "History",
            Self::OpenDictionary => "Dictionary",
            Self::Quit => "Quit",
        }
    }

    #[must_use]
    pub fn from_tray_label(label: &str) -> Option<Self> {
        Self::TRAY_MENU
            .into_iter()
            .find(|cmd| cmd.tray_label() == label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_round_trip() {
        assert_eq!(AppCommand::TRAY_MENU.len(), 4);
        assert_eq!(AppCommand::RecordOnce.tray_label(), "Record");
        assert_eq!(AppCommand::OpenHistory.tray_label(), "History");
        assert_eq!(AppCommand::OpenDictionary.tray_label(), "Dictionary");
        assert_eq!(AppCommand::Quit.tray_label(), "Quit");
        for cmd in AppCommand::TRAY_MENU {
            assert_eq!(AppCommand::from_tray_label(cmd.tray_label()), Some(cmd));
        }
        assert_eq!(AppCommand::from_tray_label("Nope"), None);
    }
}
