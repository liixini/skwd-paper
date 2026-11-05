use zeno::{Cap, Command, Fill, Join, Mask, Placement, Stroke, Style, Transform};

const OPAQUE: f32 = 255.0;
pub const ALPHA: usize = 3;
pub const CLEAR: [u8; 4] = [0; 4];
pub const WHITE: [u8; 3] = [u8::MAX; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub rgb: [u8; 3],
    pub alpha: f32,
}

impl Ink {
    pub const fn solid(rgb: [u8; 3]) -> Self {
        Self { rgb, alpha: 1.0 }
    }

    #[must_use]
    pub fn fade(self, alpha: f32) -> Self {
        Self { alpha: self.alpha * alpha, ..self }
    }

    #[must_use]
    pub fn mix(self, other: Self, share: f32) -> [u8; 3] {
        let mut mixed = self.rgb;
        for (level, &target) in mixed.iter_mut().zip(&other.rgb) {
            *level = (f32::from(*level) * (1.0 - share) + f32::from(target) * share).round() as u8;
        }
        mixed
    }
}

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub scale: f32,
    pub shift: [f32; 2],
    pub pixels: Vec<[u8; 4]>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, scale: 1.0, shift: [0.0; 2], pixels: vec![CLEAR; width * height] }
    }

    pub fn window(size: [usize; 2], scale: f32, shift: [f32; 2]) -> Self {
        Self { scale, shift, ..Self::new(size[0], size[1]) }
    }

    pub fn scaled(frame: [f32; 2], scale: f32) -> Self {
        let [width, height] = frame.map(|side| (side * scale).ceil() as usize);
        Self { scale, ..Self::new(width, height) }
    }

    #[must_use]
    pub fn blank(&self) -> Self {
        Self { scale: self.scale, shift: self.shift, ..Self::new(self.width, self.height) }
    }

    pub fn fill(&mut self, path: &[Command], ink: Ink) {
        let (coverage, place) = self.render(path, Style::Fill(Fill::NonZero));
        self.cover(&coverage, place, ink);
    }

    pub fn stroke(&mut self, path: &[Command], width: f32, ink: Ink) {
        let (coverage, place) = self.render(path, stroked(width));
        self.cover(&coverage, place, ink);
    }

    fn render(&self, path: &[Command], style: Style<'_>) -> (Vec<u8>, Placement) {
        let [dx, dy] = self.shift;
        let scaled = Transform::scale(self.scale, self.scale);
        let transform = Some(scaled.then(&Transform::translation(-dx, -dy)));
        let bounds = zeno::bounds(path, style, transform);
        let left = bounds.min.x.floor().max(0.0);
        let top = bounds.min.y.floor().max(0.0);
        let right = bounds.max.x.ceil().min(self.width as f32);
        let bottom = bounds.max.y.ceil().min(self.height as f32);
        if right <= left || bottom <= top {
            return (Vec::new(), Placement { left: 0, top: 0, width: 0, height: 0 });
        }
        let (width, height) = ((right - left) as u32, (bottom - top) as u32);
        let (coverage, _) = Mask::new(path)
            .style(style)
            .transform(transform)
            .size(width, height)
            .offset([-left, -top])
            .render();
        (coverage, Placement { left: left as i32, top: top as i32, width, height })
    }

    pub fn coverage(&self, path: &[Command], stroke: Option<f32>) -> Vec<u8> {
        let style = stroke.map_or(Style::Fill(Fill::NonZero), stroked);
        let (mask, place) = self.render(path, style);
        let mut full = vec![0; self.width * self.height];
        for row in 0..place.height as i32 {
            let y = place.top + row;
            if !(0..self.height as i32).contains(&y) {
                continue;
            }
            for column in 0..place.width as i32 {
                let x = place.left + column;
                if (0..self.width as i32).contains(&x) {
                    full[y as usize * self.width + x as usize] =
                        mask[(row * place.width as i32 + column) as usize];
                }
            }
        }
        full
    }

    pub fn cover_all(&mut self, coverage: &[u8], ink: Ink) {
        let place =
            Placement { left: 0, top: 0, width: self.width as u32, height: self.height as u32 };
        self.cover(coverage, place, ink);
    }

    pub fn cover(&mut self, coverage: &[u8], place: Placement, ink: Ink) {
        let (width, height) = (self.width as i32, self.height as i32);
        for row in 0..place.height as i32 {
            let y = place.top + row;
            if !(0..height).contains(&y) {
                continue;
            }
            for column in 0..place.width as i32 {
                let x = place.left + column;
                if !(0..width).contains(&x) {
                    continue;
                }
                let covered = coverage[(row * place.width as i32 + column) as usize];
                if covered == 0 {
                    continue;
                }
                let alpha = ink.alpha * f32::from(covered) / OPAQUE;
                let pixel = &mut self.pixels[(y * width + x) as usize];
                let keep = 1.0 - alpha;
                for (channel, &level) in pixel.iter_mut().zip(&ink.rgb) {
                    *channel =
                        (f32::from(level) * alpha + f32::from(*channel) * keep).round() as u8;
                }
                pixel[ALPHA] = (OPAQUE * alpha + f32::from(pixel[ALPHA]) * keep).round() as u8;
            }
        }
    }

    pub fn occlude(&mut self, outline: &[u8]) {
        assert_eq!(outline.len(), self.pixels.len(), "outline size");
        for (pixel, &covered) in self.pixels.iter_mut().zip(outline) {
            let keep = 1.0 - f32::from(covered) / OPAQUE;
            for channel in pixel.iter_mut() {
                *channel = (f32::from(*channel) * keep).round() as u8;
            }
        }
    }
}

fn stroked(width: f32) -> Style<'static> {
    let mut stroke = Stroke::new(width);
    stroke.cap(Cap::Round).join(Join::Round);
    Style::Stroke(stroke)
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
