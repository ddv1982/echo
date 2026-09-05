pub use echo_ipc::*;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    struct CommandContract {
        handler: &'static str,
        source: &'static str,
        payload_types: &'static [&'static str],
    }

    struct EventContract {
        name: &'static str,
        source: &'static str,
        payload_types: &'static [&'static str],
    }

    const DEVICES: &str = include_str!("commands/devices.rs");
    const LIBRARY: &str = include_str!("commands/library.rs");
    const RECORDING: &str = include_str!("commands/recording.rs");
    const SETTINGS: &str = include_str!("commands/settings.rs");
    const SHORTCUTS: &str = include_str!("commands/shortcuts.rs");
    const STATUS: &str = include_str!("commands/status.rs");
    const SYSTEM: &str = include_str!("commands/system.rs");
    const SETUP: &str = include_str!("setup.rs");

    const COMMANDS: &[CommandContract] = &[
        CommandContract {
            handler: "get_app_status",
            source: STATUS,
            payload_types: &["AppStatus"],
        },
        CommandContract {
            handler: "get_shortcut_status",
            source: SHORTCUTS,
            payload_types: &["ShortcutStatus"],
        },
        CommandContract {
            handler: "retry_shortcut",
            source: SHORTCUTS,
            payload_types: &["ShortcutStatus"],
        },
        CommandContract {
            handler: "repair_legacy_shortcut",
            source: SHORTCUTS,
            payload_types: &["LegacyShortcutSetup"],
        },
        CommandContract {
            handler: "get_history",
            source: LIBRARY,
            payload_types: &["HistoryItem"],
        },
        CommandContract {
            handler: "delete_history_item",
            source: LIBRARY,
            payload_types: &[],
        },
        CommandContract {
            handler: "clear_history",
            source: LIBRARY,
            payload_types: &[],
        },
        CommandContract {
            handler: "get_dictionary",
            source: LIBRARY,
            payload_types: &["DictionaryItem"],
        },
        CommandContract {
            handler: "add_dictionary_entry",
            source: LIBRARY,
            payload_types: &["DictionaryItem"],
        },
        CommandContract {
            handler: "add_dictionary_entries_batch",
            source: LIBRARY,
            payload_types: &["DictionaryBatchResult"],
        },
        CommandContract {
            handler: "remove_dictionary_entry",
            source: LIBRARY,
            payload_types: &[],
        },
        CommandContract {
            handler: "start_dictionary_training_sample",
            source: include_str!("commands/dictionary_training.rs"),
            payload_types: &[],
        },
        CommandContract {
            handler: "finish_dictionary_training_sample",
            source: include_str!("commands/dictionary_training.rs"),
            payload_types: &["DictionaryTrainingSample"],
        },
        CommandContract {
            handler: "cancel_dictionary_training_sample",
            source: include_str!("commands/dictionary_training.rs"),
            payload_types: &[],
        },
        CommandContract {
            handler: "start_capture",
            source: RECORDING,
            payload_types: &["RecordingSnapshot"],
        },
        CommandContract {
            handler: "stop_capture",
            source: RECORDING,
            payload_types: &["RecordingSnapshot"],
        },
        CommandContract {
            handler: "cancel_transcription",
            source: RECORDING,
            payload_types: &["RecordingSnapshot"],
        },
        CommandContract {
            handler: "stop_recording",
            source: RECORDING,
            payload_types: &[],
        },
        CommandContract {
            handler: "get_recording_level",
            source: RECORDING,
            payload_types: &[],
        },
        CommandContract {
            handler: "copy_text",
            source: LIBRARY,
            payload_types: &[],
        },
        CommandContract {
            handler: "quit_app",
            source: SYSTEM,
            payload_types: &[],
        },
        CommandContract {
            handler: "remove_stale_installs",
            source: SYSTEM,
            payload_types: &[],
        },
        CommandContract {
            handler: "get_settings",
            source: SETTINGS,
            payload_types: &["SettingsSnapshot"],
        },
        CommandContract {
            handler: "set_settings",
            source: SETTINGS,
            payload_types: &["SettingsChange", "SettingsSnapshot"],
        },
        CommandContract {
            handler: "list_models",
            source: DEVICES,
            payload_types: &["ModelInventory"],
        },
        CommandContract {
            handler: "list_gpu_devices",
            source: DEVICES,
            payload_types: &["GpuDevice"],
        },
        CommandContract {
            handler: "list_languages",
            source: DEVICES,
            payload_types: &["LanguageOptions"],
        },
        CommandContract {
            handler: "setup::get_readiness",
            source: SETUP,
            payload_types: &["Readiness"],
        },
        CommandContract {
            handler: "setup::start_setup",
            source: SETUP,
            payload_types: &["SetupPlanId"],
        },
        CommandContract {
            handler: "setup::repair_managed",
            source: SETUP,
            payload_types: &["ComponentId"],
        },
        CommandContract {
            handler: "setup::verify_managed",
            source: SETUP,
            payload_types: &["ComponentId"],
        },
        CommandContract {
            handler: "setup::remove_managed",
            source: SETUP,
            payload_types: &["ComponentId"],
        },
        CommandContract {
            handler: "setup::cancel_setup",
            source: SETUP,
            payload_types: &[],
        },
        CommandContract {
            handler: "get_microphones",
            source: DEVICES,
            payload_types: &["MicrophoneSnapshot"],
        },
        CommandContract {
            handler: "set_microphone",
            source: DEVICES,
            payload_types: &["MicrophoneSnapshot"],
        },
        CommandContract {
            handler: "test_input_device",
            source: DEVICES,
            payload_types: &["MicrophoneTestResult"],
        },
        CommandContract {
            handler: "test_microphone_fallback",
            source: DEVICES,
            payload_types: &["MicrophoneTestResult"],
        },
    ];

    const EVENTS: &[EventContract] = &[
        EventContract {
            name: "setup-event",
            source: SETUP,
            payload_types: &["SetupEvent", "InstallProgress"],
        },
        EventContract {
            name: "settings-event",
            source: include_str!("tray.rs"),
            payload_types: &[],
        },
    ];

    fn handler_names(source: &str) -> Vec<&str> {
        let body = source
            .split_once("tauri::generate_handler![")
            .expect("Tauri handler registry")
            .1
            .split_once(']')
            .expect("Tauri handler registry terminator")
            .0;
        body.lines()
            .map(|line| line.trim().trim_end_matches(','))
            .filter(|line| {
                !line.is_empty() && !line.starts_with("#[cfg") && !line.starts_with("perf::")
            })
            .collect()
    }

    fn signature<'a>(source: &'a str, handler: &str) -> &'a str {
        let name = handler.rsplit("::").next().unwrap();
        let start = source
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("missing command function {handler}"));
        source[start..]
            .split_once('{')
            .expect("command function body")
            .0
    }

    #[test]
    fn command_and_event_payloads_are_registered() {
        let main = include_str!("main.rs");
        assert_eq!(
            handler_names(main),
            COMMANDS
                .iter()
                .map(|command| command.handler)
                .collect::<Vec<_>>()
        );

        let registered = echo_ipc::registered_type_names();
        let mut manifest_types = BTreeSet::new();
        for command in COMMANDS {
            let signature = signature(command.source, command.handler);
            for payload_type in command.payload_types {
                assert!(
                    signature.contains(payload_type),
                    "{} signature does not contain {}",
                    command.handler,
                    payload_type
                );
                assert!(registered.contains(*payload_type));
                manifest_types.insert((*payload_type).to_string());
            }
        }

        for event in EVENTS {
            assert!(event.source.contains(&format!("\"{}\"", event.name)));
            for payload_type in event.payload_types {
                assert!(event.source.contains(payload_type));
                assert!(registered.contains(*payload_type));
                manifest_types.insert((*payload_type).to_string());
            }
        }
        assert_eq!(manifest_types.len(), 20);
    }
}
