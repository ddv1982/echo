use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::catalog::{ComponentId, PayloadKind};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(String);

impl OperationId {
    #[must_use]
    pub fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        Self(format!(
            "{nanos:x}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(super) fn fixture(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallPhase {
    CheckingDisk,
    Downloading,
    Verifying,
    Extracting,
    Activating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub operation_id: OperationId,
    pub component: ComponentId,
    pub phase: InstallPhase,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub resumed_from_bytes: u64,
}

impl InstallProgress {
    pub(super) fn new(
        operation_id: &OperationId,
        component: ComponentId,
        phase: InstallPhase,
        received_bytes: u64,
        total_bytes: u64,
        resumed_from_bytes: u64,
    ) -> Self {
        Self {
            operation_id: operation_id.clone(),
            component,
            phase,
            received_bytes,
            total_bytes,
            resumed_from_bytes,
        }
    }
}

#[derive(Debug)]
pub enum InstallError {
    Unsupported(String),
    Busy,
    InsufficientSpace { required: u64, available: u64 },
    Http(String),
    Range(String),
    Interrupted { received: u64, expected: u64 },
    Sha256Mismatch { expected: String, actual: String },
    UnsafeArchive(String),
    Payload(String),
    State(String),
    Io(std::io::Error),
    IoMessage(String),
    Cancelled,
    Probe(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message)
            | Self::Http(message)
            | Self::Range(message)
            | Self::UnsafeArchive(message)
            | Self::Payload(message)
            | Self::State(message)
            | Self::IoMessage(message)
            | Self::Probe(message) => formatter.write_str(message),
            Self::Busy => formatter.write_str("another managed component operation is active"),
            Self::InsufficientSpace {
                required,
                available,
            } => write!(
                formatter,
                "setup needs {required} bytes free, but {available} bytes are available"
            ),
            Self::Interrupted { received, expected } => write!(
                formatter,
                "download stopped at {received} of {expected} bytes and can be resumed"
            ),
            Self::Sha256Mismatch { expected, actual } => write!(
                formatter,
                "SHA-256 mismatch: expected {expected}, got {actual}"
            ),
            Self::Io(error) => write!(formatter, "disk error: {error}"),
            Self::Cancelled => formatter.write_str("setup cancelled; downloaded bytes were kept"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledFile {
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
    pub mode: u32,
    pub kind: PayloadKind,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationRecord {
    pub schema_version: u32,
    pub component: ComponentId,
    pub version: String,
    pub release: String,
    pub artifact_sha256: String,
    pub files: Vec<InstalledFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ManagedComponentState {
    Absent {
        resumable_bytes: u64,
    },
    Ready {
        version: String,
        bytes: u64,
        #[serde(serialize_with = "serialize_display_path")]
        root: PathBuf,
    },
    NeedsRepair {
        reason: String,
        resumable_bytes: u64,
    },
    Unsupported {
        reason: String,
    },
}
pub struct ComponentLease {
    pub(super) file: fs::File,
}

pub struct ManagedPath {
    pub root: PathBuf,
    pub lease: ComponentLease,
}

impl Drop for ComponentLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn serialize_display_path<S: serde::Serializer>(
    path: &std::path::Path,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}
