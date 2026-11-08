use anyhow::Result;

use crate::cli::Cli;
use crate::image_paper;

const EMPTY_SIZE: &str = "empty stream size";
const OUTPUT_COUNT: &str = "each --frame-stream needs its own --stream-output";
const LOGICAL_COUNT: &str = "each --frame-stream needs its own --stream-logical";

pub(crate) fn run() -> Result<()> {
    paper_runtime::init_process();

    let cli = Cli::read();
    tracing::info!(file = %cli.file, output = %cli.output, "starting skwd-wall-still");

    if !cli.frame_stream.is_empty() {
        let sizes =
            cli.frame_stream.iter().map(|size| parse_size(size)).collect::<Result<Vec<_>>>()?;
        anyhow::ensure!(
            cli.frame_fd.is_empty() || cli.frame_fd.len() == sizes.len(),
            "each --frame-stream needs its own --frame-fd when pipes are used"
        );
        anyhow::ensure!(
            cli.stream_output.is_empty() || cli.stream_output.len() == sizes.len(),
            OUTPUT_COUNT
        );
        anyhow::ensure!(
            cli.stream_logical.is_empty() || cli.stream_logical.len() == sizes.len(),
            LOGICAL_COUNT
        );
        let outputs = sizes
            .into_iter()
            .enumerate()
            .map(|(index, (width, height))| {
                let logical = cli
                    .stream_logical
                    .get(index)
                    .map(|size| parse_dimensions(size))
                    .transpose()?
                    .unwrap_or((width, height));
                Ok(image_paper::StreamOutput {
                    size: [width, height],
                    logical: [logical.0, logical.1],
                    name: cli.stream_output.get(index).unwrap_or(&cli.output).clone(),
                    fd: cli.frame_fd.get(index).copied(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        return image_paper::stream(image_paper::StreamSpec {
            path: cli.file,
            outputs,
            fill: cli.fill_mode,
            blur: cli.blur,
            dim: cli.dim,
            header: !cli.stream_no_header,
        });
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
    let (width, height) = parse_dimensions(value)?;
    if width < 16 || height < 16 {
        anyhow::bail!("frame stream dimensions must be at least 16x16");
    }
    if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 3840 * 2160 {
        anyhow::bail!("frame stream dimensions exceed the 8192-edge/3840x2160-pixel cap");
    }
    Ok((width, height))
}

fn parse_dimensions(value: &str) -> Result<(u32, u32)> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("frame stream size must be WIDTHxHEIGHT"))?;
    let width = width.parse::<u32>()?;
    let height = height.parse::<u32>()?;
    anyhow::ensure!(width > 0 && height > 0, EMPTY_SIZE);
    Ok((width, height))
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
