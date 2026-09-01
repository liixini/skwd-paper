use crate::model::{ColorMatrix, FrameRate};
use paper_geom::FillMode;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub struct Options {
    pub outputs: Option<Vec<String>>,
    pub input: PathBuf,
    pub frame_rate: FrameRate,
    pub matrix: ColorMatrix,
    pub probe_loops: u32,
    pub decode_only: bool,
    pub stream_size: Option<(u32, u32)>,
    pub fill_mode: FillMode,
    pub paused: bool,
    pub stream_no_header: bool,
}

pub enum ParseResult {
    Run(Options),
    Help,
    Version,
}

impl Options {
    pub fn parse() -> Result<ParseResult, String> {
        let mut parsed = Self::parse_from(std::env::args_os().skip(1))?;
        if let ParseResult::Run(options) = &mut parsed {
            options.probe_loops = match first_env(&[
                "SKWD_PAPER_TINIER_PROBE_LOOPS",
                "SKWD_WALL_TINY_PROBE_LOOPS",
            ])? {
                Some(value) => parse_positive_env(&value, "probe loop count")?,
                None => 0,
            };
            options.decode_only = match first_env(&[
                "SKWD_PAPER_TINIER_PROBE_DECODE_ONLY",
                "SKWD_WALL_TINY_PROBE_DECODE_ONLY",
            ])? {
                Some(value) if value == "1" && options.probe_loops > 0 => true,
                Some(_) => {
                    return Err("decode-only mode requires probe loops and must equal 1".into());
                }
                None => false,
            };
        }
        Ok(parsed)
    }

    fn parse_from(arguments: impl IntoIterator<Item = OsString>) -> Result<ParseResult, String> {
        let arguments: Vec<OsString> = arguments.into_iter().collect();
        if arguments.len() == 1 && matches!(arguments[0].to_str(), Some("-h" | "--help")) {
            return Ok(ParseResult::Help);
        }
        if arguments.len() == 1 && matches!(arguments[0].to_str(), Some("-V" | "--version")) {
            return Ok(ParseResult::Version);
        }

        let mut outputs = None;
        let mut stream_size = None;
        let mut fill_mode = FillMode::Fill;
        let mut paused = false;
        let mut stream_no_header = false;
        let mut index = 0;
        while let Some(argument) = arguments.get(index) {
            if argument == "--output" {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--output requires a Wayland output name".to_string())?;
                outputs = Some(parse_outputs(text(value, "output name")?)?);
                index += 2;
            } else if argument == "--frame-stream" {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--frame-stream requires WIDTHxHEIGHT".to_string())?;
                stream_size = Some(parse_size(text(value, "frame stream size")?)?);
                index += 2;
            } else if argument == "--fill-mode" {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--fill-mode requires a value".to_string())?;
                fill_mode = text(value, "fill mode")?
                    .parse()
                    .map_err(|()| "unknown fill mode".to_string())?;
                index += 2;
            } else if argument == "--paused" {
                paused = true;
                index += 1;
            } else if argument == "--stream-no-header" {
                stream_no_header = true;
                index += 1;
            } else {
                break;
            }
        }
        let positionals = &arguments[index..];

        if !(2..=3).contains(&positionals.len()) {
            return Err("expected an input path, frame rate, and optional color matrix".into());
        }
        let frame_rate = FrameRate::parse(text(&positionals[1], "frame rate")?)?;
        let matrix = match positionals.get(2).map(OsString::as_os_str) {
            None => ColorMatrix::Bt709,
            Some(value) if value == "bt709" => ColorMatrix::Bt709,
            Some(value) if value == "bt601" => ColorMatrix::Bt601,
            Some(_) => return Err("color matrix must be bt601 or bt709".into()),
        };
        Ok(ParseResult::Run(Self {
            outputs,
            input: PathBuf::from(&positionals[0]),
            frame_rate,
            matrix,
            probe_loops: 0,
            decode_only: false,
            stream_size,
            fill_mode,
            paused,
            stream_no_header,
        }))
    }
}

fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| "frame stream size must be WIDTHxHEIGHT".to_string())?;
    let width = width.parse::<u32>().map_err(|_| "invalid frame stream width".to_string())?;
    let height = height.parse::<u32>().map_err(|_| "invalid frame stream height".to_string())?;
    if width < 16 || height < 16 {
        return Err("frame stream dimensions must be at least 16x16".into());
    }
    if width > crate::model::MAX_FRAME_EDGE
        || height > crate::model::MAX_FRAME_EDGE
        || u64::from(width) * u64::from(height) > crate::model::MAX_FRAME_PIXELS
    {
        return Err("frame stream dimensions exceed the 8192-edge/3840x2160-pixel cap".into());
    }
    Ok((width, height))
}

fn parse_outputs(value: &str) -> Result<Vec<String>, String> {
    let mut unique = BTreeSet::new();
    let outputs = value.split(',').map(String::from).collect::<Vec<_>>();
    if outputs.is_empty()
        || outputs.iter().any(|output| output.is_empty() || output.starts_with('-'))
        || outputs.iter().any(|output| !unique.insert(output.clone()))
    {
        return Err("output names must be non-empty, unique, and not start with '-'".into());
    }
    Ok(outputs)
}

fn first_env(names: &[&str]) -> Result<Option<String>, String> {
    for name in names {
        match std::env::var(name) {
            Ok(value) => return Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(format!("{name} is not valid UTF-8"));
            }
        }
    }
    Ok(None)
}

fn text<'a>(value: &'a OsStr, label: &str) -> Result<&'a str, String> {
    value.to_str().ok_or_else(|| format!("{label} is not valid UTF-8"))
}

fn parse_positive_env(value: &str, label: &str) -> Result<u32, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{label} must be a positive integer"));
    }
    let parsed = value.parse::<u32>().map_err(|_| format!("{label} exceeds u32"))?;
    if parsed == 0 {
        return Err(format!("{label} must be positive"));
    }
    Ok(parsed)
}

pub const fn usage() -> &'static str {
    "usage: skwd-paper-tinier [--output NAME | --frame-stream WIDTHxHEIGHT] [--fill-mode MODE] [--paused] [--stream-no-header] <loop.ivf> <fps|fps_num/fps_den> [bt601|bt709]"
}

mod tests;
