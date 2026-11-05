use std::io::{self, Write};
use std::path::Path;

const CHANNELS: usize = 4;
const RGB_CHANNELS: usize = 3;
const WRITE_BUFFER: usize = 64 * 1024;

pub fn write_rgba(path: &Path, width: u32, height: u32, pixels: &[u8]) -> io::Result<()> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(CHANNELS))
        .ok_or_else(|| io::Error::other("frame too large"))?;
    if pixels.len() < expected {
        return Err(io::Error::other("freeze-frame RGBA payload is truncated"));
    }
    let parent =
        path.parent().ok_or_else(|| io::Error::other("freeze-frame path has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = io::BufWriter::with_capacity(WRITE_BUFFER, temporary.as_file_mut());
    write!(writer, "P6\n{width} {height}\n255\n")?;
    for pixel in pixels[..expected].chunks_exact(CHANNELS) {
        writer.write_all(&pixel[..RGB_CHANNELS])?;
    }
    writer.flush()?;
    drop(writer);
    temporary.as_file_mut().sync_all()?;
    temporary.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn write_error(path: &Path, message: &str) -> io::Result<()> {
    let parent =
        path.parent().ok_or_else(|| io::Error::other("freeze error path has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(message.as_bytes())?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}
