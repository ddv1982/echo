use serde::{Deserialize, Serialize};

use crate::types::FailReason;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectBackend {
    Libei,
    Ydotool,
    Xdotool,
    Wtype,
    ClipboardPaste,
}

impl InjectBackend {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Libei => "libei",
            Self::Ydotool => "ydotool",
            Self::Xdotool => "xdotool",
            Self::Wtype => "wtype",
            Self::ClipboardPaste => "clipboard-paste",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectReport {
    Typed { backend: InjectBackend },
    Pasted { backend: InjectBackend },
    ClipboardOnly,
    Failed { reason: FailReason },
}

impl InjectReport {
    #[must_use]
    pub fn failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FocusTarget {
    pub window_id: Option<String>,
    pub app_id: Option<String>,
    pub title: Option<String>,
}

impl FocusTarget {
    #[must_use]
    pub fn missing(&self) -> bool {
        self.window_id.is_none() && self.app_id.is_none() && self.title.is_none()
    }
}

pub trait Injector {
    fn focus(&self) -> Result<FocusTarget, FailReason>;
    fn inject(&self, text: &str, target: &FocusTarget) -> InjectReport;
}
