use super::{
    consume_output_releases, control_enabled_for_overlay, require_presentation_confirmation,
    should_park_still, should_release_source_frame,
};

#[test]
fn still_pending_swap() {
    assert!(!should_park_still(true, false, true, 1));
}

#[test]
fn still_parks_when_idle() {
    assert!(should_park_still(true, false, false, 1));
    assert!(!should_park_still(true, true, false, 1));
    assert!(!should_park_still(true, false, false, 0));
    assert!(!should_park_still(false, false, false, 1));
}

#[test]
fn transition_pacer_retains_outgoing_video_frame() {
    assert!(!should_release_source_frame(false, true));
    assert!(!should_release_source_frame(true, true));
    assert!(!should_release_source_frame(true, false));
    assert!(should_release_source_frame(false, false));
}

#[test]
fn held_overlay_keeps_control_reader() {
    assert!(control_enabled_for_overlay(true, true));
    assert!(!control_enabled_for_overlay(true, false));
    assert!(control_enabled_for_overlay(false, false));
}

#[test]
fn available_presentation_feedback_is_fail_closed() {
    assert!(require_presentation_confirmation(Some(true), "frame").is_ok());
    assert!(require_presentation_confirmation(Some(false), "frame").is_err());
    assert!(require_presentation_confirmation(None, "frame").is_ok());
}

#[test]
fn closed_output_no_release() {
    let mut pending = [true, true];
    assert!(!consume_output_releases(&mut pending, |si| si == 1));
    assert_eq!(pending, [true, false]);
}

#[test]
fn release_after_all_outputs() {
    let mut pending = [true, false, true];
    assert!(!consume_output_releases(&mut pending, |si| si == 0));
    assert!(consume_output_releases(&mut pending, |si| si == 2));
    assert_eq!(pending, [false, false, false]);
}
