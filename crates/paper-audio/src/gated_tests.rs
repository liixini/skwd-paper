use super::*;

#[test]
fn pipeline_gating() {
    assert!(wants_pipeline(false, 50));
    assert!(!wants_pipeline(true, 50));
    assert!(!wants_pipeline(false, 0));
    assert!(!wants_pipeline(true, 0));
}

#[test]
fn duck_is_independent_of_mute() {
    let mut gated = GatedAudio::new("/nonexistent/skwd-test-clip.mp4", false, 80);
    assert!(!gated.ducked());
    gated.set_duck(true);
    gated.set_mute(false);
    gated.set_volume(100);
    assert!(gated.ducked());
    assert!(!gated.pipeline_running());
    gated.swap("/nonexistent/skwd-test-next.mp4", false, 80);
    assert!(gated.ducked());
    gated.set_duck(false);
    assert!(!gated.ducked());
}

#[test]
fn paused_start_stays_inert() {
    let mut gated = GatedAudio::new("/nonexistent/paused.mp4", true, 80);
    gated.set_pause(true);
    gated.set_mute(false);
    gated.set_volume(42);
    assert!(!gated.pipeline_running());
    assert!(!gated.mute);
    assert_eq!(gated.volume, 42);
    gated.set_duck(true);
    gated.set_pause(false);
    assert!(!gated.pipeline_running());
    assert!(gated.ducked());
}
