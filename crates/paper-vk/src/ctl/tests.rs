#![cfg(test)]

use super::{Ctl, parse_audio_opts};
use paper_control::PaperCommand;

#[test]
fn photo_preserves_playback() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    ctl.paused = true;
    ctl.frozen_at = Some(std::time::Instant::now());
    let frozen = ctl.frozen_at;
    let mut command = PaperCommand::snapshot("/cache/applied.ppm", "DP-1");
    command.drawing_photo = Some("/cache/styled.ppm".into());
    tx.send(command).unwrap();
    assert!(ctl.poll().is_none());
    assert_eq!(ctl.drawing_photo.as_deref(), Some("/cache/styled.ppm"));
    assert_eq!(ctl.snapshot_output(), Some("DP-1"));
    assert_eq!(ctl.frozen_at, frozen);
    assert!(ctl.paused);
}

#[test]
fn snapshot_surface_match() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    assert!(ctl.snapshots_surface("DP-1"));
    tx.send(PaperCommand::snapshot("/cache/named.ppm", "DP-2")).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.snapshots_surface("DP-2"));
    assert!(!ctl.snapshots_surface("DP-1"));
    tx.send(PaperCommand::snapshot("/cache/any.ppm", "*")).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.snapshots_surface("DP-1"));
}

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
        reveal: None,
        drawing_photo: None,
        snapshot: None,
        drawing: None,
        surface: None,
        widgets: None,
        system_audio: None,
        to: String::new(),
        mute: Some(false),
        volume: Some(40),
        pause: None,
        duck: None,
        freeze: None,
        capture: None,
        shader: None,
        duration_ms: None,
        outputs: None,
        properties: None,
        pointer: None,
        mirror: None,
    })
    .unwrap();
    assert!(ctl.poll().is_none());
    assert!(!ctl.mute);
    assert_eq!(ctl.volume, 40);

    tx.send(PaperCommand {
        reveal: None,
        drawing_photo: None,
        snapshot: None,
        drawing: None,
        surface: None,
        widgets: None,
        system_audio: None,
        to: String::new(),
        mute: None,
        volume: None,
        pause: Some(true),
        duck: None,
        freeze: None,
        capture: None,
        shader: None,
        duration_ms: None,
        outputs: None,
        properties: None,
        pointer: None,
        mirror: None,
    })
    .unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.paused);

    tx.send(PaperCommand::freeze("/cache/frame.ppm")).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.freeze_pending());
    assert_eq!(ctl.take_freeze(), Some(("/cache/frame.ppm".into(), true)));

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
fn transition_finishes_with_latest_pause_request_retained() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    for paused in [true, false, true] {
        tx.send(PaperCommand::pause(paused)).unwrap();
        assert!(ctl.poll().is_none());
        assert!(!ctl.render_paused(true));
        assert_eq!(ctl.paused, paused);
        assert_eq!(ctl.render_paused(false), paused);
    }
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

#[test]
fn routed_output_pauses_stay_out_of_the_global_pause() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    tx.send(serde_json::from_str(r#"{"to":"DP-1","pause":true}"#).unwrap()).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.paused);
    assert!(ctl.take_output_pauses().is_empty());
    ctl.route_output_pauses();
    tx.send(serde_json::from_str(r#"{"to":"DP-2","pause":true}"#).unwrap()).unwrap();
    tx.send(serde_json::from_str(r#"{"to":"","pause":false}"#).unwrap()).unwrap();
    tx.send(serde_json::from_str(r#"{"to":"*","pause":true}"#).unwrap()).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.paused);
    assert_eq!(ctl.take_output_pauses(), vec![("DP-2".to_string(), true)]);
    assert!(ctl.take_output_pauses().is_empty());
}

#[test]
fn duck_command_leaves_mute_and_pause_alone() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    tx.send(PaperCommand::audio(Some(false), Some(60))).unwrap();
    tx.send(PaperCommand::duck(true)).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.ducked);
    assert!(!ctl.mute);
    assert!(!ctl.paused);
    assert_eq!(ctl.volume, 60);
    tx.send(PaperCommand::audio(Some(true), None)).unwrap();
    tx.send(PaperCommand::swap_video("/v/next.mp4", false, 70)).unwrap();
    assert!(ctl.poll().is_some());
    assert!(ctl.ducked);
    tx.send(PaperCommand::duck(false)).unwrap();
    assert!(ctl.poll().is_none());
    assert!(!ctl.ducked);
    assert!(!ctl.mute);
}

#[test]
fn surface_effects_redraw_without_resuming_or_swapping() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    ctl.set_paused(true);
    let mut command = PaperCommand::audio(None, None);
    command.surface = Some(Box::new(paper_control::SurfacePolicy {
        namespace: "stationary".into(),
        blur: 20,
        dim: 10,
    }));
    tx.send(command).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.paused);
    assert!(ctl.take_surface_change());
    assert!(!ctl.take_surface_change());
}

#[test]
fn scene_swap_keeps_wake() {
    let (_tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::from_channel(rx, None, "", true, 80, false);
    let descriptor = ctl.wake_fd().expect("owner wake");
    ctl.set_scene_voices(Vec::new(), None);
    let voice = paper_audio::Voice {
        id: "once".into(),
        name: "once".into(),
        clips: vec!["/nonexistent/once.wav".into()],
        gain: 1.0,
        mode: paper_audio::VoiceMode::Once,
        min_gap: 0.0,
        max_gap: 0.0,
        autostart: true,
    };
    ctl.set_scene_voices(vec![voice.clone()], None);
    assert_eq!(ctl.wake_fd(), Some(descriptor));
    let old_wake = ctl.wake.as_ref().unwrap().clone();
    ctl.set_scene_voices(Vec::new(), None);
    ctl.set_scene_voices(vec![voice], None);
    old_wake.poke();
    let mut poll = libc::pollfd { fd: descriptor, events: libc::POLLIN, revents: 0 };
    assert_eq!(unsafe { libc::poll(&mut poll, 1, 0) }, 1);
    assert!(ctl.poll().is_none());
    assert_eq!(unsafe { libc::poll(&mut poll, 1, 0) }, 0);
    assert_eq!(ctl.wake_fd(), Some(descriptor));
    assert_eq!(ctl.scene_voice_states(), [("once".to_string(), true)]);
}

#[test]
fn standalone_ctl_has_wake() {
    let ctl = Ctl::start_opts("", true, 80, false, false);
    assert!(ctl.wake_fd().is_some());
}

#[test]
fn spectrum_source_command() {
    let mut ctl = Ctl::start_opts("", true, 100, false, false);
    for source in [true, false] {
        let command =
            serde_json::from_value(serde_json::json!({"system_audio": source, "volume": 25}))
                .unwrap();
        assert!(ctl.reduce(command).is_none());
        assert_eq!(ctl.system_audio(), Some(source));
        assert_eq!(ctl.volume, 25);
    }
}

#[test]
fn reveal_keeps_pause() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    ctl.set_paused(true);
    let mut command = PaperCommand::audio(None, None);
    command.reveal = Some(true);
    tx.send(command).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.reveal);
    assert!(ctl.paused);
}

#[test]
fn snapshot_rollback_restores_clock() {
    for paused in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctl = Ctl::with_receiver(rx);
        ctl.set_paused(paused);
        let mut command = PaperCommand::snapshot("/cache/frame.ppm", "DP-1");
        command.pause = Some(true);
        tx.send(command).unwrap();
        ctl.poll();
        assert!(ctl.frozen_at.is_some());
        assert_eq!(ctl.take_freeze(), Some(("/cache/frame.ppm".into(), false)));
        tx.send(PaperCommand::pause(paused)).unwrap();
        ctl.poll();
        assert_eq!(ctl.paused, paused);
        assert!(ctl.frozen_at.is_none());
    }
}

#[test]
fn held_snapshot_freezes_clock() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    let mut command = PaperCommand::snapshot("/cache/frame.ppm", "DP-1");
    command.pause = Some(true);
    command.freeze = Some("/cache/frame.ppm".into());
    tx.send(command).unwrap();
    assert!(ctl.poll().is_none());
    assert!(ctl.paused);
    assert!(ctl.frozen_at.is_some());
    assert_eq!(ctl.snapshot_output(), Some("DP-1"));
    assert_eq!(ctl.take_freeze(), Some(("/cache/frame.ppm".into(), false)));
    assert!(!ctl.freeze_pending());
    tx.send(PaperCommand::pause(false)).unwrap();
    ctl.poll();
    assert!(!ctl.paused);
    assert!(ctl.frozen_at.is_none());
}

#[test]
fn snapshot_preserves_playback() {
    for paused in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctl = Ctl::with_receiver(rx);
        ctl.set_paused(paused);
        tx.send(PaperCommand::snapshot("/cache/current.ppm", "DP-1")).unwrap();
        assert!(ctl.poll().is_none());
        assert_eq!(ctl.paused, paused);
        assert!(ctl.frozen_at.is_none());
        assert!(ctl.freeze_pending());
        assert_eq!(ctl.take_freeze(), Some(("/cache/current.ppm".into(), false)));
        assert!(!ctl.freeze_pending());
    }
}

#[test]
fn retain_preserves_playback() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(rx);
    tx.send(PaperCommand::retain_outputs(&strs(&["DP-2"]))).unwrap();
    assert!(ctl.poll().is_none());
    assert_eq!(ctl.retained_outputs.take(), Some(strs(&["DP-2"])));
    assert!(!ctl.paused);
    assert!(ctl.frozen_at.is_none());
}

#[test]
fn held_release_keeps_duck() {
    let (send, receive) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(receive);
    ctl.audio = Some(super::AudioSink::Media(paper_audio::GatedAudio::new(
        "/nonexistent/held.mp4",
        true,
        80,
    )));
    for command in [
        PaperCommand::freeze("/cache/frame.ppm"),
        PaperCommand::duck(true),
        PaperCommand::audio(Some(false), Some(42)),
        PaperCommand::pause(false),
    ] {
        send.send(command).unwrap();
        assert!(ctl.poll().is_none());
        let Some(super::AudioSink::Media(audio)) = &ctl.audio else {
            panic!("missing audio");
        };
        assert!(!audio.pipeline_running());
    }
    assert!(!ctl.paused);
    assert!(!ctl.mute);
    assert!(ctl.ducked);
    assert_eq!(ctl.volume, 42);
}

#[test]
fn repeated_freeze_preserves_clock() {
    let (send, receive) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(receive);
    ctl.set_paused(true);
    let started = std::time::Instant::now();
    ctl.frozen_at = Some(started);
    for _ in 0..2 {
        send.send(PaperCommand::freeze("/cache/current.ppm")).unwrap();
        assert!(ctl.poll().is_none());
        assert_eq!(ctl.frozen_at, Some(started));
        ctl.take_freeze();
        ctl.finish_freeze();
    }
}

#[test]
fn failed_freeze_restores_pause() {
    for paused in [false, true] {
        let (send, receive) = std::sync::mpsc::channel();
        let mut ctl = Ctl::with_receiver(receive);
        ctl.set_paused(paused);
        let started = paused.then(std::time::Instant::now);
        ctl.frozen_at = started;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing/frame.ppm");
        send.send(PaperCommand::freeze(&path.display().to_string())).unwrap();
        assert!(ctl.poll().is_none());
        crate::freeze::fail_requested(&mut ctl, &anyhow::anyhow!("capture failure"));
        assert_eq!(ctl.paused, paused);
        assert_eq!(ctl.frozen_at, started);
    }
}

#[test]
fn initial_pause_freezes_transition() {
    if std::env::var_os(paper_control::PLAYBACK_ENV).is_none() {
        for paused in [false, true] {
            let playback = paper_control::PlaybackState { paused, ducked: true };
            assert!(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "ctl::tests::initial_pause_freezes_transition"])
                    .env(paper_control::PLAYBACK_ENV, serde_json::to_string(&playback).unwrap())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        return;
    }
    let playback = paper_control::PlaybackState::from_env().unwrap();
    let (_send, receive) = std::sync::mpsc::channel();
    let mut ctl = Ctl::from_channel(receive, None, "/nonexistent/startup.mp4", false, 42, true);
    assert_eq!(ctl.render_paused(true), playback.paused);
    assert!(ctl.ducked);
    let Some(super::AudioSink::Media(audio)) = &ctl.audio else {
        panic!("missing audio");
    };
    assert!(!audio.pipeline_running());
    ctl.set_paused(false);
    assert!(!ctl.render_paused(true));
    assert!(!ctl.mute);
    assert_eq!(ctl.volume, 42);
}

#[test]
fn widget_updates_keep_pause() {
    let (send, receive) = std::sync::mpsc::channel();
    let mut ctl = Ctl::with_receiver(receive);
    ctl.set_paused(true);
    let mut scene = paper_control::WidgetScene {
        widgets: vec![paper_control::Widget {
            face: paper_control::Face::Clock(paper_control::ClockStyle::Aperture),
            depth: paper_control::Depth::Front,
            pin: paper_control::Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] },
            height: None,
            output: None,
        }],
        outlines: Vec::new(),
        palette: paper_control::Palette { text: [1, 2, 3], accent: [4, 5, 6], shade: [7, 8, 9] },
        font: None,
        labels: paper_control::Labels {
            days: std::array::from_fn(|_| String::new()),
            day_names: std::array::from_fn(|_| String::new()),
            months: std::array::from_fn(|_| String::new()),
            month_names: std::array::from_fn(|_| String::new()),
        },
    };
    let mut command = PaperCommand::audio(None, None);
    command.widgets = Some(Box::new(scene.clone()));
    send.send(command).unwrap();
    assert!(ctl.poll().is_none());
    assert_eq!(ctl.take_widgets(), Some(scene.clone()));
    assert_eq!(ctl.take_widgets(), None);
    assert!(ctl.paused);

    scene.widgets.clear();
    let mut command = PaperCommand::audio(None, None);
    command.widgets = Some(Box::new(scene.clone()));
    send.send(command).unwrap();
    assert!(ctl.poll().is_none());
    assert_eq!(ctl.take_widgets(), Some(scene));
    assert!(ctl.paused);
}
