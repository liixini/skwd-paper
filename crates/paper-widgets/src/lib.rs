mod canvas;
mod clock;
mod lettering;
mod outline;
mod place;
mod scene;
mod shape;
mod tones;

use paper_control::Face;

pub const FONT: &[u8] = include_bytes!("../assets/RobotoCondensed-Bold.ttf");

pub use canvas::{Canvas, Ink};
pub use lettering::{FONT_LIMIT, Lettering, read_font};
pub use outline::{Frame, Outline};
pub use place::{Place, pin, place, sized, snap};
pub use scene::{Moment, Scene};
pub use tones::{Tones, ground, luminance};

pub fn frame(face: Face) -> [f32; 2] {
    match face {
        Face::Clock(style) => clock::frame(style),
    }
}

pub fn draw(face: Face, canvas: &mut Canvas, scene: &Scene<'_>) {
    match face {
        Face::Clock(style) => clock::draw(style, canvas, scene),
    }
}
