use super::{RenderFrame, ff};
use anyhow::{Context, Result, anyhow};
use paper_geom::FillMode;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) struct StillDecoder {
    path: String,
    pub(crate) source: (u32, u32),
    outputs: Vec<(u32, u32)>,
    mode: FillMode,
    served: bool,
    pub(crate) abort: Option<Arc<AtomicBool>>,
}

pub(crate) struct StillPixels {
    pub(crate) image: image::RgbaImage,
    pub(crate) source: (u32, u32),
}

impl StillDecoder {
    pub(crate) fn open(path: &str, outputs: &[(u32, u32)], mode: FillMode) -> Result<Self> {
        let source = image::image_dimensions(path).context("read still dimensions")?;
        Ok(Self {
            path: path.to_string(),
            source,
            outputs: outputs.to_vec(),
            mode,
            served: false,
            abort: None,
        })
    }

    pub(crate) fn next(&mut self) -> Result<(RenderFrame, f64)> {
        if self.served || self.abort.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(anyhow!("still source exhausted"));
        }
        self.served = true;
        let image = image::ImageReader::open(&self.path)?.with_guessed_format()?.decode()?;
        self.source = (image.width(), image.height());
        let size = texture_size(self.source, &self.outputs, self.mode);
        let image = if size == self.source {
            image
        } else {
            image.resize_exact(size.0, size.1, image::imageops::FilterType::Triangle)
        }
        .into_rgba8();
        if self.abort.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(anyhow!("still decode cancelled"));
        }
        let frame = nv12(&image)?;
        tracing::info!(path = %self.path, source_width = self.source.0, source_height = self.source.1, texture_width = size.0, texture_height = size.1, "prepared still image once");
        Ok((
            RenderFrame {
                frame,
                mapped: None,
                still: Some(Arc::new(StillPixels { image, source: self.source })),
            },
            0.0,
        ))
    }
}

pub(crate) fn texture_size(
    source: (u32, u32),
    outputs: &[(u32, u32)],
    mode: FillMode,
) -> (u32, u32) {
    let (sw, sh) = source;
    if sw == 0 || sh == 0 || outputs.is_empty() || matches!(mode, FillMode::Center | FillMode::Tile)
    {
        return source;
    }
    if mode == FillMode::Stretch {
        return (
            sw.min(outputs.iter().map(|d| d.0).max().unwrap().max(1)),
            sh.min(outputs.iter().map(|d| d.1).max().unwrap().max(1)),
        );
    }
    let scale = outputs
        .iter()
        .map(|&(w, h)| {
            let x = f64::from(w.max(1)) / f64::from(sw);
            let y = f64::from(h.max(1)) / f64::from(sh);
            if mode == FillMode::Fit { x.min(y) } else { x.max(y) }
        })
        .fold(0.0_f64, f64::max)
        .min(1.0);
    (
        ((f64::from(sw) * scale).ceil() as u32).max(1).min(sw),
        ((f64::from(sh) * scale).ceil() as u32).max(1).min(sh),
    )
}

fn nv12(image: &image::RgbaImage) -> Result<ff::frame::Video> {
    let (w, h) = image.dimensions();
    let mut rgba = ff::frame::Video::new(ff::format::Pixel::RGBA, w, h);
    let stride = rgba.stride(0);
    for (src, dst) in
        image.as_raw().chunks_exact(w as usize * 4).zip(rgba.data_mut(0).chunks_mut(stride))
    {
        dst[..src.len()].copy_from_slice(src);
    }
    let mut scaler = ff::software::scaling::Context::get(
        ff::format::Pixel::RGBA,
        w,
        h,
        ff::format::Pixel::NV12,
        w,
        h,
        ff::software::scaling::Flags::BILINEAR,
    )?;
    let mut frame = ff::frame::Video::empty();
    scaler.run(&rgba, &mut frame)?;
    Ok(frame)
}

#[cfg(test)]
mod tests;
