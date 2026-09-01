use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "skwd-paper", version, about = "Standalone wallpaper composition controller")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    Apply(ApplyArgs),
    Stop(StopArgs),
    Pause,
    Resume,
    Audio(AudioArgs),
    Status,
    Capabilities(CapabilitiesArgs),
    PresentPlasma(PresentPlasmaArgs),
    #[command(hide = true)]
    Serve,
}

#[derive(Args)]
pub(crate) struct PresentPlasmaArgs {
    #[arg(long)]
    pub(crate) assignment: String,
    #[arg(long)]
    pub(crate) stream_size: String,
    #[arg(long)]
    pub(crate) stream_fps: u32,
    #[arg(long)]
    pub(crate) stream_fd: i32,
    #[arg(long)]
    pub(crate) paused: bool,
}

#[derive(Args)]
pub(crate) struct ApplyArgs {
    #[arg(required_unless_present = "manifest")]
    pub(crate) output: Option<String>,
    #[arg(required_unless_present = "manifest")]
    pub(crate) path: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) kind: Option<KindArg>,
    #[arg(long, value_enum)]
    pub(crate) engine: Option<EngineArg>,
    #[arg(long)]
    pub(crate) frame_rate: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) fill_mode: Option<FillModeArg>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub(crate) mute: Option<bool>,
    #[arg(long)]
    pub(crate) volume: Option<u32>,
    #[arg(long, value_enum)]
    pub(crate) layer: Option<LayerArg>,
    #[arg(long)]
    pub(crate) transition: bool,
    #[arg(long)]
    pub(crate) transition_from: Option<String>,
    #[arg(long)]
    pub(crate) effect: Option<String>,
    #[arg(long)]
    pub(crate) duration_ms: Option<u64>,
    #[arg(long)]
    pub(crate) properties: Option<String>,
    #[arg(long, conflicts_with_all = ["output", "path", "kind", "engine", "frame_rate", "fill_mode", "mute", "volume", "layer", "transition", "transition_from", "effect", "duration_ms", "properties"])]
    pub(crate) manifest: Option<String>,
    #[arg(long)]
    pub(crate) replace_all: bool,
}

#[derive(Args)]
pub(crate) struct StopArgs {
    pub(crate) outputs: Vec<String>,
}

#[derive(Args)]
pub(crate) struct AudioArgs {
    pub(crate) outputs: Vec<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub(crate) mute: Option<bool>,
    #[arg(long)]
    pub(crate) volume: Option<u32>,
}

#[derive(Args)]
pub(crate) struct CapabilitiesArgs {
    #[arg(long = "reset-cache")]
    pub(crate) reset_decode_cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum KindArg {
    Static,
    Video,
    We,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum EngineArg {
    #[value(alias = "regular", alias = "tiny")]
    Default,
    Tinier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum FillModeArg {
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
    Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LayerArg {
    Background,
    Bottom,
    Top,
}

impl Cli {
    pub(crate) fn read() -> Self {
        <Self as Parser>::parse()
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
