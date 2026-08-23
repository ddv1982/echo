use std::time::Duration;

use echo::audio::AudioCapture;

#[test]
#[ignore = "needs ECHO_LIVE_MIC=1 and a real microphone"]
fn record_stable_device_id() {
    assert_eq!(
        std::env::var("ECHO_LIVE_MIC").ok().as_deref(),
        Some("1"),
        "set ECHO_LIVE_MIC=1 to run the live mic check"
    );
    let device = echo::audio::microphone_snapshot()
        .devices
        .into_iter()
        .next()
        .expect("need an input device");
    let capture = AudioCapture::open_exact(Some(&device.id)).expect("open device by stable ID");
    assert_eq!(capture.device_id, device.id);
    assert_eq!(capture.fallback_from, None);
    let cancel = capture.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        cancel.cancel();
    });
    let result = capture
        .record(Duration::from_secs(3), None)
        .expect("record 16 kHz mono");
    assert!(
        result.peak_rms > 0.001,
        "mic looked dead, peak_rms={}",
        result.peak_rms
    );
}
