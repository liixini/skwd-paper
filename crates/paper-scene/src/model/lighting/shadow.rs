use super::Light;
use super::cascade::{CASCADES, MAX_SHADOW_SUNS, SunView};
use crate::matrix::Mat4;
use std::collections::BTreeMap;

const AXES: usize = 3;
const COLUMN: usize = 4;
const TRANSLATION: usize = 12;
const REVERSE_NEAR: f32 = 1.0;
const POINT_CASTER_BIAS: f32 = 0.00333;
pub(super) const CASTER_BIAS: f32 = 0.0005;
const MIN_RANGE: f32 = 0.01;
const POINT_FOV: f32 = 94.0;
pub(super) const TILE_SIDE: u32 = 256;
const POINT_COLUMNS: u32 = 2;
const POINT_ROWS: u32 = 3;
const POINT_SHADOW: &str = "g_LPoint_Shadow";
const SPOT_SHADOW: &str = "g_LSpot_Shadow";
const SPOT_RIGHT: &str = "g_LSpot_ShadowRight";
const SPOT_UP: &str = "g_LSpot_ShadowUp";
const POINT_PROJECTION: &str = "g_LFeature_ShadowPointProjection";
const POINT_TRANSFORM: &str = "g_LFeature_ShadowPointProjectionTransform";
const SPOT_PROJECTION: &str = "g_LFeature_ShadowProjection";
const SPOT_TRANSFORM: &str = "g_LFeature_ShadowProjectionTransform";
const SUN_SHADOW: &str = "g_LDirectional_Shadow";
const ATLAS_SIZE: &str = "g_LShadow_Size";
const MATRIX: usize = COLUMN * COLUMN;
const PROJECTIONS: usize = super::CAPACITY + MAX_SHADOW_SUNS * CASCADES;
const FACES: [([f32; AXES], [f32; AXES], [f32; AXES]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
];

#[derive(Clone, Copy)]
pub enum ShadowView {
    Receiver,
    Caster,
}

#[derive(Default)]
pub struct ShadowLayout {
    tiles: BTreeMap<String, u32>,
    columns: u32,
    rows: u32,
}

impl ShadowLayout {
    pub fn new(lights: &[Light]) -> Option<Self> {
        let mut count = 0;
        let tiles = lights
            .iter()
            .take(super::MAX_LIGHTS)
            .filter(|light| light.shadow)
            .map(|light| {
                let tile = count;
                count += light.tile_span();
                (light.id.clone(), tile)
            })
            .collect();
        if count == 0 {
            return None;
        }
        let columns = (count as f32).sqrt().ceil() as u32;
        Some(Self { tiles, columns, rows: count.div_ceil(columns) })
    }

    pub fn matches(&self, lights: &[Light]) -> bool {
        let mut count = 0;
        let mut shadowed = 0;
        lights.iter().take(super::MAX_LIGHTS).filter(|light| light.shadow).all(|light| {
            let tile = count;
            count += light.tile_span();
            shadowed += 1;
            self.tiles.get(&light.id) == Some(&tile)
        }) && shadowed == self.tiles.len()
    }

    pub fn extent(&self) -> [u32; 2] {
        if self.tiles.is_empty() {
            return [1, 1];
        }
        [self.columns * TILE_SIDE, self.rows * TILE_SIDE]
    }

    pub fn dimensions(&self) -> [f32; COLUMN] {
        if self.tiles.is_empty() {
            return [0.0; COLUMN];
        }
        let [width, height] = self.extent();
        [self.columns as f32, self.rows as f32, width as f32, height as f32]
    }

    pub fn tile(&self, light: &Light, face: usize) -> Option<[u32; COLUMN]> {
        let tile = self.tiles.get(&light.id)?;
        if light.sun.is_some() {
            let cell = tile + face as u32;
            return (face < CASCADES).then_some([
                (cell % self.columns) * TILE_SIDE,
                (cell / self.columns) * TILE_SIDE,
                TILE_SIDE,
                TILE_SIDE,
            ]);
        }
        let x = (tile % self.columns) * TILE_SIDE;
        let y = (tile / self.columns) * TILE_SIDE;
        if light.cone.is_some() {
            return (face == 0).then_some([x, y, TILE_SIDE, TILE_SIDE]);
        }
        if face >= FACES.len() {
            return None;
        }
        let width = TILE_SIDE / POINT_COLUMNS;
        let height = TILE_SIDE / POINT_ROWS;
        Some([
            x + (face as u32 % POINT_COLUMNS) * width,
            y + (face as u32 / POINT_COLUMNS) * height,
            width,
            height,
        ])
    }

    pub fn parameters(&self, light: &Light) -> [f32; COLUMN] {
        let Some(tile) = self.tiles.get(&light.id).filter(|_| light.shadow) else {
            return [0.0; COLUMN];
        };
        let [depth, offset, scale] = light.shadow_parameters();
        [(*tile + 1) as f32, depth, offset, scale]
    }

    fn cell_transform(&self, cell: u32) -> [f32; COLUMN] {
        [
            (cell % self.columns) as f32 / self.columns as f32,
            (cell / self.columns) as f32 / self.rows as f32,
            1.0 / self.columns as f32,
            1.0 / self.rows as f32,
        ]
    }

    pub fn uniforms(&self, lights: &[Light], sun: &SunView) -> BTreeMap<String, Vec<f32>> {
        let mut points = vec![0.0; super::CAPACITY * COLUMN];
        let mut spots = points.clone();
        let mut right = points.clone();
        let mut up = points.clone();
        for (index, light) in lights
            .iter()
            .filter(|light| light.visible && light.cone.is_none() && light.sun.is_none())
            .take(super::CAPACITY)
            .enumerate()
        {
            points[index * COLUMN..(index + 1) * COLUMN].copy_from_slice(&self.parameters(light));
        }
        for (index, light) in lights
            .iter()
            .filter(|light| light.visible && light.cone.is_some())
            .take(super::CAPACITY)
            .enumerate()
        {
            spots[index * COLUMN..(index + 1) * COLUMN].copy_from_slice(&self.parameters(light));
            right[index * COLUMN..index * COLUMN + AXES].copy_from_slice(&light.up);
            up[index * COLUMN..index * COLUMN + AXES].copy_from_slice(&light.right);
        }
        let mut values = self.native_uniforms(lights, sun);
        values.extend([
            (POINT_SHADOW.into(), points),
            (SPOT_SHADOW.into(), spots),
            (SPOT_RIGHT.into(), right),
            (SPOT_UP.into(), up),
            (ATLAS_SIZE.into(), self.dimensions().to_vec()),
        ]);
        values
    }

    fn native_uniforms(&self, lights: &[Light], sun: &SunView) -> BTreeMap<String, Vec<f32>> {
        let mut values = BTreeMap::new();
        for (spot, projection_name, transform_name) in
            [(false, POINT_PROJECTION, POINT_TRANSFORM), (true, SPOT_PROJECTION, SPOT_TRANSFORM)]
        {
            let stride = if spot { MATRIX } else { COLUMN };
            let slots = if spot { PROJECTIONS } else { super::CAPACITY };
            let mut projection = vec![0.0; slots * stride];
            let mut transforms = vec![0.0; slots * COLUMN];
            let mut written = 0;
            for (index, light) in lights
                .iter()
                .filter(|light| {
                    light.visible
                        && light.sun.is_none()
                        && light.cone.is_some() == spot
                        && (light.shadow || (spot && light.cookie.is_some()))
                })
                .take(super::CAPACITY)
                .enumerate()
            {
                written = index + 1;
                if spot {
                    if let Some(matrix) = light.shadow_views(ShadowView::Receiver, sun).next() {
                        projection[index * stride..(index + 1) * stride].copy_from_slice(&matrix);
                    }
                } else {
                    let [depth, offset, _] = light.shadow_parameters();
                    projection[index * stride..(index + 1) * stride]
                        .copy_from_slice(&[depth, offset, -1.0, 0.0]);
                }
                let Some(tile) = self.tiles.get(&light.id) else { continue };
                transforms[index * COLUMN..(index + 1) * COLUMN]
                    .copy_from_slice(&self.cell_transform(*tile));
            }
            if spot {
                let suns = self.sun_slots(lights, sun, written, &mut projection, &mut transforms);
                values.insert(SUN_SHADOW.into(), suns);
            }
            values.insert(projection_name.into(), projection);
            values.insert(transform_name.into(), transforms);
        }
        values
    }

    fn sun_slots(
        &self,
        lights: &[Light],
        sun: &SunView,
        base: usize,
        projection: &mut [f32],
        transforms: &mut [f32],
    ) -> Vec<f32> {
        let mut slots = vec![0.0; super::CAPACITY * COLUMN];
        for (index, light) in lights
            .iter()
            .filter(|light| light.visible && light.sun.is_some())
            .take(super::CAPACITY)
            .enumerate()
        {
            let Some(slot) = light.sun.as_ref().and_then(|sun| sun.slot) else { continue };
            let Some(tile) = self.tiles.get(&light.id) else { continue };
            let first = base + slot * CASCADES;
            slots[index * COLUMN] = (first + 1) as f32;
            for (face, matrix) in light.shadow_views(ShadowView::Receiver, sun).enumerate() {
                let at = first + face;
                projection[at * MATRIX..(at + 1) * MATRIX].copy_from_slice(&matrix);
                transforms[at * COLUMN..(at + 1) * COLUMN]
                    .copy_from_slice(&self.cell_transform(tile + face as u32));
            }
        }
        slots
    }
}

pub(super) fn spot_axes(matrix: &Mat4) -> [[f32; AXES]; AXES] {
    [0, COLUMN * 2, COLUMN].map(|column| {
        let axis: [f32; AXES] = matrix[column..column + AXES].try_into().unwrap();
        let squared = axis.iter().map(|value| value * value).sum::<f32>();
        let inverse = crate::matrix::native_inverse_sqrt(squared);
        axis.map(|value| value * inverse)
    })
}

impl Light {
    pub fn shadow_parameters(&self) -> [f32; AXES] {
        let near = REVERSE_NEAR;
        let far = self.radius.max(near + MIN_RANGE);
        let scale = self.cone.map_or_else(
            || 1.0 / (POINT_FOV.to_radians() * 0.5).tan(),
            |cone| 1.0 / cone[1].acos().tan(),
        );
        let depth = near / (far - near);
        [depth, depth * far, scale]
    }

    fn tile_span(&self) -> u32 {
        if self.sun.is_some() { CASCADES as u32 } else { 1 }
    }

    #[must_use]
    pub fn casting(&self) -> bool {
        self.shadow && self.visible && self.sun.as_ref().is_none_or(|sun| sun.slot.is_some())
    }

    pub fn shadow_views(&self, view: ShadowView, sun: &SunView) -> impl Iterator<Item = Mat4> {
        let mut matrices = [[0.0; MATRIX]; FACES.len()];
        let count = if let Some(cascades) = &self.sun {
            let views = cascades.views(self.shadow_axes, self.forward, sun, view);
            matrices[..CASCADES].copy_from_slice(&views);
            CASCADES
        } else if self.cone.is_some() {
            let [forward, right, up] = self.shadow_axes;
            matrices[0] = self.perspective_view(view, (forward, right, up));
            1
        } else {
            for (matrix, axes) in matrices.iter_mut().zip(FACES) {
                *matrix = self.perspective_view(view, axes);
            }
            FACES.len()
        };
        matrices.into_iter().take(count)
    }

    fn perspective_view(
        &self,
        view: ShadowView,
        (forward, right, up): ([f32; AXES], [f32; AXES], [f32; AXES]),
    ) -> Mat4 {
        let [depth, mut offset, scale] = self.shadow_parameters();
        if matches!(view, ShadowView::Caster) && self.cone.is_none() {
            offset -= POINT_CASTER_BIAS;
        }
        let mut matrix = [0.0; MATRIX];
        for axis in 0..AXES {
            matrix[axis * COLUMN] = right[axis] * scale;
            matrix[axis * COLUMN + 1] = up[axis] * scale;
            matrix[axis * COLUMN + 2] = -forward[axis] * depth;
            matrix[axis * COLUMN + AXES] = forward[axis];
            matrix[TRANSLATION] -= right[axis] * self.origin[axis] * scale;
            matrix[TRANSLATION + 1] -= up[axis] * self.origin[axis] * scale;
            matrix[TRANSLATION + 2] += forward[axis] * self.origin[axis] * depth;
            matrix[TRANSLATION + AXES] -= forward[axis] * self.origin[axis];
        }
        matrix[TRANSLATION + 2] += offset;
        if matches!(view, ShadowView::Caster) && self.cone.is_some() {
            matrix[TRANSLATION + 2] -= CASTER_BIAS;
        }
        matrix
    }
}

#[cfg(test)]
#[path = "shadow_tests.rs"]
mod tests;
