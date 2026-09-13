use super::decode::decode_image;
use crate::fill_mode::{FillMode, apply_fill_mode};
use anyhow::Result;
use std::io::{BufRead, Write};

pub fn stream(
    path: &str,
    sizes: &[(u32, u32)],
    frame_fds: &[i32],
    fill_mode: FillMode,
    blur: f32,
    dim: u32,
    write_header: bool,
) -> Result<()> {
    let (source_width, source_height, source) = decode_image(path, blur, dim)?;
    let stdout = std::io::stdout();
    for (index, &(width, height)) in sizes.iter().enumerate() {
        let (_, _, frame) =
            apply_fill_mode(source_width, source_height, &source, width, height, fill_mode);
        let mut sink: Box<dyn Write> = match frame_fds.get(index) {
            Some(&fd) if fd >= 0 => Box::new(paper_runtime::plasma::frame_pipe(fd)?),
            _ => Box::new(stdout.lock()),
        };
        write_frame(&mut sink, width, height, &frame, write_header)?;
    }
    paper_runtime::plasma::frame_ready()?;
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let mut line = String::new();
    while input.read_line(&mut line)? != 0 {
        line.clear();
    }
    Ok(())
}

pub(super) fn write_frame(
    sink: &mut dyn Write,
    width: u32,
    height: u32,
    frame: &[u8],
    write_header: bool,
) -> Result<()> {
    let mut output = std::io::BufWriter::new(sink);
    if write_header {
        output.write_all(b"SKWP")?;
        output.write_all(&width.to_le_bytes())?;
        output.write_all(&height.to_le_bytes())?;
    }
    output.write_all(frame)?;
    output.flush()?;
    Ok(())
}
