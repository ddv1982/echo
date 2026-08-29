use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const EXECUTION_ARTIFACT_PREFIX: &[u8] = b"echo-whisper-execution-artifact-v3\0";
const INFERENCE_CONTRACT_PREFIX: &[u8] = b"echo-whisper-inference-contract-v3\0";
const LOCAL_ENVIRONMENT_PREFIX: &[u8] = b"echo-whisper-local-environment-v3\0";
const PERFORMANCE_EVIDENCE_PREFIX: &[u8] = b"echo-whisper-performance-evidence-v3\0";
const RELEASE_BINDING_PREFIX: &[u8] = b"echo-whisper-release-binding-v3\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError(String);

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IdentityError {}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_canonical(value: &Value, output: &mut String) -> Result<(), IdentityError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) if value.is_i64() || value.is_u64() => {
            output.push_str(&value.to_string());
        }
        Value::Number(_) => {
            return Err(IdentityError(
                "canonical JSON does not allow floating-point numbers".to_string(),
            ));
        }
        Value::String(value) => output
            .push_str(&serde_json::to_string(value).map_err(|error| {
                IdentityError(format!("could not encode JSON string: {error}"))
            })?),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).map_err(|error| {
                    IdentityError(format!("could not encode JSON key: {error}"))
                })?);
                output.push(':');
                write_canonical(&values[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, IdentityError> {
    let mut output = String::new();
    write_canonical(value, &mut output)?;
    Ok(output.into_bytes())
}

fn derive_digest(prefix: &[u8], value: &Value) -> Result<String, IdentityError> {
    let canonical = canonical_json_bytes(value)?;
    let mut digest = Sha256::new();
    digest.update(prefix);
    digest.update(canonical);
    Ok(format!("{:x}", digest.finalize()))
}

macro_rules! content_id {
    ($name:ident, $prefix:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            fn derive(value: &Value) -> Result<Self, IdentityError> {
                Ok(Self(derive_digest($prefix, value)?))
            }

            pub fn parse(value: String) -> Result<Self, IdentityError> {
                if !valid_digest(&value) {
                    return Err(IdentityError(format!(
                        "{} is not a lowercase SHA-256 digest",
                        stringify!($name)
                    )));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

content_id!(ExecutionArtifactId, EXECUTION_ARTIFACT_PREFIX);
content_id!(InferenceContractId, INFERENCE_CONTRACT_PREFIX);
content_id!(LocalEnvironmentKey, LOCAL_ENVIRONMENT_PREFIX);
content_id!(PerformanceEvidenceId, PERFORMANCE_EVIDENCE_PREFIX);
content_id!(ReleaseBindingId, RELEASE_BINDING_PREFIX);

macro_rules! validated_string {
    ($name:ident, $validator:expr, $message:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: String) -> Result<Self, IdentityError> {
                if !($validator)(&value) {
                    return Err(IdentityError($message.to_string()));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

validated_string!(Sha256Digest, valid_digest, "invalid SHA-256 digest");
validated_string!(
    CommitDigest,
    |value: &str| value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
    "invalid commit digest"
);
validated_string!(
    UuidDigest,
    |value: &str| value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
    "invalid UUID digest"
);
validated_string!(
    SafeRelativePath,
    |value: &str| {
        !value.is_empty()
            && value.is_ascii()
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'/' | b'-')
            })
            && value
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
    },
    "invalid relative path"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionArtifactInput {
    pub schema_version: u32,
    pub runtime_artifact_id: Sha256Digest,
    pub runtime_identity_sha256: Sha256Digest,
    pub runtime_relative_path: SafeRelativePath,
    pub runtime_sha256: Sha256Digest,
    pub runtime_library_bindings: std::collections::BTreeMap<String, Sha256Digest>,
    pub probe_relative_path: SafeRelativePath,
    pub probe_sha256: Sha256Digest,
    pub build_receipt_sha256: Sha256Digest,
    pub reusable_inventory_sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceTuning {
    pub threads: u16,
    pub beam_size: u16,
    pub best_of: u16,
    pub no_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceRequestPolicy {
    pub language: String,
    pub prompt: String,
    pub hints: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceBehavior {
    pub launch_schema: u32,
    pub receipt_schema: u32,
    pub telemetry_schema: u32,
    pub recovery_schema: u32,
    pub projection_sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceContractInput {
    pub schema_version: u32,
    pub protocol: String,
    pub model_sha256: Sha256Digest,
    pub vad_sha256: Option<Sha256Digest>,
    pub tuning: InferenceTuning,
    pub request_policy: InferenceRequestPolicy,
    pub behavior: InferenceBehavior,
    pub claim_scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalEnvironmentInput {
    pub schema_version: u32,
    pub architecture: String,
    pub backend: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub api_version: u32,
    pub driver_version: u32,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: UuidDigest,
    #[serde(rename = "driverUUID")]
    pub driver_uuid: UuidDigest,
    #[serde(rename = "pipelineCacheUUID")]
    pub pipeline_cache_uuid: UuidDigest,
    pub drm_driver: String,
    pub icd_manifest_sha256: Sha256Digest,
    pub icd_library_sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerformanceEvidenceInput {
    pub schema_version: u32,
    pub execution_artifact_id: ExecutionArtifactId,
    pub inference_contract_id: InferenceContractId,
    pub local_environment_key: LocalEnvironmentKey,
    pub measurement_protocol: String,
    pub corpus_manifest_sha256: Sha256Digest,
    pub coverage_manifest_sha256: Sha256Digest,
    pub observation_bundle_sha256: Sha256Digest,
    pub cache_cycle_sha256: Sha256Digest,
    pub gate_policy_sha256: Sha256Digest,
    pub accepted_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageType {
    Deb,
    Rpm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseBindingInput {
    pub schema_version: u32,
    pub package_type: PackageType,
    pub version: String,
    pub echo_commit: CommitDigest,
    pub echo_binary_sha256: Sha256Digest,
    pub bundle_marker: PackageType,
    pub production_readiness: String,
    pub acceleration_set_sha256: Sha256Digest,
    pub execution_artifact_id: ExecutionArtifactId,
    pub allowed_inference_contract_ids: Vec<InferenceContractId>,
    pub allowed_performance_evidence_ids: Vec<PerformanceEvidenceId>,
    pub reusable_inventory_sha256: Sha256Digest,
}

fn require_schema(schema_version: u32, context: &str) -> Result<(), IdentityError> {
    if schema_version != 3 {
        return Err(IdentityError(format!("{context} schemaVersion is not 3")));
    }
    Ok(())
}

impl ExecutionArtifactId {
    pub fn of(input: &ExecutionArtifactInput) -> Result<Self, IdentityError> {
        require_schema(input.schema_version, "execution artifact")?;
        if input.runtime_library_bindings.is_empty()
            || input
                .runtime_library_bindings
                .keys()
                .any(|name| name.is_empty() || name.contains('/'))
        {
            return Err(IdentityError(
                "invalid runtime library bindings".to_string(),
            ));
        }
        Self::derive(&serde_json::to_value(input).map_err(|error| {
            IdentityError(format!("could not serialize execution artifact: {error}"))
        })?)
    }
}

impl InferenceContractId {
    pub fn of(input: &InferenceContractInput) -> Result<Self, IdentityError> {
        require_schema(input.schema_version, "inference contract")?;
        if input.protocol != "oneShotCli"
            || input.claim_scope != "product-stt-corpus-v1"
            || input.tuning.threads == 0
            || input.tuning.beam_size == 0
            || input.tuning.best_of == 0
            || input.request_policy.language != "pinned"
            || input.request_policy.prompt != "empty"
            || input.request_policy.hints != "qualifiedOnly"
            || input.behavior.launch_schema == 0
            || input.behavior.receipt_schema == 0
            || input.behavior.telemetry_schema == 0
            || input.behavior.recovery_schema == 0
        {
            return Err(IdentityError("invalid inference contract".to_string()));
        }
        Self::derive(&serde_json::to_value(input).map_err(|error| {
            IdentityError(format!("could not serialize inference contract: {error}"))
        })?)
    }
}

impl LocalEnvironmentKey {
    pub fn of(input: &LocalEnvironmentInput) -> Result<Self, IdentityError> {
        require_schema(input.schema_version, "local environment")?;
        if input.architecture != "x86_64"
            || input.backend != "vulkan"
            || input.vendor_id == 0
            || input.device_id == 0
            || input.api_version == 0
            || input.drm_driver.is_empty()
        {
            return Err(IdentityError("invalid local environment".to_string()));
        }
        Self::derive(&serde_json::to_value(input).map_err(|error| {
            IdentityError(format!("could not serialize local environment: {error}"))
        })?)
    }
}

impl PerformanceEvidenceId {
    pub fn of(input: &PerformanceEvidenceInput) -> Result<Self, IdentityError> {
        require_schema(input.schema_version, "performance evidence")?;
        if input.measurement_protocol != "paired-product-sweep-v2"
            || input.accepted_at == 0
            || input.expires_at <= input.accepted_at
            || input.expires_at - input.accepted_at > 30 * 24 * 60 * 60
        {
            return Err(IdentityError("invalid performance evidence".to_string()));
        }
        Self::derive(&serde_json::to_value(input).map_err(|error| {
            IdentityError(format!("could not serialize performance evidence: {error}"))
        })?)
    }
}

impl ReleaseBindingId {
    pub fn of(input: &ReleaseBindingInput) -> Result<Self, IdentityError> {
        require_schema(input.schema_version, "release binding")?;
        if input.version.is_empty()
            || input.package_type != input.bundle_marker
            || input.production_readiness != "proof-only-until-pr16.3"
            || input.allowed_inference_contract_ids.is_empty()
            || input.allowed_performance_evidence_ids.is_empty()
            || !input
                .allowed_inference_contract_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            || !input
                .allowed_performance_evidence_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        {
            return Err(IdentityError("invalid release binding".to_string()));
        }
        Self::derive(&serde_json::to_value(input).map_err(|error| {
            IdentityError(format!("could not serialize release binding: {error}"))
        })?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture() -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/whisper-v3-identities.json");
        serde_json::from_slice(&std::fs::read(path).expect("identity fixture"))
            .expect("valid identity fixture")
    }

    fn assert_case(
        fixture: &Value,
        name: &str,
        derive: fn(&Value) -> Result<String, IdentityError>,
    ) {
        let case = &fixture["cases"][name];
        let canonical = canonical_json_bytes(&case["input"]).expect("canonical JSON");
        assert_eq!(String::from_utf8(canonical).unwrap(), case["canonical"]);
        assert_eq!(derive(&case["input"]).unwrap(), case["id"]);
    }

    #[test]
    fn cross_language_identity_fixture_matches() {
        let fixture = fixture();
        assert_case(&fixture, "executionArtifact", |value| {
            Ok(ExecutionArtifactId::derive(value)?.to_string())
        });
        assert_case(&fixture, "inferenceContract", |value| {
            Ok(InferenceContractId::derive(value)?.to_string())
        });
        assert_case(&fixture, "localEnvironment", |value| {
            Ok(LocalEnvironmentKey::derive(value)?.to_string())
        });
        assert_case(&fixture, "performanceEvidence", |value| {
            Ok(PerformanceEvidenceId::derive(value)?.to_string())
        });
        assert_case(&fixture, "releaseBinding", |value| {
            Ok(ReleaseBindingId::derive(value)?.to_string())
        });
    }

    #[test]
    fn typed_identity_records_match_fixture() {
        let fixture = fixture();
        let cases = &fixture["cases"];
        let execution: ExecutionArtifactInput =
            serde_json::from_value(cases["executionArtifact"]["input"].clone()).unwrap();
        assert_eq!(
            ExecutionArtifactId::of(&execution).unwrap().as_str(),
            cases["executionArtifact"]["id"]
        );
        let inference: InferenceContractInput =
            serde_json::from_value(cases["inferenceContract"]["input"].clone()).unwrap();
        assert_eq!(
            InferenceContractId::of(&inference).unwrap().as_str(),
            cases["inferenceContract"]["id"]
        );
        let environment: LocalEnvironmentInput =
            serde_json::from_value(cases["localEnvironment"]["input"].clone()).unwrap();
        assert_eq!(
            LocalEnvironmentKey::of(&environment).unwrap().as_str(),
            cases["localEnvironment"]["id"]
        );
        let evidence: PerformanceEvidenceInput =
            serde_json::from_value(cases["performanceEvidence"]["input"].clone()).unwrap();
        assert_eq!(
            PerformanceEvidenceId::of(&evidence).unwrap().as_str(),
            cases["performanceEvidence"]["id"]
        );
        let binding: ReleaseBindingInput =
            serde_json::from_value(cases["releaseBinding"]["input"].clone()).unwrap();
        assert_eq!(
            ReleaseBindingId::of(&binding).unwrap().as_str(),
            cases["releaseBinding"]["id"]
        );
    }

    #[test]
    fn typed_records_reject_cross_domain_fields() {
        let fixture = fixture();
        let mut execution = fixture["cases"]["executionArtifact"]["input"].clone();
        execution["echoCommit"] = Value::String("a".repeat(40));
        assert!(serde_json::from_value::<ExecutionArtifactInput>(execution).is_err());

        let mut environment = fixture["cases"]["localEnvironment"]["input"].clone();
        environment["executionArtifactId"] = Value::String("a".repeat(64));
        assert!(serde_json::from_value::<LocalEnvironmentInput>(environment).is_err());
    }

    #[test]
    fn rust_and_python_validation_boundaries_match() {
        for path in [
            "./runtime",
            "runtime//whisper-cli",
            "runtime/",
            "runtime/$cli",
        ] {
            assert!(SafeRelativePath::parse(path.to_string()).is_err(), "{path}");
        }

        let fixture = fixture();
        let mut execution: ExecutionArtifactInput =
            serde_json::from_value(fixture["cases"]["executionArtifact"]["input"].clone()).unwrap();
        execution
            .runtime_library_bindings
            .insert(String::new(), Sha256Digest::parse("a".repeat(64)).unwrap());
        assert!(ExecutionArtifactId::of(&execution).is_err());

        let mut inference: InferenceContractInput =
            serde_json::from_value(fixture["cases"]["inferenceContract"]["input"].clone()).unwrap();
        inference.request_policy.language = "automatic".to_string();
        assert!(InferenceContractId::of(&inference).is_err());
        inference.request_policy.language = "pinned".to_string();
        inference.behavior.receipt_schema = 0;
        assert!(InferenceContractId::of(&inference).is_err());
    }

    #[test]
    fn canonical_json_rejects_floats() {
        assert!(canonical_json_bytes(&serde_json::json!({"latency": 1.5})).is_err());
    }

    #[test]
    fn ids_reject_noncanonical_digests() {
        assert!(ExecutionArtifactId::parse("A".repeat(64)).is_err());
        assert!(ExecutionArtifactId::parse("a".repeat(63)).is_err());
    }
}
