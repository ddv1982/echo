use super::*;
impl ShortcutBackend {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portal => "portal",
            Self::X11 => "x11",
        }
    }
}

impl LegacyShortcutSetup {
    #[must_use]
    pub fn as_gnome_setup(&self) -> Option<GnomeShortcutSetup> {
        let state = match self.state {
            LegacyShortcutState::Missing => GnomeShortcutState::Missing,
            LegacyShortcutState::Stale => GnomeShortcutState::Stale,
            LegacyShortcutState::Conflicting => GnomeShortcutState::Conflicting,
            LegacyShortcutState::Unsupported => GnomeShortcutState::Unsupported,
            LegacyShortcutState::Ready => return None,
        };
        Some(GnomeShortcutSetup {
            state,
            detail: self.detail.clone(),
            command: self.command.clone(),
            binding: self.binding.clone(),
        })
    }
}

impl From<echo_core::WhisperRunMode> for RunMode {
    fn from(value: echo_core::WhisperRunMode) -> Self {
        match value {
            echo_core::WhisperRunMode::ColdCli => Self::ColdCli,
            echo_core::WhisperRunMode::ColdFallback => Self::ColdFallback,
        }
    }
}

impl From<echo_core::WhisperRuntimeSource> for RuntimeSource {
    fn from(value: echo_core::WhisperRuntimeSource) -> Self {
        match value {
            echo_core::WhisperRuntimeSource::Managed => Self::Managed,
            echo_core::WhisperRuntimeSource::System => Self::System,
            echo_core::WhisperRuntimeSource::Unknown => Self::Unknown,
        }
    }
}

impl From<echo_core::WhisperRuntimeBackend> for RuntimeBackend {
    fn from(value: echo_core::WhisperRuntimeBackend) -> Self {
        match value {
            echo_core::WhisperRuntimeBackend::Cpu => Self::Cpu,
            echo_core::WhisperRuntimeBackend::Cuda => Self::Cuda,
            echo_core::WhisperRuntimeBackend::Vulkan => Self::Vulkan,
            echo_core::WhisperRuntimeBackend::OpenVino => Self::OpenVino,
            echo_core::WhisperRuntimeBackend::Rocm => Self::Rocm,
            echo_core::WhisperRuntimeBackend::Unknown => Self::Unknown,
        }
    }
}

impl From<echo_core::WhisperTuningTelemetry> for TuningTelemetry {
    fn from(value: echo_core::WhisperTuningTelemetry) -> Self {
        Self {
            threads: value.threads,
            beam_size: value.beam_size,
            best_of: value.best_of,
            no_fallback: value.no_fallback,
        }
    }
}

impl From<echo_core::WhisperRecoveryTelemetry> for RecoveryTelemetry {
    fn from(value: echo_core::WhisperRecoveryTelemetry) -> Self {
        Self {
            identity_key: value.identity_key,
            accelerated_attempted: value.accelerated_attempted,
            fallback_reason: value.fallback_reason.map(Into::into),
        }
    }
}

impl From<echo_core::WhisperRecoveryReason> for RecoveryReason {
    fn from(value: echo_core::WhisperRecoveryReason) -> Self {
        match value {
            echo_core::WhisperRecoveryReason::Quarantined => Self::Quarantined,
            echo_core::WhisperRecoveryReason::QuarantineUnreadable => Self::QuarantineUnreadable,
            echo_core::WhisperRecoveryReason::RuntimeFailure => Self::RuntimeFailure,
            echo_core::WhisperRecoveryReason::Timeout => Self::Timeout,
            echo_core::WhisperRecoveryReason::MalformedOutput => Self::MalformedOutput,
            echo_core::WhisperRecoveryReason::MissingReceipt => Self::MissingReceipt,
            echo_core::WhisperRecoveryReason::ReceiptMismatch => Self::ReceiptMismatch,
            echo_core::WhisperRecoveryReason::CpuFallback => Self::CpuFallback,
            echo_core::WhisperRecoveryReason::IdentityMismatch => Self::IdentityMismatch,
        }
    }
}

impl From<&echo_core::DictEntry> for DictionaryItem {
    fn from(value: &echo_core::DictEntry) -> Self {
        Self {
            spoken: value.spoken.clone(),
            written: value.written.clone(),
            created_at: value.created_at,
        }
    }
}

impl From<echo::stt::GpuDevice> for GpuDevice {
    fn from(value: echo::stt::GpuDevice) -> Self {
        Self {
            id: VulkanDeviceId {
                device_uuid: value.id.device_uuid,
                driver_uuid: value.id.driver_uuid,
            },
            name: value.name,
            vendor_id: value.vendor_id,
            device_id: value.device_id,
            drm_driver: value.drm_driver,
            software: value.software,
        }
    }
}

impl From<echo::microphone::AudioHost> for AudioHost {
    fn from(value: echo::microphone::AudioHost) -> Self {
        match value {
            echo::microphone::AudioHost::PipeWire => Self::PipeWire,
            echo::microphone::AudioHost::PulseAudio => Self::PulseAudio,
            echo::microphone::AudioHost::Alsa => Self::Alsa,
            echo::microphone::AudioHost::CoreAudio => Self::CoreAudio,
            echo::microphone::AudioHost::Wasapi => Self::Wasapi,
            echo::microphone::AudioHost::Other => Self::Other,
        }
    }
}

impl From<echo::microphone::InputTransport> for InputTransport {
    fn from(value: echo::microphone::InputTransport) -> Self {
        match value {
            echo::microphone::InputTransport::Bluetooth => Self::Bluetooth,
            echo::microphone::InputTransport::Usb => Self::Usb,
            echo::microphone::InputTransport::BuiltIn => Self::BuiltIn,
            echo::microphone::InputTransport::Pci => Self::Pci,
            echo::microphone::InputTransport::Network => Self::Network,
            echo::microphone::InputTransport::Virtual => Self::Virtual,
            echo::microphone::InputTransport::Unknown => Self::Unknown,
        }
    }
}

impl From<echo::microphone::EndpointTier> for EndpointTier {
    fn from(value: echo::microphone::EndpointTier) -> Self {
        match value {
            echo::microphone::EndpointTier::Primary => Self::Primary,
            echo::microphone::EndpointTier::Advanced => Self::Advanced,
        }
    }
}

impl From<echo::microphone::InputDeviceInfo> for InputDevice {
    fn from(value: echo::microphone::InputDeviceInfo) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            label: value.label,
            is_default: value.is_default,
            manufacturer: value.manufacturer,
            device_type: value.device_type,
            interface_type: value.interface_type,
            address: value.address,
            driver: value.driver,
            extended: value.extended,
            host: value.host.into(),
            transport: value.transport.into(),
            tier: value.tier.into(),
            hint: value.hint,
        }
    }
}

impl From<echo::microphone::InputSelectionStatus> for MicrophoneSelection {
    fn from(value: echo::microphone::InputSelectionStatus) -> Self {
        match value {
            echo::microphone::InputSelectionStatus::SystemDefault { active } => {
                Self::SystemDefault {
                    active: active.map(Into::into),
                }
            }
            echo::microphone::InputSelectionStatus::Selected { device } => Self::Selected {
                device: device.into(),
            },
            echo::microphone::InputSelectionStatus::LegacyMatch { name, device } => {
                Self::LegacyMatch {
                    name,
                    device: device.into(),
                }
            }
            echo::microphone::InputSelectionStatus::MissingWithFallback {
                requested_id,
                requested_label,
                fallback,
            } => Self::MissingWithFallback {
                requested_id,
                requested_label,
                fallback: fallback.into(),
            },
            echo::microphone::InputSelectionStatus::MissingWithoutFallback {
                requested_id,
                requested_label,
            } => Self::MissingWithoutFallback {
                requested_id,
                requested_label,
            },
            echo::microphone::InputSelectionStatus::AmbiguousLegacyName {
                name,
                matches,
                fallback,
            } => Self::AmbiguousLegacyName {
                name,
                matches: matches.into_iter().map(Into::into).collect(),
                fallback: fallback.map(Into::into),
            },
        }
    }
}

impl From<echo::microphone::SelectionSource> for MicrophoneSource {
    fn from(value: echo::microphone::SelectionSource) -> Self {
        match value {
            echo::microphone::SelectionSource::Environment => Self::Environment,
            echo::microphone::SelectionSource::Config => Self::Config,
            echo::microphone::SelectionSource::Default => Self::Default,
        }
    }
}

impl From<echo::microphone::MicrophoneSnapshot> for MicrophoneSnapshot {
    fn from(value: echo::microphone::MicrophoneSnapshot) -> Self {
        Self {
            host: value.host.into(),
            source: value.source.into(),
            system_default: value.system_default.map(Into::into),
            system_default_is_proxy: value.system_default_is_proxy,
            devices: value.devices.into_iter().map(Into::into).collect(),
            selection: value.selection.into(),
            enumeration_warning: value.enumeration_warning,
        }
    }
}

impl From<echo::microphone::MicrophoneTestOutcome> for MicrophoneTestOutcome {
    fn from(value: echo::microphone::MicrophoneTestOutcome) -> Self {
        match value {
            echo::microphone::MicrophoneTestOutcome::Heard => Self::Heard,
            echo::microphone::MicrophoneTestOutcome::Silent => Self::Silent,
        }
    }
}

impl From<echo::microphone::MicrophoneFailure> for MicrophoneFailure {
    fn from(value: echo::microphone::MicrophoneFailure) -> Self {
        match value {
            echo::microphone::MicrophoneFailure::Disconnected => Self::Disconnected,
            echo::microphone::MicrophoneFailure::Selection => Self::Selection,
            echo::microphone::MicrophoneFailure::Permission => Self::Permission,
            echo::microphone::MicrophoneFailure::Busy => Self::Busy,
            echo::microphone::MicrophoneFailure::Unsupported => Self::Unsupported,
            echo::microphone::MicrophoneFailure::Host => Self::Host,
            echo::microphone::MicrophoneFailure::Failed => Self::Failed,
        }
    }
}

impl From<echo::microphone::MicrophoneTestResult> for MicrophoneTestResult {
    fn from(value: echo::microphone::MicrophoneTestResult) -> Self {
        match value {
            echo::microphone::MicrophoneTestResult::Completed {
                device,
                peak_rms,
                outcome,
            } => Self::Completed {
                device: device.into(),
                peak_rms,
                outcome: outcome.into(),
            },
            echo::microphone::MicrophoneTestResult::Failed {
                device,
                category,
                message,
            } => Self::Failed {
                device: device.map(Into::into),
                category: category.into(),
                message,
            },
        }
    }
}

impl From<echo::install::ComponentId> for ComponentId {
    fn from(value: echo::install::ComponentId) -> Self {
        match value {
            echo::install::ComponentId::WhisperRuntime => Self::WhisperRuntime,
            echo::install::ComponentId::WhisperVulkanRuntime => Self::WhisperVulkanRuntime,
            echo::install::ComponentId::WhisperBaseQ51 => Self::WhisperBaseQ51,
            echo::install::ComponentId::WhisperSmall => Self::WhisperSmall,
            echo::install::ComponentId::WhisperLargeV3TurboQ50 => Self::WhisperLargeV3TurboQ50,
            echo::install::ComponentId::SileroVad => Self::SileroVad,
            echo::install::ComponentId::SherpaRuntime => Self::SherpaRuntime,
            echo::install::ComponentId::ParakeetTdt06bV3Int8 => Self::ParakeetTdt06bV3Int8,
        }
    }
}

impl From<ComponentId> for echo::install::ComponentId {
    fn from(value: ComponentId) -> Self {
        match value {
            ComponentId::WhisperRuntime => Self::WhisperRuntime,
            ComponentId::WhisperVulkanRuntime => Self::WhisperVulkanRuntime,
            ComponentId::WhisperBaseQ51 => Self::WhisperBaseQ51,
            ComponentId::WhisperSmall => Self::WhisperSmall,
            ComponentId::WhisperLargeV3TurboQ50 => Self::WhisperLargeV3TurboQ50,
            ComponentId::SileroVad => Self::SileroVad,
            ComponentId::SherpaRuntime => Self::SherpaRuntime,
            ComponentId::ParakeetTdt06bV3Int8 => Self::ParakeetTdt06bV3Int8,
        }
    }
}

impl From<echo::install::SetupPlanId> for SetupPlanId {
    fn from(value: echo::install::SetupPlanId) -> Self {
        match value {
            echo::install::SetupPlanId::Recommended => Self::Recommended,
            echo::install::SetupPlanId::Parakeet => Self::Parakeet,
            echo::install::SetupPlanId::WhisperBase => Self::WhisperBase,
            echo::install::SetupPlanId::WhisperSmall => Self::WhisperSmall,
            echo::install::SetupPlanId::WhisperLargeV3Turbo => Self::WhisperLargeV3Turbo,
        }
    }
}

impl From<SetupPlanId> for echo::install::SetupPlanId {
    fn from(value: SetupPlanId) -> Self {
        match value {
            SetupPlanId::Recommended => Self::Recommended,
            SetupPlanId::Parakeet => Self::Parakeet,
            SetupPlanId::WhisperBase => Self::WhisperBase,
            SetupPlanId::WhisperSmall => Self::WhisperSmall,
            SetupPlanId::WhisperLargeV3Turbo => Self::WhisperLargeV3Turbo,
        }
    }
}

impl From<echo::install::ManagedComponentState> for ManagedComponentState {
    fn from(value: echo::install::ManagedComponentState) -> Self {
        match value {
            echo::install::ManagedComponentState::Absent { resumable_bytes } => {
                Self::Absent { resumable_bytes }
            }
            echo::install::ManagedComponentState::Ready {
                version,
                bytes,
                root,
            } => Self::Ready {
                version,
                bytes,
                root,
            },
            echo::install::ManagedComponentState::NeedsRepair {
                reason,
                resumable_bytes,
            } => Self::NeedsRepair {
                reason,
                resumable_bytes,
            },
            echo::install::ManagedComponentState::Unsupported { reason } => {
                Self::Unsupported { reason }
            }
        }
    }
}

impl From<echo::install::InstallPhase> for InstallPhase {
    fn from(value: echo::install::InstallPhase) -> Self {
        match value {
            echo::install::InstallPhase::CheckingDisk => Self::CheckingDisk,
            echo::install::InstallPhase::Downloading => Self::Downloading,
            echo::install::InstallPhase::Verifying => Self::Verifying,
            echo::install::InstallPhase::Extracting => Self::Extracting,
            echo::install::InstallPhase::Activating => Self::Activating,
        }
    }
}

impl From<echo::install::InstallProgress> for InstallProgress {
    fn from(value: echo::install::InstallProgress) -> Self {
        Self {
            operation_id: value.operation_id.as_str().to_string(),
            component: value.component.into(),
            phase: value.phase.into(),
            received_bytes: value.received_bytes,
            total_bytes: value.total_bytes,
            resumed_from_bytes: value.resumed_from_bytes,
        }
    }
}

impl From<ComponentOrigin> for ActiveComponentOrigin {
    fn from(value: ComponentOrigin) -> Self {
        match value {
            ComponentOrigin::System => Self::System,
            ComponentOrigin::External => Self::External,
        }
    }
}
