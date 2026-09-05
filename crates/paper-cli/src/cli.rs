use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "skwd-paper",
    version,
    about = "Display images, animated GIFs, videos and Wallpaper Engine scenes on Wayland"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    #[command(about = "Apply an image, animated GIF, video, or Wallpaper Engine scene")]
    Apply(ApplyArgs),
    #[command(
        about = "Stop wallpapers on the named monitors, or all monitors if no names are given"
    )]
    Stop(StopArgs),
    #[command(about = "Pause all wallpaper playback")]
    Pause,
    #[command(about = "Resume all wallpaper playback")]
    Resume,
    #[command(about = "Change wallpaper mute or volume on the named monitors")]
    Audio(AudioArgs),
    #[command(about = "Show current wallpapers and available renderers as JSON")]
    Status,
    #[command(about = "List monitor names, resolutions and scale as JSON")]
    Outputs,
    #[command(about = "List supported media types, placement modes and controls as JSON")]
    Capabilities(CapabilitiesArgs),
    #[command(hide = true)]
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
    #[arg(
        required_unless_present = "manifest",
        help = "Monitor name, comma-separated names, or ALL / '*' for every monitor"
    )]
    pub(crate) output: Option<String>,
    #[arg(
        required_unless_present = "manifest",
        help = "Local media file or Wallpaper Engine project directory"
    )]
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
    #[arg(
        long,
        help = "Fade from the current wallpaper; use --effect to choose another transition"
    )]
    pub(crate) transition: bool,
    #[arg(long)]
    pub(crate) transition_from: Option<String>,
    #[arg(long)]
    pub(crate) effect: Option<String>,
    #[arg(long, help = "Transition duration in milliseconds (50-10000; default 600)")]
    pub(crate) duration_ms: Option<u64>,
    #[arg(
        long,
        help = "Pause playback after this many seconds without input; 0 disables it. Include --replace-all when changing this setting while wallpapers are running"
    )]
    pub(crate) idle_seconds: Option<u32>,
    #[arg(long)]
    pub(crate) properties: Option<String>,
    #[arg(long, help = "Apply wallpapers together using JSON, @file, or - to read JSON from stdin", conflicts_with_all = ["output", "path", "kind", "engine", "frame_rate", "fill_mode", "mute", "volume", "layer", "transition", "transition_from", "effect", "duration_ms", "idle_seconds", "properties"])]
    pub(crate) manifest: Option<String>,
    #[arg(
        long,
        help = "Apply these wallpapers and stop wallpapers on monitors not named in this command"
    )]
    pub(crate) replace_all: bool,
}

#[derive(Args)]
pub(crate) struct StopArgs {
    #[arg(value_delimiter = ',')]
    pub(crate) outputs: Vec<String>,
}

#[derive(Args)]
pub(crate) struct AudioArgs {
    #[arg(value_delimiter = ',')]
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
