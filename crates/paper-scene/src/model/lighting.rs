use super::{
    Attachments, COLOR_FIELD, ID_FIELD, NAME_FIELD, OBJECTS_FIELD, ORIGIN_FIELD, Properties,
    ancestor_chain, id_of, number, resolve_transform, truthy, vec3,
};
use crate::matrix::{Mat4, mat4_mul, model_matrix};
use crate::model::{ANGLES_FIELD, ATTACHMENT_FIELD, PARENT_FIELD, SCALE_FIELD};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub(crate) mod directional;
pub(crate) mod spot;
mod volume;
pub use volume::{Light, Volume, lights};
pub mod cookie;
pub use cookie::{CookieLayout, Cookies};
mod cascade;
pub use cascade::{Sun, SunView};
mod shadow;
pub use shadow::{ShadowLayout, ShadowView};

const LIGHT: &str = "light";
const POINT: &str = "lpoint";
const LEGACY_POINT: &str = "point";
const CONFIG: &str = "/general/lightconfig/point";
const ORIGIN: &str = "g_LPoint_Origin";
const COLOR: &str = "g_LPoint_Color";
const LEGACY_POSITION: &str = "g_LightsPosition";
const LEGACY_COLOR_RADIUS: &str = "g_LightsColorRadius";
const EXPONENT: &str = "exponent";
const RADIUS: &str = "radius";
const INTENSITY: &str = "intensity";
pub(crate) const COMBO: &str = "LIGHTS_POINT";
pub(crate) const CAPACITY: usize = 32;
pub const MAX_LIGHTS: usize = CAPACITY * 2;
pub(crate) const COOKIE_COMBO: &str = "LIGHTS_COOKIE";
pub(crate) const COOKIE_SLOT: usize = 7;
pub(crate) const COOKIE_PREFIX: &str = "g_LSpot_Cookie";
pub(crate) const SHADOW_COMBO: &str = "SKWD_LIGHT_SHADOWS";
pub(crate) const SHADOW_PREFIX: &str = "g_LShadow_";
pub(crate) const SHADOW_FEATURE_PREFIX: &str = "g_LFeature_Shadow";
pub(crate) const SHADOW_SLOT: usize = 6;
const ARRAY_SIZE: &str = "[LIGHTS_POINT]";
const PROJECTION_SIZE: &str = "[SKWD_LIGHT_PROJECTIONS]";
const COMPONENTS: usize = 4;
const SHADOW: &str = "castshadow";
const VOLUMETRICS: &str = "castvolumetrics";
const UNSUPPORTED_KIND: &str = "light type unsupported";
const INVALID_VALUES: &str = "invalid point light values";
pub(crate) const UNIFORM_PREFIX: &str = "g_LPoint_";
pub(crate) const LEGACY_UNIFORM_PREFIX: &str = "g_Lights";
const LEGACY_CAPACITY: usize = 4;
const LEGACY_PREMULTIPLIED: &str = "g_LightsColorPremultiplied";
const PREMULTIPLIED_SLOTS: usize = LEGACY_CAPACITY - 1;
const POSITION_COMPONENTS: usize = 3;
const DEFAULT_RADIUS: f32 = 1.0;
const DEFAULT_EXPONENT: f32 = 2.0;

#[derive(Clone, Default)]
pub(crate) struct Config {
    pub count: usize,
    pub spots: usize,
    pub directionals: usize,
    pub projections: usize,
    pub shadows: bool,
    pub cookies: bool,
    pub values: BTreeMap<String, Vec<f32>>,
}

impl Config {
    pub(crate) fn source(&self, source: &str) -> String {
        source
            .replace(ARRAY_SIZE, &format!("[{}]", self.count.max(1)))
            .replace(spot::ARRAY_SIZE, &format!("[{}]", self.spots.max(1)))
            .replace(directional::ARRAY_SIZE, &format!("[{}]", self.directionals.max(1)))
            .replace(PROJECTION_SIZE, &format!("[{}]", self.projections.max(1)))
    }
}

fn count(scene: &Value) -> usize {
    scene.pointer(CONFIG).and_then(Value::as_u64).unwrap_or(0) as usize
}

pub fn casts_shadow(object: &Value, props: &Properties) -> bool {
    truthy(object.get(SHADOW), props, true)
}

pub(crate) fn load(scene: &Value, props: &Properties) -> (Config, Vec<String>) {
    let objects = scene[OBJECTS_FIELD].as_array().map_or(&[][..], Vec::as_slice);
    let mut skipped = Vec::new();
    let count = count(scene);
    if count > CAPACITY {
        skipped.push(format!("LightingV1: point capacity {count} unsupported"));
    }
    if spot::count(scene) > CAPACITY {
        skipped.push(format!("LightingV1: spot capacity {} unsupported", spot::count(scene)));
    }
    for object in objects.iter().filter(|object| object.get(LIGHT).is_some()) {
        let name = object[NAME_FIELD].as_str().unwrap_or(LIGHT);
        let reason = if object[LIGHT].as_str() == Some(spot::KIND) {
            spot::validate(object, props)
        } else if object[LIGHT].as_str() == Some(directional::KIND) {
            directional::validate(object, props)
        } else if object[LIGHT].as_str() == Some(LEGACY_POINT) {
            if truthy(object.get(SHADOW), props, false)
                || truthy(object.get(VOLUMETRICS), props, false)
                || ![INTENSITY, RADIUS, COLOR_FIELD, ORIGIN_FIELD]
                    .iter()
                    .all(|key| object.get(*key).is_some())
                || [INTENSITY, RADIUS].iter().any(|key| {
                    !number(object.get(*key), props, f32::NAN).is_finite()
                        || number(object.get(*key), props, f32::NAN) < 0.0
                })
                || [COLOR_FIELD, ORIGIN_FIELD].iter().any(|key| {
                    vec3(object.get(*key), props)
                        .is_none_or(|(x, y, z)| !x.is_finite() || !y.is_finite() || !z.is_finite())
                })
            {
                Some(INVALID_VALUES)
            } else {
                None
            }
        } else if object[LIGHT].as_str() != Some(POINT) {
            Some(UNSUPPORTED_KIND)
        } else if count == 0 {
            None
        } else if [INTENSITY, RADIUS, EXPONENT].iter().any(|key| {
            object.get(*key).is_some_and(|value| {
                let value = number(Some(value), props, f32::NAN);
                !value.is_finite() || value < 0.0
            })
        }) || object.get(COLOR_FIELD).is_some_and(|value| {
            vec3(Some(value), props)
                .is_none_or(|(r, g, b)| [r, g, b].iter().any(|value| !value.is_finite()))
        }) {
            Some(INVALID_VALUES)
        } else {
            None
        };
        if let Some(reason) = reason.or_else(|| volume::validate(object, props)) {
            skipped.push(format!("{name}: {reason}"));
        }
    }
    let legacy_points =
        objects.iter().filter(|object| object[LIGHT].as_str() == Some(LEGACY_POINT));
    if legacy_points.count() > LEGACY_CAPACITY {
        skipped.push(format!("LightingV1: legacy point capacity {LEGACY_CAPACITY} unsupported"));
    }
    let scene_lights = lights(scene, props, &Attachments::new());
    let spot_lights = scene_lights.iter().filter(|light| light.cone.is_some()).count();
    let config = Config {
        count: count.min(CAPACITY),
        spots: spot::count(scene).min(CAPACITY),
        directionals: directional::count(scene).min(CAPACITY),
        projections: spot_lights.min(CAPACITY) + cascade::budget(scene) * cascade::CASCADES,
        cookies: scene_lights.iter().any(|light| light.cookie.is_some()),
        shadows: ShadowLayout::new(&scene_lights).is_some(),
        values: uniforms(scene, props, &Attachments::new(), &SunView::of(scene, props)),
    };
    (config, skipped)
}

fn legacy_uniforms(
    scene: &Value,
    props: &Properties,
    attachments: &Attachments<'_>,
) -> BTreeMap<String, Vec<f32>> {
    let objects = scene[OBJECTS_FIELD].as_array().map_or(&[][..], Vec::as_slice);
    if !objects.iter().any(|object| object[LIGHT].as_str() == Some(LEGACY_POINT)) {
        return BTreeMap::new();
    }
    let by_id: HashMap<_, _> = objects
        .iter()
        .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
        .collect();
    let lights: Vec<_> = objects
        .iter()
        .filter(|object| object[LIGHT].as_str() == Some(LEGACY_POINT))
        .filter(|object| super::object_visible(object, &by_id, props))
        .take(LEGACY_CAPACITY)
        .collect();
    if lights.is_empty() {
        return BTreeMap::new();
    }
    let mut position = vec![0.0; POSITION_COMPONENTS * LEGACY_CAPACITY];
    let mut color_radius = vec![0.0; COMPONENTS * LEGACY_CAPACITY];
    let mut premultiplied = vec![0.0; COMPONENTS * PREMULTIPLIED_SLOTS];
    for (slot, object) in lights.into_iter().enumerate() {
        let origin = resolve_transform(object, &by_id, props, None, attachments).origin;
        position[slot * POSITION_COMPONENTS..(slot + 1) * POSITION_COMPONENTS]
            .copy_from_slice(&[origin.0, origin.1, origin.2]);
        let intensity = number(object.get(INTENSITY), props, 0.0);
        let radius = number(object.get(RADIUS), props, 0.0);
        let (r, g, b) = vec3(object.get(COLOR_FIELD), props).unwrap_or_default();
        let color = [r * intensity, g * intensity, b * intensity];
        color_radius[slot * COMPONENTS..(slot + 1) * COMPONENTS]
            .copy_from_slice(&[color[0], color[1], color[2], radius]);
        for (channel, value) in color.into_iter().enumerate() {
            let lane = if slot < PREMULTIPLIED_SLOTS {
                slot * COMPONENTS + channel
            } else {
                channel * COMPONENTS + COMPONENTS - 1
            };
            premultiplied[lane] = value * radius * radius;
        }
    }
    BTreeMap::from([
        (LEGACY_POSITION.into(), position),
        (LEGACY_COLOR_RADIUS.into(), color_radius),
        (LEGACY_PREMULTIPLIED.into(), premultiplied),
    ])
}

#[must_use]
pub fn uniforms(
    scene: &Value,
    props: &Properties,
    attachments: &Attachments<'_>,
    sun: &SunView,
) -> BTreeMap<String, Vec<f32>> {
    let mut values = spot::uniforms(scene, props, attachments);
    let directionals = directional::count(scene).min(CAPACITY);
    values.extend(directional::uniforms(
        directionals,
        &posed(scene, props, attachments, directional::KIND, directionals),
        props,
    ));
    values.extend(legacy_uniforms(scene, props, attachments));
    let lights = lights(scene, props, attachments);
    if count(scene) > 0 || spot::count(scene) > 0 || directionals > 0 {
        let layout = ShadowLayout::new(&lights).unwrap_or_default();
        values.extend(layout.uniforms(&lights, sun));
    }
    if count(scene) == 0 {
        return values;
    }
    let slots = count(scene).min(CAPACITY);
    let mut origin = vec![0.0; COMPONENTS * slots];
    let mut color = vec![0.0; COMPONENTS * slots];
    let objects = scene[OBJECTS_FIELD].as_array().map_or(&[][..], Vec::as_slice);
    let by_id: HashMap<_, _> = objects
        .iter()
        .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
        .collect();
    let points = objects.iter().filter(|object| {
        object[LIGHT].as_str() == Some(POINT) && super::object_visible(object, &by_id, props)
    });
    for (slot, object) in points.take(slots).enumerate() {
        let position = resolve_transform(object, &by_id, props, None, attachments).origin;
        let offset = slot * COMPONENTS;
        origin[offset..offset + COMPONENTS].copy_from_slice(&[
            position.0,
            position.1,
            position.2,
            number(object.get(EXPONENT), props, DEFAULT_EXPONENT),
        ]);
        color[offset..offset + COMPONENTS].copy_from_slice(&light_color(object, props));
    }
    values.extend([(ORIGIN.into(), origin), (COLOR.into(), color)]);
    values
}

fn posed<'a>(
    scene: &'a Value,
    props: &Properties,
    attachments: &Attachments<'_>,
    kind: &str,
    slots: usize,
) -> Vec<(&'a Value, Mat4)> {
    let objects = scene[OBJECTS_FIELD].as_array().map_or(&[][..], Vec::as_slice);
    let by_id: HashMap<_, _> = objects
        .iter()
        .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
        .collect();
    objects
        .iter()
        .filter(|object| object[LIGHT].as_str() == Some(kind))
        .filter(|object| crate::model::object_visible(object, &by_id, props))
        .take(slots)
        .map(|object| (object, transform(object, &by_id, props, attachments)))
        .collect()
}

fn transform(
    object: &Value,
    by_id: &HashMap<String, &Value>,
    props: &Properties,
    attachments: &Attachments<'_>,
) -> Mat4 {
    let mut matrix = crate::matrix::IDENTITY;
    for node in ancestor_chain(object, by_id).iter().rev() {
        if let Some(parent) = node.get(PARENT_FIELD).and_then(id_of)
            && let Some(name) = node.get(ATTACHMENT_FIELD).and_then(Value::as_str)
            && let Some(pose) = attachments.get(&(parent.as_str(), name))
        {
            let affine = pose.affine;
            let attachment = [
                affine.a, affine.b, 0.0, 0.0, affine.c, affine.d, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                affine.x, affine.y, pose.depth, 1.0,
            ];
            matrix = mat4_mul(&matrix, &attachment);
        }
        let (x, y, z) = vec3(node.get(ORIGIN_FIELD), props).unwrap_or_default();
        let (sx, sy, sz) = vec3(node.get(SCALE_FIELD), props).unwrap_or((1.0, 1.0, 1.0));
        let (rx, ry, rz) = vec3(node.get(ANGLES_FIELD), props).unwrap_or_default();
        matrix = mat4_mul(&matrix, &model_matrix([x, y, z], [sx, sy, sz], [rx, ry, rz]));
    }
    matrix
}

fn invalid_numbers(object: &Value, props: &Properties, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        object.get(*key).is_some_and(|value| {
            let value = number(Some(value), props, f32::NAN);
            !value.is_finite() || value < 0.0
        })
    })
}

fn invalid_vectors(object: &Value, props: &Properties, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        object.get(*key).is_some_and(|value| {
            vec3(Some(value), props)
                .is_none_or(|(x, y, z)| !x.is_finite() || !y.is_finite() || !z.is_finite())
        })
    })
}

fn light_rgb(object: &Value, props: &Properties) -> [f32; POSITION_COMPONENTS] {
    let intensity = number(object.get(INTENSITY), props, 0.0);
    let (r, g, b) = vec3(object.get(COLOR_FIELD), props).unwrap_or_default();
    [r * intensity, g * intensity, b * intensity]
}

fn light_color(object: &Value, props: &Properties) -> [f32; COMPONENTS] {
    let [r, g, b] = light_rgb(object, props);
    [r, g, b, number(object.get(RADIUS), props, DEFAULT_RADIUS)]
}

#[cfg(test)]
#[path = "lighting_tests.rs"]
mod tests;
