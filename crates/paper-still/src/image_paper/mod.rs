mod buffer_set;
mod commands;
mod decode;
mod lifecycle;
mod mirror;
mod model;
mod protocol;
mod readiness;
mod shm_pixels;
mod slide;
mod snapshot;
mod span;
mod stream;
mod surface_effects;
mod widgets;

pub use lifecycle::run;
pub use paper_control::OutputTarget;
pub use stream::stream;

#[cfg(test)]
use decode::decode_image;
#[cfg(test)]
use shm_pixels::{choose_shm_format, pack_pixels};
#[cfg(test)]
use slide::{compose_tall, ease_out_cubic, slide_source_y};
#[cfg(test)]
mod tests;
