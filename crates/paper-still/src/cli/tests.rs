#![cfg(test)]

use super::*;
use clap::Parser;

#[test]
fn version_is_headless_and_exact() {
    let error = Cli::try_parse_from(["skwd-wall-still", "--version"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    assert_eq!(error.to_string(), concat!("skwd-wall-still ", env!("CARGO_PKG_VERSION"), "\n"));
}

#[test]
fn fill_mode_parser() {
    assert_eq!(parse_fill_mode("fit"), Ok(FillMode::Fit));
    assert_eq!(parse_fill_mode("span"), Ok(FillMode::Span));
    assert!(parse_fill_mode("Fit").is_err());
    assert!(parse_fill_mode("banana").is_err());
}

#[test]
fn layer_parser() {
    assert_eq!(parse_layer("background"), Ok(paper_control::Layer::Background));
    assert_eq!(parse_layer("bottom"), Ok(paper_control::Layer::Bottom));
    assert_eq!(parse_layer("top"), Ok(paper_control::Layer::Top));
    assert!(parse_layer("overlay").is_err());
}

#[test]
fn frame_stream_contract() {
    let cli = Cli::try_parse_from([
        "skwd-wall-still",
        "*",
        "/wall/a.png",
        "--frame-stream",
        "1920x1080",
        "--fill-mode",
        "fit",
        "--stream-no-header",
    ])
    .unwrap();
    assert_eq!(cli.frame_stream.as_deref(), Some("1920x1080"));
    assert_eq!(cli.fill_mode, FillMode::Fit);
    assert!(cli.stream_no_header);
}
