use super::*;

#[test]
fn pipeline_gating() {
    assert!(wants_pipeline(false, 50));
    assert!(!wants_pipeline(true, 50));
    assert!(!wants_pipeline(false, 0));
    assert!(!wants_pipeline(true, 0));
}
