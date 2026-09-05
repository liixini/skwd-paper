use super::decode::decode_image;
use crate::fill_mode::{FillMode, apply_fill_mode};
use anyhow::Result;
use std::io::{BufRead, Write};

pub fn stream(
    path: &str,
    width: u32,
    height: u32,
    fill_mode: FillMode,
    blur: f32,
    dim: u32,
    write_header: bool,
) -> Result<()> {
    let (source_width, source_height, source) = decode_image(path, blur, dim)?;
    let (_, _, frame) =
        apply_fill_mode(source_width, source_height, &source, width, height, fill_mode);
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    if write_header {
        output.write_all(b"SKWP")?;
        output.write_all(&width.to_le_bytes())?;
        output.write_all(&height.to_le_bytes())?;
    }
    output.write_all(&frame)?;
    output.flush()?;
    paper_runtime::plasma::frame_ready()?;
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let mut line = String::new();
    while input.read_line(&mut line)? != 0 {
        line.clear();
    }
    Ok(())
}
