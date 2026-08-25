use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    WhisperRuntime,
    WhisperBaseQ51,
    WhisperSmall,
    WhisperLargeV3TurboQ50,
    SileroVad,
    SherpaRuntime,
    ParakeetTdt06bV3Int8,
}

impl ComponentId {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WhisperRuntime => "whisper-runtime",
            Self::WhisperBaseQ51 => "whisper-base-q5-1",
            Self::WhisperSmall => "whisper-small",
            Self::WhisperLargeV3TurboQ50 => "whisper-large-v3-turbo-q5-0",
            Self::SileroVad => "silero-vad",
            Self::SherpaRuntime => "sherpa-runtime",
            Self::ParakeetTdt06bV3Int8 => "parakeet-tdt-06b-v3-int8",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SetupPlanId {
    Recommended,
    Parakeet,
    WhisperBase,
    WhisperSmall,
    WhisperLargeV3Turbo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactFormat {
    Direct,
    TarGzip,
    TarBzip2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSpec {
    pub id: ComponentId,
    pub label: &'static str,
    pub version: &'static str,
    pub url: &'static str,
    pub artifact_name: &'static str,
    pub artifact_size: u64,
    pub artifact_sha256: &'static str,
    pub installed_bytes: u64,
    pub format: ArtifactFormat,
    pub inventory_key: Option<&'static str>,
}

pub const COMPONENTS: &[ComponentSpec] = &[
    ComponentSpec {
        id: ComponentId::WhisperRuntime,
        label: "Whisper runtime",
        version: "1.9.2",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-ubuntu-x64.tar.gz",
        artifact_name: "whisper-bin-ubuntu-x64.tar.gz",
        artifact_size: 9_497_583,
        artifact_sha256: "46811a3ecf584307480a220b9ef5ff81b7b22dc41577cbc274ce3afc61f753b1",
        installed_bytes: 18_284_400,
        format: ArtifactFormat::TarGzip,
        inventory_key: Some("whisper-runtime"),
    },
    ComponentSpec {
        id: ComponentId::WhisperBaseQ51,
        label: "Base multilingual Q5_1",
        version: "base-q5_1",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
        artifact_name: "ggml-base-q5_1.bin",
        artifact_size: 59_707_625,
        artifact_sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
        installed_bytes: 59_707_625,
        format: ArtifactFormat::Direct,
        inventory_key: None,
    },
    ComponentSpec {
        id: ComponentId::WhisperSmall,
        label: "Small multilingual",
        version: "small",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        artifact_name: "ggml-small.bin",
        artifact_size: 487_601_967,
        artifact_sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        installed_bytes: 487_601_967,
        format: ArtifactFormat::Direct,
        inventory_key: None,
    },
    ComponentSpec {
        id: ComponentId::WhisperLargeV3TurboQ50,
        label: "Large v3 Turbo Q5_0",
        version: "large-v3-turbo-q5_0",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        artifact_name: "ggml-large-v3-turbo-q5_0.bin",
        artifact_size: 574_041_195,
        artifact_sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        installed_bytes: 574_041_195,
        format: ArtifactFormat::Direct,
        inventory_key: None,
    },
    ComponentSpec {
        id: ComponentId::SileroVad,
        label: "Silero voice detection",
        version: "6.2.0",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin",
        artifact_name: "ggml-silero-v6.2.0.bin",
        artifact_size: 885_098,
        artifact_sha256: "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987",
        installed_bytes: 885_098,
        format: ArtifactFormat::Direct,
        inventory_key: None,
    },
    ComponentSpec {
        id: ComponentId::SherpaRuntime,
        label: "sherpa-onnx runtime",
        version: "1.13.6",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.6/sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2",
        artifact_name: "sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2",
        artifact_size: 361_356_492,
        artifact_sha256: "ba2c35a3f6ca889e6c31fe12eba292fb13eeca5cb13687e6b04ccdc23649c954",
        installed_bytes: 35_818_704,
        format: ArtifactFormat::TarBzip2,
        inventory_key: Some("sherpa-runtime"),
    },
    ComponentSpec {
        id: ComponentId::ParakeetTdt06bV3Int8,
        label: "Parakeet TDT 0.6b v3",
        version: "0.6b-v3-int8",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
        artifact_name: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
        artifact_size: 487_170_055,
        artifact_sha256: "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
        installed_bytes: 670_478_772,
        format: ArtifactFormat::TarBzip2,
        inventory_key: Some("parakeet-model"),
    },
];

#[must_use]
pub fn component(id: ComponentId) -> &'static ComponentSpec {
    COMPONENTS
        .iter()
        .find(|spec| spec.id == id)
        .expect("every ComponentId is catalogued")
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInventory {
    pub schema_version: u32,
    pub components: BTreeMap<String, ArchiveComponent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveComponent {
    pub archive: String,
    pub entries: usize,
    pub expanded_bytes: u64,
    pub payload: Vec<PayloadSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadSpec {
    pub path: String,
    pub kind: PayloadKind,
    pub size: u64,
    pub mode: u32,
    pub sha256: String,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadKind {
    File,
    Symlink,
}

pub static ARCHIVE_INVENTORY: LazyLock<ArchiveInventory> = LazyLock::new(|| {
    serde_json::from_str(include_str!("archive_inventory.json"))
        .expect("checked-in archive inventory")
});

#[must_use]
pub fn archive_component(spec: &ComponentSpec) -> Option<&'static ArchiveComponent> {
    spec.inventory_key
        .and_then(|key| ARCHIVE_INVENTORY.components.get(key))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareProfile {
    pub total_memory_bytes: Option<u64>,
}

#[must_use]
pub fn recommended_model(_profile: HardwareProfile) -> ComponentId {
    ComponentId::WhisperSmall
}

#[must_use]
pub fn plan(id: SetupPlanId, profile: HardwareProfile) -> Vec<ComponentId> {
    match id {
        SetupPlanId::Recommended => vec![
            ComponentId::WhisperRuntime,
            recommended_model(profile),
            ComponentId::SileroVad,
        ],
        SetupPlanId::Parakeet => vec![
            ComponentId::SherpaRuntime,
            ComponentId::ParakeetTdt06bV3Int8,
        ],
        SetupPlanId::WhisperBase => vec![
            ComponentId::WhisperRuntime,
            ComponentId::WhisperBaseQ51,
            ComponentId::SileroVad,
        ],
        SetupPlanId::WhisperSmall => vec![
            ComponentId::WhisperRuntime,
            ComponentId::WhisperSmall,
            ComponentId::SileroVad,
        ],
        SetupPlanId::WhisperLargeV3Turbo => vec![
            ComponentId::WhisperRuntime,
            ComponentId::WhisperLargeV3TurboQ50,
            ComponentId::SileroVad,
        ],
    }
}

#[must_use]
pub fn managed_platform_supported() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_closed_unique_and_sha256_pinned() {
        let ids: BTreeSet<_> = COMPONENTS.iter().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), COMPONENTS.len());
        for spec in COMPONENTS {
            assert!(spec.url.starts_with("https://"));
            assert_eq!(spec.artifact_sha256.len(), 64);
            assert!(spec
                .artifact_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
            assert!(spec.artifact_size > 0);
            assert!(spec.installed_bytes > 0);
        }
    }

    #[test]
    fn grounded_archive_inventory_matches_catalog() {
        assert_eq!(ARCHIVE_INVENTORY.schema_version, 1);
        for spec in COMPONENTS
            .iter()
            .filter(|spec| spec.inventory_key.is_some())
        {
            let inventory = archive_component(spec).unwrap();
            assert!(!inventory.archive.is_empty());
            assert!(inventory.entries >= inventory.payload.len());
            assert!(inventory.expanded_bytes >= spec.installed_bytes);
            assert!(!inventory.payload.is_empty());
            assert_eq!(
                inventory
                    .payload
                    .iter()
                    .map(|payload| payload.size)
                    .sum::<u64>(),
                spec.installed_bytes
            );
            for payload in &inventory.payload {
                assert_eq!(payload.sha256.len(), 64);
            }
        }
    }

    #[test]
    fn recommendation_uses_small_on_every_machine() {
        assert_eq!(
            recommended_model(HardwareProfile {
                total_memory_bytes: None
            }),
            ComponentId::WhisperSmall
        );
        assert_eq!(
            recommended_model(HardwareProfile {
                total_memory_bytes: Some(4 * 1024 * 1024 * 1024 - 1)
            }),
            ComponentId::WhisperSmall
        );
        assert_eq!(
            recommended_model(HardwareProfile {
                total_memory_bytes: Some(8 * 1024 * 1024 * 1024 - 512 * 1024 * 1024 - 1)
            }),
            ComponentId::WhisperSmall
        );
        assert_eq!(
            recommended_model(HardwareProfile {
                total_memory_bytes: Some(8 * 1024 * 1024 * 1024 - 512 * 1024 * 1024)
            }),
            ComponentId::WhisperSmall
        );
        assert_eq!(
            recommended_model(HardwareProfile {
                total_memory_bytes: Some(64 * 1024 * 1024 * 1024)
            }),
            ComponentId::WhisperSmall
        );
    }
}
