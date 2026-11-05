use super::{
    EFFECT_FILE_FIELD, EFFECT_PASSES_FIELD, EFFECTS_FIELD, ID_FIELD, Layer, LayerKind, NAME_FIELD,
    Properties, hdr, number, solid_texture, truthy, vec3,
};
use crate::effects::{Assets, CONSTANT_SHADER_VALUES as CONSTANT_VALUES_FIELD, EffectBind};
use crate::pkg::Package;
use serde_json::Value;

pub const ID: &str = "@bloom";
pub const ENABLED: &str = "bloom";
pub const STRENGTH: &str = "bloomstrength";
pub const THRESHOLD: &str = "bloomthreshold";
pub const TINT: &str = "bloomtint";
const DEFAULT_STRENGTH: f32 = 2.0;
const DEFAULT_THRESHOLD: f32 = 0.65;
const RGB_CHANNELS: usize = 3;
const UNAVAILABLE: &str = "bloom: engine bloom materials unavailable";

#[derive(Clone, Copy)]
pub struct View {
    pub canvas: (f32, f32),
    pub hdr: bool,
    pub order: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct Source<'a> {
    pub package: &'a Package,
    pub assets: &'a Assets,
    pub general: Option<&'a Value>,
    pub view: View,
}

pub fn enabled(general: Option<&Value>, props: &Properties) -> bool {
    truthy(general.and_then(|top| top.get(ENABLED)), props, false)
}

fn object(general: Option<&Value>, props: &Properties, canvas: (f32, f32)) -> Value {
    let strength = number(general.and_then(|top| top.get(STRENGTH)), props, DEFAULT_STRENGTH);
    let threshold = number(general.and_then(|top| top.get(THRESHOLD)), props, DEFAULT_THRESHOLD);
    let tint = vec3(general.and_then(|top| top.get(TINT)), props).unwrap_or((1.0, 1.0, 1.0));
    let mut object = serde_json::json!({
        (ID_FIELD): ID,
        (NAME_FIELD): ENABLED,
        (EFFECTS_FIELD): [{
            (EFFECT_FILE_FIELD): crate::effects::BLOOM_LDR_EFFECT_FILE,
            (EFFECT_PASSES_FIELD): [{(CONSTANT_VALUES_FIELD): {
                STRENGTH: strength,
                THRESHOLD: threshold,
                TINT: format!("{} {} {}", tint.0, tint.1, tint.2)
            }}]
        }]
    });
    if let Some(entry) = hdr::bloom_entry(general, props, canvas) {
        object[crate::effects::EFFECTS_FIELD] = serde_json::json!([entry]);
    }
    object
}

pub(crate) fn load(source: Source<'_>) -> (Option<Layer>, Vec<String>) {
    let Source { package, assets, general, view } = source;
    if !enabled(general, &assets.properties) {
        return (None, Vec::new());
    }
    let object = object(general, &assets.properties, view.canvas);
    let (mut effects, skipped) = crate::effects::load_effects(package, assets, &object, false);
    let mut skipped: Vec<_> =
        skipped.into_iter().map(|reason| format!("bloom: {reason}")).collect();
    for (_, binding) in
        effects.iter_mut().flat_map(|effect| &mut effect.passes).flat_map(|pass| &mut pass.binds)
    {
        if *binding == EffectBind::Previous {
            *binding = EffectBind::SceneSoFar;
        }
    }
    if !effects.iter().any(|effect| !effect.passes.is_empty()) {
        skipped.push(UNAVAILABLE.into());
        return (None, skipped);
    }
    (
        Some(Layer {
            texture_name: None,
            material: None,
            id: ID.into(),
            name: ENABLED.into(),
            visible: true,
            live_text: None,
            script_text: None,
            kind: LayerKind::Viewport { hdr: view.hdr },
            mouse: crate::mouse::LayerMouse::default(),
            texture: solid_texture(),
            puppet: None,
            geometry: None,
            center: (view.canvas.0 * 0.5, view.canvas.1 * 0.5),
            size: view.canvas,
            composition_size: None,
            scale: (1.0, 1.0),
            depth: 0.0,
            scene_order: view.order,
            alpha: 1.0,
            angle: 0.0,
            color: [1.0; RGB_CHANNELS],
            color_blend: 0,
            pad_uv: false,
            spritesheet: false,
            passthrough: true,
            fullscreen: false,
            copy_background: true,
            compose_depth: false,
            solid: false,
            base_scene_input: false,
            effects,
        }),
        skipped,
    )
}
