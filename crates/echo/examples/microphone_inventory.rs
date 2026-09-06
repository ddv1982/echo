use std::time::Duration;

use echo::audio::{microphone_inventory, AudioCapture};
use echo::microphone::MicrophoneId;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inventory = microphone_inventory();
    println!("{}", serde_json::to_string_pretty(&inventory)?);
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let ids = match arguments.as_slice() {
        [] => return Ok(()),
        [option] if option == "--record" => inventory
            .snapshot
            .devices
            .into_iter()
            .map(|device| device.id)
            .collect(),
        [option, id] if option == "--test-id" => vec![MicrophoneId::parse(id.clone())?],
        _ => return Err("usage: microphone_inventory [--record | --test-id ID]".into()),
    };
    for id in ids {
        let capture = AudioCapture::open_exact(Some(&id))?;
        let result = capture.record(Duration::from_millis(500), None)?;
        println!(
            "{}",
            serde_json::json!({ "recordedId": id, "durationMs": result.duration.as_millis(), "peakRms": result.peak_rms, "droppedSamples": result.dropped_samples })
        );
        if result.duration.is_zero() {
            return Err("microphone delivered no audio frames".into());
        }
    }
    Ok(())
}
