use std::time::Duration;

use echo::audio::AudioCapture;

#[test]
#[ignore = "needs ECHO_LIVE_MIC=1 and a real microphone"]
fn record_once() {
    assert_eq!(
        std::env::var("ECHO_LIVE_MIC").ok().as_deref(),
        Some("1"),
        "set ECHO_LIVE_MIC=1 to run the live mic check"
    );
    let capture = AudioCapture::open_default().expect("default input device");
    let cancel = capture.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        cancel.cancel();
    });
    let result = capture
        .record(Duration::from_secs(3), None)
        .expect("record 16 kHz mono");
    assert!(
        result.pcm.duration_ms() >= 1500,
        "expected about two seconds, got {} ms",
        result.pcm.duration_ms()
    );
    assert!(
        result.peak_rms > 0.001,
        "mic looked dead, peak_rms={}",
        result.peak_rms
    );
}
