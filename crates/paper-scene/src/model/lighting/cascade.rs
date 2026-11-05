use super::shadow::{CASTER_BIAS, ShadowView, TILE_SIDE};
use super::{Properties, Value, directional, number, vec3};
use crate::matrix::{Mat4, dot3};
use crate::model::{
    CAMERA_CENTER, CAMERA_EYE, CAMERA_FIELD, CAMERA_UP, CameraBasis, GENERAL_FIELD,
    PROJECTION_FIELD, PROJECTION_HEIGHT, PROJECTION_WIDTH, ProjectionMode,
};

pub(super) const CASCADES: usize = 3;
pub(super) const MAX_SHADOW_SUNS: usize = 3;
pub(super) const DISTANCE_FIELDS: [(&str, f32); CASCADES] =
    [("cascadedistance0", 3.0), ("cascadedistance1", 10.0), ("cascadedistance2", 100.0)];
const BUDGET_CONFIG: &str = "/general/lightconfig/directionalshadow";
const BUDGET_MASK: usize = 3;
const NEAR_DEPTH_SCALE: f32 = 4.0;
const FAR_DEPTH_SCALE: f32 = 1.5;
const DEPTH_CENTER: f32 = 0.5;
const VIEW_ALONG_SUN: f32 = 0.5;
const SCREEN_FORWARD: [f32; AXES] = [0.0, 0.0, -1.0];
const AXES: usize = 3;
const COLUMN: usize = 4;
const TRANSLATION: usize = 12;
const LAST_ROW: usize = 15;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SunView {
    pub eye: [f32; AXES],
    pub forward: [f32; AXES],
    pub flat: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sun {
    pub slot: Option<usize>,
    up: [f32; AXES],
    right: [f32; AXES],
    distances: [f32; CASCADES],
}

impl SunView {
    pub fn of(scene: &Value, props: &Properties) -> Self {
        let flat = ProjectionMode::of(scene) != ProjectionMode::Other;
        let authored = scene.get(CAMERA_FIELD);
        let point = |field| {
            authored
                .and_then(|camera: &Value| vec3(camera.get(field), props))
                .map(<[f32; AXES]>::from)
        };
        let forward = point(CAMERA_EYE)
            .zip(point(CAMERA_CENTER))
            .zip(point(CAMERA_UP))
            .and_then(|((eye, center), up)| CameraBasis::of(eye, center, up))
            .map_or(SCREEN_FORWARD, |basis| basis.forward);
        let eye = if flat {
            let size = scene.get(GENERAL_FIELD).and_then(|general| general.get(PROJECTION_FIELD));
            let half =
                |field| number(size.and_then(|size: &Value| size.get(field)), props, 0.0) * 0.5;
            [half(PROJECTION_WIDTH), half(PROJECTION_HEIGHT), 0.0]
        } else {
            point(CAMERA_EYE).unwrap_or_default()
        };
        Self { eye, forward, flat }
    }
}

impl Sun {
    pub(super) fn new(object: &Value, props: &Properties, matrix: &Mat4) -> Self {
        let row = |index: usize| -> [f32; AXES] {
            std::array::from_fn(|axis| matrix[index * COLUMN + axis])
        };
        let distances =
            DISTANCE_FIELDS.map(|(field, default)| number(object.get(field), props, default));
        Self { slot: None, up: row(1), right: row(2), distances }
    }

    pub(super) fn usable(&self, matrix: &Mat4) -> bool {
        let forward = matrix[..AXES].iter().map(|value| value * value).sum::<f32>();
        forward > f32::EPSILON && self.distances.iter().all(|distance| *distance > 0.0)
    }

    fn ranges(&self) -> [[f32; 2]; CASCADES] {
        let [first, second, third] = self.distances;
        let depth = second * NEAR_DEPTH_SCALE;
        [[first, depth], [second, depth], [third, (third * FAR_DEPTH_SCALE).max(depth)]]
    }

    fn origin(&self, forward: [f32; AXES], view: &SunView, width: f32) -> [f32; AXES] {
        let half = width * 0.5;
        let along = dot3(view.forward, forward) * VIEW_ALONG_SUN;
        let mut origin: [f32; AXES] = std::array::from_fn(|axis| {
            view.eye[axis] + (view.forward[axis] - forward[axis] * along) * half
        });
        if view.flat {
            origin[AXES - 1] = 0.0;
        }
        let texel = width / TILE_SIDE as f32;
        let [up, right] = [self.up, self.right].map(|row| dot3(row, origin) % texel);
        std::array::from_fn(|axis| origin[axis] - self.up[axis] * up - self.right[axis] * right)
    }

    pub(super) fn views(
        &self,
        axes: [[f32; AXES]; AXES],
        forward: [f32; AXES],
        sun: &SunView,
        view: ShadowView,
    ) -> [Mat4; CASCADES] {
        let [axis_forward, axis_right, axis_up] = axes;
        self.ranges().map(|[width, depth]| {
            let origin = self.origin(forward, sun, width);
            let scale = 2.0 / width;
            let mut matrix = [0.0; COLUMN * COLUMN];
            for axis in 0..AXES {
                matrix[axis * COLUMN] = axis_right[axis] * scale;
                matrix[axis * COLUMN + 1] = axis_up[axis] * scale;
                matrix[axis * COLUMN + 2] = -axis_forward[axis] / depth;
            }
            matrix[TRANSLATION] = -dot3(axis_right, origin) * scale;
            matrix[TRANSLATION + 1] = -dot3(axis_up, origin) * scale;
            matrix[TRANSLATION + 2] = dot3(axis_forward, origin) / depth + DEPTH_CENTER;
            matrix[LAST_ROW] = 1.0;
            if matches!(view, ShadowView::Caster) {
                matrix[TRANSLATION + 2] -= CASTER_BIAS;
            }
            matrix
        })
    }
}

pub(super) fn budget(scene: &Value) -> usize {
    let authored = scene.pointer(BUDGET_CONFIG).and_then(Value::as_u64).unwrap_or(0) as usize;
    (authored & BUDGET_MASK).min(directional::count(scene))
}

#[cfg(test)]
#[path = "cascade_tests.rs"]
mod tests;
