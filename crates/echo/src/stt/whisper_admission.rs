use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_ADMISSION_LIFETIME_SECS: u64 = 30 * 24 * 60 * 60;
pub const MAX_QUARANTINE_LIFETIME_SECS: u64 = 24 * 60 * 60;

const ADMISSION_SCHEMA_VERSION: u32 = 1;
const IDENTITY_SCHEMA_VERSION: u32 = 1;
const QUARANTINE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionTuning {
    pub threads: u16,
    pub beam_size: u8,
    pub best_of: u8,
    pub no_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionDeviceIdentity {
    pub backend: String,
    pub selected_index: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub api_version: u32,
    pub driver_version: u32,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    #[serde(rename = "driverUUID")]
    pub driver_uuid: String,
    #[serde(rename = "pipelineCacheUUID")]
    pub pipeline_cache_uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionIdentity {
    pub schema_version: u32,
    pub echo_commit: String,
    pub echo_binary_sha256: String,
    pub runtime_identity_sha256: String,
    pub model_sha256: String,
    pub vad_sha256: Option<String>,
    pub protocol: String,
    pub tuning: AdmissionTuning,
    pub language_policy: String,
    pub prompt_policy: String,
    pub device: AdmissionDeviceIdentity,
    pub driver_sha256: String,
    pub icd_manifest_sha256: String,
    pub icd_library_sha256: String,
    pub launch_contract_schema: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdmissionIdentityKey(String);

impl AdmissionIdentityKey {
    #[must_use]
    pub fn for_identity(identity: &AdmissionIdentity) -> Self {
        let canonical = serde_json::to_vec(identity)
            .expect("serializing an AdmissionIdentity into JSON cannot fail");
        let mut hasher = Sha256::new();
        hasher.update(b"echo-whisper-admission-identity-v1\0");
        hasher.update(canonical);
        Self(format!("{:x}", hasher.finalize()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionGates {
    pub complete_pairs: bool,
    pub pair_integrity: bool,
    pub sample_size: bool,
    pub backend_truth: bool,
    pub identity_match: bool,
    pub hardware_device: bool,
    pub median_reduction: bool,
    pub median_speedup: bool,
    pub p95_improved: bool,
    pub per_language_quality: bool,
    pub no_new_hallucinations: bool,
    pub receipt_consistency: bool,
    pub coverage_complete: bool,
    pub cache_evidence: bool,
    pub reset_evidence: bool,
    pub driver_icd_identity: bool,
    pub clean_child_environment: bool,
    pub exact_runtime: bool,
}

impl AdmissionGates {
    fn all_passed(&self) -> bool {
        self.complete_pairs
            && self.pair_integrity
            && self.sample_size
            && self.backend_truth
            && self.identity_match
            && self.hardware_device
            && self.median_reduction
            && self.median_speedup
            && self.p95_improved
            && self.per_language_quality
            && self.no_new_hallucinations
            && self.receipt_consistency
            && self.coverage_complete
            && self.cache_evidence
            && self.reset_evidence
            && self.driver_icd_identity
            && self.clean_child_environment
            && self.exact_runtime
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdmissionVerdict {
    Passed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionRecord {
    pub schema_version: u32,
    pub identity: AdmissionIdentity,
    pub identity_key: AdmissionIdentityKey,
    pub evidence_sha256: String,
    pub gates: AdmissionGates,
    pub verdict: AdmissionVerdict,
    pub accepted_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineRecord {
    pub schema_version: u32,
    pub identity: AdmissionIdentity,
    pub identity_key: AdmissionIdentityKey,
    pub reason: String,
    pub failure_count: u32,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionState {
    Unknown,
    Passed,
    Stopped,
    Quarantined,
}

#[must_use]
pub fn admission_state_from_bytes(
    identity: &AdmissionIdentity,
    admission_bytes: Option<&[u8]>,
    quarantine_bytes: Option<&[u8]>,
    now: u64,
) -> AdmissionState {
    if identity.schema_version != IDENTITY_SCHEMA_VERSION || !identity_is_valid(identity) {
        return AdmissionState::Unknown;
    }

    if let Some(bytes) = quarantine_bytes {
        let Ok(quarantine) = serde_json::from_slice::<QuarantineRecord>(bytes) else {
            return AdmissionState::Unknown;
        };
        if quarantine_applies(&quarantine, identity, now) {
            return AdmissionState::Quarantined;
        }
    }

    let Some(bytes) = admission_bytes else {
        return AdmissionState::Unknown;
    };
    let Ok(record) = serde_json::from_slice::<AdmissionRecord>(bytes) else {
        return AdmissionState::Unknown;
    };
    if !admission_applies(&record, identity, now) {
        return AdmissionState::Unknown;
    }
    match record.verdict {
        AdmissionVerdict::Passed if record.gates.all_passed() => AdmissionState::Passed,
        AdmissionVerdict::Passed | AdmissionVerdict::Stopped => AdmissionState::Stopped,
    }
}

fn admission_applies(record: &AdmissionRecord, identity: &AdmissionIdentity, now: u64) -> bool {
    record.schema_version == ADMISSION_SCHEMA_VERSION
        && record.identity == *identity
        && record.identity_key == AdmissionIdentityKey::for_identity(identity)
        && is_sha256(&record.evidence_sha256)
        && interval_is_current(
            record.accepted_at,
            record.expires_at,
            now,
            MAX_ADMISSION_LIFETIME_SECS,
        )
}

fn quarantine_applies(record: &QuarantineRecord, identity: &AdmissionIdentity, now: u64) -> bool {
    record.schema_version == QUARANTINE_SCHEMA_VERSION
        && record.identity == *identity
        && record.identity_key == AdmissionIdentityKey::for_identity(identity)
        && record.failure_count > 0
        && !record.reason.trim().is_empty()
        && interval_is_current(
            record.created_at,
            record.expires_at,
            now,
            MAX_QUARANTINE_LIFETIME_SECS,
        )
}

fn interval_is_current(start: u64, end: u64, now: u64, maximum: u64) -> bool {
    start <= now
        && now < end
        && end
            .checked_sub(start)
            .is_some_and(|lifetime| lifetime <= maximum)
}

fn identity_is_valid(identity: &AdmissionIdentity) -> bool {
    [
        identity.echo_binary_sha256.as_str(),
        identity.runtime_identity_sha256.as_str(),
        identity.model_sha256.as_str(),
        identity.driver_sha256.as_str(),
        identity.icd_manifest_sha256.as_str(),
        identity.icd_library_sha256.as_str(),
    ]
    .into_iter()
    .all(is_sha256)
        && identity.vad_sha256.as_deref().is_none_or(is_sha256)
        && is_lower_hex(&identity.echo_commit, 40)
        && identity.protocol == "oneShotCli"
        && identity.language_policy == "autoOrPinned"
        && identity.prompt_policy == "recognitionHints"
        && identity.tuning.threads > 0
        && identity.tuning.beam_size > 0
        && identity.tuning.best_of > 0
        && identity.device.backend == "vulkan"
        && identity.device.vendor_id > 0
        && identity.device.device_id > 0
        && [
            identity.device.device_uuid.as_str(),
            identity.device.driver_uuid.as_str(),
            identity.device.pipeline_cache_uuid.as_str(),
        ]
        .into_iter()
        .all(|value| is_lower_hex(value, 32) && !value.bytes().all(|byte| byte == b'0'))
        && identity.launch_contract_schema > 0
}

fn is_sha256(value: &str) -> bool {
    is_lower_hex(value, 64)
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 2_000_000_000;

    fn identity(seed: char) -> AdmissionIdentity {
        AdmissionIdentity {
            schema_version: 1,
            echo_commit: "4".repeat(40),
            echo_binary_sha256: seed.to_string().repeat(64),
            runtime_identity_sha256: "b".repeat(64),
            model_sha256: "c".repeat(64),
            vad_sha256: Some("d".repeat(64)),
            protocol: "oneShotCli".to_string(),
            tuning: AdmissionTuning {
                threads: 4,
                beam_size: 3,
                best_of: 5,
                no_fallback: false,
            },
            language_policy: "autoOrPinned".to_string(),
            prompt_policy: "recognitionHints".to_string(),
            device: AdmissionDeviceIdentity {
                backend: "vulkan".to_string(),
                selected_index: 0,
                vendor_id: 0x8086,
                device_id: 0x46a6,
                api_version: 4_211_006,
                driver_version: 104_865_800,
                device_uuid: "1".repeat(32),
                driver_uuid: "2".repeat(32),
                pipeline_cache_uuid: "3".repeat(32),
            },
            driver_sha256: "f".repeat(64),
            icd_manifest_sha256: "1".repeat(64),
            icd_library_sha256: "2".repeat(64),
            launch_contract_schema: 1,
        }
    }

    fn passed_gates() -> AdmissionGates {
        AdmissionGates {
            complete_pairs: true,
            pair_integrity: true,
            sample_size: true,
            backend_truth: true,
            identity_match: true,
            hardware_device: true,
            median_reduction: true,
            median_speedup: true,
            p95_improved: true,
            per_language_quality: true,
            no_new_hallucinations: true,
            receipt_consistency: true,
            coverage_complete: true,
            cache_evidence: true,
            reset_evidence: true,
            driver_icd_identity: true,
            clean_child_environment: true,
            exact_runtime: true,
        }
    }

    fn admission(identity: &AdmissionIdentity) -> AdmissionRecord {
        AdmissionRecord {
            schema_version: 1,
            identity: identity.clone(),
            identity_key: AdmissionIdentityKey::for_identity(identity),
            evidence_sha256: "a".repeat(64),
            gates: passed_gates(),
            verdict: AdmissionVerdict::Passed,
            accepted_at: NOW - 60,
            expires_at: NOW + 60,
        }
    }

    fn quarantine(identity: &AdmissionIdentity) -> QuarantineRecord {
        QuarantineRecord {
            schema_version: 1,
            identity: identity.clone(),
            identity_key: AdmissionIdentityKey::for_identity(identity),
            reason: "runtime receipt mismatch".to_string(),
            failure_count: 1,
            created_at: NOW - 60,
            expires_at: NOW + 60,
        }
    }

    fn bytes<T: Serialize>(value: &T) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    #[test]
    fn admission_states_are_fail_closed_and_exact() {
        struct Case {
            name: &'static str,
            admission: Option<AdmissionRecord>,
            quarantine: Option<QuarantineRecord>,
            now: u64,
            expected: AdmissionState,
        }

        let current = identity('a');
        let changed = identity('9');
        let mut stopped = admission(&current);
        stopped.verdict = AdmissionVerdict::Stopped;
        let mut expired = admission(&current);
        expired.expires_at = NOW;
        let changed_record = admission(&changed);
        let mut false_gate = admission(&current);
        false_gate.gates.hardware_device = false;
        let cases = [
            Case {
                name: "missing",
                admission: None,
                quarantine: None,
                now: NOW,
                expected: AdmissionState::Unknown,
            },
            Case {
                name: "passed",
                admission: Some(admission(&current)),
                quarantine: None,
                now: NOW,
                expected: AdmissionState::Passed,
            },
            Case {
                name: "stopped",
                admission: Some(stopped),
                quarantine: None,
                now: NOW,
                expected: AdmissionState::Stopped,
            },
            Case {
                name: "expired",
                admission: Some(expired),
                quarantine: None,
                now: NOW,
                expected: AdmissionState::Unknown,
            },
            Case {
                name: "changed",
                admission: Some(changed_record),
                quarantine: None,
                now: NOW,
                expected: AdmissionState::Unknown,
            },
            Case {
                name: "false gate",
                admission: Some(false_gate),
                quarantine: None,
                now: NOW,
                expected: AdmissionState::Stopped,
            },
            Case {
                name: "exact quarantine",
                admission: Some(admission(&current)),
                quarantine: Some(quarantine(&current)),
                now: NOW,
                expected: AdmissionState::Quarantined,
            },
            Case {
                name: "other identity quarantine",
                admission: Some(admission(&current)),
                quarantine: Some(quarantine(&changed)),
                now: NOW,
                expected: AdmissionState::Passed,
            },
        ];

        for case in cases {
            let admission_bytes = case.admission.as_ref().map(bytes);
            let quarantine_bytes = case.quarantine.as_ref().map(bytes);
            assert_eq!(
                admission_state_from_bytes(
                    &current,
                    admission_bytes.as_deref(),
                    quarantine_bytes.as_deref(),
                    case.now,
                ),
                case.expected,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn timestamps_and_schema_are_bounded() {
        let current = identity('a');
        let mut future = admission(&current);
        future.accepted_at = NOW + 1;
        future.expires_at = future.accepted_at + 1;
        let mut too_long = admission(&current);
        too_long.accepted_at = NOW;
        too_long.expires_at = NOW + MAX_ADMISSION_LIFETIME_SECS + 1;
        let mut unknown_field = serde_json::to_value(admission(&current)).unwrap();
        unknown_field
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), serde_json::Value::Bool(true));

        for raw in [
            bytes(&future),
            bytes(&too_long),
            serde_json::to_vec(&unknown_field).unwrap(),
        ] {
            assert_eq!(
                admission_state_from_bytes(&current, Some(&raw), None, NOW),
                AdmissionState::Unknown
            );
        }
    }

    #[test]
    fn invalid_runtime_and_device_identity_never_passes() {
        let current = identity('a');
        for changed in [
            {
                let mut value = current.clone();
                value.device.backend = "cpu".to_string();
                value
            },
            {
                let mut value = current.clone();
                value.device.device_uuid = "0".repeat(32);
                value
            },
            {
                let mut value = current.clone();
                value.tuning.threads = 0;
                value
            },
        ] {
            let record = admission(&changed);
            assert_eq!(
                admission_state_from_bytes(&changed, Some(&bytes(&record)), None, NOW),
                AdmissionState::Unknown
            );
        }
    }

    #[test]
    fn identity_key_is_stable_and_covers_every_identity_field() {
        let first = identity('a');
        let same = identity('a');
        let mut changed = identity('a');
        changed.device.driver_version += 1;

        assert_eq!(
            AdmissionIdentityKey::for_identity(&first),
            AdmissionIdentityKey::for_identity(&same)
        );
        assert_ne!(
            AdmissionIdentityKey::for_identity(&first),
            AdmissionIdentityKey::for_identity(&changed)
        );
        assert_eq!(
            AdmissionIdentityKey::for_identity(&first).as_str().len(),
            64
        );
    }
}
