use super::*;

#[test]
fn snapshot_legacy_freeze() {
    let mut command = PaperCommand::snapshot("/cache/frame.ppm", "DP-1");
    command.pause = Some(true);
    command.freeze = Some("/cache/frame.ppm".into());
    assert_eq!(round_trip(&command), command);
    assert!(
        matches!(classify_command(command), CommandClass::Freeze(path) if path == "/cache/frame.ppm")
    );
}

#[test]
fn drawing_photo_shape() {
    let command = PaperCommand::drawing_photo("/cache/styled.ppm");
    assert_eq!(command.line(), "{\"drawing_photo\":\"/cache/styled.ppm\",\"to\":\"\"}\n");
    assert_eq!(round_trip(&command), command);
    assert!(serde_json::from_str::<PaperCommand>("{}").unwrap().drawing_photo.is_none());
}

#[test]
fn widget_command_shape() {
    let scene = crate::WidgetScene {
        widgets: vec![crate::Widget {
            face: crate::Face::Clock(crate::ClockStyle::Aperture),
            depth: crate::Depth::Front,
            pin: crate::Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] },
            height: None,
            output: None,
        }],
        outlines: Vec::new(),
        palette: crate::Palette { text: [1, 2, 3], accent: [4, 5, 6], shade: [7, 8, 9] },
        font: None,
        labels: crate::Labels {
            days: std::array::from_fn(|_| String::new()),
            day_names: std::array::from_fn(|_| String::new()),
            months: std::array::from_fn(|_| String::new()),
            month_names: std::array::from_fn(|_| String::new()),
        },
    };
    let mut command = PaperCommand::audio(None, None);
    command.widgets = Some(Box::new(scene.clone()));
    assert!(
        serde_json::from_str::<serde_json::Value>(&command.line())
            .unwrap()
            .get("widgets")
            .is_some()
    );
    assert_eq!(round_trip(&command), command);
    assert_eq!(classify_command(command), CommandClass::Widgets(Box::new(scene)));
}

fn round_trip(command: &PaperCommand) -> PaperCommand {
    let line = command.line();
    assert!(line.ends_with('\n'));
    serde_json::from_str(line.trim()).unwrap()
}

#[test]
fn command_variants_round_trip() {
    for command in [
        PaperCommand::swap_video("/v/a.mp4", false, 55),
        PaperCommand::swap_paper("/v/b.mp4", "fade", 600, true, 90),
        PaperCommand::audio(Some(true), Some(30)),
        PaperCommand::audio(None, None),
        PaperCommand::pause(true),
        PaperCommand::pause(false),
        PaperCommand::duck(true),
        PaperCommand::duck(false),
        PaperCommand::freeze("/cache/frame.ppm"),
        PaperCommand::retain_outputs(&["DP-1".into(), "DP-2".into()]),
    ] {
        assert_eq!(round_trip(&command), command);
    }
}

#[test]
fn targeted_audio_round_trip() {
    let outputs = vec!["DP-1".to_string(), "DP-2".to_string()];
    let targeted = PaperCommand::audio_for(&outputs, Some(false), Some(250));
    assert_eq!(targeted.outputs.as_deref(), Some(outputs.as_slice()));
    assert_eq!(targeted.volume, Some(100));
    assert_eq!(round_trip(&targeted), targeted);
    assert_eq!(
        PaperCommand::audio(Some(true), Some(30)).line(),
        "{\"to\":\"\",\"mute\":true,\"volume\":30}\n"
    );
}

#[test]
fn wire_and_legacy_alias() {
    assert_eq!(
        PaperCommand::swap_video("/v/a.mp4", false, 250).line(),
        "{\"to\":\"/v/a.mp4\",\"mute\":false,\"volume\":100}\n"
    );
    assert_eq!(
        PaperCommand::swap_paper("/v/b.mp4", "fade", 600, true, 250).line(),
        "{\"to\":\"/v/b.mp4\",\"mute\":true,\"volume\":100,\"shader\":\"fade\",\"duration_ms\":600}\n"
    );
    assert_eq!(PaperCommand::pause(true).line(), "{\"to\":\"\",\"pause\":true}\n");
    assert_eq!(PaperCommand::duck(true).line(), "{\"to\":\"\",\"duck\":true}\n");
    assert_eq!(
        PaperCommand::freeze("/cache/frame.ppm").line(),
        "{\"to\":\"\",\"freeze\":\"/cache/frame.ppm\"}\n"
    );
    let legacy: PaperCommand = serde_json::from_str(r#"{"path":"/w/old.mp4"}"#).unwrap();
    assert_eq!(legacy.to, "/w/old.mp4");
}

#[test]
fn classify_priority_order() {
    assert_eq!(
        classify_command(PaperCommand::freeze("/cache/frame.ppm")),
        CommandClass::Freeze("/cache/frame.ppm".into())
    );
    assert_eq!(classify_command(PaperCommand::pause(true)), CommandClass::Pause(true));
    assert_eq!(classify_command(PaperCommand::duck(true)), CommandClass::Duck(true));
    let mut ducked_pause = PaperCommand::pause(false);
    ducked_pause.duck = Some(true);
    assert_eq!(classify_command(ducked_pause), CommandClass::Pause(false));
    assert_eq!(
        classify_command(PaperCommand::retain_outputs(&["DP-2".into()])),
        CommandClass::RetainOutputs(vec!["DP-2".into()])
    );
    assert_eq!(
        classify_command(PaperCommand::audio(Some(false), Some(40))),
        CommandClass::Audio { mute: Some(false), volume: Some(40) }
    );
    let swap = PaperCommand::swap_paper("/v/next.mp4", "sand-bloom", 700, true, 80);
    assert!(matches!(classify_command(swap), CommandClass::Swap(_)));
}

#[test]
fn capture_round_trip_keeps_source_and_destination_without_pause() {
    let command = PaperCommand::capture_scene("/scenes/42", "/cache/frame.png");
    assert_eq!(round_trip(&command), command);
    assert_eq!(command.pause, None);
    assert_eq!(command.freeze, None);
    assert_eq!(
        command.capture.unwrap(),
        crate::SceneCapture { source: "/scenes/42".into(), path: "/cache/frame.png".into() }
    );
}

#[test]
fn pointer_commands_quantize_and_classify() {
    let command = PaperCommand::pointer(0.25, 1.5, [true, false, true]);
    let state = command.pointer.unwrap();
    assert_eq!(state.buttons, 0b101);
    assert_eq!(state.pressed(), [true, false, true]);
    assert!((state.position()[0] - 0.25).abs() < 1e-4);
    assert!((state.position()[1] - 1.0).abs() < 1e-6);
    assert_eq!(round_trip(&command), command);
    assert!(command.line().contains("\"pointer\":{\"x\":16384,\"y\":65535,\"buttons\":5}"));
    assert!(matches!(classify_command(command), CommandClass::Pointer(_)));
    let plain: PaperCommand = serde_json::from_str("{\"to\":\"DP-1\",\"pause\":true}").unwrap();
    assert!(plain.pointer.is_none());
}

#[test]
fn mirror_command_precedence() {
    let command = PaperCommand::mirror("DP-2", "/run/user/1000/skwd-mini-lock/DP-2.sock");
    assert_eq!(
        command.line(),
        "{\"to\":\"\",\"outputs\":[\"DP-2\"],\"mirror\":{\"output\":\"DP-2\",\"socket\":\"/run/user/1000/skwd-mini-lock/DP-2.sock\"}}\n"
    );
    assert_eq!(
        classify_command(round_trip(&command)),
        CommandClass::Mirror(crate::MirrorTarget {
            output: "DP-2".into(),
            socket: "/run/user/1000/skwd-mini-lock/DP-2.sock".into()
        })
    );
    let stop: PaperCommand = serde_json::from_str(r#"{"mirror":{"output":"DP-2"}}"#).unwrap();
    assert!(
        matches!(classify_command(stop), CommandClass::Mirror(target) if target.socket.is_empty())
    );
}

#[test]
fn spectrum_source_roundtrip() {
    let command: PaperCommand =
        serde_json::from_str(r#"{"system_audio":false,"volume":35}"#).unwrap();
    assert_eq!(command.system_audio, Some(false));
    assert_eq!(command.volume, Some(35));
    let restored: PaperCommand = serde_json::from_str(&command.line()).unwrap();
    assert_eq!(restored, command);
}

#[test]
fn snapshot_wire_shape() {
    let command = PaperCommand::snapshot("/cache/current.ppm", "DP-1");
    let value = serde_json::to_value(&command).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"to":"", "snapshot":{"path":"/cache/current.ppm", "output":"DP-1"}})
    );
    assert_eq!(serde_json::from_value::<PaperCommand>(value).unwrap(), command);
}
