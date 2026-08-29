use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const REQUIRED_LANES: [&str; 5] = [
    "intelMesa",
    "amdRadv",
    "nvidiaVulkan",
    "cpuOnly",
    "dualGpu",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompatLaneStatus {
    Pass,
    Fail,
    Inconclusive,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatLane {
    pub status: CompatLaneStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatMatrix {
    pub schema_version: u32,
    pub lanes: BTreeMap<String, CompatLane>,
}

impl CompatMatrix {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: 1,
            lanes: BTreeMap::new(),
        }
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, String> {
        let raw = fs::read(path.as_ref()).map_err(|error| error.to_string())?;
        let matrix: Self =
            serde_json::from_slice(&raw).map_err(|error| format!("compat matrix: {error}"))?;
        if matrix.schema_version != 1 {
            return Err("compat matrix schemaVersion must be 1".to_string());
        }
        Ok(matrix)
    }

    #[must_use]
    pub fn load_default() -> Self {
        load_paths()
            .into_iter()
            .find_map(|path| Self::load_from(path).ok())
            .unwrap_or_else(Self::empty)
    }

    #[must_use]
    pub fn admits_auto_default(&self) -> bool {
        REQUIRED_LANES.iter().all(|lane| {
            self.lanes
                .get(*lane)
                .is_some_and(|entry| entry.status == CompatLaneStatus::Pass)
        })
    }

    #[must_use]
    pub fn factory_default(&self) -> echo_core::WhisperAccelerationPreference {
        if self.admits_auto_default() {
            echo_core::WhisperAccelerationPreference::Auto
        } else {
            echo_core::WhisperAccelerationPreference::Cpu
        }
    }
}

fn load_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("ECHO_WHISPER_COMPAT_MATRIX") {
        paths.push(PathBuf::from(path));
    }
    paths.push(echo_core::data_dir().join("whisper-compat-matrix.v1.json"));
    paths.push(PathBuf::from(
        "crates/echo/tests/fixtures/whisper-compat-matrix.v1.json",
    ));
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(host: &str) -> CompatLane {
        CompatLane {
            status: CompatLaneStatus::Pass,
            host: Some(host.to_string()),
            evidence_sha256: Some("a".repeat(64)),
        }
    }

    #[test]
    fn missing_lane_refuses_auto_default() {
        let mut matrix = CompatMatrix::empty();
        for lane in REQUIRED_LANES {
            if lane != "amdRadv" {
                matrix.lanes.insert(lane.to_string(), pass(lane));
            }
        }
        assert!(!matrix.admits_auto_default());
        assert_eq!(
            matrix.factory_default(),
            echo_core::WhisperAccelerationPreference::Cpu
        );
    }

    #[test]
    fn inconclusive_lane_refuses_auto_default() {
        let mut matrix = CompatMatrix::empty();
        for lane in REQUIRED_LANES {
            matrix.lanes.insert(lane.to_string(), pass(lane));
        }
        matrix.lanes.insert(
            "nvidiaVulkan".to_string(),
            CompatLane {
                status: CompatLaneStatus::Inconclusive,
                host: Some("nvidia".to_string()),
                evidence_sha256: None,
            },
        );
        assert!(!matrix.admits_auto_default());
    }

    #[test]
    fn complete_pass_matrix_admits_auto_default() {
        let mut matrix = CompatMatrix::empty();
        for lane in REQUIRED_LANES {
            matrix.lanes.insert(lane.to_string(), pass(lane));
        }
        assert!(matrix.admits_auto_default());
        assert_eq!(
            matrix.factory_default(),
            echo_core::WhisperAccelerationPreference::Auto
        );
    }
}
