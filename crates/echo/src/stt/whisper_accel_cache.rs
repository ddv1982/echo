use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use super::whisper_admission::{QuarantineReason, MAX_QUARANTINE_LIFETIME_SECS};
use super::whisper_identity::{
    canonical_json_bytes, ExecutionArtifactId, IdentityError, InferenceContractId, Sha256Digest,
    UuidDigest,
};

const KEY_PREFIX: &[u8] = b"echo-whisper-local-selection-v1\0";
const RECORD_SCHEMA_VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_RECORDS_PER_BUCKET: usize = 256;
static OBSERVATION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StableVulkanReceipt {
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
}

impl StableVulkanReceipt {
    fn validate(&self) -> Result<(), String> {
        if self.backend != "vulkan"
            || self.vendor_id == 0
            || self.device_id == 0
            || self.api_version == 0
            || [
                self.device_uuid.as_str(),
                self.driver_uuid.as_str(),
                self.pipeline_cache_uuid.as_str(),
            ]
            .into_iter()
            .any(|uuid| uuid.bytes().all(|byte| byte == b'0'))
        {
            return Err("invalid stable Vulkan receipt".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DriverIcdFingerprint {
    pub drm_driver: String,
    pub icd_manifest_sha256: Sha256Digest,
    pub icd_library_sha256: Sha256Digest,
}

impl DriverIcdFingerprint {
    fn validate(&self) -> Result<(), String> {
        if self.drm_driver.is_empty()
            || self.drm_driver.len() > 128
            || !self
                .drm_driver
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err("invalid DRM driver identity".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalSelectionKeyInput<'a> {
    execution_artifact_id: &'a ExecutionArtifactId,
    inference_contract_id: &'a InferenceContractId,
    stable_receipt: &'a StableVulkanReceipt,
    driver_icd: &'a DriverIcdFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct LocalSelectionKey(String);

impl LocalSelectionKey {
    pub(crate) fn derive(
        execution_artifact_id: &ExecutionArtifactId,
        inference_contract_id: &InferenceContractId,
        stable_receipt: &StableVulkanReceipt,
        driver_icd: &DriverIcdFingerprint,
    ) -> Result<Self, String> {
        stable_receipt.validate()?;
        driver_icd.validate()?;
        let value = serde_json::to_value(LocalSelectionKeyInput {
            execution_artifact_id,
            inference_contract_id,
            stable_receipt,
            driver_icd,
        })
        .map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        digest.update(KEY_PREFIX);
        digest.update(canonical_json_bytes(&value).map_err(identity_error)?);
        Ok(Self(format!("{:x}", digest.finalize())))
    }

    pub(crate) fn parse(value: String) -> Result<Self, String> {
        if valid_hex(&value, 64) {
            Ok(Self(value))
        } else {
            Err("local selection key is not a lowercase SHA-256 digest".to_string())
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for LocalSelectionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VulkanReceiptObservation {
    pub stable: StableVulkanReceipt,
    pub selected_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CalibrationVerdict {
    GpuEligible,
    CpuOnly,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CalibrationObservation {
    schema_version: u32,
    observation_id: String,
    key: LocalSelectionKey,
    verdict: CalibrationVerdict,
    cpu_infer_ms: u64,
    gpu_infer_ms: Option<u64>,
    transcript_parity: Option<bool>,
    ready_receipt: Option<VulkanReceiptObservation>,
    result_receipt: Option<VulkanReceiptObservation>,
    observed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalQuarantineObservation {
    schema_version: u32,
    observation_id: String,
    key: LocalSelectionKey,
    reason: QuarantineReason,
    created_at: u64,
    expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalSelectionSnapshot {
    pub latest_calibration: Option<CalibrationObservation>,
    pub active_quarantine: Option<LocalQuarantineObservation>,
}

pub(crate) struct NewCalibrationObservation {
    pub key: LocalSelectionKey,
    pub verdict: CalibrationVerdict,
    pub cpu_infer_ms: u64,
    pub gpu_infer_ms: Option<u64>,
    pub transcript_parity: Option<bool>,
    pub ready_receipt: Option<VulkanReceiptObservation>,
    pub result_receipt: Option<VulkanReceiptObservation>,
    pub observed_at: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalSelectionStore {
    root: PathBuf,
}

impl LocalSelectionStore {
    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn append_calibration(
        &self,
        new: NewCalibrationObservation,
    ) -> Result<CalibrationObservation, String> {
        validate_new_calibration(&new)?;
        let observation = CalibrationObservation {
            schema_version: RECORD_SCHEMA_VERSION,
            observation_id: observation_id(),
            key: new.key,
            verdict: new.verdict,
            cpu_infer_ms: new.cpu_infer_ms,
            gpu_infer_ms: new.gpu_infer_ms,
            transcript_parity: new.transcript_parity,
            ready_receipt: new.ready_receipt,
            result_receipt: new.result_receipt,
            observed_at: new.observed_at,
        };
        self.publish(&observation.key, "calibration", &observation)?;
        Ok(observation)
    }

    pub(crate) fn append_quarantine(
        &self,
        key: LocalSelectionKey,
        reason: QuarantineReason,
        created_at: u64,
    ) -> Result<LocalQuarantineObservation, String> {
        if created_at == 0 {
            return Err("local quarantine timestamp is zero".to_string());
        }
        let observation = LocalQuarantineObservation {
            schema_version: RECORD_SCHEMA_VERSION,
            observation_id: observation_id(),
            key,
            reason,
            created_at,
            expires_at: created_at.saturating_add(MAX_QUARANTINE_LIFETIME_SECS),
        };
        self.publish(&observation.key, "quarantine", &observation)?;
        Ok(observation)
    }

    pub(crate) fn snapshot(
        &self,
        key: &LocalSelectionKey,
        now: u64,
    ) -> Result<LocalSelectionSnapshot, String> {
        let mut calibration = self.read_bucket::<CalibrationObservation>(key, "calibration")?;
        for observation in &calibration {
            validate_calibration(observation, key)?;
        }
        calibration.sort_by(|left, right| {
            (left.observed_at, &left.observation_id)
                .cmp(&(right.observed_at, &right.observation_id))
        });

        let mut quarantine = self.read_bucket::<LocalQuarantineObservation>(key, "quarantine")?;
        for observation in &quarantine {
            validate_quarantine(observation, key)?;
        }
        quarantine.sort_by(|left, right| {
            (left.created_at, &left.observation_id).cmp(&(right.created_at, &right.observation_id))
        });
        let active_quarantine = quarantine
            .into_iter()
            .rev()
            .find(|record| record.created_at <= now && now < record.expires_at);

        Ok(LocalSelectionSnapshot {
            latest_calibration: calibration.pop(),
            active_quarantine,
        })
    }

    fn publish<T: Serialize>(
        &self,
        key: &LocalSelectionKey,
        bucket: &str,
        value: &T,
    ) -> Result<(), String> {
        let directory = self.bucket(key, bucket);
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let raw = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
        if raw.len() as u64 > MAX_RECORD_BYTES {
            return Err("local selection record exceeds 64 KiB".to_string());
        }
        let id = observation_id_from_value(value)?;
        let target = directory.join(format!("{id}.json"));
        let temporary = directory.join(format!(".{id}.{}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let result = (|| {
            file.write_all(&raw).map_err(|error| error.to_string())?;
            file.write_all(b"\n").map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::hard_link(&temporary, &target).map_err(|error| error.to_string())?;
            fs::remove_file(&temporary).map_err(|error| error.to_string())?;
            File::open(&directory)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn read_bucket<T: for<'de> Deserialize<'de>>(
        &self,
        key: &LocalSelectionKey,
        bucket: &str,
    ) -> Result<Vec<T>, String> {
        let directory = self.bucket(key, bucket);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        let mut paths = entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| {
            !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.') && name.ends_with(".tmp"))
        });
        paths.sort();
        if paths.len() > MAX_RECORDS_PER_BUCKET {
            return Err("local selection record bucket is over its limit".to_string());
        }
        paths.into_iter().map(|path| read_record(&path)).collect()
    }

    fn bucket(&self, key: &LocalSelectionKey, bucket: &str) -> PathBuf {
        self.root.join("keys").join(key.as_str()).join(bucket)
    }
}

fn validate_new_calibration(new: &NewCalibrationObservation) -> Result<(), String> {
    if new.observed_at == 0 || new.cpu_infer_ms == 0 {
        return Err("invalid local calibration timing".to_string());
    }
    match new.verdict {
        CalibrationVerdict::GpuEligible => {
            if new.gpu_infer_ms == Some(0)
                || new.gpu_infer_ms.is_none()
                || new.transcript_parity != Some(true)
                || new.ready_receipt.is_none()
                || new.result_receipt.is_none()
            {
                return Err("eligible GPU calibration lacks complete evidence".to_string());
            }
        }
        CalibrationVerdict::CpuOnly
        | CalibrationVerdict::Failed
        | CalibrationVerdict::Cancelled => {}
    }
    for receipt in [new.ready_receipt.as_ref(), new.result_receipt.as_ref()]
        .into_iter()
        .flatten()
    {
        receipt.stable.validate()?;
    }
    if let (Some(ready), Some(result)) = (&new.ready_receipt, &new.result_receipt) {
        if ready.stable != result.stable {
            return Err("ready and result Vulkan receipts differ".to_string());
        }
    }
    Ok(())
}

fn validate_calibration(
    observation: &CalibrationObservation,
    key: &LocalSelectionKey,
) -> Result<(), String> {
    if observation.schema_version != RECORD_SCHEMA_VERSION
        || observation.key != *key
        || !valid_hex(&observation.observation_id, 32)
    {
        return Err("invalid local calibration record".to_string());
    }
    validate_new_calibration(&NewCalibrationObservation {
        key: observation.key.clone(),
        verdict: observation.verdict,
        cpu_infer_ms: observation.cpu_infer_ms,
        gpu_infer_ms: observation.gpu_infer_ms,
        transcript_parity: observation.transcript_parity,
        ready_receipt: observation.ready_receipt.clone(),
        result_receipt: observation.result_receipt.clone(),
        observed_at: observation.observed_at,
    })
}

fn validate_quarantine(
    observation: &LocalQuarantineObservation,
    key: &LocalSelectionKey,
) -> Result<(), String> {
    if observation.schema_version != RECORD_SCHEMA_VERSION
        || observation.key != *key
        || !valid_hex(&observation.observation_id, 32)
        || observation.created_at == 0
        || observation.expires_at.checked_sub(observation.created_at)
            != Some(MAX_QUARANTINE_LIFETIME_SECS)
    {
        return Err("invalid local quarantine record".to_string());
    }
    Ok(())
}

fn read_record<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_RECORD_BYTES {
        return Err("invalid local selection record file".to_string());
    }
    let raw = fs::read(path).map_err(|error| error.to_string())?;
    let record = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    let expected_id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "local selection record filename is invalid".to_string())?;
    if value
        .get("observationId")
        .and_then(serde_json::Value::as_str)
        != Some(expected_id)
    {
        return Err("local selection record filename differs from its ID".to_string());
    }
    Ok(record)
}

fn observation_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let count = OBSERVATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut digest = Sha256::new();
    digest.update(now.to_le_bytes());
    digest.update(std::process::id().to_le_bytes());
    digest.update(count.to_le_bytes());
    format!("{:x}", digest.finalize())[..32].to_string()
}

fn observation_id_from_value<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    value
        .get("observationId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| valid_hex(value, 32))
        .map(ToOwned::to_owned)
        .ok_or_else(|| "local selection record has no valid observation ID".to_string())
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn identity_error(error: IdentityError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn digest(value: char) -> Sha256Digest {
        Sha256Digest::parse(value.to_string().repeat(64)).unwrap()
    }

    fn uuid(value: char) -> UuidDigest {
        UuidDigest::parse(value.to_string().repeat(32)).unwrap()
    }

    fn receipt() -> StableVulkanReceipt {
        StableVulkanReceipt {
            backend: "vulkan".to_string(),
            vendor_id: 0x8086,
            device_id: 0x46a6,
            api_version: 1,
            driver_version: 2,
            device_uuid: uuid('1'),
            driver_uuid: uuid('2'),
            pipeline_cache_uuid: uuid('3'),
        }
    }

    fn fingerprint() -> DriverIcdFingerprint {
        DriverIcdFingerprint {
            drm_driver: "i915".to_string(),
            icd_manifest_sha256: digest('4'),
            icd_library_sha256: digest('5'),
        }
    }

    fn key() -> LocalSelectionKey {
        LocalSelectionKey::derive(
            &ExecutionArtifactId::parse("6".repeat(64)).unwrap(),
            &InferenceContractId::parse("7".repeat(64)).unwrap(),
            &receipt(),
            &fingerprint(),
        )
        .unwrap()
    }

    fn scratch(label: &str) -> PathBuf {
        let count = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "echo-whisper-local-{label}-{}-{count}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn eligible(key: LocalSelectionKey, observed_at: u64) -> NewCalibrationObservation {
        let observed = VulkanReceiptObservation {
            stable: receipt(),
            selected_index: 4,
        };
        NewCalibrationObservation {
            key,
            verdict: CalibrationVerdict::GpuEligible,
            cpu_infer_ms: 200,
            gpu_infer_ms: Some(100),
            transcript_parity: Some(true),
            ready_receipt: Some(observed.clone()),
            result_receipt: Some(observed),
            observed_at,
        }
    }

    #[test]
    fn key_changes_for_identity_but_not_selected_index() {
        let base = key();
        let first = VulkanReceiptObservation {
            stable: receipt(),
            selected_index: 0,
        };
        let second = VulkanReceiptObservation {
            stable: receipt(),
            selected_index: 9,
        };
        assert_eq!(first.stable, second.stable);
        assert_eq!(base, key());

        let mut changed = receipt();
        changed.driver_uuid = uuid('8');
        assert_ne!(
            base,
            LocalSelectionKey::derive(
                &ExecutionArtifactId::parse("6".repeat(64)).unwrap(),
                &InferenceContractId::parse("7".repeat(64)).unwrap(),
                &changed,
                &fingerprint(),
            )
            .unwrap()
        );
    }

    #[test]
    fn immutable_records_fold_deterministically_and_quarantine_expires() {
        let root = scratch("fold");
        let store = LocalSelectionStore::at(root);
        let key = key();
        let older = store
            .append_calibration(eligible(key.clone(), 100))
            .unwrap();
        let newer = store
            .append_calibration(eligible(key.clone(), 200))
            .unwrap();
        let quarantine = store
            .append_quarantine(key.clone(), QuarantineReason::ReceiptMismatch, 250)
            .unwrap();

        let active = store.snapshot(&key, 251).unwrap();
        assert_eq!(active.latest_calibration, Some(newer));
        assert_eq!(active.active_quarantine, Some(quarantine));
        assert_ne!(active.latest_calibration, Some(older));
        assert!(store
            .snapshot(&key, 250 + MAX_QUARANTINE_LIFETIME_SECS)
            .unwrap()
            .active_quarantine
            .is_none());
    }

    #[test]
    fn corrupt_record_is_preserved_and_fails_closed() {
        let root = scratch("corrupt");
        let store = LocalSelectionStore::at(root.clone());
        let key = key();
        store
            .append_calibration(eligible(key.clone(), 100))
            .unwrap();
        let corrupt = root
            .join("keys")
            .join(key.as_str())
            .join("calibration")
            .join("ffffffffffffffffffffffffffffffff.json");
        fs::write(&corrupt, b"{not-json\n").unwrap();

        assert!(store.snapshot(&key, 101).is_err());
        assert_eq!(fs::read(&corrupt).unwrap(), b"{not-json\n");
    }

    #[test]
    fn unpublished_temporary_record_is_not_visible() {
        let root = scratch("temporary");
        let store = LocalSelectionStore::at(root.clone());
        let key = key();
        let directory = root.join("keys").join(key.as_str()).join("calibration");
        fs::create_dir_all(&directory).unwrap();
        let temporary = directory.join(".interrupted.1.tmp");
        fs::write(&temporary, b"{partial").unwrap();

        assert_eq!(
            store.snapshot(&key, 100).unwrap(),
            LocalSelectionSnapshot {
                latest_calibration: None,
                active_quarantine: None,
            }
        );
        assert_eq!(fs::read(&temporary).unwrap(), b"{partial");
    }

    #[test]
    fn process_writer_entry() {
        let Some(root) = std::env::var_os("ECHO_ACCEL_STORE_PROCESS_ROOT") else {
            return;
        };
        let observed_at = std::env::var("ECHO_ACCEL_STORE_PROCESS_AT")
            .unwrap()
            .parse()
            .unwrap();
        LocalSelectionStore::at(PathBuf::from(root))
            .append_calibration(eligible(key(), observed_at))
            .unwrap();
    }

    #[test]
    fn two_processes_publish_separate_complete_records() {
        let root = scratch("processes");
        let test_binary = std::env::current_exe().unwrap();
        let mut first = Command::new(&test_binary)
            .args([
                "--exact",
                "stt::whisper_accel_cache::tests::process_writer_entry",
            ])
            .env("ECHO_ACCEL_STORE_PROCESS_ROOT", &root)
            .env("ECHO_ACCEL_STORE_PROCESS_AT", "100")
            .spawn()
            .unwrap();
        let mut second = Command::new(test_binary)
            .args([
                "--exact",
                "stt::whisper_accel_cache::tests::process_writer_entry",
            ])
            .env("ECHO_ACCEL_STORE_PROCESS_ROOT", &root)
            .env("ECHO_ACCEL_STORE_PROCESS_AT", "200")
            .spawn()
            .unwrap();
        assert!(first.wait().unwrap().success());
        assert!(second.wait().unwrap().success());

        let snapshot = LocalSelectionStore::at(root.clone())
            .snapshot(&key(), 201)
            .unwrap();
        assert_eq!(
            snapshot.latest_calibration.map(|record| record.observed_at),
            Some(200)
        );
        let records = fs::read_dir(root.join("keys").join(key().as_str()).join("calibration"))
            .unwrap()
            .count();
        assert_eq!(records, 2);
    }
}
