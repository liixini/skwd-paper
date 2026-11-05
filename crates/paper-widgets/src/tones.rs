use paper_control::Palette;

use crate::canvas::{Ink, WHITE};

const TEXT_FLOOR: f32 = 4.5;
const CHANNELS: usize = 4;
const GROUND_STEP: usize = 4;
const ACCENT_FLOOR: f32 = 3.0;
const MUTED_SHARE: f32 = 184.0 / 255.0;
const MIX_STEP: f32 = 0.02;
const GREY_POINT: f32 = 0.179;
const BLACK: [u8; 3] = [0; 3];
const FLARE: f32 = 0.05;
const LINEAR_KNEE: f32 = 0.039_28;
const LINEAR_SLOPE: f32 = 12.92;
const GAMMA_OFFSET: f32 = 0.055;
const GAMMA_SCALE: f32 = 1.055;
const GAMMA: f32 = 2.4;
const WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tones {
    pub text: Ink,
    pub accent: Ink,
    pub muted: Ink,
    pub shade: Ink,
}

impl Tones {
    pub fn over(palette: &Palette, ground: f32) -> Self {
        let keeps = contrast(luminance(palette.text), ground);
        let swaps = contrast(luminance(palette.shade), ground);
        let (text, shade) = if keeps >= TEXT_FLOOR || keeps >= swaps {
            (palette.text, palette.shade)
        } else {
            (palette.shade, palette.text)
        };
        let muted = mix(shade, text, MUTED_SHARE);
        Self {
            text: Ink::solid(text),
            accent: Ink::solid(legible(palette.accent, ground)),
            muted: Ink::solid(muted),
            shade: Ink::solid(shade),
        }
    }
}

pub fn ground(rgba: &[u8], width: usize, area: [usize; 4]) -> f32 {
    let [left, top, columns, rows] = area;
    let height = rgba.len() / (width * CHANNELS);
    let mut total = 0.0;
    let mut count = 0.0;
    for y in (top..(top + rows).min(height)).step_by(GROUND_STEP) {
        for x in (left..(left + columns).min(width)).step_by(GROUND_STEP) {
            let at = (y * width + x) * CHANNELS;
            total += luminance([rgba[at], rgba[at + 1], rgba[at + 2]]);
            count += 1.0;
        }
    }
    if count == 0.0 { 0.0 } else { total / count }
}

pub fn luminance(rgb: [u8; 3]) -> f32 {
    rgb.iter()
        .zip(WEIGHTS)
        .map(|(&level, weight)| {
            let level = f32::from(level) / f32::from(u8::MAX);
            let linear = if level <= LINEAR_KNEE {
                level / LINEAR_SLOPE
            } else {
                ((level + GAMMA_OFFSET) / GAMMA_SCALE).powf(GAMMA)
            };
            linear * weight
        })
        .sum()
}

pub fn contrast(one: f32, other: f32) -> f32 {
    (one.max(other) + FLARE) / (one.min(other) + FLARE)
}

fn mix(from: [u8; 3], to: [u8; 3], share: f32) -> [u8; 3] {
    let mut mixed = from;
    for (level, &target) in mixed.iter_mut().zip(&to) {
        *level = (f32::from(*level) * (1.0 - share) + f32::from(target) * share).round() as u8;
    }
    mixed
}

fn legible(rgb: [u8; 3], ground: f32) -> [u8; 3] {
    let target = if ground < GREY_POINT { WHITE } else { BLACK };
    let mut share = 0.0;
    let mut current = rgb;
    while contrast(luminance(current), ground) < ACCENT_FLOOR && share < 1.0 {
        share += MIX_STEP;
        current = mix(rgb, target, share.min(1.0));
    }
    current
}

#[cfg(test)]
#[path = "tones_tests.rs"]
mod tests;
