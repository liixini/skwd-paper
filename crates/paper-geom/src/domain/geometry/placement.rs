pub fn fill_crop_rect(img_w: u32, img_h: u32, surf_w: u32, surf_h: u32) -> (u32, u32, u32, u32) {
    let img_aspect = img_w as f32 / img_h as f32;
    let surf_aspect = surf_w as f32 / surf_h as f32;
    if img_aspect > surf_aspect {
        let cw = ((img_h as f32) * surf_aspect).round().max(1.0) as u32;
        let cw = cw.min(img_w);
        ((img_w - cw) / 2, 0, cw, img_h)
    } else {
        let ch = ((img_w as f32) / surf_aspect).round().max(1.0) as u32;
        let ch = ch.min(img_h);
        (0, (img_h - ch) / 2, img_w, ch)
    }
}

pub fn fit_scaled_size(img_w: u32, img_h: u32, surf_w: u32, surf_h: u32) -> (u32, u32) {
    let img_aspect = img_w as f32 / img_h as f32;
    let surf_aspect = surf_w as f32 / surf_h as f32;
    if img_aspect > surf_aspect {
        (surf_w, ((surf_w as f32) / img_aspect).round().max(1.0) as u32)
    } else {
        (((surf_h as f32) * img_aspect).round().max(1.0) as u32, surf_h)
    }
}

pub fn cover_uv(src_w: u32, src_h: u32, surf_w: u32, surf_h: u32) -> [f32; 4] {
    let (x, y, width, height) = fill_crop_rect(src_w, src_h, surf_w, surf_h);
    let src_w = src_w.max(1) as f32;
    let src_h = src_h.max(1) as f32;
    [width as f32 / src_w, height as f32 / src_h, x as f32 / src_w, y as f32 / src_h]
}

pub fn cover_dims(sw: u32, sh: u32, tw: u32, th: u32) -> (u32, u32) {
    let scale = (f64::from(tw) / f64::from(sw)).max(f64::from(th) / f64::from(sh));
    (
        ((f64::from(sw) * scale).round() as u32).max(tw),
        ((f64::from(sh) * scale).round() as u32).max(th),
    )
}

pub fn desktop_bounds(outputs: &[(i32, i32, u32, u32)]) -> Option<(i32, i32, u32, u32)> {
    let mut it = outputs.iter().filter(|out| out.2 > 0 && out.3 > 0);
    let first = it.next()?;
    let (mut x0, mut y0) = (first.0, first.1);
    let (mut x1, mut y1) = (first.0 + first.2 as i32, first.1 + first.3 as i32);
    for out in it {
        x0 = x0.min(out.0);
        y0 = y0.min(out.1);
        x1 = x1.max(out.0 + out.2 as i32);
        y1 = y1.max(out.1 + out.3 as i32);
    }
    Some((x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

pub fn span_crop_rect(
    img_w: u32,
    img_h: u32,
    bounds: (i32, i32, u32, u32),
    output: (i32, i32, u32, u32),
) -> (u32, u32, u32, u32) {
    let (bx, by, bw, bh) = bounds;
    let (ox, oy, ow, oh) = output;
    if img_w == 0 || img_h == 0 || bw == 0 || bh == 0 || ow == 0 || oh == 0 {
        return (0, 0, img_w.max(1), img_h.max(1));
    }
    let scale = (f64::from(bw) / f64::from(img_w)).max(f64::from(bh) / f64::from(img_h));
    let vis_w = f64::from(bw) / scale;
    let vis_h = f64::from(bh) / scale;
    let img_off_x = (f64::from(img_w) - vis_w) * 0.5;
    let img_off_y = (f64::from(img_h) - vis_h) * 0.5;
    let sx = img_off_x + f64::from(ox - bx) / scale;
    let sy = img_off_y + f64::from(oy - by) / scale;
    let sw = f64::from(ow) / scale;
    let sh = f64::from(oh) / scale;
    let x = sx.round().clamp(0.0, f64::from(img_w) - 1.0) as u32;
    let y = sy.round().clamp(0.0, f64::from(img_h) - 1.0) as u32;
    let w = (sw.round() as u32).max(1).min(img_w - x);
    let h = (sh.round() as u32).max(1).min(img_h - y);
    (x, y, w, h)
}
