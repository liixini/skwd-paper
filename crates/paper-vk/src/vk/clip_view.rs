use paper_scene::matrix::Mat4;
use paper_scene::model::CameraView;

const MATRIX_COLUMNS: usize = 4;
const CLIP_X: usize = 0;
const CLIP_Y: usize = 1;
const CLIP_W: usize = 3;
const QUAD_COLUMNS: usize = 4;
const QUAD_ROWS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipView {
    linear: [[f32; 2]; 2],
    translation: [f32; 2],
}

impl ClipView {
    pub const IDENTITY: Self = Self { linear: [[1.0, 0.0], [0.0, 1.0]], translation: [0.0; 2] };

    pub fn new(view: CameraView, canvas: (f32, f32)) -> Self {
        let (w, h) = (canvas.0.max(1.0), canvas.1.max(1.0));
        let scale = view.scale();
        let aspect = w / h;
        let (sin, cos) = (-view.roll).sin_cos();
        let centre = [w * 0.5, h * 0.5];
        let [dx, dy] = [centre[0] - view.origin[0], centre[1] - view.origin[1]];
        let turned = [cos * dx - sin * dy, sin * dx + cos * dy];
        Self {
            linear: [[scale * cos, -scale * sin / aspect], [scale * sin * aspect, scale * cos]],
            translation: [
                scale * (turned[0] - centre[0]) * 2.0 / w,
                scale * (turned[1] - centre[1]) * 2.0 / h,
            ],
        }
    }

    fn oriented(self, flipped: bool) -> ([[f32; 2]; 2], [f32; 2]) {
        if !flipped {
            return (self.linear, self.translation);
        }
        let [[a, b], [c, d]] = self.linear;
        ([[a, -b], [-c, d]], [self.translation[0], -self.translation[1]])
    }

    pub fn apply(self, matrix: &mut Mat4, flipped: bool) {
        let ([[a, b], [c, d]], [tx, ty]) = self.oriented(flipped);
        for column in matrix.chunks_exact_mut(MATRIX_COLUMNS) {
            let (x, y, w) = (column[CLIP_X], column[CLIP_Y], column[CLIP_W]);
            column[CLIP_X] = a * x + b * y + tx * w;
            column[CLIP_Y] = c * x + d * y + ty * w;
        }
    }

    pub fn apply_rows(self, rows: &mut [[f32; QUAD_COLUMNS]; QUAD_ROWS], flipped: bool) {
        let ([[a, b], [c, d]], [tx, ty]) = self.oriented(flipped);
        let [x, y, w] = *rows;
        for column in 0..QUAD_COLUMNS {
            rows[CLIP_X][column] = a * x[column] + b * y[column] + tx * w[column];
            rows[CLIP_Y][column] = c * x[column] + d * y[column] + ty * w[column];
        }
    }
}

#[cfg(test)]
#[path = "clip_view_tests.rs"]
mod tests;
