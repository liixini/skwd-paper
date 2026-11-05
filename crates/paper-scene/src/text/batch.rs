use std::collections::HashMap;
use std::sync::Arc;

use crate::tex::Pixels;

const GUTTER: u32 = 1;
const RGBA_CHANNELS: usize = 4;
const ALPHA_COMPONENT: usize = RGBA_CHANNELS - 1;
const MAX_ATLAS_EDGE: u32 = super::MAX_TEXT_EDGE + GUTTER * 2;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Quad {
    pub bounds: [i32; 4],
    pub uv: [u32; 4],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Batch {
    pub canvas: [u32; 2],
    pub pixels: Pixels,
    pub quads: Vec<Quad>,
}

pub(super) struct Glyph {
    pub bounds: [i64; 4],
    pub bitmap: Vec<u8>,
}

impl Batch {
    pub(super) fn pack(canvas: [u32; 2], glyphs: Vec<Glyph>) -> Option<Self> {
        let mut images = Vec::new();
        let mut unique = HashMap::new();
        let mut placements = Vec::new();
        for glyph in glyphs {
            let [x, y, width, height] = glyph.bounds;
            let left = x.max(0);
            let top = y.max(0);
            let right = (x + width).min(i64::from(canvas[0]));
            let bottom = (y + height).min(i64::from(canvas[1]));
            if left >= right || top >= bottom {
                continue;
            }
            let bounds = [left, top, right - left, bottom - top];
            let bitmap = if bounds == glyph.bounds {
                glyph.bitmap
            } else {
                (top - y..bottom - y)
                    .flat_map(|row| {
                        let start = (row * width + left - x) as usize;
                        glyph.bitmap[start..start + bounds[2] as usize].iter().copied()
                    })
                    .collect()
            };
            let [_, _, width, height] = bounds;
            let image = (width as u32, height as u32, Arc::<[u8]>::from(bitmap));
            let index = *unique.entry(image.clone()).or_insert_with(|| {
                images.push(image);
                images.len() - 1
            });
            placements.push((index, bounds.map(|value| value as i32)));
        }
        let mut width = images.iter().map(|(width, _, _)| width + GUTTER * 2).max().unwrap_or(1);
        let area: u64 = images
            .iter()
            .map(|(width, height, _)| {
                u64::from(width + GUTTER * 2) * u64::from(height + GUTTER * 2)
            })
            .sum();
        width = width.max((area as f64).sqrt().ceil() as u32).min(MAX_ATLAS_EDGE);
        let mut offsets = Vec::with_capacity(images.len());
        let mut cursor = [0, 0];
        let mut row_height = 0;
        for (glyph_width, glyph_height, _) in &images {
            let padded = [glyph_width + GUTTER * 2, glyph_height + GUTTER * 2];
            if padded[0] > width {
                return None;
            }
            if cursor[0] + padded[0] > width {
                cursor[0] = 0;
                cursor[1] += row_height;
                row_height = 0;
            }
            offsets.push([cursor[0] + GUTTER, cursor[1] + GUTTER]);
            cursor[0] += padded[0];
            row_height = row_height.max(padded[1]);
        }
        let height = (cursor[1] + row_height).max(1);
        if height > MAX_ATLAS_EDGE {
            return None;
        }
        let mut data = vec![0; width as usize * height as usize * RGBA_CHANNELS];
        for ((glyph_width, glyph_height, bitmap), offset) in images.iter().zip(&offsets) {
            for y in 0..*glyph_height {
                for x in 0..*glyph_width {
                    let at = ((offset[1] + y) * width + offset[0] + x) as usize * RGBA_CHANNELS;
                    data[at..at + ALPHA_COMPONENT].fill(u8::MAX);
                    data[at + ALPHA_COMPONENT] = bitmap[(y * glyph_width + x) as usize];
                }
            }
        }
        let quads = placements
            .into_iter()
            .map(|(index, bounds)| {
                let offset = offsets[index];
                let [_, _, width, height] = bounds;
                Quad { bounds, uv: [offset[0], offset[1], width as u32, height as u32] }
            })
            .collect();
        Some(Self { canvas, pixels: Pixels::rgba(width, height, data), quads })
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
