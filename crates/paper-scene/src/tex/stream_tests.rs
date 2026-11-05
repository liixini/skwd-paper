use super::{Cell, pack};
use crate::tests::{push_i32, push_nul_str};

const SHEET: [i32; 2] = [4, 2];

fn pixel(sheet: u8, x: u8, y: u8) -> [u8; 4] {
    [sheet, x, y, 255]
}

pub(crate) fn sprite(frames: &[[f32; 4]], compress: bool) -> Vec<u8> {
    sprite_levels(frames, compress, 1)
}

fn sprite_levels(frames: &[[f32; 4]], compress: bool, levels: i32) -> Vec<u8> {
    let mut out = Vec::new();
    push_nul_str(&mut out, "TEXV0005");
    push_nul_str(&mut out, "TEXI0001");
    push_i32(&mut out, 0);
    push_i32(&mut out, super::FLAG_IS_GIF as i32);
    for value in [SHEET, SHEET].concat() {
        push_i32(&mut out, value);
    }
    push_i32(&mut out, 0);
    push_nul_str(&mut out, "TEXB0003");
    push_i32(&mut out, 2);
    push_i32(&mut out, -1);
    for sheet in 0..2_u8 {
        let rgba: Vec<u8> =
            (0..2_u8).flat_map(|y| (0..4_u8).flat_map(move |x| pixel(sheet, x, y))).collect();
        let body = if compress { lz4_flex::block::compress(&rgba) } else { rgba.clone() };
        push_i32(&mut out, levels);
        for _ in 0..levels {
            for value in [SHEET[0], SHEET[1], i32::from(compress), rgba.len() as i32] {
                push_i32(&mut out, value);
            }
            push_i32(&mut out, body.len() as i32);
            out.extend_from_slice(&body);
        }
    }
    push_nul_str(&mut out, "TEXS0003");
    push_i32(&mut out, frames.len() as i32);
    push_i32(&mut out, 2);
    push_i32(&mut out, 2);
    for [image, x, width, height] in frames {
        push_i32(&mut out, *image as i32);
        for value in [0.04, *x, 0.0, *width, 0.0, 0.0, *height] {
            out.extend_from_slice(&f32::to_le_bytes(value));
        }
    }
    out
}

pub(crate) const FRAMES: [[f32; 4]; 4] =
    [[0.0, 0.0, 2.0, 2.0], [0.0, 2.0, 2.0, 2.0], [1.0, 0.0, 2.0, 2.0], [1.0, 2.0, 2.0, 2.0]];

fn unpacked(cell: &Cell) -> Vec<u8> {
    let mut out = Vec::new();
    cell.unpack(&mut out).unwrap();
    out
}

#[test]
fn cells_cut_per_frame() {
    for compress in [true, false] {
        let sprite = pack(&sprite(&FRAMES, compress), 0).unwrap().unwrap();
        assert_eq!(sprite.cells.len(), FRAMES.len());
        for (cell, [image, x, _, _]) in sprite.cells.iter().zip(FRAMES) {
            assert_eq!((cell.width, cell.height), (2, 2));
            let expected: Vec<u8> = (0..2_u8)
                .flat_map(|y| (0..2_u8).flat_map(move |dx| pixel(image as u8, x as u8 + dx, y)))
                .collect();
            assert_eq!(unpacked(cell), expected);
        }
    }
}

#[test]
fn threshold_keeps_small() {
    let bytes = sprite(&FRAMES, true);
    assert!(pack(&bytes, 2 * 4 * 2 * 4).unwrap().is_none());
    assert!(pack(&bytes, 2 * 4 * 2 * 4 - 1).unwrap().is_some());
    let still = crate::tests::build_tex("TEXB0003", 0, 0, &[1; 64], true, None);
    assert!(pack(&still, 0).unwrap().is_none());
}

#[test]
fn rotated_frames_skipped() {
    let rotated = sprite(&[[0.0, 0.0, 0.0, 2.0]], true);
    assert!(pack(&rotated, 0).unwrap().is_none());
}

#[test]
fn frame_outside_sheet() {
    assert!(pack(&sprite(&[[0.0, 3.0, 2.0, 2.0]], true), 0).is_err());
    assert!(pack(&sprite(&[[2.0, 0.0, 2.0, 2.0]], true), 0).is_err());
}

#[test]
fn irregular_frames_skipped() {
    for frames in [&[[0.0, 0.5, 2.0, 2.0]][..], &[[0.0, 0.0, 2.0, 2.0], [0.0, 2.0, 2.0, 1.0]]] {
        assert!(pack(&sprite(frames, true), 0).unwrap().is_none(), "{frames:?}");
    }
    assert!(pack(&sprite_levels(&FRAMES, true, 2), 0).unwrap().is_none());
}

#[test]
fn distant_frame_rejected() {
    assert!(pack(&sprite(&[[0.0, 1e20, 2.0, 2.0]], true), 0).is_err());
}
