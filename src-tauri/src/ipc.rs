pub use echo_ipc::*;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    struct CommandContract {
        handler: &'static str,
        payload_types: &'static [&'static str],
    }

    struct EventContract {
        name: &'static str,
        payload_types: &'static [&'static str],
    }

    const COMMANDS: &[CommandContract] = &[
        CommandContract {
            handler: "get_app_status",
            payload_types: &["AppStatus"],
        },
        CommandContract {
            handler: "get_shortcut_status",
            payload_types: &["ShortcutStatus"],
        },
        CommandContract {
            handler: "retry_shortcut",
            payload_types: &["ShortcutStatus"],
        },
        CommandContract {
            handler: "repair_legacy_shortcut",
            payload_types: &["LegacyShortcutSetup"],
        },
        CommandContract {
            handler: "get_history",
            payload_types: &["HistoryItem"],
        },
        CommandContract {
            handler: "get_dictionary",
            payload_types: &["DictionaryItem"],
        },
        CommandContract {
            handler: "add_dictionary_entry",
            payload_types: &["DictionaryItem"],
        },
        CommandContract {
            handler: "remove_dictionary_entry",
            payload_types: &[],
        },
        CommandContract {
            handler: "toggle_recording",
            payload_types: &[],
        },
        CommandContract {
            handler: "stop_recording",
            payload_types: &[],
        },
        CommandContract {
            handler: "get_recording_level",
            payload_types: &[],
        },
        CommandContract {
            handler: "copy_text",
            payload_types: &[],
        },
        CommandContract {
            handler: "remove_stale_installs",
            payload_types: &[],
        },
        CommandContract {
            handler: "get_settings",
            payload_types: &["Settings"],
        },
        CommandContract {
            handler: "set_settings",
            payload_types: &["Settings"],
        },
        CommandContract {
            handler: "list_models",
            payload_types: &["ModelInventory"],
        },
        CommandContract {
            handler: "list_gpu_devices",
            payload_types: &["GpuDevice"],
        },
        CommandContract {
            handler: "list_languages",
            payload_types: &["LanguageOptions"],
        },
        CommandContract {
            handler: "setup::get_readiness",
            payload_types: &["Readiness"],
        },
        CommandContract {
            handler: "setup::start_setup",
            payload_types: &["SetupPlanId"],
        },
        CommandContract {
            handler: "setup::repair_managed",
            payload_types: &["ComponentId"],
        },
        CommandContract {
            handler: "setup::verify_managed",
            payload_types: &["ComponentId"],
        },
        CommandContract {
            handler: "setup::remove_managed",
            payload_types: &["ComponentId"],
        },
        CommandContract {
            handler: "setup::cancel_setup",
            payload_types: &[],
        },
        CommandContract {
            handler: "get_microphones",
            payload_types: &["MicrophoneSnapshot"],
        },
        CommandContract {
            handler: "set_microphone",
            payload_types: &["MicrophoneSnapshot"],
        },
        CommandContract {
            handler: "test_input_device",
            payload_types: &["MicrophoneTestResult"],
        },
        CommandContract {
            handler: "test_microphone_fallback",
            payload_types: &["MicrophoneTestResult"],
        },
    ];

    const EVENTS: &[EventContract] = &[EventContract {
        name: "setup-event",
        payload_types: &["SetupEvent", "InstallProgress"],
    }];

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
            .filter(|line| !line.is_empty())
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
        let setup = include_str!("setup.rs");
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
            let source = if command.handler.starts_with("setup::") {
                setup
            } else {
                main
            };
            let signature = signature(source, command.handler);
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
            assert!(setup.contains(&format!("\"{}\"", event.name)));
            for payload_type in event.payload_types {
                assert!(setup.contains(payload_type));
                assert!(registered.contains(*payload_type));
                manifest_types.insert((*payload_type).to_string());
            }
        }
        assert_eq!(manifest_types.len(), 16);
    }
}
