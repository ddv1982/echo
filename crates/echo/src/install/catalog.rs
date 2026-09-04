use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    WhisperRuntime,
    WhisperVulkanRuntime,
    WhisperBaseQ51,
    WhisperSmall,
    WhisperLargeV3TurboQ50,
    SileroVad,
    SherpaRuntime,
    ParakeetTdt06bV3Int8,
}

impl ComponentId {
    pub const ALL: [Self; 8] = [
        Self::WhisperRuntime,
        Self::WhisperVulkanRuntime,
        Self::WhisperBaseQ51,
        Self::WhisperSmall,
        Self::WhisperLargeV3TurboQ50,
        Self::SileroVad,
        Self::SherpaRuntime,
        Self::ParakeetTdt06bV3Int8,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WhisperRuntime => "whisper-runtime",
            Self::WhisperVulkanRuntime => "whisper-vulkan-runtime",
            Self::WhisperBaseQ51 => "whisper-base-q5-1",
            Self::WhisperSmall => "whisper-small",
            Self::WhisperLargeV3TurboQ50 => "whisper-large-v3-turbo-q5-0",
            Self::SileroVad => "silero-vad",
            Self::SherpaRuntime => "sherpa-runtime",
            Self::ParakeetTdt06bV3Int8 => "parakeet-tdt-06b-v3-int8",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Runtime,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseScope {
    Artifact,
    UpstreamSourceWithBundledDependencies,
}

impl ComponentKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Model => "model",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentProvenance {
    pub id: ComponentId,
    pub kind: ComponentKind,
    pub supplier: &'static str,
    pub distributor: &'static str,
    pub origin: &'static str,
    pub converter: Option<&'static str>,
    pub modifications: Option<&'static str>,
    pub license_id: &'static str,
    pub license_url: &'static str,
    pub license_scope: LicenseScope,
    pub bundled_dependency_license_id: Option<&'static str>,
    pub bundled_dependency_terms: Option<&'static str>,
    pub bundled_dependency_url: Option<&'static str>,
    pub provenance_note: Option<&'static str>,
    pub provenance_evidence_url: Option<&'static str>,
    pub homepage_url: &'static str,
}

pub const COMPONENT_PROVENANCE: &[ComponentProvenance] = &[
    ComponentProvenance {
        id: ComponentId::WhisperRuntime,
        kind: ComponentKind::Runtime,
        supplier: "ggml-org",
        distributor: "ggml-org",
        origin: "ggml-org",
        converter: None,
        modifications: None,
        license_id: "MIT",
        license_url: "https://github.com/ggml-org/whisper.cpp/blob/306c88f4d1286aec1bf96e544632897886af5501/LICENSE",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: None,
        provenance_evidence_url: None,
        homepage_url: "https://github.com/ggml-org/whisper.cpp/tree/306c88f4d1286aec1bf96e544632897886af5501",
    },
    ComponentProvenance {
        id: ComponentId::WhisperVulkanRuntime,
        kind: ComponentKind::Runtime,
        supplier: "Echo",
        distributor: "Echo",
        origin: "ggml-org",
        converter: None,
        modifications: Some("Vulkan-enabled build of upstream ggml-org whisper.cpp source."),
        license_id: "MIT",
        license_url: "https://github.com/ggml-org/whisper.cpp/blob/306c88f4d1286aec1bf96e544632897886af5501/LICENSE",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: None,
        provenance_evidence_url: None,
        homepage_url: "https://github.com/ggml-org/whisper.cpp/tree/306c88f4d1286aec1bf96e544632897886af5501",
    },
    ComponentProvenance {
        id: ComponentId::WhisperBaseQ51,
        kind: ComponentKind::Model,
        supplier: "ggerganov",
        distributor: "ggerganov",
        origin: "OpenAI",
        converter: Some("ggerganov"),
        modifications: Some("GGML conversion and Q5_1 quantization of OpenAI Whisper model weights."),
        license_id: "MIT",
        license_url: "https://huggingface.co/ggerganov/whisper.cpp/blob/5359861c739e955e79d9a303bcbc70fb988958b1/README.md",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: None,
        provenance_evidence_url: None,
        homepage_url: "https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1",
    },
    ComponentProvenance {
        id: ComponentId::WhisperSmall,
        kind: ComponentKind::Model,
        supplier: "ggerganov",
        distributor: "ggerganov",
        origin: "OpenAI",
        converter: Some("ggerganov"),
        modifications: Some("GGML conversion of OpenAI Whisper model weights."),
        license_id: "MIT",
        license_url: "https://huggingface.co/ggerganov/whisper.cpp/blob/5359861c739e955e79d9a303bcbc70fb988958b1/README.md",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: None,
        provenance_evidence_url: None,
        homepage_url: "https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1",
    },
    ComponentProvenance {
        id: ComponentId::WhisperLargeV3TurboQ50,
        kind: ComponentKind::Model,
        supplier: "ggerganov",
        distributor: "ggerganov",
        origin: "OpenAI",
        converter: Some("ggerganov"),
        modifications: Some("GGML conversion and Q5_0 quantization of OpenAI Whisper model weights."),
        license_id: "MIT",
        license_url: "https://huggingface.co/ggerganov/whisper.cpp/blob/5359861c739e955e79d9a303bcbc70fb988958b1/README.md",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: None,
        provenance_evidence_url: None,
        homepage_url: "https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1",
    },
    ComponentProvenance {
        id: ComponentId::SileroVad,
        kind: ComponentKind::Model,
        supplier: "ggml-org",
        distributor: "ggml-org",
        origin: "Silero Team",
        converter: Some("ggml-org"),
        modifications: Some("GGML conversion of the Silero VAD v6.2 model."),
        license_id: "MIT",
        license_url: "https://github.com/snakers4/silero-vad/blob/v6.2/LICENSE",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: None,
        provenance_evidence_url: None,
        homepage_url: "https://huggingface.co/ggml-org/whisper-vad/tree/9ffd54a1e1ee413ddf265af9913beaf518d1639b",
    },
    ComponentProvenance {
        id: ComponentId::SherpaRuntime,
        kind: ComponentKind::Runtime,
        supplier: "k2-fsa",
        distributor: "k2-fsa",
        origin: "k2-fsa",
        converter: None,
        modifications: Some("Static Linux x86_64 no-TTS build incorporating third-party dependencies."),
        license_id: "Apache-2.0",
        license_url: "https://github.com/k2-fsa/sherpa-onnx/blob/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/LICENSE",
        license_scope: LicenseScope::UpstreamSourceWithBundledDependencies,
        bundled_dependency_license_id: Some("MIT"),
        bundled_dependency_terms: Some("Bundled dependencies, including MIT-licensed ONNX Runtime, retain their own license terms; the archive is not exclusively Apache-2.0."),
        bundled_dependency_url: Some("https://github.com/k2-fsa/sherpa-onnx/tree/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/cmake"),
        provenance_note: Some("Evidence scope: the v1.13.6 tag resolves to commit 1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911; the tag-triggered .github/workflows/linux.yaml at that commit builds, names, and uploads sherpa-onnx-v1.13.6-linux-x64-static-no-tts.tar.bz2; the catalog release-asset SHA-256 digest identifies the exact downloaded bytes; pinned CMake declares static ONNX Runtime 1.27.1, while bundled dependencies retain their own terms."),
        provenance_evidence_url: Some("https://github.com/k2-fsa/sherpa-onnx/blob/1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911/.github/workflows/linux.yaml"),
        homepage_url: "https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.6",
    },
    ComponentProvenance {
        id: ComponentId::ParakeetTdt06bV3Int8,
        kind: ComponentKind::Model,
        supplier: "k2-fsa",
        distributor: "k2-fsa",
        origin: "NVIDIA",
        converter: Some("k2-fsa"),
        modifications: Some("ONNX conversion and INT8 quantization of NVIDIA Parakeet model weights."),
        license_id: "CC-BY-4.0",
        license_url: "https://creativecommons.org/licenses/by/4.0/legalcode",
        license_scope: LicenseScope::Artifact,
        bundled_dependency_license_id: None,
        bundled_dependency_terms: None,
        bundled_dependency_url: None,
        provenance_note: Some("Evidence scope: merged k2-fsa PR 2500 and its export record associate sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2 with NVIDIA origin, ONNX conversion, and INT8 quantization. Its conversion script references an unpinned NVIDIA model, so Hugging Face revision 575de92b31b2f60855bca9b70968bde5afb069ba is an attribution and license snapshot only; it does not independently attest exact source-revision byte lineage."),
        provenance_evidence_url: Some("https://github.com/k2-fsa/sherpa-onnx/pull/2500"),
        homepage_url: "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/blob/575de92b31b2f60855bca9b70968bde5afb069ba/README.md",
    },
];

#[must_use]
pub fn component_provenance(id: ComponentId) -> &'static ComponentProvenance {
    COMPONENT_PROVENANCE
        .iter()
        .find(|provenance| provenance.id == id)
        .expect("every ComponentId has provenance")
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
        id: ComponentId::WhisperVulkanRuntime,
        label: "Whisper GPU runtime",
        version: "1.9.2-vulkan",
        url: "https://github.com/ddv1982/echo/releases/download/whisper-vulkan-runtime-1.9.2/echo-whisper-vulkan-runtime.tar.gz",
        artifact_name: "echo-whisper-vulkan-runtime.tar.gz",
        artifact_size: 19_831_459,
        artifact_sha256: "3afbd9e54959392ff60c27c48f2561f696f7ac45a25de09fe24157b2af45140f",
        installed_bytes: 59_816_721,
        format: ArtifactFormat::TarGzip,
        inventory_key: Some("whisper-vulkan-runtime"),
    },
    ComponentSpec {
        id: ComponentId::WhisperBaseQ51,
        label: "Base multilingual Q5_1",
        version: "base-q5_1",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base-q5_1.bin",
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
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin",
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
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
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
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/9ffd54a1e1ee413ddf265af9913beaf518d1639b/ggml-silero-v6.2.0.bin",
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

#[must_use]
pub fn recommended_model() -> ComponentId {
    ComponentId::WhisperSmall
}

#[must_use]
pub fn plan(id: SetupPlanId) -> Vec<ComponentId> {
    match id {
        SetupPlanId::Recommended => vec![
            ComponentId::WhisperRuntime,
            recommended_model(),
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
        assert_eq!(ids, ComponentId::ALL.into_iter().collect::<BTreeSet<_>>());
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
    fn provenance_is_complete_and_uses_explicit_stable_terms() {
        let catalog_ids: BTreeSet<_> = COMPONENTS.iter().map(|spec| spec.id).collect();
        let provenance_ids: BTreeSet<_> = COMPONENT_PROVENANCE
            .iter()
            .map(|provenance| provenance.id)
            .collect();
        assert_eq!(provenance_ids.len(), COMPONENT_PROVENANCE.len());
        assert_eq!(provenance_ids, catalog_ids);
        assert_eq!(
            provenance_ids,
            ComponentId::ALL.into_iter().collect::<BTreeSet<_>>()
        );

        let kinds: BTreeSet<_> = COMPONENT_PROVENANCE
            .iter()
            .map(|provenance| provenance.kind.as_str())
            .collect();
        assert_eq!(kinds, BTreeSet::from(["model", "runtime"]));
        for provenance in COMPONENT_PROVENANCE {
            assert!(!provenance.kind.as_str().is_empty());
            assert!(!provenance.supplier.is_empty());
            assert!(!provenance.license_id.is_empty());
            assert!(provenance.license_url.starts_with("https://"));
            assert!(provenance.homepage_url.starts_with("https://"));
            if let Some(evidence_url) = provenance.provenance_evidence_url {
                assert!(evidence_url.starts_with("https://"));
            }
            assert_eq!(
                provenance.provenance_note.is_some(),
                provenance.provenance_evidence_url.is_some()
            );
            assert!(matches!(
                provenance.kind,
                ComponentKind::Runtime | ComponentKind::Model
            ));
            let expected = match provenance.id {
                ComponentId::WhisperRuntime | ComponentId::WhisperVulkanRuntime => {
                    (ComponentKind::Runtime, "MIT")
                }
                ComponentId::WhisperBaseQ51
                | ComponentId::WhisperSmall
                | ComponentId::WhisperLargeV3TurboQ50
                | ComponentId::SileroVad => (ComponentKind::Model, "MIT"),
                ComponentId::SherpaRuntime => (ComponentKind::Runtime, "Apache-2.0"),
                ComponentId::ParakeetTdt06bV3Int8 => (ComponentKind::Model, "CC-BY-4.0"),
            };
            assert_eq!((provenance.kind, provenance.license_id), expected);
        }
        let evidence_ids: BTreeSet<_> = COMPONENT_PROVENANCE
            .iter()
            .filter(|provenance| provenance.provenance_note.is_some())
            .map(|provenance| provenance.id)
            .collect();
        assert_eq!(
            evidence_ids,
            BTreeSet::from([
                ComponentId::SherpaRuntime,
                ComponentId::ParakeetTdt06bV3Int8,
            ])
        );
    }

    #[test]
    fn hugging_face_artifacts_use_verified_immutable_revisions() {
        for spec in COMPONENTS
            .iter()
            .filter(|spec| spec.url.contains("huggingface.co"))
        {
            assert!(!spec.url.contains("/resolve/main/"), "{}", spec.url);
            let expected_revision = if spec.url.contains("/whisper.cpp/") {
                "5359861c739e955e79d9a303bcbc70fb988958b1"
            } else if spec.url.contains("/whisper-vad/") {
                "9ffd54a1e1ee413ddf265af9913beaf518d1639b"
            } else {
                panic!("unrecognized Hugging Face repository: {}", spec.url);
            };
            assert!(spec.url.contains(&format!("/resolve/{expected_revision}/")));
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
    fn recommendation_and_recommended_plan_use_small() {
        assert_eq!(recommended_model(), ComponentId::WhisperSmall);
        assert!(plan(SetupPlanId::Recommended).contains(&ComponentId::WhisperSmall));
    }
}
