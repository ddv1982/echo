
pub(super) const ONE_SHOT_TIMEOUT_SECS: u64 = 15 * 60;
pub(super) const CHILD_REAP_TIMEOUT_SECS: u64 = 5;
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
