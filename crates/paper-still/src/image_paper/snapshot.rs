use super::mirror::Shown;
use super::model::App;
use crate::fill_mode::FillMode;
use anyhow::{Context, Result, ensure};
use image::RgbaImage;
use paper_runtime::mirror::{AR24, XR24};
use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::path::Path;
use wayland_client::protocol::wl_shm::Format;

const CHANNELS: usize = 4;
const ERROR_SUFFIX: &str = ".error";

impl Shown {
    fn pixels(&self) -> Result<RgbaImage> {
        self.region(self.source)
    }

    pub(super) fn region(&self, area: [u32; 4]) -> Result<RgbaImage> {
        let [x, y, width, height] = area;
        ensure!(width > 0 && height > 0, "snapshot crop is empty");
        ensure!(
            x.checked_add(width).is_some_and(|end| end <= self.layout.width)
                && y.checked_add(height).is_some_and(|end| end <= self.layout.height),
            "snapshot crop outside buffer"
        );
        let mut image = RgbaImage::new(width, height);
        let file = File::from(self.memory.as_fd().try_clone_to_owned()?);
        let plane = self.layout.planes[0];
        ensure!(plane.stride as usize >= self.layout.width as usize * CHANNELS, "snapshot stride");
        for (row, pixels) in image.as_mut().chunks_exact_mut(width as usize * CHANNELS).enumerate()
        {
            let offset = u64::from(plane.offset)
                + (u64::from(y) + row as u64) * u64::from(plane.stride)
                + u64::from(x) * CHANNELS as u64;
            file.read_exact_at(pixels, offset)?;
        }
        if matches!(self.layout.fourcc, AR24 | XR24) {
            for pixel in image.as_mut().chunks_exact_mut(CHANNELS) {
                pixel.swap(0, 2);
            }
        } else {
            ensure!(self.layout.fourcc == u32::from(Format::Abgr8888), "snapshot pixel format");
        }
        Ok(image)
    }
}

impl App {
    pub(super) fn snapshot(&mut self, request: &paper_control::FrameSnapshot) {
        let path = Path::new(&request.path);
        let result = self.snapshot_view(&request.output).and_then(|pixels| {
            paper_runtime::snapshot::write_rgba(
                path,
                pixels.width(),
                pixels.height(),
                pixels.as_raw(),
            )
            .context("write still snapshot")
        });
        if let Err(error) = result {
            let _ = paper_runtime::snapshot::write_error(
                Path::new(&format!("{}{ERROR_SUFFIX}", request.path)),
                &error.to_string(),
            );
            tracing::warn!(output = %request.output, %error, "still snapshot failed");
        }
    }

    fn snapshot_view(&mut self, output: &str) -> Result<RgbaImage> {
        let index = self
            .surfaces
            .iter()
            .position(|surface| surface.output_name == output)
            .context("snapshot output unavailable")?;
        let surface = &self.surfaces[index];
        let size = self.surface_physical(surface);
        ensure!(size.0 > 0 && size.1 > 0, "snapshot output is not configured");
        let image = if let Some(shown) = &surface.shown {
            shown.pixels()?
        } else if self.fill_mode == FillMode::Span {
            let rect = (surface.pos.0, surface.pos.1, surface.width, surface.height);
            let bounds = self.span_bounds().context("snapshot span bounds")?;
            ensure!(self.ensure_raw_pixels(), "snapshot pixels unavailable");
            let (width, height, pixels) = crate::fill_mode::span_pixels(
                self.raw_w,
                self.raw_h,
                &self.raw_pixels,
                bounds,
                rect,
                size.0,
                size.1,
            );
            RgbaImage::from_raw(width, height, pixels).context("snapshot span dimensions")?
        } else {
            self.ensure_buffer_for(size.0, size.1)?;
            let buffer = self.buffers.get(&size).context("snapshot buffer unavailable")?;
            let (memory, layout) = buffer.share()?;
            Shown { memory, source: super::mirror::full(layout.width, layout.height), layout }
                .pixels()?
        };
        if image.dimensions() == size {
            return Ok(image);
        }
        Ok(image::imageops::resize(&image, size.0, size.1, image::imageops::FilterType::Triangle))
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
