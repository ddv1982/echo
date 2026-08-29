#[cfg(test)]
use serde_json::{json, Value};
#[cfg(test)]
use sha2::{Digest, Sha256};

#[cfg(test)]
use super::whisper_admission::MAX_QUARANTINE_LIFETIME_SECS;

pub(super) const ONE_SHOT_TIMEOUT_SECS: u64 = 15 * 60;
pub(super) const CHILD_REAP_TIMEOUT_SECS: u64 = 5;
pub(super) const RECEIPT_PROBE_TIMEOUT_SECS: u64 = 15;
pub(super) const VULKAN_RECEIPT_SCHEMA: u32 = 1;
pub(super) const VULKAN_BACKEND: &str = "vulkan";
pub(super) const CLEARED_ENVIRONMENT_KEYS: [&str; 5] = [
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "MESA_SHADER_CACHE_DIR",
    "VK_DRIVER_FILES",
    "VK_ICD_FILENAMES",
];
pub(super) const CLEARED_ENVIRONMENT_PREFIXES: [&str; 26] = [
    "LD_",
    "VK_",
    "MESA_",
    "DRI_",
    "LIBGL_",
    "GALLIUM_",
    "INTEL_",
    "AMD_",
    "RADV_",
    "NVIDIA_",
    "__GL",
    "CUDA_",
    "ROCR_",
    "HIP_",
    "HSA_",
    "ONEAPI_",
    "SYCL_",
    "ZES_",
    "ZE_",
    "OPENCL_",
    "OCL_",
    "RUSTICL_",
    "GGML_",
    "OMP_",
    "OPENBLAS_",
    "LIBVA_",
];

#[must_use]
#[cfg(test)]
pub(super) fn projection() -> Value {
    json!({
        "decode": {
            "qualifiedRequestPolicy": {
                "hints": "empty",
                "language": "pinned",
                "prompt": "empty"
            },
            "qualifiedVadRetry": false,
            "runtimeDefaults": {
                "beamSize": null,
                "bestOf": null,
                "noFallback": null,
                "threads": null
            }
        },
        "launch": {
            "childReapTimeoutMs": CHILD_REAP_TIMEOUT_SECS * 1_000,
            "clearedEnvironmentKeys": CLEARED_ENVIRONMENT_KEYS,
            "clearedEnvironmentPrefixes": CLEARED_ENVIRONMENT_PREFIXES,
            "forceCpuFlag": "--no-gpu",
            "languageFlag": "-l",
            "outputFlags": ["-nt", "-oj", "-of", "-"],
            "promptFlag": "--prompt",
            "protocol": "oneShotCli",
            "receiptProbeTimeoutMs": RECEIPT_PROBE_TIMEOUT_SECS * 1_000,
            "timeoutMs": ONE_SHOT_TIMEOUT_SECS * 1_000,
            "tuningFlags": {
                "beamSize": "-bs",
                "bestOf": "-bo",
                "noFallback": "-nf",
                "threads": "-t"
            },
            "vadFlags": ["--vad", "-vm"],
            "vulkanSelectorKeys": [
                "ECHO_WHISPER_VULKAN_DEVICE_UUID",
                "ECHO_WHISPER_VULKAN_DRIVER_UUID"
            ]
        },
        "receipt": {
            "backend": VULKAN_BACKEND,
            "match": "exact",
            "requiredFields": [
                "schemaVersion",
                "backend",
                "selectedIndex",
                "vendorId",
                "deviceId",
                "apiVersion",
                "driverVersion",
                "deviceUUID",
                "driverUUID",
                "pipelineCacheUUID"
            ],
            "schema": VULKAN_RECEIPT_SCHEMA
        },
        "recovery": {
            "acceleratedValidation": [
                "model",
                "runtimeIdentity",
                "vulkanBackend",
                "receipt"
            ],
            "fallback": "managedCpuOnce",
            "quarantineLifetimeSeconds": MAX_QUARANTINE_LIFETIME_SECS,
            "requestPolicyMismatchFallsBack": true,
            "schema": 1
        },
        "telemetry": {
            "acceleratedValidationFields": [
                "engine.model",
                "runtime.identitySha256",
                "runtime.backend",
                "runtime.vulkanReceipt"
            ],
            "recoveryFields": [
                "identityKey",
                "acceleratedAttempted",
                "fallbackReason"
            ],
            "schema": 2,
            "selectionFields": [
                "preference",
                "cachedDecision",
                "localKey",
                "calibrationPending",
                "proofOnly"
            ]
        }
    })
}

#[must_use]
#[cfg(test)]
pub(super) fn projection_sha256() -> String {
    let bytes = serde_json::to_vec(&projection()).expect("behavior projection is serializable");
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_projection_matches_production_values_and_inference_fixture() {
        let behavior: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/whisper-behavior-v3.json"
        ))
        .unwrap();
        assert_eq!(behavior["schemaVersion"], 3);
        assert_eq!(behavior["projection"], projection());
        assert_eq!(behavior["projectionSha256"], projection_sha256());

        let identities: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/whisper-v3-identities.json"
        ))
        .unwrap();
        assert_eq!(
            identities["cases"]["inferenceContract"]["input"]["behavior"]["projectionSha256"],
            behavior["projectionSha256"]
        );
    }
}
