fn main() {
    #[cfg(target_os = "linux")]
    for host in [
        echo::microphone::AudioHost::PipeWire,
        echo::microphone::AudioHost::PulseAudio,
    ] {
        let snapshot = echo::microphone::availability::snapshot(host).unwrap();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({ "host": host, "metadata": snapshot.as_ref() })
            )
            .unwrap()
        );
    }
}
