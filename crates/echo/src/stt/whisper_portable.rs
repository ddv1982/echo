use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::whisper_identity::{
    canonical_json_bytes, CommitDigest, ExecutionArtifactId, ExecutionArtifactInput,
    InferenceContractId, InferenceContractInput, LocalEnvironmentInput, LocalEnvironmentKey,
    PackageType, PerformanceEvidenceId, ReleaseBindingId, Sha256Digest,
};

const PORTABLE_SCHEMA_VERSION: u32 = 1;
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_EVIDENCE_LIFETIME_SECS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecutionArtifactRecord {
    pub id: ExecutionArtifactId,
    pub value: ExecutionArtifactInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InferenceContractRecord {
    pub id: InferenceContractId,
    pub value: InferenceContractInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableSelection {
    pub schema_version: u32,
    pub execution_artifact: ExecutionArtifactRecord,
    pub inference_contracts: Vec<InferenceContractRecord>,
}

impl PortableSelection {
    pub(crate) fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        let selection: Self = parse_document(raw, "portable selection")?;
        selection.validate()?;
        Ok(selection)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != PORTABLE_SCHEMA_VERSION
            || self.execution_artifact.id
                != ExecutionArtifactId::of(&self.execution_artifact.value)
                    .map_err(|error| error.to_string())?
            || self.inference_contracts.is_empty()
            || !self
                .inference_contracts
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
            || !self
                .execution_artifact
                .value
                .runtime_relative_path
                .as_str()
                .starts_with("runtime/")
            || !self
                .execution_artifact
                .value
                .probe_relative_path
                .as_str()
                .starts_with("runtime/")
        {
            return Err("invalid portable Whisper selection".to_string());
        }
        for record in &self.inference_contracts {
            if record.id
                != InferenceContractId::of(&record.value).map_err(|error| error.to_string())?
            {
                return Err("portable inference contract ID differs".to_string());
            }
        }
        Ok(())
    }

    fn contract_ids(&self) -> Vec<InferenceContractId> {
        self.inference_contracts
            .iter()
            .map(|record| record.id.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LegacyExactRecord {
    pub performance_evidence_id: PerformanceEvidenceId,
    pub inference_contract_id: InferenceContractId,
    pub local_environment_key: LocalEnvironmentKey,
    pub local_environment: LocalEnvironmentInput,
    pub accepted_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LegacyExactIndex {
    pub schema_version: u32,
    pub execution_artifact_id: ExecutionArtifactId,
    pub records: Vec<LegacyExactRecord>,
}

impl LegacyExactIndex {
    pub(crate) fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        let index: Self = parse_document(raw, "legacy exact index")?;
        index.validate()?;
        Ok(index)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != PORTABLE_SCHEMA_VERSION
            || self.records.is_empty()
            || !self
                .records
                .windows(2)
                .all(|pair| pair[0].performance_evidence_id < pair[1].performance_evidence_id)
        {
            return Err("invalid legacy exact Whisper index".to_string());
        }
        for record in &self.records {
            if record.local_environment_key
                != LocalEnvironmentKey::of(&record.local_environment)
                    .map_err(|error| error.to_string())?
                || record.accepted_at == 0
                || record
                    .expires_at
                    .checked_sub(record.accepted_at)
                    .is_none_or(|lifetime| lifetime == 0 || lifetime > MAX_EVIDENCE_LIFETIME_SECS)
            {
                return Err("invalid legacy exact Whisper record".to_string());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableSelectionBinding {
    pub schema_version: u32,
    pub package_type: PackageType,
    pub version: String,
    pub echo_commit: CommitDigest,
    pub echo_binary_sha256: Sha256Digest,
    pub portable_selection_sha256: Sha256Digest,
    pub legacy_exact_index_sha256: Sha256Digest,
    pub execution_artifact_id: ExecutionArtifactId,
    pub allowed_inference_contract_ids: Vec<InferenceContractId>,
    pub source_release_binding_id: ReleaseBindingId,
    pub production_readiness: String,
}

impl PortableSelectionBinding {
    pub(crate) fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        let binding: Self = parse_document(raw, "portable selection binding")?;
        binding.validate()?;
        Ok(binding)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != PORTABLE_SCHEMA_VERSION
            || self.version.is_empty()
            || self.production_readiness != "local-selection-proof-only-until-pr16.4"
            || self.allowed_inference_contract_ids.is_empty()
            || !self
                .allowed_inference_contract_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        {
            return Err("invalid portable selection binding".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortableSelectionPackage {
    pub selection: PortableSelection,
    pub legacy_exact: LegacyExactIndex,
    pub binding: PortableSelectionBinding,
}

impl PortableSelectionPackage {
    pub(crate) fn from_bytes(
        selection_raw: &[u8],
        legacy_raw: &[u8],
        binding_raw: &[u8],
    ) -> Result<Self, String> {
        let selection = PortableSelection::from_bytes(selection_raw)?;
        let legacy_exact = LegacyExactIndex::from_bytes(legacy_raw)?;
        let binding = PortableSelectionBinding::from_bytes(binding_raw)?;
        if binding.portable_selection_sha256 != canonical_digest(&selection)?
            || binding.legacy_exact_index_sha256 != canonical_digest(&legacy_exact)?
            || binding.execution_artifact_id != selection.execution_artifact.id
            || legacy_exact.execution_artifact_id != selection.execution_artifact.id
            || binding.allowed_inference_contract_ids != selection.contract_ids()
            || legacy_exact.records.iter().any(|record| {
                !binding
                    .allowed_inference_contract_ids
                    .contains(&record.inference_contract_id)
            })
        {
            return Err("portable selection package binding differs".to_string());
        }
        Ok(Self {
            selection,
            legacy_exact,
            binding,
        })
    }
}

fn parse_document<T: for<'de> Deserialize<'de>>(raw: &[u8], label: &str) -> Result<T, String> {
    if raw.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("{label} exceeds 1 MiB"));
    }
    serde_json::from_slice(raw).map_err(|error| error.to_string())
}

fn canonical_digest<T: Serialize>(value: &T) -> Result<Sha256Digest, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    digest.update(canonical_json_bytes(&value).map_err(|error| error.to_string())?);
    Sha256Digest::parse(format!("{:x}", digest.finalize())).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{json, Value};

    use super::*;

    fn fixture() -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/whisper-v3-identities.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn documents() -> (
        PortableSelection,
        LegacyExactIndex,
        PortableSelectionBinding,
    ) {
        let fixture = fixture();
        let cases = &fixture["cases"];
        let execution = ExecutionArtifactRecord {
            id: serde_json::from_value(cases["executionArtifact"]["id"].clone()).unwrap(),
            value: serde_json::from_value(cases["executionArtifact"]["input"].clone()).unwrap(),
        };
        let contract = InferenceContractRecord {
            id: serde_json::from_value(cases["inferenceContract"]["id"].clone()).unwrap(),
            value: serde_json::from_value(cases["inferenceContract"]["input"].clone()).unwrap(),
        };
        let environment: LocalEnvironmentInput =
            serde_json::from_value(cases["localEnvironment"]["input"].clone()).unwrap();
        let selection = PortableSelection {
            schema_version: 1,
            execution_artifact: execution,
            inference_contracts: vec![contract],
        };
        let legacy = LegacyExactIndex {
            schema_version: 1,
            execution_artifact_id: selection.execution_artifact.id.clone(),
            records: vec![LegacyExactRecord {
                performance_evidence_id: serde_json::from_value(
                    cases["performanceEvidence"]["id"].clone(),
                )
                .unwrap(),
                inference_contract_id: selection.inference_contracts[0].id.clone(),
                local_environment_key: LocalEnvironmentKey::of(&environment).unwrap(),
                local_environment: environment,
                accepted_at: 1_700_000_000,
                expires_at: 1_700_086_400,
            }],
        };
        let binding = PortableSelectionBinding {
            schema_version: 1,
            package_type: PackageType::Deb,
            version: "0.12.5".to_string(),
            echo_commit: CommitDigest::parse("a".repeat(40)).unwrap(),
            echo_binary_sha256: Sha256Digest::parse("b".repeat(64)).unwrap(),
            portable_selection_sha256: canonical_digest(&selection).unwrap(),
            legacy_exact_index_sha256: canonical_digest(&legacy).unwrap(),
            execution_artifact_id: selection.execution_artifact.id.clone(),
            allowed_inference_contract_ids: selection.contract_ids(),
            source_release_binding_id: serde_json::from_value(
                cases["releaseBinding"]["id"].clone(),
            )
            .unwrap(),
            production_readiness: "local-selection-proof-only-until-pr16.4".to_string(),
        };
        (selection, legacy, binding)
    }

    #[test]
    fn strict_portable_package_round_trips() {
        let (selection, legacy, binding) = documents();
        let package = PortableSelectionPackage::from_bytes(
            &serde_json::to_vec(&selection).unwrap(),
            &serde_json::to_vec(&legacy).unwrap(),
            &serde_json::to_vec(&binding).unwrap(),
        )
        .unwrap();
        assert_eq!(package.selection, selection);
        assert_eq!(package.legacy_exact, legacy);
        assert_eq!(package.binding, binding);
    }

    #[test]
    fn portable_reader_rejects_unknown_duplicate_and_host_path_fields() {
        let (selection, _, _) = documents();
        let mut value = serde_json::to_value(&selection).unwrap();
        value["localEnvironments"] = json!([]);
        assert!(PortableSelection::from_bytes(&serde_json::to_vec(&value).unwrap()).is_err());

        let raw = serde_json::to_string(&selection).unwrap();
        let duplicate = raw.replacen(
            "{\"schemaVersion\":1,",
            "{\"schemaVersion\":1,\"schemaVersion\":1,",
            1,
        );
        assert!(PortableSelection::from_bytes(duplicate.as_bytes()).is_err());

        let raw = serde_json::to_string(&selection).unwrap();
        let duplicate_binding = raw.replacen(
            "\"runtimeLibraryBindings\":{",
            &format!(
                "\"runtimeLibraryBindings\":{{\"libdup.so\":\"{}\",\"libdup.so\":\"{}\",",
                "c".repeat(64),
                "d".repeat(64)
            ),
            1,
        );
        assert!(PortableSelection::from_bytes(duplicate_binding.as_bytes()).is_err());
    }

    #[test]
    fn binding_rejects_changed_selection_or_legacy_index() {
        let (mut selection, legacy, binding) = documents();
        selection.inference_contracts[0].value.tuning.threads += 1;
        assert!(PortableSelectionPackage::from_bytes(
            &serde_json::to_vec(&selection).unwrap(),
            &serde_json::to_vec(&legacy).unwrap(),
            &serde_json::to_vec(&binding).unwrap(),
        )
        .is_err());

        let (selection, mut legacy, binding) = documents();
        legacy.records[0].expires_at += 1;
        assert!(PortableSelectionPackage::from_bytes(
            &serde_json::to_vec(&selection).unwrap(),
            &serde_json::to_vec(&legacy).unwrap(),
            &serde_json::to_vec(&binding).unwrap(),
        )
        .is_err());
    }
}
