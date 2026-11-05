use super::{
    Attachments, DEFAULT_EXPONENT, EXPONENT, HashMap, ID_FIELD, LIGHT, OBJECTS_FIELD, POINT,
    Properties, SHADOW, VOLUMETRICS, Value, cascade, directional, id_of, light_color, number,
    shadow, spot, truthy,
};

const COOKIE: &str = "cookie";
const USE_COOKIE: &str = "usecookie";
const COOKIE_DEFAULT: &str = "cookie/flashlight1";
const SOURCE_SIZE: &str = "lightsourcesize";
const DENSITY: &str = "density";
const VOLUME_EXPONENT: &str = "volumetricsexponent";
const DEFAULT_DENSITY: f32 = 1.0;
const DEFAULT_VOLUME_EXPONENT: f32 = 1.0;
const TRANSLATION: usize = 12;
const AXES: usize = 3;
const COLUMN: usize = 4;
const INVALID: &str = "invalid volumetric light values";

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub id: String,
    pub origin: [f32; AXES],
    pub forward: [f32; AXES],
    pub right: [f32; AXES],
    pub up: [f32; AXES],
    pub(super) shadow_axes: [[f32; AXES]; AXES],
    pub color: [f32; AXES],
    pub radius: f32,
    pub source_size: f32,
    pub exponent: f32,
    pub cone: Option<[f32; 2]>,
    pub sun: Option<cascade::Sun>,
    pub shadow: bool,
    pub cookie: Option<String>,
    pub visible: bool,
    pub volume: Option<Volume>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    pub density: f32,
    pub exponent: f32,
}

pub(super) fn validate(object: &Value, props: &Properties) -> Option<&'static str> {
    if !truthy(object.get(VOLUMETRICS), props, false) {
        return None;
    }
    [DENSITY, VOLUME_EXPONENT]
        .iter()
        .any(|key| {
            object.get(*key).is_some_and(|value| {
                let value = number(Some(value), props, f32::NAN);
                !value.is_finite() || value < 0.0
            })
        })
        .then_some(INVALID)
}

pub fn lights(scene: &Value, props: &Properties, attachments: &Attachments<'_>) -> Vec<Light> {
    let objects = scene[OBJECTS_FIELD].as_array().map_or(&[][..], Vec::as_slice);
    let by_id: HashMap<_, _> = objects
        .iter()
        .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
        .collect();
    let budget = cascade::budget(scene);
    let mut cascaded = 0;
    objects
        .iter()
        .filter(|object| match object[LIGHT].as_str() {
            Some(POINT | spot::KIND) => true,
            Some(directional::KIND) => budget > 0,
            _ => false,
        })
        .filter_map(|object| {
            let matrix = super::transform(object, &by_id, props, attachments);
            let is_sun = object[LIGHT].as_str() == Some(directional::KIND);
            let visible = crate::model::object_visible(object, &by_id, props);
            let shadow = truthy(object.get(SHADOW), props, false);
            let sun = is_sun.then(|| {
                let mut sun = cascade::Sun::new(object, props, &matrix);
                if shadow && visible && cascaded < budget && sun.usable(&matrix) {
                    sun.slot = Some(cascaded);
                    cascaded += 1;
                }
                sun
            });
            let [red, green, blue, radius] = light_color(object, props);
            let forward: [f32; AXES] = matrix[..AXES].try_into().unwrap();
            let length = forward.iter().map(|value| value * value).sum::<f32>().sqrt();
            let forward = if length > f32::EPSILON {
                forward.map(|value| value / length)
            } else {
                [1.0, 0.0, 0.0]
            };
            Some(Light {
                id: object.get(ID_FIELD).and_then(id_of)?,
                origin: matrix[TRANSLATION..TRANSLATION + AXES].try_into().unwrap(),
                forward,
                right: unit(matrix[COLUMN..COLUMN + AXES].try_into().unwrap()),
                up: unit(matrix[COLUMN * 2..COLUMN * 2 + AXES].try_into().unwrap()),
                shadow_axes: if object[LIGHT].as_str() == Some(POINT) {
                    [[0.0; AXES]; AXES]
                } else {
                    shadow::spot_axes(&matrix)
                },
                color: [red, green, blue],
                radius,
                source_size: number(object.get(SOURCE_SIZE), props, 0.0),
                exponent: number(object.get(EXPONENT), props, DEFAULT_EXPONENT),
                cone: (object[LIGHT].as_str() == Some(spot::KIND)).then(|| {
                    [
                        number(object.get(spot::INNER), props, spot::INNER_DEFAULT),
                        number(object.get(spot::OUTER), props, spot::OUTER_DEFAULT),
                    ]
                    .map(|degrees| degrees.to_radians().cos())
                }),
                sun,
                shadow,
                cookie: (object[LIGHT].as_str() == Some(spot::KIND)
                    && truthy(object.get(USE_COOKIE), props, false))
                .then(|| {
                    object
                        .get(COOKIE)
                        .and_then(Value::as_str)
                        .filter(|path| !path.is_empty())
                        .unwrap_or(COOKIE_DEFAULT)
                        .to_owned()
                }),
                visible,
                volume: (!is_sun && truthy(object.get(VOLUMETRICS), props, false)).then(|| {
                    Volume {
                        density: number(object.get(DENSITY), props, DEFAULT_DENSITY),
                        exponent: number(
                            object.get(VOLUME_EXPONENT),
                            props,
                            DEFAULT_VOLUME_EXPONENT,
                        ),
                    }
                }),
            })
        })
        .collect()
}

fn unit(vector: [f32; AXES]) -> [f32; AXES] {
    let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length > f32::EPSILON { vector.map(|value| value / length) } else { vector }
}
