use super::{GENERAL_FIELD, Properties, number, truthy, vec3};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const MATERIAL: &str = "FOG";
const CHANNELS: usize = 3;
const PARAMETERS: usize = 4;

struct Fields {
    enabled: &'static str,
    color: &'static str,
    values: [&'static str; PARAMETERS],
    defaults: [f32; PARAMETERS],
    combo: &'static str,
    color_uniform: &'static str,
    params_uniform: &'static str,
}

const FIELDS: [Fields; 2] = [
    Fields {
        enabled: "fogdistance",
        color: "fogdistancecolor",
        values: [
            "fogdistancestart",
            "fogdistanceend",
            "fogdistancestartdensity",
            "fogdistanceenddensity",
        ],
        defaults: [1.0, 5.0, 0.0, 1.0],
        combo: "FOG_DIST",
        color_uniform: "g_FogDistanceColor",
        params_uniform: "g_FogDistanceParams",
    },
    Fields {
        enabled: "fogheight",
        color: "fogheightcolor",
        values: ["fogheightstart", "fogheightend", "fogheightstartdensity", "fogheightenddensity"],
        defaults: [1.0, -3.0, 0.0, 1.0],
        combo: "FOG_HEIGHT",
        color_uniform: "g_FogHeightColor",
        params_uniform: "g_FogHeightParams",
    },
];

#[derive(Clone, Copy, Default)]
pub struct Volume {
    pub enabled: [f32; FIELDS.len()],
    pub distance: [f32; PARAMETERS],
    pub height: [f32; PARAMETERS],
}

impl Volume {
    pub fn load(scene: &Value, props: &Properties) -> Self {
        let config = Config::load(scene, props);
        Self {
            enabled: config.bands.map(|band| f32::from(band.enabled)),
            distance: config.bands[0].params,
            height: config.bands[1].params,
        }
    }
}

#[derive(Clone, Copy)]
struct Band {
    enabled: bool,
    color: [f32; CHANNELS],
    params: [f32; PARAMETERS],
}

impl Band {
    fn load(fields: &Fields, general: Option<&Value>, props: &Properties) -> Self {
        let values: [f32; PARAMETERS] = std::array::from_fn(|index| {
            number(
                general.and_then(|value| value.get(fields.values[index])),
                props,
                fields.defaults[index],
            )
        });
        let [start, end, low, high] = values;
        let color = vec3(general.and_then(|value| value.get(fields.color)), props)
            .map_or([0.0; CHANNELS], |(r, g, b)| [r, g, b]);
        Self {
            enabled: truthy(general.and_then(|value| value.get(fields.enabled)), props, false),
            color,
            params: [start, end - start, low, high - low],
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Config {
    bands: [Band; FIELDS.len()],
}

impl Default for Config {
    fn default() -> Self {
        Self::load(&Value::Null, &Properties::new())
    }
}

impl Config {
    pub(crate) fn load(scene: &Value, props: &Properties) -> Self {
        let general = scene.get(GENERAL_FIELD);
        Self { bands: std::array::from_fn(|index| Band::load(&FIELDS[index], general, props)) }
    }

    pub(crate) fn combos(&self, combos: &mut BTreeMap<String, i64>) {
        let material = combos.get(MATERIAL).copied().unwrap_or(0) != 0;
        for (band, fields) in self.bands.iter().zip(&FIELDS) {
            combos.insert(fields.combo.into(), i64::from(material && band.enabled));
        }
    }

    pub(crate) fn uniform(&self, name: &str) -> Option<Vec<f32>> {
        self.bands.iter().zip(&FIELDS).find_map(|(band, fields)| {
            if name == fields.color_uniform {
                Some(band.color.to_vec())
            } else if name == fields.params_uniform {
                Some(band.params.to_vec())
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests;
