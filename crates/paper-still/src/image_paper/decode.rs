use anyhow::{Context, Result};

const BLUR_DECODE_CAP: u32 = 1600;

pub(super) fn decode_image(file_path: &str, blur: f32, dim: u32) -> Result<(u32, u32, Vec<u8>)> {
    let mut image = image::ImageReader::open(file_path)
        .with_context(|| format!("opening image: {file_path}"))?
        .with_guessed_format()
        .with_context(|| format!("sniffing image format: {file_path}"))?
        .decode()
        .with_context(|| format!("decoding image: {file_path}"))?;
    if blur > 0.0 {
        let (width, height) = (image.width(), image.height());
        if width.max(height) > BLUR_DECODE_CAP {
            let scale = BLUR_DECODE_CAP as f32 / width.max(height) as f32;
            image = image.resize(
                ((width as f32 * scale) as u32).max(1),
                ((height as f32 * scale) as u32).max(1),
                image::imageops::FilterType::Triangle,
            );
        }
    }
    let mut rgba = if blur > 0.0 {
        image::imageops::blur(&image.into_rgba8(), blur)
    } else {
        image.into_rgba8()
    };
    if dim > 0 {
        let retained_percent = 100u32.saturating_sub(dim);
        for pixel in rgba.pixels_mut() {
            pixel[0] = (u32::from(pixel[0]) * retained_percent / 100) as u8;
            pixel[1] = (u32::from(pixel[1]) * retained_percent / 100) as u8;
            pixel[2] = (u32::from(pixel[2]) * retained_percent / 100) as u8;
        }
    }
    let (width, height) = rgba.dimensions();
    Ok((width, height, rgba.into_raw()))
}
