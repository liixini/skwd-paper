use image::imageops::{self};
use image::{ImageBuffer, Rgba, RgbaImage};
pub use paper_geom::FillMode;
use paper_geom::{fill_crop_rect, fit_scaled_size, span_crop_rect};

type SrcImage<'a> = ImageBuffer<Rgba<u8>, &'a [u8]>;

pub fn apply_fill_mode(
    img_w: u32,
    img_h: u32,
    pixels: &[u8],
    surf_w: u32,
    surf_h: u32,
    mode: FillMode,
) -> (u32, u32, Vec<u8>) {
    if surf_w == 0 || surf_h == 0 {
        return (img_w, img_h, pixels.to_vec());
    }
    if img_w == 0 || img_h == 0 {
        return (surf_w, surf_h, opaque_black(surf_w, surf_h));
    }
    if img_w == surf_w && img_h == surf_h {
        return (img_w, img_h, pixels.to_vec());
    }

    let Some(src) = SrcImage::from_raw(img_w, img_h, pixels) else {
        return (surf_w, surf_h, opaque_black(surf_w, surf_h));
    };

    let out: RgbaImage = match mode {
        FillMode::Stretch => resample_view(&src, 0, 0, img_w, img_h, surf_w, surf_h),
        FillMode::Fill | FillMode::Span => {
            let (cx, cy, cw, ch) = fill_crop_rect(img_w, img_h, surf_w, surf_h);
            resample_view(&src, cx, cy, cw, ch, surf_w, surf_h)
        }
        FillMode::Fit => {
            let (sw, sh) = fit_scaled_size(img_w, img_h, surf_w, surf_h);
            let scaled = resample_view(&src, 0, 0, img_w, img_h, sw, sh);
            let mut canvas = opaque_black_image(surf_w, surf_h);
            let off_x = ((surf_w - sw) / 2) as i64;
            let off_y = ((surf_h - sh) / 2) as i64;
            imageops::overlay(&mut canvas, &scaled, off_x, off_y);
            canvas
        }
        FillMode::Center => center_canvas(&src, img_w, img_h, surf_w, surf_h),
        FillMode::Tile => tile_canvas(&src, img_w, img_h, surf_w, surf_h),
    };

    (out.width(), out.height(), out.into_raw())
}

fn center_canvas(
    src: &SrcImage<'_>,
    img_w: u32,
    img_h: u32,
    surf_w: u32,
    surf_h: u32,
) -> RgbaImage {
    if img_w >= surf_w && img_h >= surf_h {
        let cx = (img_w - surf_w) / 2;
        let cy = (img_h - surf_h) / 2;
        return resample_view(src, cx, cy, surf_w, surf_h, surf_w, surf_h);
    }

    let mut canvas = opaque_black_image(surf_w, surf_h);
    if img_w >= surf_w {
        let cx = (img_w - surf_w) / 2;
        let cropped = resample_view(src, cx, 0, surf_w, img_h, surf_w, img_h);
        let off_y = ((surf_h - img_h) / 2) as i64;
        imageops::overlay(&mut canvas, &cropped, 0, off_y);
    } else if img_h >= surf_h {
        let cy = (img_h - surf_h) / 2;
        let cropped = resample_view(src, 0, cy, img_w, surf_h, img_w, surf_h);
        let off_x = ((surf_w - img_w) / 2) as i64;
        imageops::overlay(&mut canvas, &cropped, off_x, 0);
    } else {
        let off_x = ((surf_w - img_w) / 2) as i64;
        let off_y = ((surf_h - img_h) / 2) as i64;
        imageops::overlay(&mut canvas, src, off_x, off_y);
    }
    canvas
}

fn tile_canvas(src: &SrcImage<'_>, img_w: u32, img_h: u32, surf_w: u32, surf_h: u32) -> RgbaImage {
    let mut canvas = opaque_black_image(surf_w, surf_h);
    let mut y: i64 = 0;
    while y < surf_h as i64 {
        let mut x: i64 = 0;
        while x < surf_w as i64 {
            imageops::overlay(&mut canvas, src, x, y);
            x += img_w as i64;
        }
        y += img_h as i64;
    }
    canvas
}

fn triangle_weights(dst: u32, src: u32) -> Vec<(usize, Vec<f32>)> {
    let ratio = src as f32 / dst as f32;
    let sratio = ratio.max(1.0);
    let support = sratio;
    (0..dst)
        .map(|idx| {
            let center = (idx as f32 + 0.5) * ratio;
            let left = (center - support).floor().max(0.0) as usize;
            let right = ((center + support).ceil() as usize).min(src as usize).max(left + 1);
            let mut ws = Vec::with_capacity(right - left);
            let mut sum = 0.0f32;
            for tap in left..right {
                let t = ((tap as f32 + 0.5) - center) / sratio;
                let wt = (1.0 - t.abs()).max(0.0);
                sum += wt;
                ws.push(wt);
            }
            if sum > 0.0 {
                for wt in &mut ws {
                    *wt /= sum;
                }
            }
            (left, ws)
        })
        .collect()
}

pub fn span_pixels(
    img_w: u32,
    img_h: u32,
    pixels: &[u8],
    bounds: (i32, i32, u32, u32),
    output: (i32, i32, u32, u32),
    dst_w: u32,
    dst_h: u32,
) -> (u32, u32, Vec<u8>) {
    if dst_w == 0 || dst_h == 0 {
        return (1, 1, opaque_black(1, 1));
    }
    if img_w == 0 || img_h == 0 || pixels.len() < (img_w as usize) * (img_h as usize) * 4 {
        return (dst_w, dst_h, opaque_black(dst_w, dst_h));
    }
    let (cx, cy, cw, ch) = span_crop_rect(img_w, img_h, bounds, output);
    let xw = triangle_weights(dst_w, cw);
    let yw = triangle_weights(dst_h, ch);
    let out = resample_loop(pixels, img_w, cx, cy, &xw, &yw, dst_w, dst_h);
    (dst_w, dst_h, out)
}

fn resample_row(
    raw: &[u8],
    src_w: u32,
    cx: u32,
    cy: u32,
    src_y: usize,
    xw: &[(usize, Vec<f32>)],
    out: &mut [f32],
) {
    let stride = src_w as usize * 4;
    let row = &raw[(cy as usize + src_y) * stride..];
    for (dx, (left, ws)) in xw.iter().enumerate() {
        let mut acc = [0.0f32; 4];
        for (tap, wt) in ws.iter().enumerate() {
            let off = (cx as usize + left + tap) * 4;
            acc[0] += row[off] as f32 * wt;
            acc[1] += row[off + 1] as f32 * wt;
            acc[2] += row[off + 2] as f32 * wt;
            acc[3] += row[off + 3] as f32 * wt;
        }
        out[dx * 4..dx * 4 + 4].copy_from_slice(&acc);
    }
}

fn resample_view(
    src: &SrcImage<'_>,
    cx: u32,
    cy: u32,
    cw: u32,
    ch: u32,
    dw: u32,
    dh: u32,
) -> RgbaImage {
    let xw = triangle_weights(dw, cw);
    let yw = triangle_weights(dh, ch);
    let out = resample_loop(src.as_raw(), src.width(), cx, cy, &xw, &yw, dw, dh);
    RgbaImage::from_raw(dw, dh, out).unwrap_or_else(|| opaque_black_image(dw, dh))
}

#[allow(clippy::too_many_arguments)]
fn resample_loop(
    raw: &[u8],
    src_w: u32,
    cx: u32,
    cy: u32,
    xw: &[(usize, Vec<f32>)],
    yw: &[(usize, Vec<f32>)],
    dw: u32,
    dh: u32,
) -> Vec<u8> {
    let row_len = dw as usize * 4;
    let mut cache: std::collections::VecDeque<(usize, Vec<f32>)> =
        std::collections::VecDeque::new();
    let mut spare: Vec<Vec<f32>> = Vec::new();
    let mut acc = vec![0.0f32; row_len];
    let mut out = vec![0u8; row_len * dh as usize];
    for (dy, (top, ws)) in yw.iter().enumerate() {
        while cache.front().is_some_and(|(y, _)| *y < *top) {
            if let Some((_, row)) = cache.pop_front() {
                spare.push(row);
            }
        }
        for sy in cache.back().map_or(*top, |(y, _)| y + 1)..top + ws.len() {
            let mut row = spare.pop().unwrap_or_else(|| vec![0.0f32; row_len]);
            resample_row(raw, src_w, cx, cy, sy, xw, &mut row);
            cache.push_back((sy, row));
        }
        acc.fill(0.0);
        let y0 = cache.front().map_or(*top, |(y, _)| *y);
        for (tap, wt) in ws.iter().enumerate() {
            let Some((_, row)) = cache.get(top + tap - y0) else {
                continue;
            };
            for (dst, src) in acc.iter_mut().zip(row.iter()) {
                *dst += src * wt;
            }
        }
        for (dst, val) in out[dy * row_len..(dy + 1) * row_len].iter_mut().zip(acc.iter()) {
            *dst = (val + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn opaque_black(w: u32, h: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    for px in buf.chunks_exact_mut(4) {
        px[3] = 255;
    }
    buf
}

fn opaque_black_image(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_raw(w, h, opaque_black(w, h)).unwrap()
}

mod tests;
