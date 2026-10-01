use super::{SwDecoder, ff};
use anyhow::{Context, Result, anyhow};
use paper_geom::FillMode;

const FRAME_LIMIT: usize = 240;

pub(crate) fn rgba_near(path: &str, seconds: f64, bounds: (u32, u32)) -> Result<image::RgbaImage> {
    let frame = frame_near(path, seconds)?;
    let source = (frame.width(), frame.height());
    let (width, height) = super::still::texture_size(source, &[bounds], FillMode::Fit);
    let mut rgba = ff::frame::Video::empty();
    ff::software::scaling::Context::get(
        frame.format(),
        source.0,
        source.1,
        ff::format::Pixel::RGBA,
        width,
        height,
        ff::software::scaling::Flags::BICUBIC,
    )
    .context("thumbnail scaler")?
    .run(&frame, &mut rgba)
    .context("thumbnail conversion")?;
    let row = width as usize * 4;
    let pixels = rgba
        .data(0)
        .chunks(rgba.stride(0))
        .take(height as usize)
        .flat_map(|line| &line[..row])
        .copied()
        .collect();
    image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| anyhow!("thumbnail frame size mismatch"))
}

fn frame_near(path: &str, seconds: f64) -> Result<ff::frame::Video> {
    let mut decoder = SwDecoder::open(path)?;
    let duration = decoder.ictx.duration();
    let target = if decoder.still || duration <= 0 {
        0.0
    } else {
        seconds.min(duration as f64 / f64::from(ff::ffi::AV_TIME_BASE) / 2.0)
    };
    let timestamp = (target * f64::from(ff::ffi::AV_TIME_BASE)) as i64;
    if target > 0.0 && decoder.ictx.seek(timestamp, ..=timestamp).is_ok() {
        decoder.decoder.flush();
    }
    let (mut frame, mut pts) = decoder.next_raw()?;
    for _ in 0..FRAME_LIMIT {
        if pts + 0.01 >= target {
            break;
        }
        (frame, pts) = decoder.next_raw()?;
    }
    Ok(frame)
}

#[cfg(test)]
mod tests;
