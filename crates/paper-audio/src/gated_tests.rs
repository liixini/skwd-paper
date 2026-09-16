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
