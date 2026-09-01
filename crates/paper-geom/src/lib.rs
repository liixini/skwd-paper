#![deny(unsafe_code)]

pub mod domain;

pub use domain::geometry::{
    FillMode, cover_dims, cover_uv, desktop_bounds, fill_crop_rect, fill_uv_remap, fit_scaled_size,
    span_crop_rect,
};
