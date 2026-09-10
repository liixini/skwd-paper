#![cfg(test)]

use super::{Ctl, parse_audio_opts};
use paper_control::PaperCommand;

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[test]
fn audio_opts_parse() {
    assert_eq!(parse_audio_opts(&strs(&[])), (true, 80));
    assert_eq!(parse_audio_opts(&strs(&["-o", "mute=no;volume=55"])), (false, 55));
    assert_eq!(parse_audio_opts(&strs(&["-o", "mute=yes;volume=250"])), (true, 100));
    assert_eq!(parse_audio_opts(&strs(&["-o", "volume=banana"])), (true, 80));
    assert_eq!(parse_audio_opts(&strs(&["--fill-mode", "fill"])), (true, 80));
}

#[test]
fn ctl_reduce() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);

    tx.send(PaperCommand {
        to: String::new(),
        mute: Some(false),
        volume: Some(40),
        pause: None,
        freeze: None,
        capture: None,
        shader: None,
        duration_ms: None,
        outputs: None,
        properties: None,
    })
    .unwrap();
    assert!(ctl.poll().is_none());
    assert!(!ctl.mute);
    assert_eq!(ctl.volume, 40);

    tx.send(PaperCommand {
        to: String::new(),
        mute: None,
        volume: None,
        pause: Some(true),
        freeze: None,
        capture: None,
        shader: None,
        duration_ms: None,
        outputs: None,
        properties: None,
    })
    .unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.paused);

    tx.send(PaperCommand::freeze("/cache/frame.ppm")).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.freeze_pending());
    assert_eq!(ctl.take_freeze().as_deref(), Some("/cache/frame.ppm"));

    tx.send(PaperCommand::swap_video("/v/next.mp4", true, 70)).unwrap();
    let req = ctl.poll().expect("swap command yields a swap");
    assert_eq!(req.to, "/v/next.mp4");
    assert_eq!(req.duration_ms, 0);
    assert!(ctl.mute);
    assert_eq!(ctl.volume, 70);

    tx.send(PaperCommand::swap_paper("/v/fade.mp4", "fade", 600, false, 90)).unwrap();
    let req = ctl.poll().expect("swap_paper yields a swap");
    assert_eq!(req.duration_ms, 600);
    assert_eq!(req.shader.as_deref(), Some("fade"));
    assert!(!ctl.mute);

    tx.send(PaperCommand::swap_video("/v/a.mp4", true, 10)).unwrap();
    tx.send(PaperCommand::swap_video("/v/b.mp4", true, 10)).unwrap();
    let req = ctl.poll().expect("swap");
    assert_eq!(req.to, "/v/b.mp4");
}

#[test]
fn ctl_owns_wake_read_end() {
    let (_tx, rx) = std::sync::mpsc::channel();
    let wake = paper_runtime::wake::make_pipe().expect("pipe2");
    let read_fd = wake.read_fd();
    let ctl = Ctl::from_channel(rx, Some(wake), "", true, 80, false);
    assert_eq!(ctl.wake_fd(), Some(read_fd));
    drop(ctl);
}

#[test]
fn sender_guard_survives_drop() {
    let (_tx, rx) = std::sync::mpsc::channel();
    let sender_guard = paper_runtime::wake::make_pipe().expect("pipe2");
    let ctl = Ctl::from_channel(rx, Some(sender_guard.clone()), "", true, 80, false);
    drop(ctl);
    sender_guard.poke();
    sender_guard.drain();
}

#[test]
fn direct_pause_state_updates() {
    let (_tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    ctl.set_paused(true);
    assert!(ctl.paused);
    ctl.set_paused(false);
    assert!(!ctl.paused);
}

#[test]
fn scene_capture_preserves_playback_and_pause_state() {
    for paused in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctl = Ctl::with_receiver(rx);
        ctl.set_paused(paused);
        tx.send(PaperCommand::capture_scene("/scene/a", "/cache/frame.png")).unwrap();
        assert!(ctl.poll().is_none());
        assert_eq!(ctl.paused, paused);
        assert!(!ctl.freeze_pending());
        let capture = ctl.take_capture().unwrap();
        assert_eq!(capture.source, "/scene/a");
        assert_eq!(capture.path, "/cache/frame.png");
        assert!(ctl.take_capture().is_none());
    }
}
