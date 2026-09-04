use super::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
fn generated_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src/generated/ipc.ts")
}
fn device() -> InputDevice {
    InputDevice {
        id: "input-1".to_string(),
        label: "Input".to_string(),
        is_default: true,
        manufacturer: None,
        device_type: None,
        interface_type: None,
        address: None,
        driver: None,
        extended: Vec::new(),
        host: AudioHost::PipeWire,
        transport: InputTransport::BuiltIn,
        tier: EndpointTier::Primary,
        hint: "Built in".to_string(),
    }
}
fn value(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}
fn device_value() -> Value {
    json!({
        "id": "input-1",
        "label": "Input",
        "isDefault": true,
        "manufacturer": null,
        "deviceType": null,
        "interfaceType": null,
        "address": null,
        "driver": null,
        "extended": [],
        "host": "pipe-wire",
        "transport": "built-in",
        "tier": "primary",
        "hint": "Built in"
    })
}
#[test]
fn generated_contract_is_current() {
    let path = generated_path();
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        actual,
        typescript_contract(),
        "regenerate with `cargo run -p echo-ipc-gen`"
    );
}
#[test]
fn generated_contract_is_deterministic() {
    assert_eq!(typescript_contract(), typescript_contract());
    assert_eq!(
        typescript_contract().matches("export type ").count(),
        registered_type_names().len()
    );
}

mod module_schema {
    use ts_rs::TS;

    #[derive(TS)]
    pub struct ModuleType {
        pub value: String,
    }
}

mod module_reexport {
    pub use super::module_schema::ModuleType as ReexportedModuleType;
}

type ModuleTypeAlias = module_reexport::ReexportedModuleType;

macro_rules! fixture_schema_types {
    ($callback:ident $($prefix:tt)*) => {
        $callback! {
            $($prefix)*
            module_reexport::ReexportedModuleType => ModuleTypeAlias,
        }
    };
}

fixture_schema_types!(export_schema_types);

#[test]
fn registry_resolves_module_types_aliases_and_reexports_through_rust_types() {
    let config = Config::default();
    macro_rules! declare_fixture_schema_types {
        ($config:expr; $($export:path => $ty:ty),+ $(,)?) => {
            declarations!($config, $($ty),+)
        };
    }
    let (contract, names) = fixture_schema_types!(declare_fixture_schema_types &config;);
    let fixture = ReexportedModuleType {
        value: "module-safe".to_string(),
    };

    assert_eq!(fixture.value, "module-safe");
    assert!(contract.contains("export type ModuleType = { value: string, };"));
    assert_eq!(names, BTreeSet::from(["ModuleType".to_string()]));
}

#[test]
fn component_ids_preserve_wire_spellings_and_accept_catalog_aliases() {
    let values = [
        ComponentId::WhisperRuntime,
        ComponentId::WhisperVulkanRuntime,
        ComponentId::WhisperBaseQ51,
        ComponentId::WhisperSmall,
        ComponentId::WhisperLargeV3TurboQ50,
        ComponentId::SileroVad,
        ComponentId::SherpaRuntime,
        ComponentId::ParakeetTdt06bV3Int8,
    ];
    assert_eq!(
        values.map(|value| serde_json::to_value(value).unwrap()),
        [
            json!("whisper-runtime"),
            json!("whisper-vulkan-runtime"),
            json!("whisper-base-q51"),
            json!("whisper-small"),
            json!("whisper-large-v3-turbo-q50"),
            json!("silero-vad"),
            json!("sherpa-runtime"),
            json!("parakeet-tdt06b-v3-int8"),
        ]
    );
    assert_eq!(
        serde_json::from_value::<ComponentId>(json!("whisper-base-q5-1")).unwrap(),
        ComponentId::WhisperBaseQ51
    );
    assert_eq!(
        serde_json::from_value::<ComponentId>(json!("whisper-large-v3-turbo-q5-0")).unwrap(),
        ComponentId::WhisperLargeV3TurboQ50
    );
    assert_eq!(
        serde_json::from_value::<ComponentId>(json!("parakeet-tdt-06b-v3-int8")).unwrap(),
        ComponentId::ParakeetTdt06bV3Int8
    );
}
#[test]
fn shortcut_status_serializes_every_variant() {
    let desired = "Super+Alt+Space".to_string();
    let values = [
        ShortcutStatus::Probing {
            desired: desired.clone(),
        },
        ShortcutStatus::Active {
            desired: desired.clone(),
            effective: desired.clone(),
            backend: ShortcutBackend::Portal,
            activation: None,
            verification_identity: "portal:key".to_string(),
        },
        ShortcutStatus::GnomeReady {
            desired: desired.clone(),
            effective: desired.clone(),
            detail: "ready".to_string(),
            command: "echo-desktop".to_string(),
            binding: "<Super><Alt>space".to_string(),
            activation: None,
            verification_identity: "gnome:key".to_string(),
        },
        ShortcutStatus::GnomeSetup {
            desired: desired.clone(),
            setup: GnomeShortcutSetup {
                state: GnomeShortcutState::Missing,
                detail: "missing".to_string(),
                command: "echo-desktop".to_string(),
                binding: "<Super><Alt>space".to_string(),
            },
        },
        ShortcutStatus::Manual {
            desired: desired.clone(),
            command: "echo-desktop".to_string(),
            detail: "manual".to_string(),
        },
        ShortcutStatus::Failed {
            desired: desired.clone(),
            detail: "failed".to_string(),
        },
        ShortcutStatus::Unsupported {
            desired,
            detail: "unsupported".to_string(),
        },
    ];
    assert_eq!(
        values.map(value),
        [
            json!({ "kind": "probing", "desired": "Super+Alt+Space" }),
            json!({
                "kind": "active",
                "desired": "Super+Alt+Space",
                "effective": "Super+Alt+Space",
                "backend": "portal",
                "activation": null,
                "verificationIdentity": "portal:key"
            }),
            json!({
                "kind": "gnome-ready",
                "desired": "Super+Alt+Space",
                "effective": "Super+Alt+Space",
                "detail": "ready",
                "command": "echo-desktop",
                "binding": "<Super><Alt>space",
                "activation": null,
                "verificationIdentity": "gnome:key"
            }),
            json!({
                "kind": "gnome-setup",
                "desired": "Super+Alt+Space",
                "setup": {
                    "state": "missing",
                    "detail": "missing",
                    "command": "echo-desktop",
                    "binding": "<Super><Alt>space"
                }
            }),
            json!({
                "kind": "manual",
                "desired": "Super+Alt+Space",
                "command": "echo-desktop",
                "detail": "manual"
            }),
            json!({
                "kind": "failed",
                "desired": "Super+Alt+Space",
                "detail": "failed"
            }),
            json!({
                "kind": "unsupported",
                "desired": "Super+Alt+Space",
                "detail": "unsupported"
            })
        ]
    );
}
#[test]
fn microphone_selection_serializes_every_variant() {
    let input = device();
    let values = [
        MicrophoneSelection::SystemDefault {
            active: Some(input.clone()),
        },
        MicrophoneSelection::Selected {
            device: input.clone(),
        },
        MicrophoneSelection::LegacyMatch {
            name: "Input".to_string(),
            device: input.clone(),
        },
        MicrophoneSelection::MissingWithFallback {
            requested_id: "missing".to_string(),
            requested_label: "Missing".to_string(),
            fallback: input.clone(),
        },
        MicrophoneSelection::MissingWithoutFallback {
            requested_id: "missing".to_string(),
            requested_label: "Missing".to_string(),
        },
        MicrophoneSelection::AmbiguousLegacyName {
            name: "Input".to_string(),
            matches: vec![input.clone()],
            fallback: Some(input),
        },
    ];
    let input = device_value();
    assert_eq!(
        values.map(value),
        [
            json!({ "kind": "system-default", "active": input.clone() }),
            json!({ "kind": "selected", "device": input.clone() }),
            json!({
                "kind": "legacy-match",
                "name": "Input",
                "device": input.clone()
            }),
            json!({
                "kind": "missing-with-fallback",
                "requestedId": "missing",
                "requestedLabel": "Missing",
                "fallback": input.clone()
            }),
            json!({
                "kind": "missing-without-fallback",
                "requestedId": "missing",
                "requestedLabel": "Missing"
            }),
            json!({
                "kind": "ambiguous-legacy-name",
                "name": "Input",
                "matches": [input.clone()],
                "fallback": input
            })
        ]
    );
}
#[test]
fn microphone_test_serializes_every_variant() {
    let values = [
        MicrophoneTestResult::Completed {
            device: device(),
            peak_rms: 0.5,
            dropped_samples: 17,
            outcome: MicrophoneTestOutcome::Heard,
        },
        MicrophoneTestResult::Failed {
            device: None,
            category: MicrophoneFailure::Busy,
            message: "busy".to_string(),
        },
    ];
    assert_eq!(
        values.map(value),
        [
            json!({
                "kind": "completed",
                "device": device_value(),
                "peakRms": 0.5,
                "droppedSamples": 17,
                "outcome": "heard"
            }),
            json!({
                "kind": "failed",
                "device": null,
                "category": "busy",
                "message": "busy"
            })
        ]
    );
}
#[cfg(feature = "desktop")]
#[test]
fn microphone_test_projection_preserves_dropped_samples() {
    let result = echo::microphone::MicrophoneTestResult::Completed {
        device: echo::microphone::InputDeviceInfo {
            id: echo::microphone::MicrophoneId::parse("input-1").unwrap(),
            label: "Input".to_string(),
            is_default: true,
            manufacturer: None,
            device_type: None,
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
            host: echo::microphone::AudioHost::PipeWire,
            transport: echo::microphone::InputTransport::BuiltIn,
            tier: echo::microphone::EndpointTier::Primary,
            hint: "Built in".to_string(),
        },
        peak_rms: 0.5,
        dropped_samples: 23,
        outcome: echo::microphone::MicrophoneTestOutcome::Heard,
    };

    let projected = MicrophoneTestResult::from(result);
    assert_eq!(value(projected)["droppedSamples"], json!(23));
}
#[test]
fn managed_state_serializes_every_variant() {
    let values = [
        ManagedComponentState::Absent { resumable_bytes: 0 },
        ManagedComponentState::Ready {
            version: "1".to_string(),
            bytes: 1,
            root: "/tmp".to_string(),
        },
        ManagedComponentState::NeedsRepair {
            reason: "changed".to_string(),
            resumable_bytes: 0,
        },
        ManagedComponentState::Unsupported {
            reason: "platform".to_string(),
        },
    ];
    assert_eq!(
        values.map(value),
        [
            json!({ "kind": "absent", "resumableBytes": 0 }),
            json!({ "kind": "ready", "version": "1", "bytes": 1, "root": "/tmp" }),
            json!({
                "kind": "needs-repair",
                "reason": "changed",
                "resumableBytes": 0
            }),
            json!({ "kind": "unsupported", "reason": "platform" })
        ]
    );
}
#[test]
fn setup_event_serializes_every_variant() {
    let progress = InstallProgress {
        operation_id: "operation".to_string(),
        component: ComponentId::WhisperRuntime,
        phase: InstallPhase::Downloading,
        received_bytes: 1,
        total_bytes: 2,
        resumed_from_bytes: 0,
    };
    let values = [
        SetupEvent::Progress { progress },
        SetupEvent::Finished {
            operation_id: "operation".to_string(),
        },
        SetupEvent::Cancelled {
            operation_id: "operation".to_string(),
        },
        SetupEvent::Failed {
            operation_id: "operation".to_string(),
            error: "failed".to_string(),
        },
    ];
    assert_eq!(
        values.map(value),
        [
            json!({
                "kind": "progress",
                "progress": {
                    "operationId": "operation",
                    "component": "whisper-runtime",
                    "phase": "downloading",
                    "receivedBytes": 1,
                    "totalBytes": 2,
                    "resumedFromBytes": 0
                }
            }),
            json!({ "kind": "finished", "operationId": "operation" }),
            json!({ "kind": "cancelled", "operationId": "operation" }),
            json!({
                "kind": "failed",
                "operationId": "operation",
                "error": "failed"
            })
        ]
    );
}
#[test]
fn nullable_performance_fields_are_present() {
    let value = serde_json::to_value(LastRun {
        engine: "whisper".to_string(),
        binary: None,
        model_path: None,
        multilingual: None,
        vad: None,
        infer_ms: 1,
        language: None,
        language_probability: None,
        performance: None,
    })
    .unwrap();
    assert_eq!(value["performance"], Value::Null);
    let performance = serde_json::to_value(LastRunPerformance {
        mode: RunMode::ColdCli,
        runtime_source: RuntimeSource::System,
        backend: RuntimeBackend::Cpu,
        device: None,
        total_ms: 1,
        audio_encode_ms: 0,
        child_wall_ms: 1,
        parse_ms: 0,
        attempt_count: 1,
        tuning: TuningTelemetry {
            threads: None,
            beam_size: None,
            best_of: None,
            no_fallback: None,
        },
        acceleration_skip: None,
        recovery: None,
    })
    .unwrap();
    assert_eq!(performance["accelerationSkip"], Value::Null);
    assert_eq!(performance["recovery"], Value::Null);
    assert_eq!(
        serde_json::to_value(RecoveryTelemetry {
            identity_key: "gpu".to_string(),
            accelerated_attempted: false,
            fallback_reason: None,
        })
        .unwrap(),
        json!({ "identityKey": "gpu", "acceleratedAttempted": false })
    );
    assert_eq!(
        serde_json::to_value(RecoveryTelemetry {
            identity_key: "gpu".to_string(),
            accelerated_attempted: true,
            fallback_reason: Some(RecoveryReason::RuntimeFailure),
        })
        .unwrap(),
        json!({
            "identityKey": "gpu",
            "acceleratedAttempted": true,
            "fallbackReason": "runtimeFailure"
        })
    );
    assert_eq!(
        serde_json::to_value(VulkanDeviceId {
            device_uuid: "device".to_string(),
            driver_uuid: "driver".to_string(),
        })
        .unwrap(),
        json!({ "deviceUUID": "device", "driverUUID": "driver" })
    );
}

#[cfg(feature = "desktop")]
#[test]
fn every_core_acceleration_skip_has_an_ipc_projection() {
    use echo_core::WhisperAccelerationSkip as Core;

    for (core, ipc) in [
        (Core::RuntimeMissing, AccelerationSkipReason::RuntimeMissing),
        (
            Core::NoDeviceEnumerated,
            AccelerationSkipReason::NoDeviceEnumerated,
        ),
        (
            Core::PinnedDeviceAbsent,
            AccelerationSkipReason::PinnedDeviceAbsent,
        ),
        (
            Core::DeviceQuarantined,
            AccelerationSkipReason::DeviceQuarantined,
        ),
        (
            Core::CpuFallbackMissing,
            AccelerationSkipReason::CpuFallbackMissing,
        ),
        (Core::DeviceNotReady, AccelerationSkipReason::DeviceNotReady),
    ] {
        assert_eq!(AccelerationSkipReason::from(core), ipc);
    }
}
