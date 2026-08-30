use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::catalog::{self, component, ArtifactFormat, ComponentId};
use super::download::{download_verified, forget_partial, DiskSpace, DownloadSpec, HttpTransport};
use super::extract::extract_archive;
use super::payload::{
    copy_cancellable, expected_files_for, extraction_plan, verify_payload_cancellable,
};
use super::store::ManagedStore;
use super::types::{
    ActivationRecord, InstallError, InstallPhase, InstallProgress, ManagedComponentState,
    OperationId,
};

pub struct Installer<'a> {
    pub store: ManagedStore,
    pub transport: &'a dyn HttpTransport,
    pub disk: &'a dyn DiskSpace,
    pub probe: &'a dyn RuntimeProbe,
}

pub trait RuntimeProbe: Send + Sync {
    fn probe(
        &self,
        component: ComponentId,
        binary: &Path,
        cancel: &AtomicBool,
    ) -> Result<(), InstallError>;
}

#[derive(Default)]
pub struct CommandRuntimeProbe;

impl RuntimeProbe for CommandRuntimeProbe {
    fn probe(
        &self,
        component: ComponentId,
        binary: &Path,
        cancel: &AtomicBool,
    ) -> Result<(), InstallError> {
        // Run it the way the app runs it. Every launch path sets the library
        // directory from the binary's own folder, because a managed runtime
        // ships its shared objects beside the executable and is not on the
        // system library path. Probing without it tests a configuration that
        // never occurs, and rejects any payload that does not happen to carry
        // an $ORIGIN runpath.
        let mut command = std::process::Command::new(binary);
        if let Some(parent) = binary.parent() {
            command.env("LD_LIBRARY_PATH", parent);
        }
        let mut child = command
            .arg("--help")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| {
                InstallError::Probe(format!("cannot run {}: {error}", binary.display()))
            })?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(InstallError::Cancelled);
            }
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(InstallError::Probe(format!(
                        "{} failed its runtime probe with {status}",
                        component.as_str()
                    )))
                };
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(InstallError::Probe(format!(
                    "{} did not answer its runtime probe within 30 seconds",
                    component.as_str()
                )));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Installer<'_> {
    pub fn ensure_plan(
        &self,
        components: &[ComponentId],
        repair: bool,
        operation: &OperationId,
        cancel: &AtomicBool,
        mut progress: impl FnMut(InstallProgress),
    ) -> Result<Vec<ActivationRecord>, InstallError> {
        let mut records = Vec::with_capacity(components.len());
        for id in components {
            records.push(self.ensure_component(*id, repair, operation, cancel, &mut progress)?);
        }
        Ok(records)
    }

    pub fn ensure_component(
        &self,
        id: ComponentId,
        repair: bool,
        operation: &OperationId,
        cancel: &AtomicBool,
        progress: impl FnMut(InstallProgress),
    ) -> Result<ActivationRecord, InstallError> {
        self.ensure_spec(component(id), repair, operation, cancel, progress)
    }

    pub(super) fn ensure_spec(
        &self,
        spec: &catalog::ComponentSpec,
        repair: bool,
        operation: &OperationId,
        cancel: &AtomicBool,
        mut progress: impl FnMut(InstallProgress),
    ) -> Result<ActivationRecord, InstallError> {
        let _operation = self.store.operation_shared()?;
        let id = spec.id;
        let expected = expected_files_for(spec);
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        if matches!(
            self.store.status_with(spec, &expected, repair),
            ManagedComponentState::Ready { .. }
        ) {
            return self.store.read_active(id)?.ok_or_else(|| {
                InstallError::State("ready component lost its activation".to_string())
            });
        }
        let download = DownloadSpec::from(spec);
        let artifact = download_verified(
            self.store.root(),
            &download,
            self.transport,
            self.disk,
            operation,
            cancel,
            &mut progress,
        )?;
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let stage = self
            .store
            .managed()
            .join("staging")
            .join(operation.as_str())
            .join(id.as_str());
        if stage.exists() {
            fs::remove_dir_all(&stage)?;
        }
        let payload = stage.join("payload");
        fs::create_dir_all(&payload)?;
        progress(InstallProgress::new(
            operation,
            id,
            InstallPhase::Extracting,
            0,
            spec.installed_bytes,
            0,
        ));
        let installed = (|| {
            match spec.format {
                ArtifactFormat::Direct => {
                    let destination = payload.join(spec.artifact_name);
                    copy_cancellable(&artifact, &destination, cancel)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&destination, fs::Permissions::from_mode(0o644))?;
                    }
                }
                ArtifactFormat::TarGzip | ArtifactFormat::TarBzip2 => extract_archive(
                    &artifact,
                    &payload,
                    &extraction_plan(id).expect("archive has extraction plan"),
                    cancel,
                )?,
            }
            verify_payload_cancellable(&payload, &expected, true, Some(cancel))?;
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallError::Cancelled);
            }
            let runtime_binary = match id {
                ComponentId::WhisperRuntime | ComponentId::WhisperVulkanRuntime => {
                    Some(payload.join("whisper-cli"))
                }
                ComponentId::SherpaRuntime => Some(payload.join("sherpa-onnx-offline")),
                ComponentId::WhisperBaseQ51
                | ComponentId::WhisperSmall
                | ComponentId::WhisperLargeV3TurboQ50
                | ComponentId::SileroVad
                | ComponentId::ParakeetTdt06bV3Int8 => None,
            };
            if let Some(binary) = runtime_binary {
                self.probe.probe(id, &binary, cancel)?;
                verify_payload_cancellable(&payload, &expected, true, Some(cancel))?;
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallError::Cancelled);
            }
            progress(InstallProgress::new(
                operation,
                id,
                InstallPhase::Activating,
                spec.installed_bytes,
                spec.installed_bytes,
                0,
            ));
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallError::Cancelled);
            }
            self.store.activate_with(spec, expected, &stage, operation)
        })();
        let record = match installed {
            Ok(record) => record,
            Err(error) => {
                let _ = fs::remove_dir_all(&stage);
                return Err(error);
            }
        };
        forget_partial(self.store.root(), &download);
        Ok(record)
    }
}
