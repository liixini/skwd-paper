mod image;
pub(super) mod puppet;
pub use puppet::UV_ROTATION;
pub mod shadow;
mod texture_channels;
pub(crate) use image::Image;
pub use puppet::{
    ALT_MATRICES as PUPPET_ALT_MATRICES, INPUTS as PUPPET_INPUTS, MATRICES as PUPPET_MATRICES,
    NORMAL_MATRICES as PUPPET_NORMAL_MATRICES, Palette, Skin, bone_uniform,
};
pub use texture_channels::Channels;

use super::{
    Assets, CONSTANT_SHADER_VALUES as CONSTANTS, EffectPass, PassBlend, build_material, combos_of,
    texture_shipped, texture_slot,
};
use crate::{model::Texture, pkg::Package};
use serde_json::Value;

const STAGES: [&str; 2] = ["vert", "frag"];
const MATERIAL: &str = "material";
pub(super) const PASSES: &str = "passes";
pub(super) const SHADER: &str = "shader";
const TEXTURES: &str = "textures";
const BLENDING: &str = "blending";
const TRANSLUCENT: &str = "translucent";
const GENERIC: &str = "genericimage";
const GENERIC2: &str = "genericimage2";
const GENERIC3: &str = "genericimage3";
const GENERIC4: &str = "genericimage4";
pub(super) const PASSTHROUGH: &str = "passthrough";
const COMPOSE: &str = "composelayer";
const FLAT: &str = "flat";
const VERSION: &str = "VERSION";
const TEXTURE_FORMAT: &str = "TEX0FORMAT";
const SIMPLE_COMBOS: [&str; 6] = [
    VERSION,
    TEXTURE_FORMAT,
    puppet::UV_TRANSFORM,
    "SKINNING",
    crate::puppet::alpha::COMBO,
    "BONECOUNT",
];
const DEPTH_STATES: [&str; 2] = ["depthtest", "depthwrite"];
const DISABLED: &str = "disabled";
const CULL: &str = "cullmode";
const NO_CULL: &str = "nocull";
const NORMAL_CULL: &str = "normal";
const ENABLED: &str = "enabled";
const LEGACY_CONSTANTS: [&str; 3] = ["Brightness", "Bright", "Alpha"];
const LIGHTING: &str = "LIGHTING";
const PRELIGHTING: &str = "PRELIGHTING";
const REFLECTION: &str = "REFLECTION";
const IDLE_COMBOS: [&str; 2] = [LIGHTING, REFLECTION];
const MAX_BASE_PASSES: usize = 2;

pub(crate) struct Options {
    pub source_texture: bool,
    pub effects: bool,
    pub blend: Option<PassBlend>,
}

pub struct Prelit {
    pub pass: EffectPass,
    pub projection: crate::matrix::Mat4,
}

pub struct Material {
    pub pass: EffectPass,
    pub blend: PassBlend,
    pub skin: Option<Skin>,
    pub prelit: Option<Prelit>,
    pub state: ImageState,
    pub texture_copy: bool,
}

impl Material {
    pub fn copyable(&self) -> bool {
        self.texture_copy && self.skin.is_none()
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum DepthMode {
    Enabled,
    #[default]
    Disabled,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum CullMode {
    #[default]
    None,
    Normal,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageState {
    pub test: DepthMode,
    pub write: DepthMode,
    pub cull: CullMode,
}

impl ImageState {
    fn parse(pass: &Value, path: &str) -> Result<Self, String> {
        let depth = |field: &str| match pass.get(field) {
            None | Some(Value::Null) => Ok(DepthMode::Disabled),
            Some(Value::String(value)) if value == DISABLED => Ok(DepthMode::Disabled),
            Some(Value::String(value)) if value == ENABLED => Ok(DepthMode::Enabled),
            _ => Err(format!("base material {path} {field} unsupported")),
        };
        let cull = match pass.get(CULL) {
            None | Some(Value::Null) => CullMode::None,
            Some(Value::String(value)) if value == NO_CULL => CullMode::None,
            Some(Value::String(value)) if value == NORMAL_CULL => CullMode::Normal,
            _ => return Err(format!("base material {path} culling unsupported")),
        };
        Ok(Self { test: depth(DEPTH_STATES[0])?, write: depth(DEPTH_STATES[1])?, cull })
    }

    pub fn has_depth(self) -> bool {
        self.test == DepthMode::Enabled || self.write == DepthMode::Enabled
    }

    fn has_write(self) -> bool {
        self.write == DepthMode::Enabled
    }
}

pub struct MeshMaterial {
    pub alpha_sampler: Option<usize>,
    pub pass: EffectPass,
    pub blend: PassBlend,
    pub test: DepthMode,
    pub write: DepthMode,
    pub cull: CullMode,
}

fn engine_input(bind: &super::EffectBind) -> bool {
    matches!(
        bind,
        super::EffectBind::Reflection
            | super::EffectBind::ShadowAtlas
            | super::EffectBind::LightCookie
            | super::EffectBind::SceneHistory
            | super::EffectBind::LayerComposite { .. }
            | super::EffectBind::SceneSoFar
    )
}

pub(crate) fn load_mesh(
    pkg: &Package,
    assets: &Assets,
    path: &str,
    bones: Option<usize>,
) -> Result<MeshMaterial, String> {
    let mut material =
        json(pkg, assets, path).ok_or_else(|| format!("missing mesh material {path}"))?;
    if let Some(bones) = bones {
        let passes = material
            .get_mut(PASSES)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| format!("mesh material {path} has no passes"))?;
        for pass in passes {
            let mut combos = combos_of(pass.get(super::COMBOS_FIELD));
            combos.insert(puppet::SKINNING.into(), 1);
            combos.insert(puppet::BONE_COUNT.into(), bones as i64);
            pass[super::COMBOS_FIELD] =
                serde_json::to_value(combos).map_err(|err| err.to_string())?;
        }
    }
    if let Some(slots) = material
        .get_mut(PASSES)
        .and_then(Value::as_array_mut)
        .and_then(|passes| passes.first_mut())
        .and_then(|pass| pass.get_mut(TEXTURES))
        .and_then(Value::as_array_mut)
    {
        for slot in slots {
            if super::texture_name(slot).is_some_and(|name| !texture_shipped(pkg, assets, name)) {
                *slot = Value::Null;
            }
        }
    }
    let passes =
        material[PASSES].as_array().ok_or_else(|| format!("mesh material {path} has no passes"))?;
    if passes.len() != 1 {
        return Err(format!("mesh material {path} multipass unsupported"));
    }
    let authored = &passes[0];
    let state = |field: &str| match authored.get(field).and_then(Value::as_str) {
        None | Some(ENABLED) => Ok(DepthMode::Enabled),
        Some(DISABLED) => Ok(DepthMode::Disabled),
        _ => Err(format!("mesh material {path} {field} unsupported")),
    };
    let test = state(DEPTH_STATES[0])?;
    let write = state(DEPTH_STATES[1])?;
    let cull = match authored.get(CULL).and_then(Value::as_str) {
        Some(NO_CULL) => CullMode::None,
        None | Some(NORMAL_CULL) => CullMode::Normal,
        _ => return Err(format!("mesh material {path} culling unsupported")),
    };
    let blend = PassBlend::parse(authored[BLENDING].as_str())
        .ok_or_else(|| format!("mesh material {path} blending unsupported"))?;
    let mut pass = build_material(pkg, assets, &material, None, None, None, None)
        .and_then(|mut passes| passes.pop())
        .ok_or_else(|| format!("mesh material {path} shader unavailable"))?;
    if let Some(name) = authored[TEXTURES]
        .as_array()
        .and_then(|slots| slots.first())
        .and_then(super::texture_name)
        .filter(|name| name.starts_with(super::RENDER_TARGET_PREFIX))
        && pass.binds.iter().all(|(slot, _)| *slot != 0)
    {
        pass.binds.push((0, super::EffectBind::parse(name, None)));
    }
    pass.filter_binds();
    if pass.binds.iter().any(|(_, bind)| !engine_input(bind)) {
        return Err(format!("mesh material {path} render target inputs unsupported"));
    }
    let albedo = authored[TEXTURES]
        .as_array()
        .and_then(|slots| slots.first())
        .and_then(super::texture_name)
        .or_else(|| {
            pass.fragment
                .samplers
                .iter()
                .chain(&pass.vertex.samplers)
                .find(|sampler| sampler.index == 0)
                .and_then(|sampler| sampler.default.as_deref())
        });
    if let Some(texture) = albedo.and_then(|name| texture_slot(pkg, assets, Some(name))) {
        if pass.textures.is_empty() {
            pass.textures.push(Some(texture));
        } else {
            pass.textures[0] = Some(texture);
        }
    }
    if pass.vertex.samplers.iter().chain(&pass.fragment.samplers).any(|sampler| {
        let slot = sampler.index as usize;
        !pass.binds.iter().any(|(bound, bind)| *bound == slot && engine_input(bind))
            && pass.textures.get(slot).and_then(Option::as_ref).is_none()
    }) {
        return Err(format!("mesh material {path} sampler texture missing"));
    }
    pass.target = None;
    let alpha_sampler = shadow::albedo(&pass).map(|sampler| sampler.index as usize);
    Ok(MeshMaterial { alpha_sampler, pass, blend, test, write, cull })
}

fn json(pkg: &Package, assets: &Assets, path: &str) -> Option<Value> {
    pkg.find_json(path)
        .ok()
        .flatten()
        .or_else(|| assets.read(path).and_then(|source| crate::json::parse(source.as_bytes()).ok()))
}

fn material_passes<'a>(material: &'a Value, path: &str) -> Result<&'a [Value], String> {
    let passes = material
        .get(PASSES)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("base material {path} has no passes"))?;
    if passes.len() > MAX_BASE_PASSES {
        return Err(format!("base material {path} has too many passes"));
    }
    if passes.is_empty() {
        return Err(format!("base material {path} has no passes"));
    }
    for pass in passes {
        if !pass.is_object() {
            return Err(format!("base material {path} pass is not an object"));
        }
    }
    Ok(passes)
}

pub(crate) fn scene_input(material: &Value, own_id: Option<&str>) -> Option<super::EffectBind> {
    let source = material[PASSES].as_array()?.first()?[TEXTURES].as_array()?.first()?.as_str()?;
    let binding = super::EffectBind::parse(source, own_id);
    (bound_input(&binding)
        || matches!(binding, super::EffectBind::SceneHistory | super::EffectBind::Reflection))
    .then_some(binding)
}

fn bound_input(bind: &super::EffectBind) -> bool {
    matches!(bind, super::EffectBind::SceneSoFar | super::EffectBind::LayerComposite { .. })
}

fn cpu_skinned(mesh: &crate::puppet::Mesh) -> bool {
    mesh.morph.as_ref().is_none_or(|morph| !morph.has_modifiers())
        && mesh.texture_channels.is_none()
}

fn simple(pkg: &Package, model: &Value, pass: &Value, idle_lighting: bool) -> bool {
    let shader = pass.get(SHADER).map_or(Some(GENERIC4), Value::as_str);
    let Some(shader) = shader else { return false };
    let composition =
        model[PASSTHROUGH].as_bool() == Some(true) && matches!(shader, PASSTHROUGH | COMPOSE);
    if pass
        .get(super::USER_SHADER_VALUES)
        .and_then(Value::as_object)
        .is_some_and(|bindings| !bindings.is_empty())
        || !(composition
            || matches!(shader, GENERIC | GENERIC2 | GENERIC4 | FLAT)
            || (idle_lighting && shader == GENERIC3))
        || STAGES.iter().any(|stage| pkg.find(&format!("shaders/{shader}.{stage}")).is_some())
    {
        return false;
    }
    let combos = combos_of(pass.get(super::COMBOS_FIELD));
    let constants = pass.get(CONSTANTS).and_then(Value::as_object);
    let legacy = shader == GENERIC || (shader == GENERIC2 && !combos.contains_key(VERSION));
    combos.iter().all(|(key, value)| {
        SIMPLE_COMBOS.contains(&key.as_str())
            || (idle_lighting && *value == 0 && IDLE_COMBOS.contains(&key.as_str()))
    }) && pass.get(TEXTURES).and_then(Value::as_array).is_none_or(|slots| slots.len() <= 1)
        && pass.get(BLENDING).and_then(Value::as_str).is_none_or(|blend| blend == TRANSLUCENT)
        && constants.is_none_or(|values| {
            values.iter().all(|(key, value)| {
                legacy && LEGACY_CONSTANTS.contains(&key.as_str()) && value.is_number()
            })
        })
}

#[expect(
    clippy::too_many_lines,
    reason = "Authored image material pass validation and compilation"
)]
pub(crate) fn load(
    pkg: &Package,
    assets: &Assets,
    object: &Value,
    texture: &Texture,
    mut puppet: Option<&mut crate::model::Puppet>,
    options: &Options,
) -> Result<Option<Material>, String> {
    let Options { source_texture, effects, blend } = *options;
    let Some(Image { model, material, path }) = Image::load(pkg, assets, object)? else {
        return Ok(None);
    };
    let path = path.as_str();
    let first = material
        .get(PASSES)
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .ok_or_else(|| format!("base material {path} has no passes"))?;
    if !first.is_object() {
        return Err(format!("base material {path} pass is not an object"));
    }
    let state = ImageState::parse(first, path)?;
    let own_id = object.get(crate::model::ID_FIELD).and_then(crate::model::id_of);
    let scene_input = first
        .get(TEXTURES)
        .and_then(Value::as_array)
        .and_then(|slots| slots.first())
        .and_then(Value::as_str)
        .map(|name| super::EffectBind::parse(name, own_id.as_deref()));
    let plain = !scene_input.as_ref().is_some_and(bound_input)
        || model[PASSTHROUGH].as_bool() == Some(true);
    let generic = simple(pkg, &model, first, true);
    let texture_copy = generic && first[SHADER].as_str() != Some(FLAT);
    if let Some(puppet) = puppet.as_deref_mut() {
        puppet.cpu = plain && cpu_skinned(&puppet.mesh) && generic && !state.has_depth();
        puppet.cull = state.cull == CullMode::Normal;
    }
    if plain
        && puppet.as_ref().is_none_or(|puppet| {
            !puppet.mesh.alpha
                && puppet.mesh.morph.is_none()
                && puppet.mesh.texture_channels.is_none()
        })
        && simple(pkg, &model, first, false)
        && (state == ImageState::default()
            || (effects && !state.has_write() && model[PASSTHROUGH].as_bool() == Some(true)))
    {
        return Ok(None);
    }
    let frame = puppet
        .as_deref()
        .map(|puppet| crate::puppet::authored(&puppet.mesh, &puppet.layers, Some(0.0)))
        .transpose()
        .map_err(|error| error.to_string())?;
    let authored =
        PassBlend::parse(Some(first.get(BLENDING).and_then(Value::as_str).unwrap_or(TRANSLUCENT)))
            .ok_or_else(|| format!("base material {path} blending unsupported"))?;
    let blend = blend.unwrap_or(authored);
    let mut first = first.clone();
    let mut combos = combos_of(first.get(super::COMBOS_FIELD));
    if let Some(frame) = &frame {
        puppet::combos(
            &mut combos,
            frame.bones.len(),
            puppet.as_ref().is_some_and(|puppet| puppet.mesh.morph.is_some()),
        )?;
        if puppet.as_ref().is_some_and(|puppet| puppet.mesh.morph.is_some()) {
            combos.insert(crate::puppet::morph::COMBO.into(), 1);
        }
        if puppet
            .as_ref()
            .and_then(|puppet| puppet.mesh.morph.as_ref())
            .is_some_and(crate::puppet::morph::Morph::has_modifiers)
        {
            combos.insert(crate::puppet::morph::MODIFIER_COMBO.into(), 1);
        }
    }
    let build = PassBuild {
        pkg,
        assets,
        texture,
        path,
        bones: frame.as_ref().map(|frame| frame.bones.len()),
        morph: puppet.as_ref().and_then(|puppet| puppet.mesh.morph.as_ref()),
        blend,
        scene_input,
        source_texture,
    };
    let baked = effects && IDLE_COMBOS.iter().any(|key| combos.get(*key) == Some(&1));
    let mut prelit = None;
    if baked {
        let mut lit = combos.clone();
        lit.insert(PRELIGHTING.into(), 1);
        if let (Some(frame), Some(puppet)) = (&frame, puppet.as_deref()) {
            let pass = build.pass(&first, lit)?;
            puppet::validate(&pass, texture, frame.bones.len())?;
            prelit = Some(Prelit {
                pass,
                projection: puppet::prelit_projection(&puppet.mesh.vertices)?,
            });
            combos.extend(IDLE_COMBOS.map(|key| (key.to_string(), 0)));
            first[TEXTURES].as_array_mut().into_iter().for_each(|slots| slots.truncate(1));
        } else {
            combos = lit;
        }
    }
    if puppet.as_ref().is_some_and(|puppet| puppet.mesh.alpha) {
        combos.insert(crate::puppet::alpha::COMBO.into(), 1);
    }
    let pass = build.pass(&first, combos)?;
    if let Some(frame) = &frame {
        puppet::validate(&pass, texture, frame.bones.len())?;
    }
    let skin = match (puppet, frame) {
        (Some(puppet), Some(frame)) => {
            let channels = texture_channels::load(pkg, assets, &mut puppet.mesh)?;
            let mut skin = puppet::admit(puppet, frame, &pass, texture)?;
            skin.channels = channels;
            Some(skin)
        }
        _ => None,
    };
    Ok(Some(Material { pass, blend, skin, prelit, state, texture_copy }))
}

pub(crate) fn depth_tested(pkg: &Package, assets: &Assets, object: &Value) -> bool {
    Image::load(pkg, assets, object).ok().flatten().is_some_and(|image| {
        image
            .material
            .get(PASSES)
            .and_then(Value::as_array)
            .and_then(|passes| passes.first())
            .is_some_and(|first| {
                ImageState::parse(first, &image.path)
                    .is_ok_and(|state| state.test == DepthMode::Enabled)
            })
    })
}

struct PassBuild<'a> {
    pkg: &'a Package,
    assets: &'a Assets,
    texture: &'a Texture,
    path: &'a str,
    bones: Option<usize>,
    morph: Option<&'a crate::puppet::morph::Morph>,
    blend: PassBlend,
    scene_input: Option<super::EffectBind>,
    source_texture: bool,
}

impl PassBuild<'_> {
    fn pass(
        &self,
        first: &Value,
        mut combos: std::collections::BTreeMap<String, i64>,
    ) -> Result<EffectPass, String> {
        let path = self.path;
        combos.insert(TEXTURE_FORMAT.into(), self.texture.we_format());
        let mut first = first.clone();
        first[super::COMBOS_FIELD] = serde_json::to_value(combos).expect("shader combos");
        let material = serde_json::json!({(PASSES):[first]});
        let mut pass = build_material(
            self.pkg,
            self.assets,
            &material,
            None,
            None,
            None,
            Some(super::MaterialInput {
                bones: self.bones,
                component_mask: self.texture.component_mask,
            }),
        )
        .and_then(|mut passes| passes.pop())
        .ok_or_else(|| format!("base material {path} shader unavailable"))?;
        if let Some(morph) = self.morph {
            pass.textures.resize_with(crate::puppet::morph::TEXTURE_SLOT + 1, || None);
            pass.textures[crate::puppet::morph::TEXTURE_SLOT] =
                Some(morph.texture().map_err(|error| error.to_string())?);
        }
        pass.blend = self.blend;
        if let Some(input) = self.scene_input.as_ref().filter(|input| bound_input(input)) {
            pass.binds.push((0, input.clone()));
        }
        pass.filter_binds();
        if pass.binds.iter().any(|(_, binding)| match binding {
            super::EffectBind::SceneHistory
            | super::EffectBind::Reflection
            | super::EffectBind::ShadowAtlas
            | super::EffectBind::LightCookie => false,
            super::EffectBind::SceneSoFar | super::EffectBind::LayerComposite { .. } => {
                self.source_texture
            }
            _ => true,
        }) {
            return Err(format!("base material {path} render target inputs unsupported"));
        }
        if !self.source_texture
            && pass.vertex.samplers.iter().chain(&pass.fragment.samplers).any(|sampler| {
                let slot = sampler.index as usize;
                !pass.binds.iter().any(|(bound, _)| *bound == slot)
                    && pass.textures.get(slot).and_then(Option::as_ref).is_none()
            })
        {
            return Err(format!("base material {path} sampler texture missing"));
        }
        Ok(pass)
    }
}

#[cfg(test)]
#[path = "material_tests.rs"]
mod tests;
