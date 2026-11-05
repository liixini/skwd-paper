use super::{Properties, number, truthy, vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) const EFFECT_FILE: &str = "effects/skwd/bloom_hdr.json";
const CONFIG_FIELD: &str = "hdr";
const BLOOM_FIELD: &str = "bloom";
const PROJECTION_FIELD: &str = "orthogonalprojection";
const POSTPROCESSING_FIELD: &str = "postprocessing";
const ULTRA_POSTPROCESSING: &str = "ultra";
const DISPLAY_HDR_POSTPROCESSING: &str = "displayhdr";
const STRENGTH_FIELD: &str = "bloomhdrstrength";
const THRESHOLD_FIELD: &str = "bloomhdrthreshold";
const FEATHER_FIELD: &str = "bloomhdrfeather";
const SCATTER_FIELD: &str = "bloomhdrscatter";
const ITERATIONS_FIELD: &str = "bloomhdriterations";
const TINT_FIELD: &str = "bloomtint";
pub(crate) const BRIGHTNESS_FIELD: &str = "brightness";
const RENDER_VAR: &str = "g_RenderVar0";
const FULL_FRAME: &str = "_rt_FullFrameBuffer";
const FBO_FORMAT: &str = "rgba16f";
const DOWNSAMPLE: &str = "materials/util/hdr_downsample.json";
const BLOOM_DOWNSAMPLE: &str = "materials/util/hdr_downsample_bloom.json";
const UPSAMPLE: &str = "materials/util/hdr_upsample.json";
const CUBIC_UPSAMPLE: &str = "materials/util/hdr_upsample_cubic.json";
const COMBINE: &str = "materials/util/combine_hdr_upsample.json";
const LEVELS: [&str; 8] = [
    "_rt_Hdr2",
    "_rt_Hdr4",
    "_rt_Hdr8",
    "_rt_Hdr16",
    "_rt_Hdr32",
    "_rt_Hdr64",
    "_rt_Hdr128",
    "_rt_Hdr256",
];
const KNEE_EPSILON: f32 = 1e-5;
const DEFAULT_SCATTER: f32 = f32::from_bits(0x3fcf_3b64);
const STRENGTH_UNIFORM: &str = "bloomstrength";
const BLEND_UNIFORM: &str = "blend";
const SCATTER_UNIFORM: &str = "scatter";

#[derive(Serialize)]
struct Entry {
    file: &'static str,
    hdr: Bloom,
    passes: Vec<Override>,
}

#[derive(Serialize)]
struct Plan {
    fbos: Vec<Target>,
    passes: Vec<Pass>,
}

#[derive(Serialize)]
struct Target {
    name: &'static str,
    scale: u32,
    format: &'static str,
}

#[derive(Serialize)]
struct Pass {
    material: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'static str>,
    bind: Vec<Bind>,
}

#[derive(Serialize)]
struct Bind {
    index: usize,
    name: &'static str,
}

#[derive(Serialize)]
struct Override {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    constantshadervalues: BTreeMap<&'static str, Value>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Bloom {
    strength: f32,
    threshold: f32,
    feather: f32,
    scatter: f32,
    iterations: usize,
    tint: [f32; 3],
    canvas: [u32; 2],
}

impl Default for Bloom {
    fn default() -> Self {
        Self {
            strength: 2.0,
            threshold: 1.0,
            feather: 0.1,
            scatter: DEFAULT_SCATTER,
            iterations: LEVELS.len(),
            tint: [1.0; 3],
            canvas: [1; 2],
        }
    }
}

pub(crate) fn enabled(general: Option<&Value>, props: &Properties) -> bool {
    truthy(general.and_then(|value| value.get(CONFIG_FIELD)), props, false)
        && truthy(general.and_then(|value| value.get(BLOOM_FIELD)), props, false)
        && general
            .and_then(|value| value.get(PROJECTION_FIELD))
            .and_then(|value| value.get(POSTPROCESSING_FIELD))
            .and_then(Value::as_str)
            .is_some_and(|mode| matches!(mode, ULTRA_POSTPROCESSING | DISPLAY_HDR_POSTPROCESSING))
}

pub(crate) fn bloom_entry(
    general: Option<&Value>,
    props: &Properties,
    canvas: (f32, f32),
) -> Option<Value> {
    if !enabled(general, props) {
        return None;
    }
    let defaults = Bloom::default();
    let read = |key, fallback| number(general.and_then(|value| value.get(key)), props, fallback);
    let dimensions = [canvas.0.max(1.0) as u32, canvas.1.max(1.0) as u32];
    let available =
        (u32::BITS - dimensions[0].min(dimensions[1]).leading_zeros() - 1).max(1) as usize;
    let bloom = Bloom {
        strength: read(STRENGTH_FIELD, defaults.strength),
        threshold: read(THRESHOLD_FIELD, defaults.threshold),
        feather: read(FEATHER_FIELD, defaults.feather),
        scatter: read(SCATTER_FIELD, defaults.scatter),
        iterations: (read(ITERATIONS_FIELD, defaults.iterations as f32) as usize)
            .clamp(1, available.min(LEVELS.len())),
        tint: vec3(general.and_then(|value| value.get(TINT_FIELD)), props)
            .map_or(defaults.tint, |(r, g, b)| [r, g, b]),
        canvas: dimensions,
    };
    if bloom.iterations == 1 {
        return None;
    }
    let (_, passes) = bloom.plan();
    serde_json::to_value(Entry { file: EFFECT_FILE, hdr: bloom, passes }).ok()
}

pub(crate) fn definition(file: &str, entry: &Value) -> Option<Value> {
    if file != EFFECT_FILE {
        return None;
    }
    let bloom: Bloom = serde_json::from_value(entry.get(CONFIG_FIELD)?.clone()).ok()?;
    if !(1..=LEVELS.len()).contains(&bloom.iterations) || bloom.canvas.contains(&0) {
        return None;
    }
    serde_json::to_value(bloom.plan().0).ok()
}

pub(crate) fn configure_targets(file: &str, targets: &mut [crate::effects::Fbo]) {
    if file != EFFECT_FILE {
        return;
    }
    for target in targets {
        if let crate::effects::FboSize::Scale(scale) = target.size {
            target.size = crate::effects::FboSize::ScaleFloor(scale);
        }
    }
}

pub(crate) fn configure_pass(
    file: &str,
    entry: &Value,
    index: usize,
    pass: &mut crate::effects::EffectPass,
) {
    if file != EFFECT_FILE {
        return;
    }
    if let Ok(offset) = serde_json::from_value::<[f32; 4]>(
        entry[super::EFFECT_PASSES_FIELD][index][crate::effects::CONSTANT_SHADER_VALUES]
            [RENDER_VAR]
            .clone(),
    ) {
        pass.values.insert(RENDER_VAR.to_owned(), offset.to_vec());
    }
}

pub(super) fn resize(effects: &mut [crate::effects::Effect], canvas: [u32; 2]) {
    use crate::effects::EffectBind;
    for pass in effects.iter_mut().flat_map(|effect| &mut effect.passes) {
        let Some(target) = pass
            .target
            .as_deref()
            .and_then(|target| LEVELS.iter().position(|level| *level == target))
        else {
            continue;
        };
        let scale = pass.binds.iter().find_map(|(slot, binding)| {
            if *slot != 0 {
                return None;
            }
            match binding {
                EffectBind::SceneSoFar => Some(1),
                EffectBind::Named(name) => {
                    LEVELS.iter().position(|level| *level == name).map(|index| 1 << (index + 1))
                }
                _ => None,
            }
        });
        if let Some(scale) = scale {
            let scale = scale.min(1 << (target + 1));
            pass.values.insert(RENDER_VAR.to_owned(), sample_offset(scale, canvas).to_vec());
        }
    }
}

fn sample_offset(scale: u32, canvas: [u32; 2]) -> [f32; 4] {
    let x = scale as f32 / canvas[0] as f32;
    let y = scale as f32 / canvas[1] as f32;
    [x, y, -x, -y]
}

impl Bloom {
    fn plan(&self) -> (Plan, Vec<Override>) {
        let mut passes = Vec::new();
        let mut overrides = Vec::new();
        let mut fbos = Vec::new();
        let knee = self.threshold * self.feather;
        let strength =
            self.strength / (self.scatter.powi(self.iterations.saturating_sub(2) as i32) + 1.0);
        for (index, target) in LEVELS.iter().enumerate().take(self.iterations) {
            let scale = 1u32 << (index + 1);
            fbos.push(Target { name: target, scale, format: FBO_FORMAT });
            let source = if index == 0 { FULL_FRAME } else { LEVELS[index - 1] };
            passes.push(Pass {
                material: if index == 0 { BLOOM_DOWNSAMPLE } else { DOWNSAMPLE },
                target: Some(target),
                bind: vec![Bind { index: 0, name: source }],
            });
            let mut values = self.offset(scale / 2);
            if index == 0 {
                values.insert(STRENGTH_UNIFORM, json!(strength));
                values.insert(
                    BLEND_UNIFORM,
                    json!([
                        self.threshold,
                        self.threshold - knee,
                        knee * 2.0,
                        0.5 * 0.5 / (knee + KNEE_EPSILON)
                    ]),
                );
                values.insert(TINT_FIELD, json!(self.tint));
            }
            overrides.push(Override { constantshadervalues: values });
        }
        for index in (1..self.iterations).rev() {
            passes.push(Pass {
                material: if index >= self.iterations.saturating_sub(2) {
                    CUBIC_UPSAMPLE
                } else {
                    UPSAMPLE
                },
                target: Some(LEVELS[index - 1]),
                bind: vec![Bind { index: 0, name: LEVELS[index] }],
            });
            let mut values = self.offset(1 << index);
            values.insert(SCATTER_UNIFORM, json!(self.scatter));
            overrides.push(Override { constantshadervalues: values });
        }
        passes.push(Pass {
            material: COMBINE,
            target: None,
            bind: vec![Bind { index: 0, name: FULL_FRAME }, Bind { index: 1, name: LEVELS[0] }],
        });
        overrides.push(Override {
            constantshadervalues: BTreeMap::from([(RENDER_VAR, json!([1.0, 0.0, 0.0, 0.0]))]),
        });
        (Plan { fbos, passes }, overrides)
    }

    fn offset(&self, scale: u32) -> BTreeMap<&'static str, Value> {
        BTreeMap::from([(RENDER_VAR, json!(sample_offset(scale, self.canvas)))])
    }
}

#[cfg(test)]
#[path = "hdr_tests.rs"]
mod tests;
