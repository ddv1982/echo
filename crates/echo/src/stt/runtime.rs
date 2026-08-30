use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use echo_core::{WhisperRuntimeBackend, WhisperRuntimeSource};
use sha2::{Digest, Sha256};

use crate::install::{ComponentId, ManagedPath, ManagedStore};
use crate::which::path_of;

use super::{
    InstalledModel, ModelCache, ModelInventory, WhisperFamily, WhisperRuntimeCandidate,
    WhisperRuntimeLaunch,
};

pub(crate) fn whisper_runtime_launch(cli: &Path) -> WhisperRuntimeLaunch {
    let Some(parent) = cli.parent() else {
        return WhisperRuntimeLaunch::default();
    };
    let libraries = std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".so"))
        .filter_map(|entry| entry.path().canonicalize().ok())
        .filter(|path| path.is_file())
        .collect::<BTreeSet<_>>();
    let identity_sha256 = runtime_identity(cli, &libraries).ok();
    WhisperRuntimeLaunch {
        library_dir: (!libraries.is_empty()).then(|| parent.to_path_buf()),
        identity_sha256,
        ..WhisperRuntimeLaunch::default()
    }
}

fn runtime_identity(cli: &Path, libraries: &BTreeSet<PathBuf>) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"echo-whisper-runtime-v1\0");
    for path in std::iter::once(cli.to_path_buf()).chain(unique_libraries(libraries)?) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        hasher.update(u64::try_from(name.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(name.as_bytes());
        let mut file = std::fs::File::open(&path)?;
        hasher.update(file.metadata()?.len().to_le_bytes());
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn unique_libraries(libraries: &BTreeSet<PathBuf>) -> std::io::Result<Vec<PathBuf>> {
    let mut by_content = BTreeMap::<(u64, [u8; 32]), PathBuf>::new();
    for path in libraries {
        let size = path.metadata()?.len();
        let digest = file_sha256(path)?;
        by_content
            .entry((size, digest))
            .and_modify(|selected| {
                let rank = |value: &Path| {
                    let name = value.file_name().unwrap_or_default().to_string_lossy();
                    (name.len(), name.into_owned())
                };
                if rank(path) > rank(selected) {
                    *selected = path.clone();
                }
            })
            .or_insert_with(|| path.clone());
    }
    let mut selected = by_content.into_values().collect::<Vec<_>>();
    selected.sort();
    Ok(selected)
}

fn file_sha256(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

pub struct ManagedSelection {
    pub paths: Vec<PathBuf>,
    pub leases: Vec<ManagedPath>,
}

pub struct SpeechRuntimeInventory {
    pub models: ModelInventory,
    pub whisper_runtimes: Vec<WhisperRuntimeCandidate>,
    pub parakeet_binary: Option<PathBuf>,
    store: ManagedStore,
    managed_roots: BTreeMap<ComponentId, PathBuf>,
    provenance: BTreeMap<PathBuf, ComponentId>,
}

impl SpeechRuntimeInventory {
    #[must_use]
    pub fn from_cache(cache: &ModelCache) -> Self {
        let store = ManagedStore::new(cache.dir());
        let mut managed_roots = BTreeMap::new();
        let mut provenance = BTreeMap::new();
        let mut active = |id| {
            let root = store.candidate_root(id)?;
            managed_roots.insert(id, root.clone());
            Some(root)
        };
        let mut whisper_runtimes = Vec::new();
        if let Some(root) = active(ComponentId::WhisperRuntime) {
            let cli = root.join("whisper-cli");
            let server = root.join("whisper-server");
            if cli.is_file() {
                provenance.insert(cli.clone(), ComponentId::WhisperRuntime);
                let server = server.is_file().then(|| {
                    provenance.insert(server.clone(), ComponentId::WhisperRuntime);
                    server
                });
                whisper_runtimes.push(WhisperRuntimeCandidate {
                    source: WhisperRuntimeSource::Managed,
                    backend: WhisperRuntimeBackend::Cpu,
                    launch: whisper_runtime_launch(&cli),
                    cli,
                    server,
                });
            }
        }
        if let Some(root) = active(ComponentId::WhisperVulkanRuntime) {
            let cli = root.join("whisper-cli");
            if cli.is_file() {
                provenance.insert(cli.clone(), ComponentId::WhisperVulkanRuntime);
                whisper_runtimes.push(WhisperRuntimeCandidate {
                    source: WhisperRuntimeSource::Managed,
                    backend: WhisperRuntimeBackend::Vulkan,
                    launch: whisper_runtime_launch(&cli),
                    cli,
                    server: None,
                });
            }
        }
        if let Some(cli) = ["whisper-cli", "whisper-cpp", "whisper"]
            .into_iter()
            .find_map(path_of)
        {
            let sibling = cli.parent().map(|parent| parent.join("whisper-server"));
            let server = sibling
                .filter(|path| path.is_file())
                .or_else(|| path_of("whisper-server"));
            if whisper_runtimes
                .iter()
                .all(|candidate| candidate.cli != cli)
            {
                whisper_runtimes.push(WhisperRuntimeCandidate {
                    source: WhisperRuntimeSource::System,
                    backend: WhisperRuntimeBackend::Unknown,
                    launch: whisper_runtime_launch(&cli),
                    cli,
                    server,
                });
            }
        }
        let parakeet_binary = active(ComponentId::SherpaRuntime)
            .map(|root| {
                let path = root.join("sherpa-onnx-offline");
                provenance.insert(path.clone(), ComponentId::SherpaRuntime);
                path
            })
            .filter(|path| path.is_file())
            .or_else(|| {
                ["sherpa-onnx-offline", "sherpa-onnx"]
                    .into_iter()
                    .find_map(path_of)
            });
        let mut models = cache.inventory();
        for (id, name, family, quantisation) in [
            (
                ComponentId::WhisperBaseQ51,
                "base-q5_1",
                WhisperFamily::Base,
                Some("q5_1"),
            ),
            (
                ComponentId::WhisperSmall,
                "small",
                WhisperFamily::Small,
                None,
            ),
            (
                ComponentId::WhisperLargeV3TurboQ50,
                "large-v3-turbo-q5_0",
                WhisperFamily::LargeV3Turbo,
                Some("q5_0"),
            ),
        ] {
            if let Some(root) = active(id) {
                let path = root.join(format!("ggml-{name}.bin"));
                if path.is_file() {
                    provenance.insert(path.clone(), id);
                    models.whisper.retain(|model| model.name != name);
                    models.whisper.push(InstalledModel {
                        name: name.to_string(),
                        path: path.clone(),
                        family,
                        multilingual: true,
                        quantisation: quantisation.map(str::to_string),
                        size_bytes: path.metadata().map(|metadata| metadata.len()).unwrap_or(0),
                    });
                }
            }
        }
        if let Some(root) = active(ComponentId::SileroVad) {
            let path = root.join("ggml-silero-v6.2.0.bin");
            if path.is_file() {
                provenance.insert(path.clone(), ComponentId::SileroVad);
                models.vad.insert(0, path);
            }
        }
        if let Some(root) = active(ComponentId::ParakeetTdt06bV3Int8) {
            if parakeet_present(&root) {
                provenance.insert(root.clone(), ComponentId::ParakeetTdt06bV3Int8);
                models.parakeet = Some(root);
            }
        }
        Self {
            models,
            whisper_runtimes,
            parakeet_binary,
            store,
            managed_roots,
            provenance,
        }
    }

    pub fn lock_selected(&self, paths: &[PathBuf]) -> Result<ManagedSelection, String> {
        let components: BTreeSet<_> = paths
            .iter()
            .filter_map(|path| self.provenance.get(path).copied())
            .collect();
        let mut locked = BTreeMap::new();
        for component in components {
            let managed = self
                .store
                .active_root_leased(component)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    format!(
                        "managed {} was removed during resolution",
                        component.as_str()
                    )
                })?;
            locked.insert(component, managed);
        }
        let resolved = paths
            .iter()
            .map(|path| {
                let Some(component) = self.provenance.get(path) else {
                    return Ok(path.clone());
                };
                let old_root = self
                    .managed_roots
                    .get(component)
                    .ok_or_else(|| "managed component has no discovery root".to_string())?;
                let relative = path
                    .strip_prefix(old_root)
                    .map_err(|_| "managed path escaped its discovery root".to_string())?;
                Ok(locked
                    .get(component)
                    .expect("component was locked")
                    .root
                    .join(relative))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(ManagedSelection {
            paths: resolved,
            leases: locked.into_values().collect(),
        })
    }
}

fn parakeet_present(root: &Path) -> bool {
    [
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
    ]
    .iter()
    .all(|name| root.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::catalog::{component, PayloadKind};
    use crate::install::{ActivationRecord, InstalledFile};

    #[test]
    fn runtime_identity_covers_cli_and_adjacent_libraries() {
        let root =
            std::env::temp_dir().join(format!("echo-runtime-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let cli = root.join("whisper-cli");
        let library = root.join("libwhisper.so.1");
        std::fs::write(&cli, b"cli").unwrap();
        std::fs::write(&library, b"library-v1").unwrap();
        let first = whisper_runtime_launch(&cli);
        assert_eq!(first.library_dir.as_deref(), Some(root.as_path()));
        assert_eq!(first.identity_sha256.as_deref().map(str::len), Some(64));
        std::fs::write(root.join("libwhisper.so"), b"library-v1").unwrap();
        std::fs::write(root.join("libwhisper.so.0"), b"library-v1").unwrap();
        assert_eq!(
            first.identity_sha256,
            whisper_runtime_launch(&cli).identity_sha256
        );
        std::fs::write(&library, b"library-v2").unwrap();
        let second = whisper_runtime_launch(&cli);
        assert_ne!(first.identity_sha256, second.identity_sha256);
        std::fs::write(&cli, b"cli-v2").unwrap();
        let third = whisper_runtime_launch(&cli);
        assert_ne!(second.identity_sha256, third.identity_sha256);
    }

    #[test]
    fn runtime_identity_ignores_a_changed_loader_alias() {
        let root = std::env::temp_dir().join(format!(
            "echo-runtime-library-bindings-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let cli = root.join("whisper-cli");
        std::fs::write(&cli, b"cli").unwrap();
        std::fs::write(root.join("libwhisper.so.1.9.2"), b"whisper").unwrap();
        std::fs::write(root.join("libwhisper.so.1"), b"whisper").unwrap();
        std::fs::write(root.join("libggml.so.0.18.1"), b"ggml").unwrap();

        let original_identity = whisper_runtime_launch(&cli).identity_sha256;
        std::fs::write(root.join("libwhisper.so.1"), b"ggml").unwrap();

        assert_eq!(
            original_identity,
            whisper_runtime_launch(&cli).identity_sha256
        );
    }

    fn install_sparse_managed_model(root: &Path, id: ComponentId) -> PathBuf {
        let spec = component(id);
        let release_name = format!("{}-1", spec.artifact_sha256);
        let release = root
            .join("managed/components")
            .join(id.as_str())
            .join("releases")
            .join(&release_name);
        let payload = release.join("payload");
        std::fs::create_dir_all(&payload).unwrap();
        let model = payload.join(spec.artifact_name);
        std::fs::File::create(&model)
            .unwrap()
            .set_len(spec.artifact_size)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&model, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let record = ActivationRecord {
            schema_version: 1,
            component: id,
            version: spec.version.to_string(),
            release: release_name,
            artifact_sha256: spec.artifact_sha256.to_string(),
            files: vec![InstalledFile {
                relative_path: spec.artifact_name.to_string(),
                size: spec.artifact_size,
                sha256: spec.artifact_sha256.to_string(),
                mode: 0o644,
                kind: PayloadKind::File,
                link_target: None,
            }],
        };
        let raw = serde_json::to_vec(&record).unwrap();
        echo_core::write_atomic(&release.join("receipt.json"), &raw).unwrap();
        echo_core::write_atomic(
            &root
                .join("managed/active")
                .join(format!("{}.json", id.as_str())),
            &raw,
        )
        .unwrap();
        crate::install::trust_payload_fixture(&payload, &record.files);
        model
    }

    #[test]
    fn empty_managed_store_preserves_external_inventory() {
        let root = std::env::temp_dir().join(format!("echo-runtime-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("ggml-small.bin"), []).unwrap();
        let inventory = SpeechRuntimeInventory::from_cache(&ModelCache::at(&root));
        assert!(inventory
            .models
            .whisper
            .iter()
            .any(|model| model.name == "small"));
    }

    #[test]
    fn healthy_managed_model_wins_and_corruption_falls_back_to_manual() {
        let root =
            std::env::temp_dir().join(format!("echo-runtime-managed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let manual = root.join("ggml-small.bin");
        std::fs::write(&manual, []).unwrap();
        let managed = install_sparse_managed_model(&root, ComponentId::WhisperSmall);
        let inventory = SpeechRuntimeInventory::from_cache(&ModelCache::at(&root));
        assert_eq!(inventory.models.best_whisper().unwrap().path, managed);
        assert_eq!(
            inventory
                .models
                .whisper
                .iter()
                .find(|model| model.name == "small")
                .unwrap()
                .path,
            managed
        );
        let locked = inventory
            .lock_selected(std::slice::from_ref(&managed))
            .unwrap();
        assert_eq!(locked.paths, vec![managed.clone()]);
        assert_eq!(locked.leases.len(), 1);
        drop(locked);

        std::fs::File::options()
            .write(true)
            .open(&managed)
            .unwrap()
            .set_len(1)
            .unwrap();
        let inventory = SpeechRuntimeInventory::from_cache(&ModelCache::at(&root));
        assert_eq!(inventory.models.best_whisper().unwrap().path, manual);
    }
}
