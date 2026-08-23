use echo_core::MicrophoneSelection;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct MicrophoneId(String);

impl MicrophoneId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, String> {
        let raw = raw.into();
        if raw.trim().is_empty() || raw.chars().any(char::is_control) {
            return Err("microphone id is empty or contains control characters".to_string());
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub id: MicrophoneId,
    pub label: String,
    pub is_default: bool,
    pub manufacturer: Option<String>,
    pub device_type: Option<String>,
    pub interface_type: Option<String>,
    pub address: Option<String>,
    pub driver: Option<String>,
    pub extended: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum InputSelectionStatus {
    SystemDefault {
        active: Option<InputDeviceInfo>,
    },
    Selected {
        device: InputDeviceInfo,
    },
    LegacyMatch {
        name: String,
        device: InputDeviceInfo,
    },
    MissingWithFallback {
        requested_id: String,
        requested_label: String,
        fallback: InputDeviceInfo,
    },
    MissingWithoutFallback {
        requested_id: String,
        requested_label: String,
    },
    AmbiguousLegacyName {
        name: String,
        matches: Vec<InputDeviceInfo>,
        fallback: Option<InputDeviceInfo>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionSource {
    Environment,
    Config,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneSnapshot {
    pub source: SelectionSource,
    pub devices: Vec<InputDeviceInfo>,
    pub selection: InputSelectionStatus,
    pub enumeration_warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MicrophoneTestOutcome {
    Heard,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MicrophoneFailure {
    Disconnected,
    Selection,
    Permission,
    Busy,
    Unsupported,
    Host,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MicrophoneTestResult {
    Completed {
        device: InputDeviceInfo,
        peak_rms: f32,
        outcome: MicrophoneTestOutcome,
    },
    Failed {
        device: Option<InputDeviceInfo>,
        category: MicrophoneFailure,
        message: String,
    },
}

#[must_use]
pub fn selection_from_sources(
    environment: Option<&str>,
    config: Option<&MicrophoneSelection>,
    devices: &[InputDeviceInfo],
) -> (Option<MicrophoneSelection>, SelectionSource) {
    if let Some(raw) = environment.map(str::trim).filter(|raw| !raw.is_empty()) {
        let selection = devices
            .iter()
            .find(|device| device.id.as_str() == raw)
            .map(|device| MicrophoneSelection::Device {
                id: raw.to_string(),
                last_seen_label: device.label.clone(),
            })
            .unwrap_or_else(|| MicrophoneSelection::LegacyName {
                name: raw.to_string(),
            });
        return (Some(selection), SelectionSource::Environment);
    }
    match config {
        Some(selection) => (Some(selection.clone()), SelectionSource::Config),
        None => (None, SelectionSource::Default),
    }
}

#[must_use]
pub fn resolve_selection(
    selection: Option<&MicrophoneSelection>,
    devices: &[InputDeviceInfo],
) -> InputSelectionStatus {
    let fallback = || devices.iter().find(|device| device.is_default).cloned();
    match selection {
        None => InputSelectionStatus::SystemDefault { active: fallback() },
        Some(MicrophoneSelection::Device {
            id,
            last_seen_label,
        }) => match devices.iter().find(|device| device.id.as_str() == id) {
            Some(device) => InputSelectionStatus::Selected {
                device: device.clone(),
            },
            None => match fallback() {
                Some(fallback) => InputSelectionStatus::MissingWithFallback {
                    requested_id: id.clone(),
                    requested_label: last_seen_label.clone(),
                    fallback,
                },
                None => InputSelectionStatus::MissingWithoutFallback {
                    requested_id: id.clone(),
                    requested_label: last_seen_label.clone(),
                },
            },
        },
        Some(MicrophoneSelection::LegacyName { name }) => {
            let matches: Vec<_> = devices
                .iter()
                .filter(|device| device.label == *name)
                .cloned()
                .collect();
            match matches.as_slice() {
                [device] => InputSelectionStatus::LegacyMatch {
                    name: name.clone(),
                    device: device.clone(),
                },
                [] => match fallback() {
                    Some(fallback) => InputSelectionStatus::MissingWithFallback {
                        requested_id: String::new(),
                        requested_label: name.clone(),
                        fallback,
                    },
                    None => InputSelectionStatus::MissingWithoutFallback {
                        requested_id: String::new(),
                        requested_label: name.clone(),
                    },
                },
                _ => InputSelectionStatus::AmbiguousLegacyName {
                    name: name.clone(),
                    matches,
                    fallback: fallback(),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, label: &str, is_default: bool) -> InputDeviceInfo {
        InputDeviceInfo {
            id: MicrophoneId::parse(id).unwrap(),
            label: label.into(),
            is_default,
            manufacturer: None,
            device_type: None,
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
        }
    }

    fn devices() -> Vec<InputDeviceInfo> {
        vec![
            device("alsa:default", "Built-in", true),
            device("alsa:usb-one", "USB Mic", false),
            device("alsa:usb-two", "USB Mic", false),
        ]
    }

    #[test]
    fn duplicate_legacy_names_are_ambiguous() {
        let selection = MicrophoneSelection::LegacyName {
            name: "USB Mic".into(),
        };
        assert!(matches!(
            resolve_selection(Some(&selection), &devices()),
            InputSelectionStatus::AmbiguousLegacyName { matches, .. } if matches.len() == 2
        ));
    }

    #[test]
    fn stable_id_selects_one_duplicate_label() {
        let selection = MicrophoneSelection::Device {
            id: "alsa:usb-two".into(),
            last_seen_label: "USB Mic".into(),
        };
        assert!(matches!(
            resolve_selection(Some(&selection), &devices()),
            InputSelectionStatus::Selected { device } if device.id.as_str() == "alsa:usb-two"
        ));
    }

    #[test]
    fn missing_stable_id_names_the_default_fallback() {
        let selection = MicrophoneSelection::Device {
            id: "alsa:gone".into(),
            last_seen_label: "Travel Mic".into(),
        };
        assert!(matches!(
            resolve_selection(Some(&selection), &devices()),
            InputSelectionStatus::MissingWithFallback { fallback, .. }
                if fallback.id.as_str() == "alsa:default"
        ));
    }

    #[test]
    fn missing_stable_id_without_default_stays_missing() {
        let selection = MicrophoneSelection::Device {
            id: "alsa:gone".into(),
            last_seen_label: "Travel Mic".into(),
        };
        assert!(matches!(
            resolve_selection(Some(&selection), &[]),
            InputSelectionStatus::MissingWithoutFallback { .. }
        ));
    }

    #[test]
    fn unique_legacy_name_is_migratable() {
        let selection = MicrophoneSelection::LegacyName {
            name: "Built-in".into(),
        };
        assert!(matches!(
            resolve_selection(Some(&selection), &devices()),
            InputSelectionStatus::LegacyMatch { device, .. }
                if device.id.as_str() == "alsa:default"
        ));
    }

    #[test]
    fn no_selection_follows_the_default() {
        assert!(matches!(
            resolve_selection(None, &devices()),
            InputSelectionStatus::SystemDefault { active: Some(device) }
                if device.id.as_str() == "alsa:default"
        ));
    }

    #[test]
    fn environment_accepts_an_exact_id_before_legacy_name_matching() {
        let (selection, source) = selection_from_sources(
            Some("alsa:usb-two"),
            Some(&MicrophoneSelection::LegacyName {
                name: "Built-in".into(),
            }),
            &devices(),
        );
        assert_eq!(source, SelectionSource::Environment);
        assert!(matches!(
            selection,
            Some(MicrophoneSelection::Device { id, .. }) if id == "alsa:usb-two"
        ));
    }

    #[test]
    fn environment_legacy_name_can_be_ambiguous() {
        let (selection, _) = selection_from_sources(Some("USB Mic"), None, &devices());
        assert!(matches!(
            resolve_selection(selection.as_ref(), &devices()),
            InputSelectionStatus::AmbiguousLegacyName { matches, .. } if matches.len() == 2
        ));
    }
}
