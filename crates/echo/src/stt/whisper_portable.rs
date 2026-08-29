// Orphaned by the planner's removal. Deleted in the next phase; kept for
// one commit so the planner deletion stands alone.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::whisper_identity::{
    canonical_json_bytes, CommitDigest, ExecutionArtifactId, ExecutionArtifactInput,
    InferenceContractId, InferenceContractInput, LocalEnvironmentInput, LocalEnvironmentKey,
    PackageType, PerformanceEvidenceId, SafeRelativePath, Sha256Digest,
};
use super::{runtime_library_bindings, whisper_runtime_launch, WhisperRuntimeLaunch};

const PORTABLE_SCHEMA_VERSION: u32 = 1;
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_EVIDENCE_LIFETIME_SECS: u64 = 30 * 24 * 60 * 60;
const VERIFICATION_STAMP_SCHEMA: u32 = 1;

pub(crate) fn installed_package_root(echo_binary: &Path) -> Option<PathBuf> {
    let parent = echo_binary.parent()?;
    let prefix = parent.parent()?;
    [
        parent.join("whisper-acceleration"),
        prefix.join("lib/echo/whisper-acceleration"),
        prefix.join("lib/io.github.ddv1982.echo/whisper-acceleration"),
    ]
    .into_iter()
    .find(|candidate| candidate.join("portable-selection.v1.json").is_file())
}

#[allow(dead_code)]
pub(crate) fn portable_execution_id(root: &Path) -> Option<ExecutionArtifactId> {
    let raw = fs::read(root.join("portable-selection.v1.json")).ok()?;
    PortableSelection::from_bytes(&raw)
        .ok()
        .map(|selection| selection.execution_artifact.id)
}

pub(crate) fn qualified_contract_by_id<'a>(
    contracts: &'a [InferenceContractRecord],
    expected: &InferenceContractId,
) -> Option<&'a InferenceContractRecord> {
    contracts
        .iter()
        .find(|contract| contract.id == *expected && qualified_contract(contract))
}

pub(crate) fn resolve_qualified_contract<'a>(
    contracts: &'a [InferenceContractRecord],
    model_sha256: &Sha256Digest,
    vad_sha256: &Option<Sha256Digest>,
    expected: Option<&InferenceContractId>,
) -> Result<Option<&'a InferenceContractRecord>, String> {
    let matches = contracts
        .iter()
        .filter(|contract| {
            qualified_contract(contract)
                && contract.value.model_sha256 == *model_sha256
                && contract.value.vad_sha256 == *vad_sha256
                && expected.is_none_or(|expected| expected == &contract.id)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [contract] => Ok(Some(*contract)),
        _ => Err("qualified inference contract is ambiguous".to_string()),
    }
}

fn qualified_contract(contract: &InferenceContractRecord) -> bool {
    contract.value.protocol == "oneShotCli"
        && contract.value.request_policy.language == "pinned"
        && contract.value.request_policy.prompt == "empty"
        && contract.value.request_policy.hints == "qualifiedOnly"
}

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
    pub calibration_fixture: CalibrationFixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CalibrationFixture {
    pub relative_path: SafeRelativePath,
    pub sha256: Sha256Digest,
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
            || !self
                .calibration_fixture
                .relative_path
                .as_str()
                .starts_with("calibration/")
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
    pub source_acceleration_set_sha256: Sha256Digest,
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

#[derive(Debug, Clone)]
pub(crate) struct InstalledPortableSelection {
    pub package: PortableSelectionPackage,
    pub root: PathBuf,
    pub runtime: PathBuf,
    pub probe: PathBuf,
    pub calibration_fixture: PathBuf,
    echo_binary: PathBuf,
    build_receipt: PathBuf,
    binding_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationStamp {
    schema_version: u32,
    binding_digest: Sha256Digest,
    files: Vec<VerifiedFileStamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifiedFileStamp {
    path: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    link_target: Option<PathBuf>,
}

impl InstalledPortableSelection {
    #[cfg(test)]
    pub(crate) fn for_test(
        package: PortableSelectionPackage,
        root: PathBuf,
        runtime: PathBuf,
        probe: PathBuf,
        calibration_fixture: PathBuf,
    ) -> Self {
        Self {
            package,
            echo_binary: root.join("echo-desktop"),
            build_receipt: root.join("runtime/build-receipt.json"),
            binding_digest: Sha256Digest::parse("f".repeat(64)).expect("test digest"),
            root,
            runtime,
            probe,
            calibration_fixture,
        }
    }

    pub(crate) fn runtime_launch(&self) -> WhisperRuntimeLaunch {
        WhisperRuntimeLaunch {
            library_dir: self.runtime.parent().map(Path::to_path_buf),
            identity_sha256: Some(
                self.package
                    .selection
                    .execution_artifact
                    .value
                    .runtime_identity_sha256
                    .as_str()
                    .to_string(),
            ),
            ..WhisperRuntimeLaunch::default()
        }
    }

    pub(crate) fn open_cached(
        root: &Path,
        echo_binary: &Path,
        state_root: &Path,
    ) -> Result<Self, String> {
        let installed = Self::parse(root, echo_binary)?;
        let stamp = installed.verification_stamp()?;
        let path = state_root.join("package-verification.json");
        let cached = fs::read(&path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<VerificationStamp>(&raw).ok());
        let trusted_cache_source = installed.cache_source_is_trusted()?;
        if !trusted_cache_source || cached.as_ref() != Some(&stamp) {
            installed.verify_files()?;
            if trusted_cache_source {
                fs::create_dir_all(state_root).map_err(|error| error.to_string())?;
                let raw = serde_json::to_vec_pretty(&stamp).map_err(|error| error.to_string())?;
                echo_core::write_atomic(&path, &raw)?;
            }
        }
        Ok(installed)
    }

    fn parse(root: &Path, echo_binary: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        if root.join("acceleration-set.v3.json").exists() || root.join("cache-seeds").exists() {
            return Err("portable selection package contains proof-only host material".to_string());
        }
        let selection_raw =
            fs::read(root.join("portable-selection.v1.json")).map_err(|error| error.to_string())?;
        let legacy_raw =
            fs::read(root.join("legacy-exact-index.v1.json")).map_err(|error| error.to_string())?;
        let binding_raw = fs::read(root.join("portable-selection-binding.v1.json"))
            .map_err(|error| error.to_string())?;
        let binding_digest = Sha256Digest::parse(format!("{:x}", Sha256::digest(&binding_raw)))
            .map_err(|error| error.to_string())?;
        let package =
            PortableSelectionPackage::from_bytes(&selection_raw, &legacy_raw, &binding_raw)?;
        let echo_binary = echo_binary
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let execution = &package.selection.execution_artifact.value;
        let runtime = package_file(&root, execution.runtime_relative_path.as_str())?;
        let probe = package_file(&root, execution.probe_relative_path.as_str())?;
        let calibration_fixture = package_file(
            &root,
            package.selection.calibration_fixture.relative_path.as_str(),
        )?;
        let build_receipt = package_file(
            &root,
            &format!(
                "{}/build-receipt.json",
                runtime
                    .parent()
                    .and_then(|parent| parent.strip_prefix(&root).ok())
                    .ok_or_else(|| "portable runtime path is invalid".to_string())?
                    .to_string_lossy()
            ),
        )?;
        Ok(Self {
            package,
            root,
            runtime,
            probe,
            calibration_fixture,
            echo_binary,
            build_receipt,
            binding_digest,
        })
    }

    fn verify_files(&self) -> Result<(), String> {
        let execution = &self.package.selection.execution_artifact.value;
        if self.package.binding.echo_binary_sha256 != sha256_file(&self.echo_binary)? {
            return Err("portable selection package belongs to another Echo binary".to_string());
        }
        if sha256_file(&self.runtime)? != execution.runtime_sha256
            || sha256_file(&self.probe)? != execution.probe_sha256
            || sha256_file(&self.build_receipt)? != execution.build_receipt_sha256
            || sha256_file(&self.calibration_fixture)?
                != self.package.selection.calibration_fixture.sha256
            || whisper_runtime_launch(&self.runtime)
                .identity_sha256
                .as_deref()
                != Some(execution.runtime_identity_sha256.as_str())
        {
            return Err("portable selection runtime files differ".to_string());
        }
        let bindings =
            runtime_library_bindings(&self.runtime).map_err(|error| error.to_string())?;
        if bindings.len() != execution.runtime_library_bindings.len()
            || bindings.iter().any(|(name, digest)| {
                execution
                    .runtime_library_bindings
                    .get(name)
                    .is_none_or(|expected| expected.as_str() != digest)
            })
        {
            return Err("portable runtime library bindings differ".to_string());
        }
        Ok(())
    }

    fn verification_stamp(&self) -> Result<VerificationStamp, String> {
        let mut paths = BTreeSet::from([
            self.echo_binary.clone(),
            self.root.join("portable-selection.v1.json"),
            self.root.join("legacy-exact-index.v1.json"),
            self.root.join("portable-selection-binding.v1.json"),
            self.runtime.clone(),
            self.probe.clone(),
            self.build_receipt.clone(),
            self.calibration_fixture.clone(),
        ]);
        let runtime_root = self
            .runtime
            .parent()
            .ok_or_else(|| "portable runtime has no parent".to_string())?;
        for name in self
            .package
            .selection
            .execution_artifact
            .value
            .runtime_library_bindings
            .keys()
        {
            paths.insert(runtime_root.join(name));
        }
        let files = paths
            .into_iter()
            .map(|path| verified_file_stamp(&path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VerificationStamp {
            schema_version: VERIFICATION_STAMP_SCHEMA,
            binding_digest: self.binding_digest.clone(),
            files,
        })
    }

    fn cache_source_is_trusted(&self) -> Result<bool, String> {
        let stamp = self.verification_stamp()?;
        if !root_owned_read_only(&self.root)? {
            return Ok(false);
        }
        for file in stamp.files {
            if !root_owned_read_only(&file.path)? {
                return Ok(false);
            }
            if file.link_target.is_some()
                && !root_owned_read_only(
                    &file
                        .path
                        .canonicalize()
                        .map_err(|error| error.to_string())?,
                )?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
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

fn package_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("portable selection path escapes its package".to_string());
    }
    Ok(path)
}

pub(crate) fn sha256_file(path: &Path) -> Result<Sha256Digest, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Sha256Digest::parse(format!("{:x}", digest.finalize())).map_err(|error| error.to_string())
}

fn verified_file_stamp(path: &Path) -> Result<VerifiedFileStamp, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
        return Err("portable verification input is not a file or symlink".to_string());
    }
    let link_target = if metadata.file_type().is_symlink() {
        Some(fs::read_link(path).map_err(|error| error.to_string())?)
    } else {
        None
    };
    Ok(VerifiedFileStamp {
        path: path.to_path_buf(),
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
        link_target,
    })
}

fn root_owned_read_only(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    Ok(metadata.uid() == 0 && metadata.mode() & 0o022 == 0)
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
            calibration_fixture: CalibrationFixture {
                relative_path: SafeRelativePath::parse(
                    "calibration/english-canary.wav".to_string(),
                )
                .unwrap(),
                sha256: Sha256Digest::parse("8".repeat(64)).unwrap(),
            },
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
            source_acceleration_set_sha256: Sha256Digest::parse("9".repeat(64)).unwrap(),
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

    #[test]
    fn verification_stamp_changes_when_same_size_file_changes() {
        let root = std::env::temp_dir().join(format!("echo-portable-stamp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("runtime");
        fs::write(&path, b"a").unwrap();
        let before = verified_file_stamp(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(&path, b"b").unwrap();
        assert_ne!(before, verified_file_stamp(&path).unwrap());
    }

    #[test]
    fn contract_resolution_requires_one_exact_qualified_scope() {
        let (mut selection, _, _) = documents();
        let first = selection.inference_contracts[0].clone();
        let mut second = first.clone();
        second.value.tuning.threads += 1;
        second.id = InferenceContractId::of(&second.value).unwrap();
        selection.inference_contracts.push(second);
        selection
            .inference_contracts
            .sort_by(|left, right| left.id.cmp(&right.id));
        assert!(resolve_qualified_contract(
            &selection.inference_contracts,
            &first.value.model_sha256,
            &first.value.vad_sha256,
            None,
        )
        .is_err());
        assert_eq!(
            resolve_qualified_contract(
                &selection.inference_contracts,
                &first.value.model_sha256,
                &first.value.vad_sha256,
                Some(&first.id),
            )
            .unwrap()
            .map(|contract| &contract.id),
            Some(&first.id)
        );
    }
}
