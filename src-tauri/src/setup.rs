use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use echo::install::catalog::{
    component, managed_platform_supported, plan, recommended_model, HardwareProfile,
};
use echo::install::{
    CommandRuntimeProbe, ComponentId, DiskSpace, InstallProgress, Installer, ManagedComponentState,
    ManagedStore, OperationId, SetupPlanId, SystemDisk, UreqTransport,
};
use echo::stt::ModelCache;
use serde::Serialize;
use tauri::{Emitter, State};

#[derive(Debug, Clone, PartialEq, Eq)]
enum SetupAction {
    Plan(SetupPlanId, bool),
    Repair(ComponentId),
    Remove(ComponentId),
    Verify(ComponentId),
}

struct ActiveOperation {
    id: OperationId,
    action: SetupAction,
    cancel: Arc<AtomicBool>,
    progress: Option<InstallProgress>,
}

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponent {
    origin: &'static str,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStatusDto {
    id: ComponentId,
    label: String,
    managed: ManagedComponentState,
    external: Vec<ExternalComponent>,
    active_origin: Option<&'static str>,
    activity: Option<InstallProgress>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStatusDto {
    id: SetupPlanId,
    label: &'static str,
    components: Vec<ComponentId>,
    satisfied: bool,
    download_bytes: u64,
    required_free_bytes: u64,
    available_bytes: Option<u64>,
    disk_ready: bool,
    disk_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessDto {
    managed_supported: bool,
    unsupported_reason: Option<String>,
    total_memory_bytes: Option<u64>,
    recommended_model: ComponentId,
    components: Vec<ComponentStatusDto>,
    plans: Vec<PlanStatusDto>,
    microphone_ready: bool,
    speech_ready: bool,
    has_successful_dictation: bool,
    first_run_complete: bool,
    active_operation: Option<OperationId>,
    active_cancellable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum SetupEvent {
    Progress {
        progress: InstallProgress,
    },
    Finished {
        operation_id: OperationId,
    },
    Cancelled {
        operation_id: OperationId,
    },
    Failed {
        operation_id: OperationId,
        error: String,
    },
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
    id: ComponentId,
) -> Vec<ExternalComponent> {
    match id {
        ComponentId::WhisperRuntime => whisper_runtime
            .map(|path| {
                vec![ExternalComponent {
                    origin: "system",
                    path: path.to_string_lossy().into_owned(),
                }]
            })
            .unwrap_or_default(),
        ComponentId::SherpaRuntime => sherpa_runtime
            .map(|path| {
                vec![ExternalComponent {
                    origin: "system",
                    path: path.to_string_lossy().into_owned(),
                }]
            })
            .unwrap_or_default(),
        ComponentId::WhisperBaseQ51 => external_model(inventory, "base-q5_1"),
        ComponentId::WhisperSmall => external_model(inventory, "small"),
        ComponentId::WhisperLargeV3TurboQ50 => external_model(inventory, "large-v3-turbo-q5_0"),
        ComponentId::SileroVad => inventory
            .vad
            .iter()
            .map(|path| ExternalComponent {
                origin: "external",
                path: path.to_string_lossy().into_owned(),
            })
            .collect(),
        ComponentId::ParakeetTdt06bV3Int8 => inventory
            .parakeet
            .clone()
            .map(|path| {
                vec![ExternalComponent {
                    origin: "external",
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
            origin: "external",
            path: model.path.to_string_lossy().into_owned(),
        })
        .collect()
}

fn managed_ready(state: &ManagedComponentState) -> bool {
    matches!(state, ManagedComponentState::Ready { .. })
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
    fn snapshot(&self) -> ReadinessDto {
        let cache = ModelCache::from_env();
        let store = ManagedStore::new(cache.dir());
        let (active_operation, active_cancellable, activity) = {
            let active = self.active.lock().expect("setup operation lock");
            (
                active.as_ref().map(|operation| operation.id.clone()),
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
        let external_inventory = cache.inventory();
        let whisper_runtime = ["whisper-cli", "whisper-cpp", "whisper"]
            .into_iter()
            .find_map(echo::which::path_of);
        let sherpa_runtime = ["sherpa-onnx-offline", "sherpa-onnx"]
            .into_iter()
            .find_map(echo::which::path_of);
        let mut components = Vec::new();
        for spec in echo::install::catalog::COMPONENTS {
            let managed = if managed_platform_supported() {
                store.status(spec.id, false)
            } else {
                ManagedComponentState::Unsupported {
                    reason: "Echo-managed speech setup supports Linux x86_64 only".to_string(),
                }
            };
            let external = external_components(
                &external_inventory,
                whisper_runtime.as_deref(),
                sherpa_runtime.as_deref(),
                spec.id,
            );
            let active_origin = if managed_ready(&managed) {
                Some("managed")
            } else if !external.is_empty() {
                Some(external[0].origin)
            } else {
                None
            };
            components.push(ComponentStatusDto {
                id: spec.id,
                label: spec.label.to_string(),
                managed,
                external,
                active_origin,
                activity: activity
                    .as_ref()
                    .filter(|progress| progress.component == spec.id)
                    .cloned(),
            });
        }
        let hardware = HardwareProfile {
            total_memory_bytes: total_memory_bytes(),
        };
        let plan_ids = [
            SetupPlanId::Recommended,
            SetupPlanId::Parakeet,
            SetupPlanId::WhisperBase,
            SetupPlanId::WhisperSmall,
            SetupPlanId::WhisperLargeV3Turbo,
        ];
        let _ = std::fs::create_dir_all(cache.dir());
        let available_bytes = SystemDisk.available_bytes(cache.dir()).ok().flatten();
        let plans = plan_ids
            .into_iter()
            .map(|id| {
                let ids = plan(id, hardware);
                let satisfied = ids.iter().all(|id| {
                    components.iter().find(|component| component.id == *id).is_some_and(
                        |component| {
                            managed_ready(&component.managed) || !component.external.is_empty()
                        },
                    )
                });
                let (download_bytes, required_free_bytes) = plan_space(
                    ids.iter().filter_map(|component_id| {
                        let status = components
                            .iter()
                            .find(|component| component.id == *component_id)?;
                        if managed_ready(&status.managed) || !status.external.is_empty() {
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
                PlanStatusDto {
                    id,
                    label: match id {
                        SetupPlanId::Recommended => "Recommended",
                        SetupPlanId::Parakeet => "Parakeet",
                        SetupPlanId::WhisperBase => "Whisper Base",
                        SetupPlanId::WhisperSmall => "Whisper Small",
                        SetupPlanId::WhisperLargeV3Turbo => "Whisper Large Turbo",
                    },
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
                    components: ids,
                    satisfied,
                }
            })
            .collect();
        let microphone_ready = echo::audio::AudioCapture::default_input_ready().is_ok();
        let speech_ready = echo::stt::engine_summary().1;
        let has_successful_dictation = echo_core::History::load().ok().is_some_and(|history| {
            history
                .rows()
                .iter()
                .any(|row| !row.text.trim().is_empty() && !row.inject.failed())
        });
        ReadinessDto {
            managed_supported: managed_platform_supported(),
            unsupported_reason: (!managed_platform_supported()).then(|| {
                "Managed setup is available on Linux x86_64. Use a system runtime and manual models on this platform."
                    .to_string()
            }),
            total_memory_bytes: hardware.total_memory_bytes,
            recommended_model: recommended_model(hardware),
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
        let mut active = self.active.lock().expect("setup operation lock");
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
                let mut last_progress_phase = None;
                let mut last_progress_emit = Instant::now() - Duration::from_secs(1);
                let mut emit_progress = |progress: InstallProgress| {
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
                    if let Some(active) =
                        worker_state.lock().expect("setup operation lock").as_mut()
                    {
                        if active.id == operation_id {
                            active.progress = Some(progress.clone());
                        }
                    }
                    let _ = app.emit("setup-event", SetupEvent::Progress { progress });
                };
                let result = match action {
                    SetupAction::Plan(plan_id, managed_copy) => {
                        let hardware = HardwareProfile {
                            total_memory_bytes: total_memory_bytes(),
                        };
                        let inventory = cache.inventory();
                        let whisper_runtime = ["whisper-cli", "whisper-cpp", "whisper"]
                            .into_iter()
                            .find_map(echo::which::path_of);
                        let sherpa_runtime = ["sherpa-onnx-offline", "sherpa-onnx"]
                            .into_iter()
                            .find_map(echo::which::path_of);
                        let components = plan(plan_id, hardware)
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
                                    activate_plan_config(plan_id, hardware)
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
                super::health_invalidate();
                let event = match result {
                    Ok(()) => SetupEvent::Finished {
                        operation_id: operation_id.clone(),
                    },
                    Err(echo::install::InstallError::Cancelled) => SetupEvent::Cancelled {
                        operation_id: operation_id.clone(),
                    },
                    Err(error) => SetupEvent::Failed {
                        operation_id: operation_id.clone(),
                        error: error.to_string(),
                    },
                };
                let mut active = worker_state.lock().expect("setup operation lock");
                if active
                    .as_ref()
                    .is_some_and(|active| active.id == operation_id)
                {
                    *active = None;
                }
                drop(active);
                let _ = app.emit("setup-event", event);
            });
        if let Err(error) = spawned {
            let mut active = active_state.lock().expect("setup operation lock");
            if active.as_ref().is_some_and(|active| active.id == id) {
                *active = None;
            }
            return Err(error.to_string());
        }
        Ok(id)
    }
}

fn activate_plan_config(
    plan_id: SetupPlanId,
    hardware: HardwareProfile,
) -> Result<(), echo::install::InstallError> {
    super::update_file_config(|config| {
        match plan_id {
            SetupPlanId::Parakeet => config.engine = Some(echo_core::EngineChoice::Parakeet),
            SetupPlanId::Recommended
            | SetupPlanId::WhisperBase
            | SetupPlanId::WhisperSmall
            | SetupPlanId::WhisperLargeV3Turbo => {
                config.engine = Some(echo_core::EngineChoice::Whisper);
                let model = match plan_id {
                    SetupPlanId::Recommended => recommended_model(hardware),
                    SetupPlanId::WhisperBase => ComponentId::WhisperBaseQ51,
                    SetupPlanId::WhisperSmall => ComponentId::WhisperSmall,
                    SetupPlanId::WhisperLargeV3Turbo => ComponentId::WhisperLargeV3TurboQ50,
                    SetupPlanId::Parakeet => unreachable!(),
                };
                config.whisper_model = Some(
                    match model {
                        ComponentId::WhisperBaseQ51 => "base-q5_1",
                        ComponentId::WhisperSmall => "small",
                        ComponentId::WhisperLargeV3TurboQ50 => "large-v3-turbo-q5_0",
                        _ => return Err("invalid Whisper model plan".to_string()),
                    }
                    .to_string(),
                );
            }
        }
        Ok(())
    })
    .map_err(echo::install::InstallError::IoMessage)
}

#[tauri::command]
pub fn get_readiness(state: State<'_, SetupService>) -> ReadinessDto {
    state.snapshot()
}

#[tauri::command]
pub fn start_setup(
    plan: SetupPlanId,
    managed_copy: bool,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<OperationId, String> {
    state.start(SetupAction::Plan(plan, managed_copy), app)
}

#[tauri::command]
pub fn repair_managed(
    component: ComponentId,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<OperationId, String> {
    state.start(SetupAction::Repair(component), app)
}

#[tauri::command]
pub fn verify_managed(
    component: ComponentId,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<OperationId, String> {
    state.start(SetupAction::Verify(component), app)
}

#[tauri::command]
pub fn remove_managed(
    component: ComponentId,
    state: State<'_, SetupService>,
    app: tauri::AppHandle,
) -> Result<OperationId, String> {
    state.start(SetupAction::Remove(component), app)
}

#[tauri::command]
pub fn cancel_setup(operation: OperationId, state: State<'_, SetupService>) -> bool {
    state
        .active
        .lock()
        .expect("setup operation lock")
        .as_ref()
        .filter(|active| active.id == operation)
        .map(|active| {
            active.cancel.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::plan_space;

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
}
