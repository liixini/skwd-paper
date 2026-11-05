use std::io;
use std::path::Path;

use paper_control::DrawingManifest;
use paper_geom::{FillMode, fill_crop_rect, fit_scaled_size};

const OUTLINE_CHANNEL: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub image: [u32; 2],
    pub buffer: [u32; 2],
    pub mode: FillMode,
}

pub struct Outline {
    pub width: usize,
    pub height: usize,
    pub mask: Vec<u8>,
}

impl Outline {
    pub fn new(width: usize, height: usize, mask: Vec<u8>) -> Option<Self> {
        (width > 0 && height > 0 && mask.len() == width * height).then_some(Self {
            width,
            height,
            mask,
        })
    }

    pub fn load(manifest: &Path, source: &Path) -> io::Result<Self> {
        let (_, texture, _) = DrawingManifest::open(manifest, source)?;
        let texture = image::open(&texture).map_err(io::Error::other)?.into_rgb8();
        let (width, height) = texture.dimensions();
        let mask = texture.pixels().map(|pixel| pixel[OUTLINE_CHANNEL]).collect();
        Self::new(width as usize, height as usize, mask)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty outline texture"))
    }

    pub fn crop(&self, frame: Frame, origin: [u32; 2], size: [usize; 2]) -> Option<Vec<u8>> {
        let map = Mapping::of(frame)?;
        let [width, height] = size;
        let mut cut = vec![0; width * height];
        for row in 0..height {
            for column in 0..width {
                let at =
                    [origin[0] as f32 + column as f32 + 0.5, origin[1] as f32 + row as f32 + 0.5];
                if let Some([u, v]) = map.source(at) {
                    cut[row * width + column] =
                        self.sample([u * self.width as f32, v * self.height as f32]);
                }
            }
        }
        Some(cut)
    }

    fn sample(&self, at: [f32; 2]) -> u8 {
        let x = (at[0] - 0.5).clamp(0.0, (self.width - 1) as f32);
        let y = (at[1] - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (left, top) = (x.floor() as usize, y.floor() as usize);
        let (right, bottom) = ((left + 1).min(self.width - 1), (top + 1).min(self.height - 1));
        let (dx, dy) = (x - left as f32, y - top as f32);
        let at = |column: usize, row: usize| f32::from(self.mask[row * self.width + column]);
        let upper = at(left, top) * (1.0 - dx) + at(right, top) * dx;
        let lower = at(left, bottom) * (1.0 - dx) + at(right, bottom) * dx;
        (upper * (1.0 - dy) + lower * dy).round() as u8
    }
}

struct Mapping {
    shift: [f32; 2],
    span: [f32; 2],
    window: [f32; 4],
    repeat: Option<[f32; 2]>,
}

impl Mapping {
    fn of(frame: Frame) -> Option<Self> {
        let [image_width, image_height] = frame.image;
        let [width, height] = frame.buffer;
        if image_width == 0 || image_height == 0 || width == 0 || height == 0 {
            return None;
        }
        let image = [image_width as f32, image_height as f32];
        let buffer = [width as f32, height as f32];
        let whole = [0.0, 0.0, buffer[0], buffer[1]];
        let plain = Self { shift: [0.0; 2], span: [1.0; 2], window: whole, repeat: None };
        match frame.mode {
            FillMode::Stretch => Some(plain),
            FillMode::Fill => {
                let (left, top, crop_width, crop_height) =
                    fill_crop_rect(image_width, image_height, width, height);
                Some(Self {
                    shift: [left as f32 / image[0], top as f32 / image[1]],
                    span: [crop_width as f32 / image[0], crop_height as f32 / image[1]],
                    ..plain
                })
            }
            FillMode::Fit => {
                let (fit_width, fit_height) =
                    fit_scaled_size(image_width, image_height, width, height);
                let left = ((width - fit_width) / 2) as f32;
                let top = ((height - fit_height) / 2) as f32;
                Some(Self { window: [left, top, fit_width as f32, fit_height as f32], ..plain })
            }
            FillMode::Center => {
                let offset = |side: u32, length: u32| {
                    if side >= length {
                        ((side - length) / 2) as f32
                    } else {
                        -(((length - side) / 2) as f32)
                    }
                };
                let shift = [offset(image_width, width), offset(image_height, height)];
                Some(Self {
                    shift: [shift[0] / image[0], shift[1] / image[1]],
                    span: [buffer[0] / image[0], buffer[1] / image[1]],
                    ..plain
                })
            }
            FillMode::Tile => Some(Self { repeat: Some(image), ..plain }),
            FillMode::Span => None,
        }
    }

    fn source(&self, at: [f32; 2]) -> Option<[f32; 2]> {
        if let Some([width, height]) = self.repeat {
            return Some([at[0].rem_euclid(width) / width, at[1].rem_euclid(height) / height]);
        }
        let [left, top, width, height] = self.window;
        let local = [(at[0] - left) / width, (at[1] - top) / height];
        let source =
            [self.shift[0] + local[0] * self.span[0], self.shift[1] + local[1] * self.span[1]];
        local.iter().chain(&source).all(|value| (0.0..1.0).contains(value)).then_some(source)
    }
}

#[cfg(test)]
#[path = "outline_tests.rs"]
mod tests;
