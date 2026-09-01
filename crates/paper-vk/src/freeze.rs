use crate::ctl;
use anyhow::{Context, Result, anyhow};
use ffmpeg_the_third as ff;
use std::io::Write;
use std::path::Path;

pub(crate) fn write_requested(ctl: &mut ctl::Ctl, frame: &ff::frame::Video) -> Result<bool> {
    let Some(path) = ctl.take_freeze() else {
        return Ok(false);
    };
    if let Err(error) = write_frame(Path::new(&path), frame) {
        let message = format!("{error:#}");
        let _ = write_error(Path::new(&format!("{path}.error")), &message);
        ctl.cancel_freeze();
        tracing::error!(path, error = %message, "skwd-wall-vk: freeze frame failed");
        return Ok(false);
    }
    tracing::info!(path, "skwd-wall-vk: freeze frame ready");
    Ok(true)
}

pub(crate) fn write_last_requested(
    ctl: &mut ctl::Ctl,
    frame: Option<&ff::frame::Video>,
) -> Result<bool> {
    if !ctl.freeze_pending() {
        return Ok(false);
    }
    let frame = frame.ok_or_else(|| anyhow!("freeze requested before a frame was presented"))?;
    write_requested(ctl, frame)
}

pub(crate) fn write_rgba_requested(
    ctl: &mut ctl::Ctl,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<bool> {
    let Some(path) = ctl.take_freeze() else {
        return Ok(false);
    };
    if let Err(error) = write_rgba_ppm(Path::new(&path), width, height, pixels) {
        let message = format!("{error:#}");
        let _ = write_error(Path::new(&format!("{path}.error")), &message);
        ctl.cancel_freeze();
        tracing::error!(path, error = %message, "skwd-wall-vk: scene freeze frame failed");
        return Ok(false);
    }
    tracing::info!(path, "skwd-wall-vk: scene freeze frame ready");
    Ok(true)
}

pub(crate) fn fail_requested(ctl: &mut ctl::Ctl, error: &anyhow::Error) {
    let Some(path) = ctl.take_freeze() else {
        return;
    };
    let message = format!("{error:#}");
    let _ = write_error(Path::new(&format!("{path}.error")), &message);
    ctl.cancel_freeze();
    tracing::error!(path, error = %message, "skwd-wall-vk: scene freeze frame failed");
}

fn write_frame(path: &Path, frame: &ff::frame::Video) -> Result<()> {
    let transferred;
    let source = if matches!(frame.format(), ff::format::Pixel::VULKAN | ff::format::Pixel::VAAPI) {
        transferred = transfer_hardware(frame)?;
        &transferred
    } else {
        frame
    };
    let mut rgb = ff::frame::Video::empty();
    let mut scaler = ff::software::scaling::Context::get(
        source.format(),
        source.width(),
        source.height(),
        ff::format::Pixel::RGB24,
        source.width(),
        source.height(),
        ff::software::scaling::Flags::BILINEAR,
    )
    .context("freeze-frame scaler")?;
    scaler.run(source, &mut rgb).context("freeze-frame conversion")?;
    write_rgb_ppm(path, rgb.width(), rgb.height(), rgb.data(0), rgb.stride(0))
}

fn transfer_hardware(frame: &ff::frame::Video) -> Result<ff::frame::Video> {
    let context = unsafe {
        let reference = (*frame.as_ptr()).hw_frames_ctx;
        if reference.is_null() || (*reference).data.is_null() {
            return Err(anyhow!("freeze-frame hardware context is unavailable"));
        }
        &*(*reference).data.cast::<ff::ffi::AVHWFramesContext>()
    };
    let format = ff::format::Pixel::from(context.sw_format);
    let mut transferred = ff::frame::Video::new(format, frame.width(), frame.height());
    let result =
        unsafe { ff::ffi::av_hwframe_transfer_data(transferred.as_mut_ptr(), frame.as_ptr(), 0) };
    if result < 0 {
        return Err(anyhow!(
            "freeze-frame transfer failed ({result}, {format:?}, {}x{}, context {}x{})",
            frame.width(),
            frame.height(),
            context.width,
            context.height
        ));
    }
    Ok(transferred)
}

fn write_rgb_ppm(path: &Path, width: u32, height: u32, pixels: &[u8], stride: usize) -> Result<()> {
    let row = usize::try_from(width)?.checked_mul(3).ok_or_else(|| anyhow!("frame too wide"))?;
    let rows = usize::try_from(height)?;
    if stride < row || pixels.len() < stride.saturating_mul(rows) {
        return Err(anyhow!("freeze-frame RGB payload is truncated"));
    }
    let parent = path.parent().ok_or_else(|| anyhow!("freeze-frame path has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).context("create freeze frame")?;
    write!(temporary, "P6\n{width} {height}\n255\n")?;
    for y in 0..rows {
        temporary.write_all(&pixels[y * stride..y * stride + row])?;
    }
    temporary.as_file_mut().sync_all()?;
    temporary.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

fn write_rgba_ppm(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
    let pixel_count = usize::try_from(width)?
        .checked_mul(usize::try_from(height)?)
        .ok_or_else(|| anyhow!("frame too large"))?;
    let expected = pixel_count.checked_mul(4).ok_or_else(|| anyhow!("frame too large"))?;
    if pixels.len() < expected {
        return Err(anyhow!("freeze-frame RGBA payload is truncated"));
    }
    let parent = path.parent().ok_or_else(|| anyhow!("freeze-frame path has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).context("create freeze frame")?;
    write!(temporary, "P6\n{width} {height}\n255\n")?;
    for pixel in pixels[..expected].chunks_exact(4) {
        temporary.write_all(&pixel[..3])?;
    }
    temporary.as_file_mut().sync_all()?;
    temporary.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

fn write_error(path: &Path, message: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("freeze error path has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(message.as_bytes())?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppm_atomic_row_padding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("frame.ppm");
        let pixels = [255, 0, 0, 0, 255, 0, 9, 9, 9];
        write_rgb_ppm(&path, 2, 1, &pixels, 9).unwrap();
        let image = image::open(&path).unwrap().into_rgb8();
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(image.into_raw(), [255, 0, 0, 0, 255, 0]);
        assert!(write_rgb_ppm(&path, 2, 1, &pixels, 9).is_err());
    }
    #[test]
    fn ppm_from_rgba() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("frame.ppm");
        let pixels = [255, 0, 0, 9, 0, 255, 0, 8];
        write_rgba_ppm(&path, 2, 1, &pixels).unwrap();
        let image = image::open(&path).unwrap().into_rgb8();
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(image.into_raw(), [255, 0, 0, 0, 255, 0]);
    }
}
