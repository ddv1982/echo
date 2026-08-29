use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_ADMISSION_LIFETIME_SECS: u64 = 30 * 24 * 60 * 60;
pub const MAX_QUARANTINE_LIFETIME_SECS: u64 = 24 * 60 * 60;
pub const MAX_ADMISSION_SET_BYTES: usize = 1024 * 1024;
pub const MAX_ADMISSION_RECORDS: usize = 128;
pub const MAX_PACKAGE_ENTRIES: usize = 4096;
pub const MAX_PACKAGE_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

const ADMISSION_SET_SCHEMA_VERSION: u32 = 2;
const IDENTITY_SCHEMA_VERSION: u32 = 1;
const QUARANTINE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionTuning {
    pub threads: u16,
    pub beam_size: u8,
    pub best_of: u8,
    pub no_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    pub drm_driver: String,
    pub icd_manifest_sha256: String,
    pub icd_library_sha256: String,
    pub launch_contract_schema: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdmissionIdentityKey(String);

impl AdmissionIdentityKey {
    #[must_use]
    pub fn for_identity(identity: &AdmissionIdentity) -> Self {
        let canonical =
            serde_json::to_vec(identity).expect("AdmissionIdentity serialization cannot fail");
        let mut hasher = Sha256::new();
        hasher.update(b"echo-whisper-admission-identity-v1\0");
        hasher.update(canonical);
        Self(format!("{:x}", hasher.finalize()))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn parse(value: String) -> Result<Self, String> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err("Whisper acceleration key is not a lowercase SHA-256 digest".to_string())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedRuntimeArtifacts {
    pub runtime_relative_path: String,
    #[serde(deserialize_with = "deserialize_unique_map")]
    pub runtime_library_bindings: BTreeMap<String, String>,
    pub probe_relative_path: String,
    pub probe_sha256: String,
}

fn deserialize_unique_map<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct UniqueMap;
    impl<'de> Visitor<'de> for UniqueMap {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map with unique library aliases")
        }
        fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = access.next_entry::<String, String>()? {
                if values.insert(key.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate library alias {key}"
                    )));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(UniqueMap)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheSeedArtifact {
    pub relative_path: String,
    pub sha256: String,
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
    pub stability_success: bool,
    pub memory_evidence: bool,
    pub memory_floor: bool,
    pub swap_stable: bool,
}

impl AdmissionGates {
    pub(crate) fn all_passed(&self) -> bool {
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
            && self.stability_success
            && self.memory_evidence
            && self.memory_floor
            && self.swap_stable
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
pub struct ModelAdmission {
    pub identity: AdmissionIdentity,
    pub identity_key: AdmissionIdentityKey,
    pub evidence_sha256: String,
    pub icd_manifest_path: String,
    pub icd_library_path: String,
    pub cache_seed: CacheSeedArtifact,
    pub gates: AdmissionGates,
    pub verdict: AdmissionVerdict,
    pub accepted_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageEntryKind {
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageEntry {
    pub path: String,
    pub kind: PackageEntryKind,
    pub bytes: u64,
    pub sha256: Option<String>,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionSet {
    pub schema_version: u32,
    pub shared: SharedRuntimeArtifacts,
    pub records: Vec<ModelAdmission>,
    pub inventory: Vec<PackageEntry>,
}

impl AdmissionSet {
    pub fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        if raw.len() > MAX_ADMISSION_SET_BYTES {
            return Err("Whisper admission set exceeds 1 MiB".into());
        }
        let set: Self = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
        set.validate()?;
        Ok(set)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != ADMISSION_SET_SCHEMA_VERSION
            || self.records.is_empty()
            || self.records.len() > MAX_ADMISSION_RECORDS
            || self.inventory.is_empty()
            || self.inventory.len() > MAX_PACKAGE_ENTRIES
        {
            return Err("unsupported or unbounded Whisper admission set".into());
        }
        if !safe_relative(&self.shared.runtime_relative_path)
            || !safe_relative(&self.shared.probe_relative_path)
            || !is_sha256(&self.shared.probe_sha256)
            || self.shared.runtime_library_bindings.is_empty()
            || !self
                .shared
                .runtime_library_bindings
                .iter()
                .all(|(name, digest)| safe_library_name(name) && is_sha256(digest))
        {
            return Err("invalid shared Whisper runtime artifacts".into());
        }
        let mut keys = BTreeSet::new();
        let mut identities = BTreeSet::new();
        let mut cache_paths = BTreeSet::new();
        let package_identity = &self.records[0].identity;
        for record in &self.records {
            if !identity_is_valid(&record.identity)
                || !same_package_contract(package_identity, &record.identity)
                || record.identity_key != AdmissionIdentityKey::for_identity(&record.identity)
                || !keys.insert(record.identity_key.clone())
                || !identities.insert(record.identity.clone())
                || !cache_paths.insert(record.cache_seed.relative_path.clone())
                || !is_sha256(&record.evidence_sha256)
                || !is_sha256(&record.cache_seed.sha256)
                || !safe_relative(&record.cache_seed.relative_path)
                || record.cache_seed.relative_path
                    != format!("cache-seeds/{}", record.identity_key.as_str())
                || !Path::new(&record.icd_manifest_path).is_absolute()
                || !Path::new(&record.icd_library_path).is_absolute()
            {
                return Err("invalid or duplicate Whisper admission identity".into());
            }
        }
        let mut paths = BTreeSet::new();
        let mut total = 0_u64;
        for entry in &self.inventory {
            total = total
                .checked_add(entry.bytes)
                .ok_or_else(|| "Whisper package size overflow".to_string())?;
            if entry.path == "admission-set.json"
                || !safe_relative(&entry.path)
                || !paths.insert(entry.path.clone())
                || entry.bytes > MAX_PACKAGE_ENTRY_BYTES
                || total > MAX_PACKAGE_BYTES
            {
                return Err("invalid or unbounded Whisper package inventory".into());
            }
            match entry.kind {
                PackageEntryKind::File
                    if entry.sha256.as_deref().is_some_and(is_sha256)
                        && entry.link_target.is_none() => {}
                PackageEntryKind::Symlink
                    if entry.sha256.is_none()
                        && entry.link_target.as_deref().is_some_and(safe_relative) => {}
                PackageEntryKind::File | PackageEntryKind::Symlink => {
                    return Err("invalid Whisper package inventory entry".into())
                }
            }
        }
        let runtime_parent = Path::new(&self.shared.runtime_relative_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let required_shared = std::iter::once(self.shared.runtime_relative_path.clone())
            .chain(std::iter::once(self.shared.probe_relative_path.clone()))
            .chain(self.shared.runtime_library_bindings.keys().map(|alias| {
                runtime_parent
                    .join(alias)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            }));
        if required_shared
            .into_iter()
            .any(|path| !paths.contains(&path))
            || self.records.iter().any(|record| {
                let prefix = format!("{}/", record.cache_seed.relative_path);
                !paths.iter().any(|path| path.starts_with(&prefix))
            })
        {
            return Err("Whisper package inventory omits a referenced artifact".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineRecord {
    pub schema_version: u32,
    pub identity_key: AdmissionIdentityKey,
    pub reason: QuarantineReason,
    pub failure_count: u32,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuarantineReason {
    RuntimeFailure,
    Timeout,
    MalformedOutput,
    MissingReceipt,
    ReceiptMismatch,
    CpuFallback,
    IdentityMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionState {
    Unknown,
    Passed,
    Stopped,
    Quarantined,
}

#[must_use]
pub(crate) fn admission_state(
    record: &ModelAdmission,
    identity: &AdmissionIdentity,
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
    if record.identity != *identity
        || record.identity_key != AdmissionIdentityKey::for_identity(identity)
        || !is_sha256(&record.evidence_sha256)
        || !is_sha256(&record.cache_seed.sha256)
        || !safe_relative(&record.cache_seed.relative_path)
        || !interval_is_current(
            record.accepted_at,
            record.expires_at,
            now,
            MAX_ADMISSION_LIFETIME_SECS,
        )
    {
        return AdmissionState::Unknown;
    }
    match record.verdict {
        AdmissionVerdict::Passed if record.gates.all_passed() => AdmissionState::Passed,
        AdmissionVerdict::Passed | AdmissionVerdict::Stopped => AdmissionState::Stopped,
    }
}

fn quarantine_applies(record: &QuarantineRecord, identity: &AdmissionIdentity, now: u64) -> bool {
    record.schema_version == QUARANTINE_SCHEMA_VERSION
        && record.identity_key == AdmissionIdentityKey::for_identity(identity)
        && record.failure_count > 0
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
    identity.schema_version == IDENTITY_SCHEMA_VERSION
        && [
            identity.echo_binary_sha256.as_str(),
            identity.runtime_identity_sha256.as_str(),
            identity.model_sha256.as_str(),
            identity.icd_manifest_sha256.as_str(),
            identity.icd_library_sha256.as_str(),
        ]
        .into_iter()
        .all(is_sha256)
        && identity.vad_sha256.as_deref().is_none_or(is_sha256)
        && is_lower_hex(&identity.echo_commit, 40)
        && identity.protocol == "oneShotCli"
        && identity.language_policy == "pinned"
        && identity.prompt_policy == "empty"
        && identity.tuning.threads > 0
        && identity.tuning.beam_size > 0
        && identity.tuning.best_of > 0
        && identity.device.backend == "vulkan"
        && identity.device.vendor_id > 0
        && identity.device.device_id > 0
        && !identity.drm_driver.is_empty()
        && [
            identity.device.device_uuid.as_str(),
            identity.device.driver_uuid.as_str(),
            identity.device.pipeline_cache_uuid.as_str(),
        ]
        .into_iter()
        .all(|value| is_lower_hex(value, 32) && !value.bytes().all(|byte| byte == b'0'))
        && identity.launch_contract_schema > 0
}

fn same_package_contract(first: &AdmissionIdentity, candidate: &AdmissionIdentity) -> bool {
    first.echo_commit == candidate.echo_commit
        && first.echo_binary_sha256 == candidate.echo_binary_sha256
        && first.runtime_identity_sha256 == candidate.runtime_identity_sha256
        && first.vad_sha256 == candidate.vad_sha256
        && first.protocol == candidate.protocol
        && first.language_policy == candidate.language_policy
        && first.prompt_policy == candidate.prompt_policy
        && first.launch_contract_schema == candidate.launch_contract_schema
}

fn safe_library_name(value: &str) -> bool {
    value.contains(".so") && Path::new(value).components().count() == 1 && safe_relative(value)
}
pub(crate) fn safe_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}
pub(crate) fn is_sha256(value: &str) -> bool {
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
            protocol: "oneShotCli".into(),
            tuning: AdmissionTuning {
                threads: 4,
                beam_size: 1,
                best_of: 2,
                no_fallback: true,
            },
            language_policy: "pinned".into(),
            prompt_policy: "empty".into(),
            device: AdmissionDeviceIdentity {
                backend: "vulkan".into(),
                selected_index: 0,
                vendor_id: 0x8086,
                device_id: 0x46a6,
                api_version: 1,
                driver_version: 1,
                device_uuid: "1".repeat(32),
                driver_uuid: "2".repeat(32),
                pipeline_cache_uuid: "3".repeat(32),
            },
            drm_driver: "i915".into(),
            icd_manifest_sha256: "e".repeat(64),
            icd_library_sha256: "f".repeat(64),
            launch_contract_schema: 1,
        }
    }

    fn gates() -> AdmissionGates {
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
            stability_success: true,
            memory_evidence: true,
            memory_floor: true,
            swap_stable: true,
        }
    }

    fn record(identity: &AdmissionIdentity) -> ModelAdmission {
        let identity_key = AdmissionIdentityKey::for_identity(identity);
        ModelAdmission {
            identity: identity.clone(),
            identity_key: identity_key.clone(),
            evidence_sha256: "a".repeat(64),
            icd_manifest_path: "/usr/share/vulkan/icd.d/intel_icd.json".into(),
            icd_library_path: "/usr/lib/libvulkan_intel.so".into(),
            cache_seed: CacheSeedArtifact {
                relative_path: format!("cache-seeds/{}", identity_key.as_str()),
                sha256: "f".repeat(64),
            },
            gates: gates(),
            verdict: AdmissionVerdict::Passed,
            accepted_at: NOW - 60,
            expires_at: NOW + 60,
        }
    }

    fn quarantine(identity: &AdmissionIdentity) -> Vec<u8> {
        serde_json::to_vec(&QuarantineRecord {
            schema_version: 1,
            identity_key: AdmissionIdentityKey::for_identity(identity),
            reason: QuarantineReason::ReceiptMismatch,
            failure_count: 1,
            created_at: NOW - 60,
            expires_at: NOW + 60,
        })
        .unwrap()
    }

    #[test]
    fn admission_states_are_fail_closed_and_exact() {
        let current = identity('a');
        let changed = identity('9');
        let passed = record(&current);
        assert_eq!(
            admission_state(&passed, &current, None, NOW),
            AdmissionState::Passed
        );

        let mut stopped = passed.clone();
        stopped.verdict = AdmissionVerdict::Stopped;
        assert_eq!(
            admission_state(&stopped, &current, None, NOW),
            AdmissionState::Stopped
        );

        let mut expired = passed.clone();
        expired.expires_at = NOW;
        assert_eq!(
            admission_state(&expired, &current, None, NOW),
            AdmissionState::Unknown
        );
        assert_eq!(
            admission_state(&record(&changed), &current, None, NOW),
            AdmissionState::Unknown
        );

        let mut false_gate = passed.clone();
        false_gate.gates.memory_evidence = false;
        assert_eq!(
            admission_state(&false_gate, &current, None, NOW),
            AdmissionState::Stopped
        );
        assert_eq!(
            admission_state(&passed, &current, Some(&quarantine(&current)), NOW),
            AdmissionState::Quarantined
        );
        assert_eq!(
            admission_state(&passed, &current, Some(&quarantine(&changed)), NOW),
            AdmissionState::Passed
        );
    }

    #[test]
    fn timestamps_identity_and_schema_are_strict() {
        let current = identity('a');
        let mut future = record(&current);
        future.accepted_at = NOW + 1;
        future.expires_at = NOW + 2;
        assert_eq!(
            admission_state(&future, &current, None, NOW),
            AdmissionState::Unknown
        );

        let mut too_long = record(&current);
        too_long.accepted_at = NOW;
        too_long.expires_at = NOW + MAX_ADMISSION_LIFETIME_SECS + 1;
        assert_eq!(
            admission_state(&too_long, &current, None, NOW),
            AdmissionState::Unknown
        );

        let mut cpu = current.clone();
        cpu.device.backend = "cpu".into();
        assert_eq!(
            admission_state(&record(&cpu), &cpu, None, NOW),
            AdmissionState::Unknown
        );
        let mut zero_threads = current.clone();
        zero_threads.tuning.threads = 0;
        assert_eq!(
            admission_state(&record(&zero_threads), &zero_threads, None, NOW),
            AdmissionState::Unknown
        );
        let mut bad_uuid = current.clone();
        bad_uuid.device.device_uuid = "0".repeat(32);
        assert_eq!(
            admission_state(&record(&bad_uuid), &bad_uuid, None, NOW),
            AdmissionState::Unknown
        );
        let mut bad_schema = current.clone();
        bad_schema.schema_version = 2;
        assert_eq!(
            admission_state(&record(&bad_schema), &bad_schema, None, NOW),
            AdmissionState::Unknown
        );
    }

    #[test]
    fn promotion_identity_key_matches_the_cross_language_contract() {
        let mut value = identity('a');
        value.tuning = AdmissionTuning {
            threads: 4,
            beam_size: 3,
            best_of: 5,
            no_fallback: false,
        };
        value.device.api_version = 4_211_006;
        value.device.driver_version = 104_865_800;
        value.device.device_uuid = "8680a6460c0000000002000000000000".into();
        value.device.driver_uuid = "ee99561e45e1e718c6121d36d8345582".into();
        value.device.pipeline_cache_uuid = "35e9eb9761bf7afc9291ffc449ddf849".into();
        assert_eq!(
            AdmissionIdentityKey::for_identity(&value).as_str(),
            "1aafa0c27dc5c344c14f2c43685ed182b4650469ffed13d6bbfbc7663fffd360"
        );
    }
    #[test]
    fn identity_key_is_stable_and_full_identity_bound() {
        let first = identity('a');
        let mut changed = first.clone();
        changed.device.driver_version += 1;
        assert_eq!(
            AdmissionIdentityKey::for_identity(&first),
            AdmissionIdentityKey::for_identity(&first)
        );
        assert_ne!(
            AdmissionIdentityKey::for_identity(&first),
            AdmissionIdentityKey::for_identity(&changed)
        );
    }

    #[test]
    fn package_contract_allows_models_and_hardware_but_rejects_shared_drift() {
        let first = identity('a');
        let mut other = identity('9');
        other.echo_binary_sha256 = first.echo_binary_sha256.clone();
        other.model_sha256 = "9".repeat(64);
        other.device.device_id += 1;
        assert!(same_package_contract(&first, &other));

        let mutations: [fn(&mut AdmissionIdentity); 3] = [
            |value| value.echo_commit = "5".repeat(40),
            |value| value.vad_sha256 = None,
            |value| value.prompt_policy = "changed".to_string(),
        ];
        for mutate in mutations {
            let mut changed = other.clone();
            mutate(&mut changed);
            assert!(!same_package_contract(&first, &changed));
        }
    }

    #[test]
    fn safe_paths_reject_escape_and_non_normal_components() {
        assert!(safe_relative("runtime/whisper-cli"));
        for path in ["", "/tmp/x", "../x", "a/../x", "./x"] {
            assert!(!safe_relative(path), "{path}");
        }
    }
}
