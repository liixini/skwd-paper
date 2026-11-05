use super::{
    Attachments, CAPACITY, COLOR_FIELD, COMPONENTS, DEFAULT_EXPONENT, EXPONENT, INTENSITY,
    ORIGIN_FIELD, Properties, RADIUS, Value, light_color, number,
};
use crate::model::{ANGLES_FIELD, SCALE_FIELD};
use std::collections::BTreeMap;

pub(crate) const KIND: &str = "lspot";
pub(crate) const COMBO: &str = "LIGHTS_SPOT";
pub(crate) const PREFIX: &str = "g_LSpot_";
pub(super) const ARRAY_SIZE: &str = "[LIGHTS_SPOT]";
const CONFIG: &str = "/general/lightconfig/spot";
const ORIGIN: &str = "g_LSpot_Origin";
const COLOR: &str = "g_LSpot_Color";
const DIRECTION: &str = "g_LSpot_Direction";
const COOKIE_RIGHT: &str = "g_LSpot_CookieRight";
const COOKIE_UP: &str = "g_LSpot_CookieUp";
const FALLOFF: &str = "g_LSpot_Exponent";
pub(super) const INNER: &str = "innercone";
pub(super) const OUTER: &str = "outercone";
pub(super) const INNER_DEFAULT: f32 = 20.0;
pub(super) const OUTER_DEFAULT: f32 = 30.0;
const AXES: usize = 3;
const TRANSLATION: usize = 12;
const INVALID: &str = "invalid spot light values";

pub(super) fn count(scene: &Value) -> usize {
    scene.pointer(CONFIG).and_then(Value::as_u64).unwrap_or(0) as usize
}

pub(super) fn validate(object: &Value, props: &Properties) -> Option<&'static str> {
    let invalid =
        super::invalid_numbers(object, props, &[INTENSITY, RADIUS, EXPONENT, INNER, OUTER])
            || super::invalid_vectors(
                object,
                props,
                &[ORIGIN_FIELD, COLOR_FIELD, ANGLES_FIELD, SCALE_FIELD],
            );
    invalid.then_some(INVALID)
}

pub(super) fn uniforms(
    scene: &Value,
    props: &Properties,
    attachments: &Attachments<'_>,
) -> BTreeMap<String, Vec<f32>> {
    let slots = count(scene).min(CAPACITY);
    if slots == 0 {
        return BTreeMap::new();
    }
    let mut origin = vec![0.0; COMPONENTS * slots];
    let mut color = origin.clone();
    let mut direction = origin.clone();
    let mut falloff = origin.clone();
    let mut cookie_right = origin.clone();
    let mut cookie_up = origin.clone();
    for (slot, (object, matrix)) in
        super::posed(scene, props, attachments, KIND, slots).into_iter().enumerate()
    {
        let offset = slot * COMPONENTS;
        let vector: [f32; AXES] = matrix[..AXES].try_into().unwrap();
        let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        let vector = if length > f32::EPSILON {
            vector.map(|value| value / length)
        } else {
            [1.0, 0.0, 0.0]
        };
        origin[offset..offset + AXES].copy_from_slice(&matrix[TRANSLATION..TRANSLATION + AXES]);
        origin[offset + AXES] = number(object.get(INNER), props, INNER_DEFAULT).to_radians().cos();
        direction[offset..offset + AXES].copy_from_slice(&vector);
        direction[offset + AXES] =
            number(object.get(OUTER), props, OUTER_DEFAULT).to_radians().cos();
        color[offset..offset + COMPONENTS].copy_from_slice(&light_color(object, props));
        falloff[offset] = number(object.get(EXPONENT), props, DEFAULT_EXPONENT);
        for (values, column) in [(&mut cookie_right, COMPONENTS * 2), (&mut cookie_up, COMPONENTS)]
        {
            let axis = &matrix[column..column + AXES];
            let length = axis.iter().map(|value| value * value).sum::<f32>().sqrt();
            if length > f32::EPSILON {
                for index in 0..AXES {
                    values[offset + index] = axis[index] / length;
                }
            }
        }
        cookie_right[offset + AXES] =
            1.0 / number(object.get(OUTER), props, OUTER_DEFAULT).to_radians().tan();
    }
    BTreeMap::from([
        (ORIGIN.into(), origin),
        (COLOR.into(), color),
        (DIRECTION.into(), direction),
        (FALLOFF.into(), falloff),
        (COOKIE_RIGHT.into(), cookie_right),
        (COOKIE_UP.into(), cookie_up),
    ])
}
