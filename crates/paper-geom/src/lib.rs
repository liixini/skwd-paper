#![deny(unsafe_code)]

mod background;
mod compose;
pub mod domain;

pub use background::{Background, blurred_background};
pub use compose::{Raster, compose, span_pixels};

pub use domain::geometry::{
    FillMode, cover_dims, cover_uv, desktop_bounds, fill_crop_rect, fill_uv_remap, fit_scaled_size,
    span_crop_rect,
};
