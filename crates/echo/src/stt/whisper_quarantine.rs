use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::whisper_admission::{
    AdmissionIdentityKey, QuarantineReason, QuarantineRecord, MAX_QUARANTINE_LIFETIME_SECS,
};

const DOCUMENT_SCHEMA_VERSION: u32 = 1;
const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuarantineDocument {
    schema_version: u32,
    records: Vec<QuarantineRecord>,
}

pub struct QuarantineStore {
    path: PathBuf,
    lock_path: PathBuf,
}

impl QuarantineStore {
    #[must_use]
    pub fn at(path: PathBuf) -> Self {
        let lock_path = path.with_extension("lock");
        Self { path, lock_path }
    }

    pub fn is_active(&self, key: &AdmissionIdentityKey, now: u64) -> Result<bool, String> {
        self.with_lock(|| {
            let document = self.read_document()?;
            Ok(document.records.iter().any(|record| {
                record.identity_key == *key
                    && record.failure_count > 0
                    && interval_is_current(record.created_at, record.expires_at, now)
            }))
        })
    }

    pub fn record_failure(
        &self,
        key: &AdmissionIdentityKey,
        reason: QuarantineReason,
        now: u64,
    ) -> Result<(), String> {
        self.with_lock(|| {
            let mut document = self.read_document()?;
            document
                .records
                .retain(|record| record.expires_at > now && valid_record(record));
            if let Some(record) = document
                .records
                .iter_mut()
                .find(|record| record.identity_key == *key)
            {
                record.reason = reason;
                record.failure_count = record.failure_count.saturating_add(1);
                record.created_at = now;
                record.expires_at = now.saturating_add(MAX_QUARANTINE_LIFETIME_SECS);
            } else {
                document.records.push(QuarantineRecord {
                    schema_version: RECORD_SCHEMA_VERSION,
                    identity_key: key.clone(),
                    reason,
                    failure_count: 1,
                    created_at: now,
                    expires_at: now.saturating_add(MAX_QUARANTINE_LIFETIME_SECS),
                });
            }
            let raw = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
            echo_core::write_atomic(&self.path, &raw)
        })
    }

    fn with_lock<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .map_err(|error| error.to_string())?;
        lock.lock_exclusive().map_err(|error| error.to_string())?;
        let result = operation();
        let unlock = FileExt::unlock(&lock).map_err(|error| error.to_string());
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }

    fn read_document(&self) -> Result<QuarantineDocument, String> {
        let raw = match fs::read(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(QuarantineDocument {
                    schema_version: DOCUMENT_SCHEMA_VERSION,
                    records: Vec::new(),
                });
            }
            Err(error) => return Err(error.to_string()),
        };
        let document: QuarantineDocument =
            serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
        if document.schema_version != DOCUMENT_SCHEMA_VERSION
            || document.records.iter().any(|record| !valid_record(record))
        {
            return Err("unsupported or invalid Whisper quarantine state".to_string());
        }
        Ok(document)
    }
}

fn valid_record(record: &QuarantineRecord) -> bool {
    record.schema_version == RECORD_SCHEMA_VERSION
        && record.identity_key.as_str().len() == 64
        && record
            .identity_key
            .as_str()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && record.failure_count > 0
        && record
            .expires_at
            .checked_sub(record.created_at)
            .is_some_and(|lifetime| lifetime <= MAX_QUARANTINE_LIFETIME_SECS)
}

fn interval_is_current(start: u64, end: u64, now: u64) -> bool {
    start <= now
        && now < end
        && end
            .checked_sub(start)
            .is_some_and(|lifetime| lifetime <= MAX_QUARANTINE_LIFETIME_SECS)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};

    use super::*;

    fn key(value: char) -> AdmissionIdentityKey {
        serde_json::from_str(&format!("\"{}\"", value.to_string().repeat(64))).unwrap()
    }

    fn scratch(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "echo-whisper-quarantine-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path.join("quarantine.json")
    }

    #[test]
    fn quarantine_is_exact_bounded_and_persistent() {
        let path = scratch("exact");
        let store = QuarantineStore::at(path.clone());
        let first = key('a');
        let other = key('b');
        store
            .record_failure(&first, QuarantineReason::ReceiptMismatch, 100)
            .unwrap();
        assert!(store.is_active(&first, 100).unwrap());
        assert!(!store.is_active(&other, 100).unwrap());
        assert!(!store
            .is_active(&first, 100 + MAX_QUARANTINE_LIFETIME_SECS)
            .unwrap());
        let reopened = QuarantineStore::at(path);
        assert!(reopened.is_active(&first, 101).unwrap());
    }

    #[test]
    fn locked_updates_preserve_distinct_identities() {
        let path = scratch("concurrent");
        let barrier = Arc::new(Barrier::new(3));
        let handles = ['a', 'b'].map(|value| {
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            std::thread::spawn(move || {
                barrier.wait();
                QuarantineStore::at(path)
                    .record_failure(&key(value), QuarantineReason::RuntimeFailure, 200)
                    .unwrap();
            })
        });
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }
        let store = QuarantineStore::at(path);
        assert!(store.is_active(&key('a'), 201).unwrap());
        assert!(store.is_active(&key('b'), 201).unwrap());
    }

    #[test]
    fn corrupt_state_disables_acceleration_without_destroying_evidence() {
        let path = scratch("corrupt");
        fs::write(&path, b"not json").unwrap();
        let store = QuarantineStore::at(path.clone());
        assert!(store.is_active(&key('a'), 300).is_err());
        assert_eq!(fs::read(path).unwrap(), b"not json");
    }
}
