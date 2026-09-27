#![deny(unsafe_code)]

mod background;
pub mod domain;

pub use background::blurred_background;

pub use domain::geometry::{
    FillMode, cover_dims, cover_uv, desktop_bounds, fill_crop_rect, fill_uv_remap, fit_scaled_size,
    span_crop_rect,
};
