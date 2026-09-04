mod devices;
mod dictionary_training;
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
pub(super) use dictionary_training::{
    cancel_dictionary_training_sample, finish_dictionary_training_sample,
    start_dictionary_training_sample, DictionaryTrainingCaptures,
};
pub(super) use library::{
    add_dictionary_entries_batch, add_dictionary_entry, clear_history, copy_text,
    delete_history_item, get_dictionary, get_history, remove_dictionary_entry,
};
pub(super) use recording::{
    get_recording_level, start_recording_thread, stop_recording, toggle_recording,
};
#[cfg(feature = "status-perf-probe")]
pub(super) use settings::run_test_hook;
pub(super) use settings::{get_settings, set_settings};
pub(super) use shortcuts::{get_shortcut_status, repair_legacy_shortcut, retry_shortcut};
pub(super) use status::get_app_status;
pub(super) use system::{quit_app, remove_stale_installs};

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    fn assert_async_fn0<Output, Function, Returned>(_: Function)
    where
        Function: FnOnce() -> Returned,
        Returned: Future<Output = Output>,
    {
    }

    fn assert_async_fn1<Argument, Output, Function, Returned>(_: Function)
    where
        Function: FnOnce(Argument) -> Returned,
        Returned: Future<Output = Output>,
    {
    }

    fn assert_async_fn2<Argument1, Argument2, Output, Function, Returned>(_: Function)
    where
        Function: FnOnce(Argument1, Argument2) -> Returned,
        Returned: Future<Output = Output>,
    {
    }

    #[test]
    fn blocking_command_boundaries_are_async_without_being_invoked() {
        assert_async_fn0::<Result<echo_desktop::ipc::AppStatus, String>, _, _>(get_app_status);
        assert_async_fn0::<Result<echo_desktop::ipc::ModelInventory, String>, _, _>(list_models);
        assert_async_fn0::<Result<Vec<echo_desktop::ipc::HistoryItem>, String>, _, _>(get_history);
        assert_async_fn0::<Result<Vec<echo_desktop::ipc::DictionaryItem>, String>, _, _>(
            get_dictionary,
        );
        assert_async_fn2::<String, String, Result<echo_desktop::ipc::DictionaryItem, String>, _, _>(
            add_dictionary_entry,
        );
        assert_async_fn2::<
            String,
            Vec<String>,
            Result<echo_desktop::ipc::DictionaryBatchResult, String>,
            _,
            _,
        >(add_dictionary_entries_batch);
        assert_async_fn2::<String, String, Result<bool, String>, _, _>(remove_dictionary_entry);
        assert_async_fn0::<Result<echo_desktop::ipc::ShortcutStatus, String>, _, _>(retry_shortcut);
        assert_async_fn0::<Result<echo_desktop::ipc::LegacyShortcutSetup, String>, _, _>(
            repair_legacy_shortcut,
        );
        assert_async_fn0::<Result<Vec<String>, String>, _, _>(remove_stale_installs);
        assert_async_fn1::<
            Option<String>,
            Result<echo_desktop::ipc::MicrophoneTestResult, String>,
            _,
            _,
        >(test_input_device);
        assert_async_fn0::<Result<echo_desktop::ipc::MicrophoneTestResult, String>, _, _>(
            test_microphone_fallback,
        );
        assert_async_fn1::<
            tauri::State<'static, DictionaryTrainingCaptures>,
            Result<String, String>,
            _,
            _,
        >(start_dictionary_training_sample);
    }
}
