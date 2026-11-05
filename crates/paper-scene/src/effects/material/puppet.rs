use crate::effects::{EffectPass, material::Texture};
use crate::model::Puppet;
use crate::puppet::{BonePose, Frame};
use crate::shader::UniformKind;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const INPUTS: [(&str, &str); 7] = [
    ("a_Position", "vec3"),
    ("a_Normal", "vec3"),
    ("a_Tangent4", "vec4"),
    ("a_BlendIndices", "uvec4"),
    ("a_BlendWeights", "vec4"),
    ("a_TexCoord", "vec2"),
    (crate::puppet::morph::POSITION, "vec4"),
];

pub(crate) const SKINNING: &str = "SKINNING";
pub(crate) const BONE_COUNT: &str = "BONECOUNT";
const BONE_PREFIX: &str = "g_Bones";
pub const UV_TRANSFORM: &str = "SPRITESHEET";
pub const UV_ROTATION: &str = "g_Texture0Rotation";
const BONES: [&str; 3] = ["g_Bones[1]", "g_Bones[2]", "g_Bones[BONECOUNT]"];
const REQUIRED_INPUTS: [(&str, &str); 3] = [INPUTS[0], INPUTS[3], INPUTS[4]];
pub const MATRICES: [&str; 3] =
    ["g_ModelMatrix", "g_ModelViewProjectionMatrix", "g_ViewProjectionMatrix"];
pub const NORMAL_MATRICES: [&str; 2] = ["g_NormalModelMatrix", "g_AltNormalModelMatrix"];
pub const ALT_MATRICES: [&str; 2] = ["g_AltModelMatrix", "g_AltViewProjectionMatrix"];
const LIT_COMBOS: [&str; 9] = [
    super::LIGHTING,
    super::REFLECTION,
    super::PRELIGHTING,
    "NORMALMAP",
    "PBRMASKS",
    "METALLIC_MAP",
    "ROUGHNESS_MAP",
    "REFLECTION_MAP",
    "EMISSIVE_MAP",
];
const TEXTURE_FORMAT_PREFIX: &str = "TEX";
const TEXTURE_FORMAT_SUFFIX: &str = "FORMAT";
const PRELIT_UV_TOLERANCE: f32 = 1.0 / 1024.0;
const PRELIT_DEPTH: f32 = 0.5;
use crate::puppet::VERTEX_STRIDE;
const INDEX_WORDS: std::ops::Range<usize> = 10..14;
const MATRIX_FLOATS: usize = 16;

pub struct Skin {
    pub vertices: Vec<u8>,
    pub indices: Vec<u16>,
    pub palette: Palette,
    pub channels: Option<super::Channels>,
}

pub struct Palette {
    pub bones: Arc<[Option<BonePose>]>,
    pub uniform: String,
    pub morph: Option<crate::puppet::morph::Uniforms>,
    pub channels: Vec<f32>,
    pub alpha: Option<crate::puppet::alpha::Uniforms>,
}

pub(crate) fn combos(
    combos: &mut BTreeMap<String, i64>,
    bones: usize,
    morph: bool,
) -> Result<(), String> {
    if let Some((key, value)) = combos.iter().find(|(key, value)| match key.as_str() {
        SKINNING => **value != 1,
        UV_TRANSFORM => !matches!(**value, 0 | 1),
        crate::puppet::alpha::COMBO => !matches!(**value, 0 | 1),
        BONE_COUNT => **value != bones as i64,
        crate::puppet::morph::COMBO => **value != i64::from(morph),
        crate::puppet::morph::MODIFIER_COMBO => **value != 0 && (!morph || **value != 1),
        super::TEXTURE_FORMAT | super::VERSION => false,
        crate::effects::SHADER_VERSION_COMBO => **value != crate::effects::SHADER_VERSION,
        crate::model::lighting::COMBO
        | crate::model::lighting::spot::COMBO
        | crate::model::lighting::directional::COMBO => {
            !(0..=crate::model::lighting::CAPACITY as i64).contains(*value)
        }
        crate::model::lighting::SHADOW_COMBO | crate::model::lighting::COOKIE_COMBO => {
            !matches!(**value, 0 | 1)
        }
        crate::model::fog::MATERIAL => !matches!(**value, 0 | 1),
        name if LIT_COMBOS.contains(&name) => !matches!(**value, 0 | 1),
        name if name.starts_with(TEXTURE_FORMAT_PREFIX)
            && name.ends_with(TEXTURE_FORMAT_SUFFIX) =>
        {
            false
        }
        _ => **value != 0,
    }) {
        return Err(format!("puppet material combo {key}={value} unsupported"));
    }
    combos.insert(UV_TRANSFORM.into(), 1);
    combos.insert(SKINNING.into(), 1);
    combos.insert(BONE_COUNT.into(), bones as i64);
    Ok(())
}

pub(super) fn validate(
    pass: &EffectPass,
    texture: &Texture,
    bones: usize,
) -> Result<String, String> {
    if pass.frame_dependent()
        || pass.binds.iter().any(|(_, bind)| {
            !matches!(
                bind,
                crate::effects::EffectBind::ShadowAtlas
                    | crate::effects::EffectBind::LightCookie
                    | crate::effects::EffectBind::SceneHistory
            )
        })
        || pass.target.is_some()
        || !texture.is_static()
        || pass.textures.iter().flatten().any(|texture| !texture.is_static())
    {
        return Err("puppet material dynamic input unsupported".into());
    }
    let inputs = crate::shader::vertex_inputs(&pass.vertex.source)?;
    let mut required = REQUIRED_INPUTS;
    if inputs.iter().any(|input| input.name == crate::puppet::morph::POSITION) {
        required[0] = INPUTS[INPUTS.len() - 1];
    }
    if required
        .iter()
        .any(|required| !inputs.iter().any(|input| (input.name, input.kind) == *required))
        || inputs.iter().any(|input| !INPUTS.contains(&(input.name, input.kind)))
    {
        return Err("puppet material vertex input unsupported".into());
    }
    if pass.vertex.uniforms.iter().chain(&pass.fragment.uniforms).any(|uniform| {
        if MATRICES.contains(&uniform.name.as_str())
            || ALT_MATRICES.contains(&uniform.name.as_str())
        {
            return uniform.kind != UniformKind::Mat4 || uniform.count != 1;
        }
        if NORMAL_MATRICES.contains(&uniform.name.as_str()) {
            return uniform.kind != UniformKind::Mat3 || uniform.count != 1;
        }
        match uniform.kind {
            UniformKind::Mat4 => uniform.count != 1 || !MATRICES.contains(&uniform.name.as_str()),
            UniformKind::Mat4x3 => {
                !(BONES.contains(&uniform.name.as_str())
                    || (uniform.name == crate::puppet::morph::TRANSFORMS
                        && uniform.count == crate::puppet::morph::TARGETS))
            }
            UniformKind::Mat3 => true,
            _ => false,
        }
    }) {
        return Err("puppet material matrix contract unsupported".into());
    }
    Ok(bone_uniform(pass, bones)?.name.clone())
}

pub(super) fn prelit_projection(
    vertices: &[crate::puppet::Vertex],
) -> Result<crate::matrix::Mat4, String> {
    let axis = |position: fn(&crate::puppet::Vertex) -> f32,
                uv: fn(&crate::puppet::Vertex) -> f32| {
        let range = |value: fn(&crate::puppet::Vertex) -> f32| {
            vertices.iter().map(value).fold((f32::MAX, f32::MIN), |(low, high), sample| {
                (low.min(sample), high.max(sample))
            })
        };
        let (low, high) = range(position);
        let (near, far) = range(uv);
        ((far - near) / (high - low), low, near, far)
    };
    let (u_scale, x_low, u_low, _) = axis(|vertex| vertex.position[0], |vertex| vertex.uv[0]);
    let (v_scale, y_low, _, v_high) = axis(|vertex| vertex.position[1], |vertex| vertex.uv[1]);
    let affine = u_scale.is_finite()
        && v_scale.is_finite()
        && vertices.iter().all(|vertex| {
            let u = u_low + (vertex.position[0] - x_low) * u_scale;
            let v = v_high - (vertex.position[1] - y_low) * v_scale;
            (u - vertex.uv[0]).abs() <= PRELIT_UV_TOLERANCE
                && (v - vertex.uv[1]).abs() <= PRELIT_UV_TOLERANCE
        });
    if !affine {
        return Err("puppet prelit texture mapping unsupported".into());
    }
    Ok([
        2.0 * u_scale,
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 * v_scale,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 * (u_low - x_low * u_scale) - 1.0,
        1.0 - 2.0 * (v_high + y_low * v_scale),
        PRELIT_DEPTH,
        1.0,
    ])
}

pub fn bone_uniform(pass: &EffectPass, bones: usize) -> Result<&crate::shader::Uniform, String> {
    let mut uniforms = pass
        .vertex
        .uniforms
        .iter()
        .chain(&pass.fragment.uniforms)
        .filter(|uniform| uniform.name.split('[').next() == Some(BONE_PREFIX));
    let first = uniforms.next().ok_or("puppet material palette missing")?;
    if std::iter::once(first).chain(uniforms).any(|uniform| {
        !BONES.contains(&uniform.name.as_str())
            || uniform.kind != UniformKind::Mat4x3
            || uniform.count != bones
            || uniform.std140_span() != bones * MATRIX_FLOATS * size_of::<f32>()
    }) {
        return Err("puppet material palette contract unsupported".into());
    }
    Ok(first)
}

pub(super) fn admit(
    puppet: &mut Puppet,
    frame: Frame,
    pass: &EffectPass,
    texture: &Texture,
) -> Result<Skin, String> {
    let uniform = validate(pass, texture, frame.bones.len())?;
    let raw =
        puppet.mesh.raw_vertices.as_ref().ok_or("puppet material requires modern vertices")?;
    let stride =
        if puppet.mesh.morph.is_some() { crate::puppet::MORPH_STRIDE } else { VERTEX_STRIDE };
    let index_words = if puppet.mesh.morph.is_some() {
        INDEX_WORDS.start + 1..INDEX_WORDS.end + 1
    } else {
        INDEX_WORDS
    };
    if raw.len() != puppet.mesh.vertices.len() * stride
        || raw.chunks_exact(stride).any(|vertex| {
            vertex.chunks_exact(size_of::<f32>()).enumerate().any(|(word, bytes)| {
                !index_words.contains(&word)
                    && !f32::from_le_bytes(bytes.try_into().expect("vertex word")).is_finite()
            })
        })
    {
        return Err("puppet material attributes invalid".into());
    }
    let samples = puppet.layers.iter().map(|layer| (*layer, 0.0)).collect::<Vec<_>>();
    let mut morph = crate::puppet::morph::uniforms(&puppet.mesh, &samples, None)
        .map_err(|error| error.to_string())?;
    if let Some(morph) = &mut morph {
        morph.pose(&puppet.mesh, &frame.bones).map_err(|error| error.to_string())?;
    }
    let channels = crate::puppet::channels::weights(&puppet.mesh, &samples)
        .map_err(|error| error.to_string())?;
    let alpha = crate::puppet::alpha::uniforms(&puppet.mesh, &pass.vertex.uniforms, &samples)
        .map_err(|error| error.to_string())?;
    let mut vertices = puppet.mesh.raw_vertices.take().expect("modern vertices");
    if !pass.vertex.uniforms.iter().any(|uniform| uniform.name == UV_ROTATION) {
        let (u, v) =
            if puppet.mesh.texture_channels.is_some() { (1.0, 1.0) } else { texture.uv_scale() };
        for vertex in vertices.chunks_exact_mut(stride) {
            for (bytes, scale) in vertex[stride - size_of::<[f32; 2]>()..]
                .chunks_exact_mut(size_of::<f32>())
                .zip([u, v])
            {
                let value = f32::from_le_bytes(bytes.try_into().expect("vertex uv")) * scale;
                bytes.copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(Skin {
        vertices,
        indices: if puppet.mesh.draw_order.is_some() {
            puppet.mesh.indices.clone()
        } else {
            std::mem::take(&mut puppet.mesh.indices)
        },
        palette: Palette { bones: frame.bones, uniform, morph, channels, alpha },
        channels: None,
    })
}
