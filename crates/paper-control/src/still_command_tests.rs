use super::*;

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
