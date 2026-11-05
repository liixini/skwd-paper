use super::*;

#[test]
fn snapshot_wire_compatible() {
    let command = crate::PaperCommand::snapshot("/cache/frame.ppm", "DP-1");
    let still: StillCommand = serde_json::from_str(&command.line()).unwrap();
    assert_eq!(
        still.snapshot.unwrap(),
        crate::FrameSnapshot { path: "/cache/frame.ppm".into(), output: "DP-1".into() }
    );
    assert!(serde_json::from_str::<StillCommand>("{}").unwrap().snapshot.is_none());
}

#[test]
fn still_ndjson_shape() {
    let command = StillCommand::new("/w/new.png");
    assert_eq!(command.line(), "{\"path\":\"/w/new.png\"}\n");
    assert_eq!(serde_json::from_str::<StillCommand>(command.line().trim()).unwrap(), command);

    let slide = StillCommand::slide("/w/n.png", "up", 300);
    assert_eq!(slide.line(), "{\"path\":\"/w/n.png\",\"slide\":\"up\",\"duration_ms\":300}\n");
    let preload = StillCommand::preload(vec!["/w/a.png".into(), "/w/b.png".into()]);
    assert_eq!(preload.line(), "{\"path\":\"\",\"preload\":[\"/w/a.png\",\"/w/b.png\"]}\n");
    for invalid in [r#"{"path":7}"#, "{", r#"{"preload":"/w/a.png"}"#] {
        assert!(serde_json::from_str::<StillCommand>(invalid).is_err());
    }
    let empty: StillCommand = serde_json::from_str("{}").unwrap();
    assert!(empty.path.is_empty() && empty.preload.is_empty() && empty.slide.is_none());
}

#[test]
fn fill_optional() {
    let command = StillCommand::new("/w/new.png").with_fill("center");
    assert_eq!(command.line(), "{\"path\":\"/w/new.png\",\"fill\":\"center\"}\n");
    assert_eq!(StillCommand::new("/w/new.png").with_fill("").fill, None);
    let legacy: StillCommand = serde_json::from_str(r#"{"path":"/w/a.png"}"#).unwrap();
    assert_eq!(legacy.fill, None);
}

#[test]
fn reveal_is_explicit_and_legacy_commands_remain_unchanged() {
    let reveal = StillCommand::reveal();
    assert_eq!(reveal.line(), "{\"path\":\"\",\"reveal\":true}\n");
    assert_eq!(serde_json::from_str::<StillCommand>(&reveal.line()).unwrap(), reveal);
    for input in ["{}", r#"{"path":"/wall/a.png"}"#, r#"{"preload":["/wall/a.png"]}"#] {
        assert!(!serde_json::from_str::<StillCommand>(input).unwrap().reveal);
    }
    assert!(serde_json::from_str::<StillCommand>(r#"{"reveal":"true"}"#).is_err());
}

#[test]
fn mirror_target_shape() {
    let start = StillCommand::mirror("DP-1", "/run/user/1000/skwd-mini-lock/DP-1.sock");
    assert_eq!(
        start.line(),
        "{\"path\":\"\",\"mirror\":{\"output\":\"DP-1\",\"socket\":\"/run/user/1000/skwd-mini-lock/DP-1.sock\"}}\n"
    );
    assert_eq!(serde_json::from_str::<StillCommand>(&start.line()).unwrap(), start);
    let stop: StillCommand = serde_json::from_str(r#"{"mirror":{"output":"DP-1"}}"#).unwrap();
    assert_eq!(stop.mirror.unwrap().socket, "");
    assert!(
        serde_json::from_str::<StillCommand>(r#"{"path":"/w/a.png"}"#).unwrap().mirror.is_none()
    );
}
