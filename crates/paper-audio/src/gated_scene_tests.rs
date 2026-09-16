#![cfg(test)]

use super::*;
use crate::mixer::VoiceMode;

fn voice(clip: &str) -> Voice {
    Voice {
        id: "1".into(),
        name: "test".into(),
        clips: vec![clip.into()],
        gain: 1.0,
        mode: VoiceMode::Loop,
        min_gap: 0.0,
        max_gap: 0.0,
        autostart: true,
    }
}

#[test]
fn muted_scene_no_mixer() {
    let clips = vec![voice("/nonexistent/skwd-test-clip.mp3")];
    assert!(!GatedScene::new(clips.clone(), true, 80).pipeline_running());
    assert!(!GatedScene::new(clips, false, 0).pipeline_running());
}

#[test]
fn empty_scene_inert() {
    let mut gated = GatedScene::new(Vec::new(), false, 80);
    assert!(!gated.pipeline_running());
    assert_eq!(gated.voice_count(), 0);
    gated.set_mute(false);
    gated.set_volume(100);
    gated.set_pause(true);
    gated.set_pause(false);
    assert!(!gated.pipeline_running());
}

#[test]
fn swap_drops_mixer() {
    let mut gated = GatedScene::new(vec![voice("/nonexistent/skwd-test-clip.mp3")], false, 80);
    assert!(!gated.pipeline_running());
    gated.swap(Vec::new(), false, 80);
    assert!(!gated.pipeline_running());
    assert_eq!(gated.voice_count(), 0);
}

#[test]
fn voice_operations_track_intent_without_a_mixer() {
    let mut silent = voice("/nonexistent/skwd-test-clip.mp3");
    silent.autostart = false;
    let mut gated = GatedScene::new(vec![silent], true, 80);
    assert!(!gated.pipeline_running());
    assert!(!gated.voice_playing("1"));
    assert!(gated.voice("1", VoiceOp::Play));
    assert!(gated.voice_playing("1"));
    assert!(gated.voice("1", VoiceOp::Pause));
    assert!(!gated.voice_playing("1"));
    assert!(!gated.voice("missing", VoiceOp::Stop));
    assert!(!gated.pipeline_running());
}

#[test]
fn duck_survives_voice_swap() {
    let mut gated = GatedScene::new(vec![voice("/nonexistent/skwd-test-clip.mp3")], false, 80);
    gated.set_duck(true);
    gated.swap(vec![voice("/nonexistent/skwd-test-other.mp3")], false, 100);
    assert!(gated.ducked());
    assert!(!gated.pipeline_running());
    gated.set_duck(false);
    assert!(!gated.ducked());
}
