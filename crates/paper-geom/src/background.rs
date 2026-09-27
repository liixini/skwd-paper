pub fn blurred_background(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    bgra: bool,
) -> Vec<u8> {
    let (cx, cy, cw, ch) = crate::fill_crop_rect(source_width, source_height, width, height);
    let small_w = width.clamp(1, 96) as usize;
    let small_h =
        ((u64::from(height) * small_w as u64 / u64::from(width.max(1))) as usize).clamp(1, 96);
    let mut small = vec![[0.0f32; 3]; small_w * small_h];
    for y in 0..small_h {
        for x in 0..small_w {
            let sx = cx + ((x as u64 * u64::from(cw)) / small_w as u64) as u32;
            let sy = cy + ((y as u64 * u64::from(ch)) / small_h as u64) as u32;
            let offset = ((sy * source_width + sx) * 4) as usize;
            for (channel, value) in small[y * small_w + x].iter_mut().enumerate() {
                let index = if bgra { 2 - channel } else { channel };
                *value = f32::from(source[offset + index]);
            }
        }
    }
    for horizontal in [true, false] {
        let original = small.clone();
        for y in 0..small_h {
            for x in 0..small_w {
                let mut sum = [0.0f32; 3];
                for delta in -6isize..=6 {
                    let sx = if horizontal {
                        (x as isize + delta).clamp(0, small_w as isize - 1) as usize
                    } else {
                        x
                    };
                    let sy = if horizontal {
                        y
                    } else {
                        (y as isize + delta).clamp(0, small_h as isize - 1) as usize
                    };
                    for channel in 0..3 {
                        sum[channel] += original[sy * small_w + sx][channel];
                    }
                }
                small[y * small_w + x] = sum.map(|value| value / 13.0);
            }
        }
    }
    let mut output = vec![255; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let fy = (y as f32 + 0.5) * small_h as f32 / height as f32 - 0.5;
        let y0 = fy.max(0.0) as usize;
        let y1 = (y0 + 1).min(small_h - 1);
        let wy = fy.max(0.0).fract();
        for x in 0..width as usize {
            let fx = (x as f32 + 0.5) * small_w as f32 / width as f32 - 0.5;
            let x0 = fx.max(0.0) as usize;
            let x1 = (x0 + 1).min(small_w - 1);
            let wx = fx.max(0.0).fract();
            for c in 0..3 {
                let top =
                    small[y0 * small_w + x0][c] * (1.0 - wx) + small[y0 * small_w + x1][c] * wx;
                let bottom =
                    small[y1 * small_w + x0][c] * (1.0 - wx) + small[y1 * small_w + x1][c] * wx;
                output[(y * width as usize + x) * 4 + c] =
                    (top * (1.0 - wy) + bottom * wy).round() as u8;
            }
        }
    }
    output
}
