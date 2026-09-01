use anyhow::Result;

use crate::cli::Cli;
use crate::image_paper;

pub(crate) fn run() -> Result<()> {
    paper_runtime::init_process();

    let cli = Cli::read();
    tracing::info!(file = %cli.file, output = %cli.output, "starting skwd-wall-still");

    if let Some(size) = cli.frame_stream.as_deref() {
        let (width, height) = parse_size(size)?;
        return image_paper::stream(
            &cli.file,
            width,
            height,
            cli.fill_mode,
            cli.blur,
            cli.dim,
            !cli.stream_no_header,
        );
    }

    let target = image_paper::OutputTarget::from_arg(&cli.output);
    image_paper::run(
        target,
        &cli.file,
        cli.persist,
        cli.fill_mode,
        &cli.namespace,
        cli.layer,
        cli.blur,
        cli.dim,
    )
}

fn parse_size(value: &str) -> Result<(u32, u32)> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("frame stream size must be WIDTHxHEIGHT"))?;
    let width = width.parse::<u32>()?;
    let height = height.parse::<u32>()?;
    if width < 16 || height < 16 {
        anyhow::bail!("frame stream dimensions must be at least 16x16");
    }
    if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 3840 * 2160 {
        anyhow::bail!("frame stream dimensions exceed the 8192-edge/3840x2160-pixel cap");
    }
    Ok((width, height))
}
