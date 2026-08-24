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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioHost {
    PipeWire,
    PulseAudio,
    Alsa,
    CoreAudio,
    Wasapi,
    Other,
}

impl AudioHost {
    #[must_use]
    pub fn from_cpal_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "pipewire" => Self::PipeWire,
            "pulseaudio" => Self::PulseAudio,
            "alsa" => Self::Alsa,
            "coreaudio" => Self::CoreAudio,
            "wasapi" => Self::Wasapi,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputTransport {
    Bluetooth,
    Usb,
    BuiltIn,
    Pci,
    Network,
    Virtual,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EndpointTier {
    Primary,
    Advanced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInputDescriptor {
    pub id: MicrophoneId,
    pub host: AudioHost,
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
    pub host: AudioHost,
    pub transport: InputTransport,
    pub tier: EndpointTier,
    pub hint: String,
}

impl From<RawInputDescriptor> for InputDeviceInfo {
    fn from(raw: RawInputDescriptor) -> Self {
        let transport = input_transport(&raw);
        let tier = classify_input(&raw);
        let hint = input_hint(&raw, transport);
        let label =
            if raw.host == AudioHost::PipeWire && raw.id.as_str() == "pipewire:input_default" {
                "System default".to_string()
            } else {
                raw.label
            };
        Self {
            id: raw.id,
            label,
            is_default: raw.is_default,
            manufacturer: raw.manufacturer,
            device_type: raw.device_type,
            interface_type: raw.interface_type,
            address: raw.address,
            driver: raw.driver,
            extended: raw.extended,
            host: raw.host,
            transport,
            tier,
            hint,
        }
    }
}

#[must_use]
pub fn classify_input(raw: &RawInputDescriptor) -> EndpointTier {
    let id = raw.id.as_str().to_ascii_lowercase();
    if matches!(raw.host, AudioHost::PipeWire | AudioHost::PulseAudio) {
        let device_type = raw.device_type.as_deref().unwrap_or_default();
        let label = raw.label.trim_start().to_ascii_lowercase();
        let playback_endpoint = contains_any(
            &id,
            &[
                ":sink_default",
                ":output_default",
                ":alsa_output",
                ":bluez_output",
                ":monitor",
                ".monitor",
                "_monitor",
                "-monitor",
            ],
        ) || matches!(
            device_type.to_ascii_lowercase().as_str(),
            "speaker" | "headphones"
        ) || label.starts_with("monitor of ");
        return if playback_endpoint {
            EndpointTier::Advanced
        } else {
            EndpointTier::Primary
        };
    }

    let searchable = descriptor_text(raw);
    let virtual_metadata = contains_any(
        &searchable,
        &[
            "virtual",
            "monitor",
            "loopback",
            "sink",
            "output_default",
            "playback",
            "null audio",
        ],
    );
    if virtual_metadata {
        return EndpointTier::Advanced;
    }
    if raw.host != AudioHost::Alsa {
        return EndpointTier::Primary;
    }

    if id.contains(":hw:") || id.contains(":plughw:") {
        return EndpointTier::Primary;
    }
    if contains_any(
        &searchable,
        &[
            "alsa:default",
            "alsa:pulse",
            "alsa:pipewire",
            "alsa:dmix",
            "alsa:dsnoop",
            "alsa:plug",
            "alsa:rate",
            "samplerate",
            "speex",
            "downmix",
            "upmix",
            "softvol",
            "sound server",
            "rate converter",
            "sof-hda-dsp",
        ],
    ) {
        return EndpointTier::Advanced;
    }
    // ALSA exposes named PCM definitions as devices. Only raw hardware
    // endpoints earned a primary row above; unknown names remain available
    // under technical endpoints instead of looking like physical mics.
    EndpointTier::Advanced
}

#[must_use]
pub fn is_system_default_proxy(device: &InputDeviceInfo) -> bool {
    device.host == AudioHost::PipeWire && device.id.as_str() == "pipewire:input_default"
}

#[must_use]
pub fn selectable_inputs(devices: &[InputDeviceInfo]) -> Vec<InputDeviceInfo> {
    devices
        .iter()
        .filter(|device| !is_system_default_proxy(device))
        .cloned()
        .collect()
}

fn descriptor_text(raw: &RawInputDescriptor) -> String {
    std::iter::once(raw.id.as_str())
        .chain(std::iter::once(raw.label.as_str()))
        .chain(raw.manufacturer.as_deref())
        .chain(raw.device_type.as_deref())
        .chain(raw.interface_type.as_deref())
        .chain(raw.driver.as_deref())
        .chain(raw.extended.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn input_transport(raw: &RawInputDescriptor) -> InputTransport {
    let searchable = descriptor_text(raw);
    if searchable.contains("bluetooth") || searchable.contains("bluez") {
        InputTransport::Bluetooth
    } else if searchable.contains("usb") {
        InputTransport::Usb
    } else if contains_any(&searchable, &["built-in", "built in", "internal"]) {
        InputTransport::BuiltIn
    } else if searchable.contains("pci") {
        InputTransport::Pci
    } else if contains_any(&searchable, &["network", "airplay", "dante"]) {
        InputTransport::Network
    } else if contains_any(&searchable, &["virtual", "monitor", "loopback", "sink"]) {
        InputTransport::Virtual
    } else {
        InputTransport::Unknown
    }
}

fn input_hint(raw: &RawInputDescriptor, transport: InputTransport) -> String {
    let transport = match transport {
        InputTransport::Bluetooth => Some("Bluetooth"),
        InputTransport::Usb => Some("USB"),
        InputTransport::BuiltIn => Some("Built in"),
        InputTransport::Pci => Some("Internal PCI"),
        InputTransport::Network => Some("Network"),
        InputTransport::Virtual => Some("Virtual endpoint"),
        InputTransport::Unknown => None,
    };
    [
        transport,
        raw.device_type.as_deref(),
        raw.manufacturer.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.eq_ignore_ascii_case("unknown"))
    .fold(Vec::<&str>::new(), |mut parts, part| {
        if !parts.iter().any(|known| known.eq_ignore_ascii_case(part)) {
            parts.push(part);
        }
        parts
    })
    .join(" · ")
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
    pub host: AudioHost,
    pub source: SelectionSource,
    pub system_default: Option<InputDeviceInfo>,
    pub system_default_is_proxy: bool,
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
            .or_else(|| {
                raw.parse::<cpal::DeviceId>()
                    .ok()
                    .map(|_| MicrophoneSelection::Device {
                        id: raw.to_string(),
                        last_seen_label: raw.to_string(),
                    })
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
    let fallback = || {
        devices
            .iter()
            .find(|device| device.is_default)
            .or_else(|| devices.first())
            .cloned()
    };
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
            host: AudioHost::Alsa,
            transport: InputTransport::Unknown,
            tier: EndpointTier::Primary,
            hint: String::new(),
        }
    }

    fn raw(host: AudioHost, id: &str, label: &str) -> RawInputDescriptor {
        RawInputDescriptor {
            id: MicrophoneId::parse(id).unwrap(),
            host,
            label: label.into(),
            is_default: false,
            manufacturer: None,
            device_type: None,
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
        }
    }

    #[test]
    fn pipewire_bluetooth_source_keeps_friendly_identity() {
        let mut descriptor = raw(
            AudioHost::PipeWire,
            "pipewire:bluez_input.48_5F_99_00_11_22.0",
            "Pixel Buds Pro",
        );
        descriptor.interface_type = Some("Bluetooth".into());
        descriptor.device_type = Some("Headset".into());
        let presented = InputDeviceInfo::from(descriptor);
        assert_eq!(presented.label, "Pixel Buds Pro");
        assert_eq!(presented.transport, InputTransport::Bluetooth);
        assert_eq!(presented.tier, EndpointTier::Primary);
        assert_eq!(presented.hint, "Bluetooth · Headset");
    }

    #[test]
    fn pipewire_default_proxy_is_distinct_from_selectable_sources() {
        let proxy = InputDeviceInfo::from(raw(
            AudioHost::PipeWire,
            "pipewire:input_default",
            "default_input",
        ));
        let physical = InputDeviceInfo::from(raw(
            AudioHost::PipeWire,
            "pipewire:bluez_input.48_5F_99_00_11_22.0",
            "Pixel Buds Pro",
        ));
        assert!(is_system_default_proxy(&proxy));
        assert_eq!(proxy.label, "System default");
        assert_eq!(
            selectable_inputs(&[proxy, physical.clone()]),
            vec![physical]
        );
    }

    #[test]
    fn native_playback_sinks_and_monitors_are_advanced() {
        for (id, label, device_type) in [
            ("pipewire:sink_default", "default_sink", None),
            ("pipewire:output_default", "default_output", None),
            ("pulseaudio:42", "Monitor of Built-in Audio", None),
            (
                "pipewire:alsa_output.pci-0000_00_1f.3.analog-stereo",
                "Built-in Audio",
                Some("Speaker"),
            ),
        ] {
            let mut descriptor = raw(AudioHost::PipeWire, id, label);
            descriptor.device_type = device_type.map(str::to_string);
            assert_eq!(classify_input(&descriptor), EndpointTier::Advanced, "{id}");
        }
    }

    #[test]
    fn pipewire_speakerphone_input_stays_primary() {
        let mut descriptor = raw(
            AudioHost::PipeWire,
            "pipewire:alsa_input.usb-Jabra_Speak_510-00.mono-fallback",
            "Jabra Speakerphone Microphone",
        );
        descriptor.device_type = Some("Microphone".to_string());
        assert_eq!(classify_input(&descriptor), EndpointTier::Primary);
    }

    #[test]
    fn sparse_pulse_source_is_primary() {
        assert_eq!(
            classify_input(&raw(
                AudioHost::PulseAudio,
                "pulseaudio:42",
                "Jabra Evolve2 65"
            )),
            EndpointTier::Primary
        );
    }

    #[test]
    fn alsa_plugins_and_aliases_are_advanced() {
        for (id, label) in [
            ("alsa:pipewire", "PipeWire Sound Server"),
            ("alsa:dsnoop:CARD=sofhdadsp,DEV=6", "sof-hda-dsp,"),
            (
                "alsa:speexrate",
                "Rate Converter Plugin Using Speex Resampler",
            ),
            ("alsa:upmix", "Plugin for channel upmix (4,6,8)"),
            ("alsa:sysdefault:CARD=PCH", "HDA Intel PCH"),
        ] {
            assert_eq!(
                classify_input(&raw(AudioHost::Alsa, id, label)),
                EndpointTier::Advanced,
                "{id}"
            );
        }
    }

    #[test]
    fn alsa_hardware_endpoints_remain_primary() {
        for id in ["alsa:hw:CARD=USB,DEV=0", "alsa:plughw:CARD=PCH,DEV=0"] {
            assert_eq!(
                classify_input(&raw(AudioHost::Alsa, id, "Microphone")),
                EndpointTier::Primary,
                "{id}"
            );
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
    fn missing_stable_id_uses_first_input_when_no_default_exists() {
        let selection = MicrophoneSelection::Device {
            id: "alsa:gone".into(),
            last_seen_label: "Travel Mic".into(),
        };
        let inputs = vec![device("alsa:usb-one", "USB Mic", false)];
        assert!(matches!(
            resolve_selection(Some(&selection), &inputs),
            InputSelectionStatus::MissingWithFallback { fallback, .. }
                if fallback.id.as_str() == "alsa:usb-one"
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

    #[test]
    fn disconnected_environment_id_stays_an_id() {
        #[cfg(target_os = "linux")]
        let raw = "alsa:gone";
        #[cfg(target_os = "macos")]
        let raw = "coreaudio:gone";
        #[cfg(target_os = "windows")]
        let raw = "wasapi:gone";
        let (selection, source) = selection_from_sources(Some(raw), None, &devices());
        assert_eq!(source, SelectionSource::Environment);
        assert!(matches!(
            selection,
            Some(MicrophoneSelection::Device { id, .. }) if id == raw
        ));
    }
}
