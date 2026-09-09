use clap::Parser;
use paper_control::Layer;

use crate::fill_mode::FillMode;

#[derive(Parser, Debug)]
#[command(name = "skwd-wall-still")]
#[command(version)]
#[command(about = "Lightweight wallpaper renderer for static images (wl_shm only, no GPU)")]
pub(crate) struct Cli {
    pub(crate) output: String,
    pub(crate) file: String,
    #[arg(long = "persist")]
    pub(crate) persist: bool,
    #[arg(long = "fill-mode", default_value = "fill", value_parser = parse_fill_mode)]
    pub(crate) fill_mode: FillMode,
    #[arg(long = "namespace", default_value = "skwd-paper")]
    pub(crate) namespace: String,
    #[arg(long = "layer", default_value = "background", value_parser = parse_layer)]
    pub(crate) layer: Layer,
    #[arg(long = "blur", default_value_t = 0.0)]
    pub(crate) blur: f32,
    #[arg(long = "dim", default_value_t = 0)]
    pub(crate) dim: u32,
    #[arg(long = "frame-stream")]
    pub(crate) frame_stream: Option<String>,
    #[arg(long = "stream-no-header")]
    pub(crate) stream_no_header: bool,
}

impl Cli {
    pub(crate) fn read() -> Self {
        Self::parse()
    }
}

fn parse_fill_mode(value: &str) -> Result<FillMode, String> {
    value.parse().map_err(|()| {
        format!("unknown fill mode {value:?}; expected fill, fit, stretch, center, tile, or span")
    })
}

fn parse_layer(value: &str) -> Result<Layer, String> {
    match value {
        "background" => Ok(Layer::Background),
        "bottom" => Ok(Layer::Bottom),
        "top" => Ok(Layer::Top),
        "overlay" => Ok(Layer::Overlay),
        _ => Err(format!("unknown layer {value:?}; expected background, bottom, top, or overlay")),
    }
}

mod tests;
