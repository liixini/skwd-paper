use super::*;

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
