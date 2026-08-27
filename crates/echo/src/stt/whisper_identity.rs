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
            pub fn derive(value: &Value) -> Result<Self, IdentityError> {
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
    fn canonical_json_rejects_floats() {
        assert!(canonical_json_bytes(&serde_json::json!({"latency": 1.5})).is_err());
    }

    #[test]
    fn ids_reject_noncanonical_digests() {
        assert!(ExecutionArtifactId::parse("A".repeat(64)).is_err());
        assert!(ExecutionArtifactId::parse("a".repeat(63)).is_err());
    }
}
