mod devices;
mod library;
mod recording;
mod settings;
mod shortcuts;
mod status;
mod system;

pub(super) use devices::{
    get_microphones, list_gpu_devices, list_languages, list_models, set_microphone,
    test_input_device, test_microphone_fallback,
};
pub(super) use library::{
    add_dictionary_entry, copy_text, get_dictionary, get_history, remove_dictionary_entry,
};
pub(super) use recording::{
    get_recording_level, start_recording_thread, stop_recording, toggle_recording,
};
pub(super) use settings::{get_settings, set_settings};
pub(super) use shortcuts::{get_shortcut_status, repair_legacy_shortcut, retry_shortcut};
pub(super) use status::get_app_status;
pub(super) use system::remove_stale_installs;
