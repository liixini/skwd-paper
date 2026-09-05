use super::*;

#[test]
fn version_is_headless_and_exact() {
    let error = <Cli as Parser>::try_parse_from(["skwd-paper", "--version"])
        .err()
        .expect("version exits through clap");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    assert_eq!(error.to_string(), concat!("skwd-paper ", env!("CARGO_PKG_VERSION"), "\n"));
}

#[test]
fn apply_all_options() {
    let cli = <Cli as Parser>::try_parse_from([
        "skwd-paper",
        "apply",
        "DP-1",
        "/wall/a.mp4",
        "--kind",
        "video",
        "--engine",
        "default",
        "--fill-mode",
        "fit",
        "--mute",
        "false",
        "--volume",
        "45",
        "--layer",
        "top",
        "--transition",
        "--transition-from",
        "/wall/old.png",
        "--effect",
        "sand-bloom",
        "--duration-ms",
        "700",
        "--replace-all",
    ])
    .unwrap();
    let Command::Apply(args) = cli.command else { panic!("expected apply") };
    assert_eq!(args.output.as_deref(), Some("DP-1"));
    assert_eq!(args.path.as_deref(), Some("/wall/a.mp4"));
    assert_eq!(args.kind, Some(KindArg::Video));
    assert_eq!(args.engine, Some(EngineArg::Default));
    assert_eq!(args.fill_mode, Some(FillModeArg::Fit));
    assert_eq!(args.mute, Some(false));
    assert_eq!(args.volume, Some(45));
    assert_eq!(args.layer, Some(LayerArg::Top));
    assert!(args.transition);
    assert_eq!(args.transition_from.as_deref(), Some("/wall/old.png"));
    assert_eq!(args.effect.as_deref(), Some("sand-bloom"));
    assert_eq!(args.duration_ms, Some(700));
    assert!(args.replace_all);
}

#[test]
fn pause_resume_audio() {
    assert!(matches!(
        <Cli as Parser>::try_parse_from(["skwd-paper", "pause"]).unwrap().command,
        Command::Pause
    ));
    assert!(matches!(
        <Cli as Parser>::try_parse_from(["skwd-paper", "resume"]).unwrap().command,
        Command::Resume
    ));
    let cli = <Cli as Parser>::try_parse_from([
        "skwd-paper",
        "audio",
        "DP-1",
        "DP-2",
        "--mute",
        "false",
        "--volume",
        "55",
    ])
    .unwrap();
    let Command::Audio(args) = cli.command else { panic!("expected audio") };
    assert_eq!(args.outputs, ["DP-1", "DP-2"]);
    assert_eq!(args.mute, Some(false));
    assert_eq!(args.volume, Some(55));
}

#[test]
fn plasma_presenter_contract() {
    let assignment = r#"{"outputs":["DP-1"],"source":{"kind":"static","path":"/wall/a.png"}}"#;
    let cli = <Cli as Parser>::try_parse_from([
        "skwd-paper",
        "present-plasma",
        "--assignment",
        assignment,
        "--stream-size",
        "1920x1080",
        "--stream-fps",
        "60",
        "--stream-fd",
        "3",
        "--paused",
    ])
    .unwrap();
    let Command::PresentPlasma(args) = cli.command else { panic!("expected Plasma presenter") };
    assert_eq!(args.assignment, assignment);
    assert_eq!(args.stream_size, "1920x1080");
    assert_eq!(args.stream_fps, 60);
    assert_eq!(args.stream_fd, 3);
    assert!(args.paused);
}

#[test]
fn manifest_exclusive() {
    let manifest =
        r#"{"assignments":[{"outputs":["DP-1"],"source":{"kind":"static","path":"/wall/a.png"}}]}"#;
    assert!(
        <Cli as Parser>::try_parse_from(["skwd-paper", "apply", "--manifest", manifest]).is_ok()
    );
    assert!(
        <Cli as Parser>::try_parse_from([
            "skwd-paper",
            "apply",
            "--manifest",
            manifest,
            "--volume",
            "30",
        ])
        .is_err()
    );
    assert!(<Cli as Parser>::try_parse_from(["skwd-paper", "apply", "DP-1"]).is_err());
}

#[test]
fn capabilities_reset_cache() {
    let cli =
        <Cli as Parser>::try_parse_from(["skwd-paper", "capabilities", "--reset-cache"]).unwrap();
    let Command::Capabilities(args) = cli.command else { panic!("expected capabilities") };
    assert!(args.reset_decode_cache);
}

#[test]
fn output_discovery_and_comma_separated_controls() {
    assert!(matches!(
        Cli::try_parse_from(["skwd-paper", "outputs"]).unwrap().command,
        Command::Outputs
    ));
    let Command::Stop(stop) =
        Cli::try_parse_from(["skwd-paper", "stop", "DP-1,DP-2"]).unwrap().command
    else {
        panic!("expected stop")
    };
    assert_eq!(stop.outputs, ["DP-1", "DP-2"]);
    let Command::Audio(audio) =
        Cli::try_parse_from(["skwd-paper", "audio", "DP-1,DP-2", "--mute", "true"])
            .unwrap()
            .command
    else {
        panic!("expected audio")
    };
    assert_eq!(audio.outputs, ["DP-1", "DP-2"]);
    assert!(
        Cli::try_parse_from(["skwd-paper", "apply", "--manifest", "{}", "--idle-seconds", "30"])
            .is_err()
    );
}
