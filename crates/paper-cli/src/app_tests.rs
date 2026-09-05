use super::*;
use clap::Parser;

#[test]
fn named_outputs_share_one_transaction_and_expose_idle_policy() {
    let cli = crate::cli::Cli::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1,DP-2",
        "/wall/a.png",
        "--transition",
        "--idle-seconds",
        "30",
        "--replace-all",
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    let request = apply_request(args).unwrap();
    assert_eq!(request.assignments.len(), 1);
    assert_eq!(request.assignments[0].outputs, ["DP-1", "DP-2"]);
    assert_eq!(request.policy.unwrap().idle_seconds, Some(30));
    assert!(request.replace_all);
}

#[test]
fn output_shorthand_preserves_protocol_validation() {
    for (output, valid) in
        [("ALL", true), ("*", true), ("DP-1,DP-1", false), ("DP-1,", false), ("DP-1,ALL", false)]
    {
        let cli = crate::cli::Cli::try_parse_from(["skwd-paper", "apply", output, "/wall/a.png"])
            .unwrap();
        let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
        let request = apply_request(args);
        assert_eq!(request.is_ok(), valid, "{output}");
        if valid {
            assert_eq!(request.unwrap().assignments[0].outputs, ["*"]);
        }
    }
}

#[test]
fn relative_paths_are_resolved_before_contacting_the_controller() {
    let cli = crate::cli::Cli::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "next.png",
        "--transition-from",
        "old.png",
    ])
    .unwrap();
    let crate::cli::Command::Apply(args) = cli.command else { panic!("expected apply") };
    let request = apply_request(args).unwrap();
    let current = std::env::current_dir().unwrap();
    assert_eq!(Path::new(&request.assignments[0].source.path), current.join("next.png"));
    assert_eq!(
        Path::new(request.assignments[0].transition.as_ref().unwrap().from.as_ref().unwrap()),
        current.join("old.png")
    );
}

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
