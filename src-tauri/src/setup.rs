use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use echo::install::catalog::{component, managed_platform_supported, plan, recommended_model};
use echo::install::{
    CommandRuntimeProbe, ComponentId as CoreComponentId, DiskSpace,
    InstallProgress as CoreInstallProgress, Installer,
    ManagedComponentState as CoreManagedComponentState, ManagedStore, OperationId,
    SetupPlanId as CoreSetupPlanId, SystemDisk, UreqTransport,
};
use echo::stt::ModelCache;
use echo_desktop::ipc::{
    ActiveComponentOrigin, ComponentId, ComponentOrigin, ComponentStatus, ExternalComponent,
    InstallProgress, ManagedComponentState, Readiness, SetupEvent, SetupPlan, SetupPlanId,
};
use tauri::{Emitter, Manager, State};

#[derive(Debug, Clone, PartialEq, Eq)]
enum SetupAction {
    Plan(CoreSetupPlanId, bool),
    Repair(CoreComponentId),
    Remove(CoreComponentId),
    Verify(CoreComponentId),
}

struct ActiveOperation {
    id: OperationId,
    action: SetupAction,
    cancel: Arc<AtomicBool>,
    progress: Option<CoreInstallProgress>,
}

fn lock_active_operation(
    active: &Mutex<Option<ActiveOperation>>,
) -> std::sync::MutexGuard<'_, Option<ActiveOperation>> {
    match active.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("setup: recovering poisoned active-operation state");
            active.clear_poison();
            poisoned.into_inner()
        }
    }
}

#[derive(Clone)]
pub struct SetupService {
    active: Arc<Mutex<Option<ActiveOperation>>>,
}

impl Default for SetupService {
    fn default() -> Self {
        let cache = ModelCache::from_env();
        for problem in ManagedStore::new(cache.dir()).recover() {
            eprintln!("managed setup recovery: {problem}");
        }
        Self {
            active: Arc::new(Mutex::new(None)),
        }
    }
}

fn total_memory_bytes() -> Option<u64> {
    let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
    raw.lines().find_map(|line| {
        let kib = line
            .strip_prefix("MemTotal:")?
            .trim()
            .strip_suffix("kB")?
            .trim();
        kib.parse::<u64>().ok()?.checked_mul(1024)
    })
}

fn external_components(
    inventory: &echo::stt::ModelInventory,
    whisper_runtime: Option<&std::path::Path>,
    sherpa_runtime: Option<&std::path::Path>,
    id: CoreComponentId,
) -> Vec<ExternalComponent> {
    match id {
        CoreComponentId::WhisperRuntime => whisper_runtime
            .map(|path| {
                vec![ExternalComponent {
                    origin: ComponentOrigin::System,
                    path: path.to_string_lossy().into_owned(),
                }]
            })
            .unwrap_or_default(),
        CoreComponentId::SherpaRuntime => sherpa_runtime
            .map(|path| {
                vec![ExternalComponent {
                    origin: ComponentOrigin::System,
                    path: path.to_string_lossy().into_owned(),
                }]
            })
            .unwrap_or_default(),
        // GPU selection uses Echo's managed Vulkan probe, so a system
        // whisper-cli does not satisfy this managed component.
        CoreComponentId::WhisperVulkanRuntime => Vec::new(),
        CoreComponentId::WhisperBaseQ51 => external_model(inventory, "base-q5_1"),
        CoreComponentId::WhisperSmall => external_model(inventory, "small"),
        CoreComponentId::WhisperLargeV3TurboQ50 => external_model(inventory, "large-v3-turbo-q5_0"),
        CoreComponentId::SileroVad => inventory
            .vad
            .iter()
            .map(|path| ExternalComponent {
                origin: ComponentOrigin::External,
                path: path.to_string_lossy().into_owned(),
            })
            .collect(),
        CoreComponentId::ParakeetTdt06bV3Int8 => inventory
            .parakeet
            .clone()
            .map(|path| {
                vec![ExternalComponent {
                    origin: ComponentOrigin::External,
                    path: path.to_string_lossy().into_owned(),
                }]
            })
            .unwrap_or_default(),
    }
}

fn external_model(inventory: &echo::stt::ModelInventory, name: &str) -> Vec<ExternalComponent> {
    inventory
        .whisper
        .iter()
        .filter(|model| model.name == name)
        .map(|model| ExternalComponent {
            origin: ComponentOrigin::External,
            path: model.path.to_string_lossy().into_owned(),
        })
        .collect()
}

fn managed_ready(state: &CoreManagedComponentState) -> bool {
    matches!(state, CoreManagedComponentState::Ready { .. })
}

fn plan_space(components: impl IntoIterator<Item = (u64, u64, u64)>) -> (u64, u64) {
    let mut download_bytes = 0u64;
    let mut retained_growth = 0u64;
    let mut required_free_bytes = 0u64;
    for (artifact_bytes, installed_bytes, resumable_bytes) in components {
        let download = artifact_bytes.saturating_sub(resumable_bytes);
        let working = download.saturating_add(installed_bytes);
        let component_required = working.saturating_add((working / 10).max(256 * 1024 * 1024));
        required_free_bytes =
            required_free_bytes.max(retained_growth.saturating_add(component_required));
        retained_growth =
            retained_growth.saturating_add(installed_bytes.saturating_sub(resumable_bytes));
        download_bytes = download_bytes.saturating_add(download);
    }
    (download_bytes, required_free_bytes)
}

impl SetupService {
    #[must_use]
    pub(crate) fn snapshot(&self) -> Readiness {
        let runtime = echo::stt::SpeechRuntimeInventory::from_cache(&ModelCache::from_env());
        let (file, config_error) = echo::settings::config_for_display();
        let env = echo::transcribe::EnvOptions::read();
        let available =
            echo::transcribe::EngineAvailabilitySnapshot::for_process(&env, &file, &runtime);
        let resolved = echo::transcribe::resolve_run(&Default::default(), &env, &file, &available);
        let resolved = if config_error.is_none() {
            resolved.as_ref().ok()
        } else {
            None
        };
        self.snapshot_from(&file, &runtime, resolved)
    }

    pub(crate) fn snapshot_from(
        &self,
        file: &echo_core::Config,
        runtime: &echo::stt::SpeechRuntimeInventory,
        resolved: Option<&echo::transcribe::ResolvedRun>,
    ) -> Readiness {
        let cache = &runtime.cache;
        let (active_operation, active_cancellable, activity) = {
            let active = lock_active_operation(&self.active);
            (
                active
                    .as_ref()
                    .map(|operation| operation.id.as_str().to_string()),
                active.as_ref().is_some_and(|operation| {
                    matches!(
                        operation.action,
                        SetupAction::Plan(..) | SetupAction::Repair(..)
                    )
                }),
                active
                    .as_ref()
                    .and_then(|operation| operation.progress.clone()),
            )
        };
        let mut components = Vec::new();
        for spec in echo::install::catalog::COMPONENTS {
            let managed = if managed_platform_supported() {
                runtime.managed[&spec.id].clone()
            } else {
                CoreManagedComponentState::Unsupported {
                    reason: "Echo-managed speech setup supports Linux x86_64 only".to_string(),
                }
            };
            let external = external_components(
                &runtime.external_models,
                runtime.system_whisper.as_deref(),
                runtime.system_sherpa.as_deref(),
                spec.id,
            );
            let active_origin = if managed_ready(&managed) {
                Some(ActiveComponentOrigin::Managed)
            } else if !external.is_empty() {
                Some(external[0].origin.into())
            } else {
                None
            };
            components.push(ComponentStatus {
                id: spec.id.into(),
                label: spec.label.to_string(),
                managed: managed.into(),
                external,
                active_origin,
                activity: activity
                    .as_ref()
                    .filter(|progress| progress.component == spec.id)
                    .cloned()
                    .map(InstallProgress::from),
            });
        }
        let total_memory_bytes = total_memory_bytes();
        let plan_ids = [
            CoreSetupPlanId::Recommended,
            CoreSetupPlanId::Parakeet,
            CoreSetupPlanId::WhisperBase,
            CoreSetupPlanId::WhisperSmall,
            CoreSetupPlanId::WhisperLargeV3Turbo,
        ];
        let _ = std::fs::create_dir_all(cache.dir());
        let available_bytes = SystemDisk.available_bytes(cache.dir()).ok().flatten();
        let plans = plan_ids
            .into_iter()
            .map(|id| {
                let ids = plan(id);
                let satisfied = ids.iter().all(|id| {
                    components
                        .iter()
                        .find(|component| component.id == (*id).into())
                        .is_some_and(|component| {
                            matches!(component.managed, ManagedComponentState::Ready { .. })
                                || !component.external.is_empty()
                        })
                });
                let (download_bytes, required_free_bytes) = plan_space(
                    ids.iter().filter_map(|component_id| {
                        let status = components
                            .iter()
                            .find(|component| component.id == (*component_id).into())?;
                        if matches!(status.managed, ManagedComponentState::Ready { .. })
                            || !status.external.is_empty()
                        {
                            return None;
                        }
                        let resumable = match status.managed {
                            ManagedComponentState::Absent { resumable_bytes }
                            | ManagedComponentState::NeedsRepair {
                                resumable_bytes, ..
                            } => resumable_bytes,
                            ManagedComponentState::Ready { .. }
                            | ManagedComponentState::Unsupported { .. } => 0,
                        };
                        let spec = component(*component_id);
                        Some((spec.artifact_size, spec.installed_bytes, resumable))
                    }),
                );
                let disk_ready = available_bytes.is_none_or(|available| available >= required_free_bytes);
                SetupPlan {
                    id: id.into(),
                    label: match id {
                        CoreSetupPlanId::Recommended => "Recommended",
                        CoreSetupPlanId::Parakeet => "Parakeet",
                        CoreSetupPlanId::WhisperBase => "Whisper Base",
                        CoreSetupPlanId::WhisperSmall => "Whisper Small",
                        CoreSetupPlanId::WhisperLargeV3Turbo => "Whisper Large v3 Turbo Q5_0",
                    }
                    .to_string(),
                    download_bytes,
                    required_free_bytes,
                    available_bytes,
                    disk_ready,
                    disk_reason: (!disk_ready).then(|| {
                        format!(
                            "Needs {required_free_bytes} bytes free; {available} bytes are available",
                            available = available_bytes.unwrap_or(0)
                        )
                    }),
                    components: ids.into_iter().map(Into::into).collect(),
                    satisfied,
                }
            })
            .collect();
        let microphone_ready = echo::audio::AudioCapture::default_input_ready().is_ok();
        let speech_ready = resolved.is_some_and(|run| {
            echo::transcribe::prepare_resolved(Default::default(), file, run.clone(), runtime)
                .is_ok()
        });
        let has_successful_dictation =
            echo_core::History::load_read_only()
                .ok()
                .is_some_and(|history| {
                    history
                        .rows()
                        .iter()
                        .any(|row| !row.text.trim().is_empty() && !row.inject.failed())
                });
        Readiness {
            managed_supported: managed_platform_supported(),
            unsupported_reason: (!managed_platform_supported()).then(|| {
                "Managed setup is available on Linux x86_64. Use a system runtime and manual models on this platform."
                    .to_string()
            }),
            total_memory_bytes,
            recommended_model: recommended_model().into(),
            components,
            plans,
            microphone_ready,
            speech_ready,
            has_successful_dictation,
            first_run_complete: microphone_ready && speech_ready && has_successful_dictation,
            active_operation,
            active_cancellable,
        }
    }

    fn start(&self, action: SetupAction, app: tauri::AppHandle) -> Result<OperationId, String> {
        if !managed_platform_supported() {
            return Err("Echo-managed speech setup supports Linux x86_64 only".to_string());
        }
        let mut active = lock_active_operation(&self.active);
        if let Some(operation) = active.as_ref() {
            if operation.action == action {
                return Ok(operation.id.clone());
            }
            return Err("another speech setup operation is already running".to_string());
        }
        let id = OperationId::new();
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveOperation {
            id: id.clone(),
            action: action.clone(),
            cancel: cancel.clone(),
            progress: None,
        });
        drop(active);
        let active_state = self.active.clone();
        let worker_state = active_state.clone();
        let operation_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("echo-setup-{}", id.as_str()))
            .spawn(move || {
                let cache = ModelCache::from_env();
                let transport = UreqTransport::default();
                let disk = SystemDisk;
                let probe = CommandRuntimeProbe;
                let installer = Installer {
                    store: ManagedStore::new(cache.dir()),
                    transport: &transport,
                    disk: &disk,
                    probe: &probe,
                };
                let config_service = app
                    .state::<crate::settings::ConfigMutationService>()
                    .inner()
                    .clone();
                let mut last_progress_phase = None;
                let mut last_progress_emit = Instant::now() - Duration::from_secs(1);
                let mut emit_progress = |progress: CoreInstallProgress| {
                    let phase_changed = last_progress_phase != Some(progress.phase);
                    let complete =
                        progress.total_bytes > 0 && progress.received_bytes >= progress.total_bytes;
                    if !phase_changed
                        && !complete
                        && last_progress_emit.elapsed() < Duration::from_millis(100)
                    {
                        return;
                    }
                    last_progress_phase = Some(progress.phase);
                    last_progress_emit = Instant::now();
                    if let Some(active) = lock_active_operation(&worker_state).as_mut() {
                        if active.id == operation_id {
                            active.progress = Some(progress.clone());
                        }
                    }
                    let _ = app.emit(
                        "setup-event",
                        SetupEvent::Progress {
                            progress: progress.into(),
                        },
                    );
                };
                let result = match action {
                    SetupAction::Plan(plan_id, managed_copy) => {
                        let inventory = cache.inventory();
                        let whisper_runtime = ["whisper-cli", "whisper-cpp", "whisper"]
                            .into_iter()
                            .find_map(echo::which::path_of);
                        let sherpa_runtime = ["sherpa-onnx-offline", "sherpa-onnx"]
                            .into_iter()
                            .find_map(echo::which::path_of);
                        let components = plan(plan_id)
                            .into_iter()
                            .filter(|component| {
                                managed_copy
                                    || external_components(
                                        &inventory,
                                        whisper_runtime.as_deref(),
                                        sherpa_runtime.as_deref(),
                                        *component,
                                    )
                                    .is_empty()
                            })
                            .collect::<Vec<_>>();
                        installer
                            .ensure_plan(
                                &components,
                                false,
                                &operation_id,
                                &cancel,
                                &mut emit_progress,
                            )
                            .and_then(|_| {
                                if cancel.load(Ordering::Relaxed) {
                                    Err(echo::install::InstallError::Cancelled)
                                } else {
                                    config_service
                                        .apply_setup_plan_blocking(plan_id, Arc::clone(&cancel))
                                }
                            })
                    }
                    SetupAction::Repair(component) => installer
                        .ensure_component(
                            component,
                            true,
                            &operation_id,
                            &cancel,
                            &mut emit_progress,
                        )
                        .map(|_| ()),
                    SetupAction::Remove(component) => installer.store.remove(component),
                    SetupAction::Verify(component) => installer.store.verify(component),
                };
                crate::status::health_invalidate();
                let event = match result {
                    Ok(()) => SetupEvent::Finished {
                        operation_id: operation_id.as_str().to_string(),
                    },
                    Err(echo::install::InstallError::Cancelled) => SetupEvent::Cancelled {
                        operation_id: operation_id.as_str().to_string(),
                    },
                    Err(error) => SetupEvent::Failed {
                        operation_id: operation_id.as_str().to_string(),
                        error: error.to_string(),
                    },
                };
                let mut active = lock_active_operation(&worker_state);
                if active
                    .as_ref()
                    .is_some_and(|active| active.id == operation_id)
                {
                    *active = None;
                }
                let tray_request = crate::tray::request();
                drop(active);
                crate::tray::refresh_requested(&app, tray_request);
                let _ = app.emit("setup-event", event);
            });
        if let Err(error) = spawned {
            let mut active = lock_active_operation(&active_state);
            if active.as_ref().is_some_and(|active| active.id == id) {
                *active = None;
            }
            return Err(error.to_string());
        }
        Ok(id)
    }
}

#[tauri::command]
pub async fn get_readiness(state: State<'_, SetupService>) -> Result<Readiness, String> {
    let service = state.inner().clone();
    crate::blocking::run_blocking("readiness snapshot", move || service.snapshot()).await
}

#[tauri::command]
pub fn start_setup(
    plan: SetupPlanId,
    managed_copy: bool,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    state
        .start(SetupAction::Plan(plan.into(), managed_copy), app)
        .map(|operation| operation.as_str().to_string())
}

#[tauri::command]
pub fn repair_managed(
    component: ComponentId,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    state
        .start(SetupAction::Repair(component.into()), app)
        .map(|operation| operation.as_str().to_string())
}

#[tauri::command]
pub fn verify_managed(
    component: ComponentId,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    state
        .start(SetupAction::Verify(component.into()), app)
        .map(|operation| operation.as_str().to_string())
}

#[tauri::command]
pub fn remove_managed(
    component: ComponentId,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    state
        .start(SetupAction::Remove(component.into()), app)
        .map(|operation| operation.as_str().to_string())
}

#[tauri::command]
pub fn cancel_setup(operation: String, state: State<'_, SetupService>) -> bool {
    lock_active_operation(&state.active)
        .as_ref()
        .filter(|active| active.id.as_str() == operation)
        .map(|active| {
            active.cancel.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{get_readiness, lock_active_operation, plan_space, Readiness, SetupService};
    use crate::settings::apply_plan_config;
    use echo::install::SetupPlanId;
    use echo_core::{Config, EngineChoice};
    use std::future::Future;
    use std::sync::{Arc, Mutex};
    use tauri::Manager;

    fn assert_async_readiness(_: impl Future<Output = Result<Readiness, String>>) {}

    #[test]
    fn readiness_snapshot_yields_before_collection() {
        let app = tauri::test::mock_builder()
            .manage(SetupService {
                active: Arc::new(Mutex::new(None)),
            })
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        assert_async_readiness(get_readiness(app.state()));
    }

    #[test]
    fn setup_service_clone_shares_active_operation() {
        let service = SetupService {
            active: Arc::new(Mutex::new(None)),
        };
        let clone = service.clone();

        assert!(Arc::ptr_eq(&service.active, &clone.active));
    }

    #[test]
    fn poisoned_active_operation_state_is_recovered() {
        let active = Arc::new(Mutex::new(None));
        let poison = Arc::clone(&active);
        assert!(std::thread::spawn(move || {
            let _guard = poison.lock().unwrap();
            panic!("poison active operation state");
        })
        .join()
        .is_err());

        assert!(lock_active_operation(&active).is_none());
        assert!(!active.is_poisoned());
    }

    #[test]
    fn plan_disk_check_includes_payloads_retained_by_earlier_components() {
        let runtime = (10, 20, 0);
        let model = (100, 100, 0);
        let (_, one_component) = plan_space([model]);
        let (download, plan_required) = plan_space([runtime, model]);
        assert_eq!(download, 110);
        assert!(plan_required >= one_component + runtime.1);
    }

    #[test]
    fn resumable_bytes_reduce_download_and_retained_growth() {
        let (download, required) = plan_space([(100, 100, 40), (20, 20, 0)]);
        assert_eq!(download, 80);
        let (_, without_resume) = plan_space([(100, 100, 0), (20, 20, 0)]);
        assert!(required < without_resume);
    }

    #[test]
    fn parakeet_activation_clears_a_dormant_whisper_pin() {
        let mut config = Config {
            engine: Some(EngineChoice::Whisper),
            whisper_model: Some("small".to_string()),
            ..Config::default()
        };
        apply_plan_config(&mut config, SetupPlanId::Parakeet).unwrap();
        assert_eq!(config.engine, Some(EngineChoice::Parakeet));
        assert_eq!(config.whisper_model, None);
    }

    #[test]
    fn recommended_activation_pins_whisper_small() {
        let mut config = Config::default();
        apply_plan_config(&mut config, SetupPlanId::Recommended).unwrap();
        assert_eq!(config.engine, Some(EngineChoice::Whisper));
        assert_eq!(config.whisper_model.as_deref(), Some("small"));
    }
}
