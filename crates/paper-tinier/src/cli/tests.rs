#![cfg(test)]

use super::*;

#[test]
fn positional_contract() {
    let parsed = Options::parse_from(["wall.ivf", "30000/1001", "bt601"].map(OsString::from));
    let ParseResult::Run(options) = parsed.unwrap() else {
        panic!("expected run options");
    };
    assert_eq!(options.input, PathBuf::from("wall.ivf"));
    assert_eq!(options.frame_rate, FrameRate { numerator: 30_000, denominator: 1001 });
    assert_eq!(options.matrix, ColorMatrix::Bt601);
}

#[test]
fn version_is_a_headless_parse_result() {
    for flag in ["-V", "--version"] {
        assert!(matches!(
            Options::parse_from([flag].map(OsString::from)),
            Ok(ParseResult::Version)
        ));
    }
}

#[test]
fn explicit_output_flag() {
    let parsed = Options::parse_from(["--output", "DP-1", "wall.ivf", "30"].map(OsString::from));
    let ParseResult::Run(options) = parsed.unwrap() else {
        panic!("expected run options");
    };
    assert_eq!(options.outputs.as_deref(), Some(["DP-1".into()].as_slice()));
    assert_eq!(options.matrix, ColorMatrix::Bt709);
}

#[test]
fn output_set_uniqueness() {
    let parsed =
        Options::parse_from(["--output", "DP-1,DP-2", "wall.ivf", "30"].map(OsString::from));
    let ParseResult::Run(options) = parsed.unwrap() else {
        panic!("expected run options");
    };
    assert_eq!(options.outputs.unwrap(), ["DP-1", "DP-2"]);
    for output in ["", "DP-1,", "DP-1,DP-1", "-bad"] {
        let parsed =
            Options::parse_from(["--output", output, "wall.ivf", "30"].map(OsString::from));
        assert!(parsed.is_err(), "{output}");
    }
}

#[test]
fn flags_after_positionals() {
    let parsed = Options::parse_from(["wall.ivf", "30", "--output", "DP-1"].map(OsString::from));
    assert!(parsed.is_err());
}

#[test]
fn frame_stream_contract() {
    let parsed = Options::parse_from(
        [
            "--frame-stream",
            "1920x1080",
            "--fill-mode",
            "fit",
            "--paused",
            "--stream-no-header",
            "wall.ivf",
            "30",
        ]
        .map(OsString::from),
    );
    let ParseResult::Run(options) = parsed.unwrap() else {
        panic!("expected run options");
    };
    assert_eq!(options.stream_size, Some((1920, 1080)));
    assert_eq!(options.fill_mode, FillMode::Fit);
    assert!(options.paused);
    assert!(options.stream_no_header);
    assert!(options.outputs.is_none());
}
