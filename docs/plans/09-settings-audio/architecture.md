# Architecture synthesis

Three isolated candidates explored the full shape. A `gpt-5.4` cross-judge scored the SOL candidate highest at 25 of 25. Luna scored 20 and G55 scored 17.

## Chosen shape

The SOL candidate is the base.

- CPAL remains the sole discovery and capture authority.
- Native PipeWire and PulseAudio hosts provide session-server sources and friendly metadata. ALSA remains the runtime fallback.
- `audio.rs` owns CPAL handles and boundary extraction.
- `microphone.rs` owns pure endpoint classification, selection resolution, and the UI-safe projection.
- Exact endpoint IDs remain authoritative. The app never auto-migrates by label, address, or fingerprint.
- A distinct system-default projection is grafted from Luna so the UI can always name the default source without duplicating policy.
- Frontend code owns setup copy and disclosure. The backend remains authoritative for readiness, plans, components, operations, and security.
- Native details elements own disclosure state. No extra React state machine is added.

## Core data shape

```rust
enum AudioHost {
    PipeWire,
    PulseAudio,
    Alsa,
    Other,
}

enum InputTransport {
    Bluetooth,
    Usb,
    BuiltIn,
    Pci,
    Network,
    Virtual,
    Unknown,
}

enum EndpointTier {
    Primary,
    Advanced,
}

struct RawInputDescriptor {
    id: MicrophoneId,
    host: AudioHost,
    label: String,
    is_default: bool,
    manufacturer: Option<String>,
    device_type: Option<String>,
    interface_type: Option<String>,
    address: Option<String>,
    driver: Option<String>,
    extended: Vec<String>,
}

struct InputDeviceInfo {
    id: MicrophoneId,
    label: String,
    hint: String,
    is_default: bool,
    host: AudioHost,
    transport: InputTransport,
    tier: EndpointTier,
    technical: EndpointTechnical,
}

struct MicrophoneSnapshot {
    host: AudioHost,
    source: SelectionSource,
    system_default: Option<InputDeviceInfo>,
    devices: Vec<InputDeviceInfo>,
    selection: InputSelectionStatus,
    enumeration_warning: Option<String>,
}
```

Dominant access stays linear over a small device list. A map, grouped representative, persisted fingerprint, and second migration command were rejected because they add identity mechanisms without improving capture safety.

## Setup deviation from the base candidate

The base candidate proposed replacing the readiness API with a nested setup schema. That does not earn its cost yet. The existing snapshot already contains all components, plans, origins, disk state, and active progress. v0.9 first subtracts the catalog dump through a pure TypeScript presenter. A backend operation field is added only if the real UI cannot state progress truthfully without it.

This deviation follows the laziness rule and keeps installer security code outside a UI-only redesign.

## Responsive shape

- Collapse the sidebar around 920 to 960 pixels, not at the 760-pixel native minimum.
- Give Settings a maximum width and `min-width: 0` throughout the shell and row children.
- Stack rows based on Settings content width. Use a viewport fallback if the supported WebKitGTK baseline cannot provide container queries.
- Allow status, errors, and technical paths to wrap.
- Treat 760, 761, and the new navigation breakpoint minus one, exact, and plus one as required regression widths.

## Rejected shapes

- React-only ALSA filtering. Bluetooth metadata remains unavailable and capture identity still diverges from the session server.
- A second `wpctl` or `pactl` device authority. Mapping its objects back to ALSA IDs is unstable during hotplug.
- Automatic label or fingerprint migration. A wrong guess can record from the wrong microphone.
- Backend-authored UI copy and disclosure roles. It couples Rust installer contracts to ordinary product wording.
- Weighted overall setup percentages. Verify, extract, and activation costs are not measured, so a smooth percentage would be invented.
