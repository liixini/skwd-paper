mod fill_mode;
mod placement;

pub use fill_mode::{FillMode, fill_uv_remap};
pub use placement::{
    cover_dims, cover_uv, desktop_bounds, fill_crop_rect, fit_scaled_size, span_crop_rect,
};

mod tests;
