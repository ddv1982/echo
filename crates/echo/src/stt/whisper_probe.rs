use std::collections::BTreeSet;

use echo_core::WhisperRuntimeBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperRuntimeObservation {
    pub backend: WhisperRuntimeBackend,
    pub device: Option<String>,
}

enum SelectedGpu<'a> {
    None,
    One(&'a str),
    Conflict,
}

#[must_use]
pub fn observe_runtime(stderr: &str) -> Option<WhisperRuntimeObservation> {
    let selected = match selected_gpu(stderr) {
        SelectedGpu::None => None,
        SelectedGpu::One(selected) => Some(selected),
        SelectedGpu::Conflict => return None,
    };
    if let Some(selected) = selected {
        if gpu_initialization_failed(stderr, selected) {
            return cpu_observation(stderr);
        }
        let (backend, index) = selected_backend(selected)?;
        let device = match backend {
            WhisperRuntimeBackend::Vulkan => vulkan_device(stderr, index),
            WhisperRuntimeBackend::Cuda | WhisperRuntimeBackend::Rocm => {
                cuda_family_device(stderr, index)
            }
            _ => None,
        };
        return Some(WhisperRuntimeObservation { backend, device });
    }

    openvino_observation(stderr).or_else(|| cpu_observation(stderr))
}

fn selected_gpu(stderr: &str) -> SelectedGpu<'_> {
    let selected = stderr
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("whisper_backend_init_gpu: using ")?
                .strip_suffix(" backend")
        })
        .collect::<BTreeSet<_>>();
    match selected.len() {
        0 => SelectedGpu::None,
        1 => SelectedGpu::One(selected.first().copied().expect("one selected backend")),
        _ => SelectedGpu::Conflict,
    }
}

fn gpu_initialization_failed(stderr: &str, selected: &str) -> bool {
    let expected = format!("whisper_backend_init_gpu: failed to initialize {selected} backend");
    stderr.lines().any(|line| line.trim() == expected)
}

fn selected_backend(selected: &str) -> Option<(WhisperRuntimeBackend, usize)> {
    for (prefix, backend) in [
        ("Vulkan", WhisperRuntimeBackend::Vulkan),
        ("CUDA", WhisperRuntimeBackend::Cuda),
        ("ROCm", WhisperRuntimeBackend::Rocm),
    ] {
        if let Some(index) = selected.strip_prefix(prefix) {
            return index.parse().ok().map(|index| (backend, index));
        }
    }
    None
}

fn vulkan_device(stderr: &str, selected_index: usize) -> Option<String> {
    let prefix = format!("ggml_vulkan: {selected_index} = ");
    stderr.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&prefix)?;
        Some(value.split(" |").next().unwrap_or(value).trim().to_string())
    })
}

fn cuda_family_device(stderr: &str, selected_index: usize) -> Option<String> {
    let prefix = format!("Device {selected_index}: ");
    stderr.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&prefix)?;
        Some(value.split(',').next().unwrap_or(value).trim().to_string())
    })
}

fn openvino_observation(stderr: &str) -> Option<WhisperRuntimeObservation> {
    let failed = stderr.lines().any(|line| {
        line.trim()
            .starts_with("whisper_ctx_init_openvino_encoder: failed to init OpenVINO encoder")
    });
    let loaded = stderr
        .lines()
        .any(|line| line.trim() == "whisper_ctx_init_openvino_encoder: OpenVINO model loaded");
    if failed || !loaded {
        return None;
    }
    let device = stderr.lines().find_map(|line| {
        let line = line.trim();
        if !line.starts_with("whisper_openvino_init:") {
            return None;
        }
        let value = line.split("device = ").nth(1)?;
        let device = value.split(',').next().unwrap_or(value).trim();
        (!device.is_empty()).then(|| device.to_string())
    })?;
    Some(WhisperRuntimeObservation {
        backend: WhisperRuntimeBackend::OpenVino,
        device: Some(device),
    })
}

fn cpu_observation(stderr: &str) -> Option<WhisperRuntimeObservation> {
    stderr
        .lines()
        .any(|line| {
            let line = line.trim();
            line == "whisper_backend_init_gpu: no GPU found"
                || (line.starts_with("whisper_model_load:") && line.contains("CPU total size ="))
        })
        .then_some(WhisperRuntimeObservation {
            backend: WhisperRuntimeBackend::Cpu,
            device: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_vulkan_index_binds_its_physical_device() {
        let stderr = r#"
ggml_vulkan: 0 = llvmpipe (LLVM) | uma: 0
ggml_vulkan: 1 = AMD Radeon RX 7800 XT (RADV) | uma: 0
whisper_backend_init_gpu: using Vulkan1 backend
"#;
        assert_eq!(
            observe_runtime(stderr),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Vulkan,
                device: Some("AMD Radeon RX 7800 XT (RADV)".to_string()),
            })
        );
    }

    #[test]
    fn selected_software_vulkan_device_remains_visible_for_policy_rejection() {
        let stderr = "ggml_vulkan: 0 = llvmpipe (LLVM 20.1.8) | uma: 0\n\
            whisper_backend_init_gpu: using Vulkan0 backend\n";
        assert_eq!(
            observe_runtime(stderr),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Vulkan,
                device: Some("llvmpipe (LLVM 20.1.8)".to_string()),
            })
        );
    }

    #[test]
    fn cuda_and_rocm_bind_matching_device_records() {
        for (name, backend, device) in [
            ("CUDA1", WhisperRuntimeBackend::Cuda, "NVIDIA RTX 4080"),
            (
                "ROCm1",
                WhisperRuntimeBackend::Rocm,
                "AMD Radeon RX 7900 XTX",
            ),
        ] {
            let stderr = format!(
                "  Device 0: ignored, VRAM: 1 MiB\n  Device 1: {device}, VRAM: 2 MiB\n\
                 whisper_backend_init_gpu: using {name} backend\n"
            );
            assert_eq!(
                observe_runtime(&stderr),
                Some(WhisperRuntimeObservation {
                    backend,
                    device: Some(device.to_string()),
                })
            );
        }
    }

    #[test]
    fn openvino_and_cpu_are_observed_without_a_main_gpu() {
        let openvino = "whisper_backend_init_gpu: no GPU found\n\
            whisper_openvino_init: path_model = encoder.xml, device = GPU, cache_dir = cache\n\
            whisper_ctx_init_openvino_encoder: OpenVINO model loaded\n";
        assert_eq!(
            observe_runtime(openvino),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::OpenVino,
                device: Some("GPU".to_string()),
            })
        );
        assert_eq!(
            observe_runtime("whisper_backend_init_gpu: no GPU found\n"),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
            })
        );
        let failed = "whisper_backend_init_gpu: no GPU found\n\
            whisper_openvino_init: path_model = encoder.xml, device = GPU, cache_dir = cache\n\
            whisper_ctx_init_openvino_encoder: failed to init OpenVINO encoder\n";
        assert_eq!(
            observe_runtime(failed),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
            })
        );
        let empty_device = "whisper_backend_init_gpu: no GPU found\n\
            whisper_openvino_init: path_model = encoder.xml, device = , cache_dir = cache\n\
            whisper_ctx_init_openvino_encoder: OpenVINO model loaded\n";
        assert_eq!(
            observe_runtime(empty_device),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
            })
        );
        let unrelated_success = "whisper_backend_init_gpu: no GPU found\n\
            whisper_openvino_init: path_model = encoder.xml, device = GPU, cache_dir = cache\n\
            other: OpenVINO model loaded\n";
        assert_eq!(
            observe_runtime(unrelated_success),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
            })
        );
        let conflicting = "whisper_backend_init_gpu: no GPU found\n\
            whisper_openvino_init: path_model = encoder.xml, device = GPU, cache_dir = cache\n\
            whisper_ctx_init_openvino_encoder: OpenVINO model loaded\n\
            whisper_ctx_init_openvino_encoder: failed to init OpenVINO encoder from encoder.xml\n";
        assert_eq!(
            observe_runtime(conflicting),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
            })
        );
    }

    #[test]
    fn enumeration_requests_and_conflicts_do_not_invent_selection() {
        assert_eq!(
            observe_runtime("ggml_vulkan: 0 = Intel Iris Xe | uma: 1\nuse gpu = 1\n"),
            None
        );
        assert_eq!(
            observe_runtime(
                "whisper_backend_init_gpu: using Vulkan0 backend\n\
                 whisper_backend_init_gpu: using CUDA0 backend\n\
                 whisper_backend_init_gpu: no GPU found\n"
            ),
            None
        );
        assert_eq!(
            observe_runtime("whisper_backend_init_gpu: using mystery0 backend\n"),
            None
        );
    }

    #[test]
    fn failed_gpu_initialization_falls_back_only_with_cpu_evidence() {
        let stderr = "whisper_backend_init_gpu: using Vulkan0 backend\n\
            whisper_backend_init_gpu: failed to initialize Vulkan0 backend\n\
            whisper_model_load: CPU total size = 573.40 MB\n";
        assert_eq!(
            observe_runtime(stderr),
            Some(WhisperRuntimeObservation {
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
            })
        );
        assert_eq!(
            observe_runtime(
                "whisper_backend_init_gpu: using Vulkan0 backend\n\
                 whisper_backend_init_gpu: failed to initialize Vulkan0 backend\n"
            ),
            None
        );
    }
}
