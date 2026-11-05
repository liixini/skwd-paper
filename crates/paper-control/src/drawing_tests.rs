use super::*;

fn fixture() -> (tempfile::TempDir, DrawingTransition, PathBuf, serde_json::Value) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.png");
    std::fs::write(&source, b"source").unwrap();
    std::fs::write(dir.path().join("drawing.png"), b"texture").unwrap();
    std::fs::write(dir.path().join("strokes.bin"), b"strokes").unwrap();
    let metadata = std::fs::metadata(&source).unwrap();
    let manifest = serde_json::json!({
        "version": 3,
        "source": {"path":source,"size":metadata.len(),"mtime_ns":metadata.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos(),"sha256":source_hash(&source).unwrap()},
        "width":64,"height":32,"texture":"drawing.png","strokes":"strokes.bin","timing_width":32,"timing_height":16,"focus":[0.3,0.6]
    });
    let transition = DrawingTransition {
        artifact: dir.path().join("manifest.json"),
        colours: DrawingColours::default(),
        ink: DrawingInk::Plain,
    };
    std::fs::write(&transition.artifact, serde_json::to_vec(&manifest).unwrap()).unwrap();
    (dir, transition, source, manifest)
}

#[test]
fn accepts_matching_source() {
    let (_dir, transition, source, _) = fixture();
    let (manifest, texture, strokes) = transition.load(&source).unwrap();
    assert_eq!((manifest.width, manifest.height), (64, 32));
    assert_eq!((manifest.timing_width, manifest.timing_height), (32, 16));
    assert_eq!(texture.file_name().unwrap(), "drawing.png");
    assert_eq!(strokes.file_name().unwrap(), "strokes.bin");
}

#[test]
fn rejects_stale_source() {
    let (_dir, transition, source, _) = fixture();
    std::fs::write(&source, b"changed source").unwrap();
    assert!(transition.load(&source).is_err());
}

#[test]
fn rejects_invalid_artifacts() {
    for (field, value) in [
        ("version", serde_json::json!(1)),
        ("version", serde_json::json!(2)),
        ("version", serde_json::json!(4)),
        ("width", serde_json::json!(4097)),
        ("height", serde_json::json!(0)),
        ("timing_width", serde_json::json!(0)),
        ("timing_height", serde_json::json!(4097)),
        ("strokes", serde_json::json!("../strokes.bin")),
        ("texture", serde_json::json!("../drawing.png")),
        ("texture", serde_json::json!("/drawing.png")),
        ("focus", serde_json::json!([1.1, 0.0])),
    ] {
        let (_dir, transition, source, mut manifest) = fixture();
        manifest[field] = value;
        std::fs::write(&transition.artifact, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(transition.load(&source).is_err(), "{field}");
    }
}

#[test]
fn rejects_texture_symlink() {
    let (dir, transition, source, mut manifest) = fixture();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape.png")).unwrap();
    manifest["texture"] = serde_json::json!("escape.png");
    std::fs::write(&transition.artifact, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(transition.load(&source).is_err());
}

#[test]
fn drawing_wire_contract() {
    let transition: DrawingTransition =
        serde_json::from_str(r#"{"artifact":"/cache/manifest.json","finish":"swarm"}"#).unwrap();
    assert_eq!(transition.colours, DrawingColours::default());
    assert_eq!(transition.ink, DrawingInk::Plain);
    assert_eq!(
        serde_json::to_string(&transition).unwrap(),
        r#"{"artifact":"/cache/manifest.json","colours":{"paper":[241,238,229],"ink":[0,0,0]},"ink":"plain"}"#
    );
    let chalk: DrawingTransition = serde_json::from_str(
        r#"{"artifact":"/a.json","colours":{"paper":[20,24,22],"ink":[250,250,245]},"ink":"rainbow"}"#,
    )
    .unwrap();
    assert_eq!(chalk.colours, DrawingColours { paper: [20, 24, 22], ink: [250, 250, 245] });
    assert_eq!(chalk.ink, DrawingInk::Rainbow);
    assert!(
        serde_json::from_str::<DrawingTransition>(r#"{"artifact":"/a.json","ink":"gold"}"#)
            .is_err()
    );
}

#[test]
fn drawing_requires_static() {
    let transition = crate::TransitionPolicy {
        drawing_source: None,
        drawing: Some(DrawingTransition {
            artifact: "/cache/drawing.json".into(),
            colours: DrawingColours::default(),
            ink: DrawingInk::Plain,
        }),
        effect: Some(DRAWING_EFFECT.into()),
        duration_ms: Some(DRAWING_DURATION_MAX),
        ..Default::default()
    };
    let mut assignment =
        crate::Assignment::new(vec!["DP-1".into()], crate::Source::static_file("/wall/a.png"));
    assignment.transition = Some(transition.clone());
    let mut request =
        crate::ApplyRequest { assignments: vec![assignment], replace_all: false, policy: None };
    request.validate().unwrap();
    request.assignments[0].source = crate::Source::video("/video/a.mp4");
    assert!(request.validate().is_err());
    let mut missing = transition;
    missing.drawing = None;
    assert!(missing.validate().is_err());
}

#[test]
fn rejects_same_metadata_change() {
    let (_dir, transition, source, _) = fixture();
    let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
    std::fs::write(&source, b"mutate").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(transition.load(&source).is_err());
}

#[test]
fn describes_its_source() {
    let (dir, transition, source, _) = fixture();
    assert!(DrawingManifest::describes(&transition.artifact, &source));
    let other = dir.path().join("other.png");
    std::fs::write(&other, b"other").unwrap();
    assert!(!DrawingManifest::describes(&transition.artifact, &other));
    assert!(!DrawingManifest::describes(&dir.path().join("missing.json"), &source));
}
