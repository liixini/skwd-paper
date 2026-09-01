use super::*;
use clap::Parser;

#[test]
fn manifest_literal_and_file() {
    let json =
        r#"{"assignments":[{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/a.mp4"}}]}"#;
    assert_eq!(read_manifest(json).unwrap(), json);

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("composition.json");
    std::fs::write(&path, json).unwrap();
    let value = format!("@{}", path.display());
    assert_eq!(read_manifest(&value).unwrap(), json);

    let cli =
        <crate::cli::Cli as Parser>::try_parse_from(["skwd-paper", "apply", "--manifest", &value])
            .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    let request = apply_request(args).unwrap();
    assert_eq!(request.assignments[0].source.path, "/wall/a.mp4");
}

#[test]
fn manifest_file_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("too-large.json");
    std::fs::write(&path, vec![b'x'; (MAX_MANIFEST + 1) as usize]).unwrap();
    assert!(read_manifest(&format!("@{}", path.display())).is_err());
}

#[test]
fn transition_policy() {
    let cli = <crate::cli::Cli as Parser>::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "/wall/a.mp4",
        "--transition",
        "--effect",
        "fade",
        "--duration-ms",
        "800",
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    let request = apply_request(args).unwrap();
    let transition = request.assignments[0].transition.as_ref().unwrap();
    assert_eq!(transition.effect(), "fade");
    assert_eq!(transition.duration_ms(), 800);
}

#[test]
fn tinier_ivf_source() {
    let cli = <crate::cli::Cli as Parser>::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "/wall/loop.ivf",
        "--engine",
        "tinier",
        "--frame-rate",
        "30000/1001",
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    let request = apply_request(args).unwrap();
    let source = &request.assignments[0].source;
    assert_eq!(source.engine, Some(VideoEngine::Tinier));
    assert_eq!(source.frame_rate.as_deref(), Some("30000/1001"));
}

#[test]
fn engine_aliases() {
    for value in ["default", "regular", "tiny"] {
        let cli = <crate::cli::Cli as Parser>::try_parse_from([
            "skwd-paper",
            "apply",
            "DP-1",
            "/wall/loop.mp4",
            "--engine",
            value,
        ])
        .unwrap();
        let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
        let request = apply_request(args).unwrap();
        assert_eq!(request.assignments[0].source.engine, Some(VideoEngine::Default));
    }
}

#[test]
fn scene_properties_map() {
    let cli = <crate::cli::Cli as Parser>::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "/wall/workshop/431960/123",
        "--kind",
        "we",
        "--properties",
        r#"{"tint":"1 0 0","fade":0.5}"#,
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    let request = apply_request(args).unwrap();
    let properties = request.assignments[0].source.properties.as_ref().unwrap();
    assert_eq!(properties.get("tint").unwrap(), &serde_json::json!("1 0 0"));
    assert_eq!(properties.get("fade").unwrap(), &serde_json::json!(0.5));
}

#[test]
fn scene_properties_rejected() {
    let cli = <crate::cli::Cli as Parser>::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "/wall/item",
        "--kind",
        "we",
        "--properties",
        "[1,2]",
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    assert!(apply_request(args).is_err());

    let cli = <crate::cli::Cli as Parser>::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "/wall/a.mp4",
        "--kind",
        "video",
        "--properties",
        r#"{"tint":"1 0 0"}"#,
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    assert!(apply_request(args).is_err());
}
