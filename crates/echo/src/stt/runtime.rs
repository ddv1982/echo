use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::install::{ComponentId, ManagedPath, ManagedStore};
use crate::which::path_of;

use super::{InstalledModel, ModelCache, ModelInventory, WhisperFamily};

pub struct ManagedSelection {
    pub paths: Vec<PathBuf>,
    pub leases: Vec<ManagedPath>,
}

pub struct SpeechRuntimeInventory {
    pub models: ModelInventory,
    pub whisper_binary: Option<PathBuf>,
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
        let whisper_binary = active(ComponentId::WhisperRuntime)
            .map(|root| {
                let path = root.join("whisper-cli");
                provenance.insert(path.clone(), ComponentId::WhisperRuntime);
                path
            })
            .filter(|path| path.is_file())
            .or_else(|| ["whisper-cli", "whisper-cpp", "whisper"].into_iter().find_map(path_of));
        let parakeet_binary = active(ComponentId::SherpaRuntime)
            .map(|root| {
                let path = root.join("sherpa-onnx-offline");
                provenance.insert(path.clone(), ComponentId::SherpaRuntime);
                path
            })
            .filter(|path| path.is_file())
            .or_else(|| ["sherpa-onnx-offline", "sherpa-onnx"].into_iter().find_map(path_of));
        let mut models = cache.inventory();
        for (id, name, family, quantisation) in [
            (ComponentId::WhisperBaseQ51, "base-q5_1", WhisperFamily::Base, Some("q5_1")),
            (ComponentId::WhisperSmall, "small", WhisperFamily::Small, None),
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
            whisper_binary,
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
                .ok_or_else(|| format!("managed {} was removed during resolution", component.as_str()))?;
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
    use crate::install::{ActivationRecord, InstalledFile};
    use crate::install::catalog::{component, PayloadKind};

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
        model
    }

    #[test]
    fn empty_managed_store_preserves_external_inventory() {
        let root = std::env::temp_dir().join(format!("echo-runtime-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("ggml-small.bin"), []).unwrap();
        let inventory = SpeechRuntimeInventory::from_cache(&ModelCache::at(&root));
        assert!(inventory.models.whisper.iter().any(|model| model.name == "small"));
    }

    #[test]
    fn healthy_managed_model_wins_and_corruption_falls_back_to_manual() {
        let root = std::env::temp_dir().join(format!("echo-runtime-managed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let manual = root.join("ggml-small.bin");
        std::fs::write(&manual, []).unwrap();
        let managed = install_sparse_managed_model(&root, ComponentId::WhisperSmall);
        let inventory = SpeechRuntimeInventory::from_cache(&ModelCache::at(&root));
        assert_eq!(inventory.models.best_whisper().unwrap().path, managed);
        let locked = inventory.lock_selected(std::slice::from_ref(&managed)).unwrap();
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
