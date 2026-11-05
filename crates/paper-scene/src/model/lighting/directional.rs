use super::{
    COLOR_FIELD, COMPONENTS, INTENSITY, Properties, VOLUMETRICS, Value, cascade, light_rgb, truthy,
};
use crate::matrix::Mat4;
use crate::model::{ANGLES_FIELD, SCALE_FIELD};
use std::collections::BTreeMap;

const CONFIG: &str = "/general/lightconfig/directional";
pub(crate) const KIND: &str = "ldirectional";
pub(crate) const COMBO: &str = "LIGHTS_DIRECTIONAL";
pub(crate) const PREFIX: &str = "g_LDirectional_";
pub(super) const ARRAY_SIZE: &str = "[LIGHTS_DIRECTIONAL]";
const COLOR: &str = "g_LDirectional_Color";
const DIRECTION: &str = "g_LDirectional_Direction";
const UNIT_DISTANCE: f32 = 1.0;
const AXES: usize = 3;
const INVALID: &str = "invalid directional light values";
const VOLUMETRICS_UNSUPPORTED: &str = "directional light volumetrics unsupported";
const DARK_DIRECTION: [f32; COMPONENTS] = [0.0, 0.0, -1.0, 0.0];

pub(super) fn count(scene: &Value) -> usize {
    scene.pointer(CONFIG).and_then(Value::as_u64).unwrap_or(0) as usize
}

pub(super) fn validate(object: &Value, props: &Properties) -> Option<&'static str> {
    let [first, second, third] = cascade::DISTANCE_FIELDS.map(|(field, _)| field);
    if super::invalid_numbers(object, props, &[INTENSITY, first, second, third])
        || super::invalid_vectors(object, props, &[COLOR_FIELD, ANGLES_FIELD, SCALE_FIELD])
    {
        return Some(INVALID);
    }
    truthy(object.get(VOLUMETRICS), props, false).then_some(VOLUMETRICS_UNSUPPORTED)
}

pub(super) fn uniforms(
    slots: usize,
    lights: &[(&Value, Mat4)],
    props: &Properties,
) -> BTreeMap<String, Vec<f32>> {
    if slots == 0 {
        return BTreeMap::new();
    }
    let mut color = vec![0.0; COMPONENTS * slots];
    let mut direction: Vec<f32> = std::iter::repeat_n(DARK_DIRECTION, slots).flatten().collect();
    for (slot, (object, matrix)) in lights.iter().enumerate() {
        let offset = slot * COMPONENTS;
        let [x, y, z, ..] = *matrix;
        let length = (x * x + y * y + z * z).sqrt();
        if length <= f32::EPSILON {
            continue;
        }
        direction[offset..offset + AXES].copy_from_slice(&[-x / length, -y / length, -z / length]);
        let [r, g, b] = light_rgb(object, props);
        color[offset..offset + COMPONENTS].copy_from_slice(&[r, g, b, UNIT_DISTANCE]);
    }
    BTreeMap::from([(COLOR.into(), color), (DIRECTION.into(), direction)])
}

#[cfg(test)]
#[path = "directional_tests.rs"]
mod tests;
