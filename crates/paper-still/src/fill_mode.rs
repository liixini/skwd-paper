pub use paper_geom::{FillMode, span_pixels};

pub fn apply_fill_mode(
    img_w: u32,
    img_h: u32,
    pixels: &[u8],
    surf_w: u32,
    surf_h: u32,
    mode: FillMode,
) -> (u32, u32, Vec<u8>) {
    let background = paper_control::worker_background();
    let source = paper_geom::Raster { width: img_w, height: img_h, pixels };
    paper_geom::compose(source, [surf_w, surf_h], mode, background)
}
