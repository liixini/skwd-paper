use super::{FLOAT_BYTES, MORPH_STRIDE, cstring, find, le_f32, read_f32, read_u16, read_u32, take};
use anyhow::{Result, anyhow};

const SIGNATURE: &[u8] = b"MDMP0001\0";
const MAX_SHAPES: usize = 1024;
const SECTION_TERMINATOR: [u8; 1] = [0];
const DELTA_AXES: usize = 3;
const POSITION_COMPONENTS: usize = 4;
const NORMAL_DELTAS: u32 = 1 << 10;
const TANGENT_DELTAS: u32 = 1 << 11;
const ALPHA_DELTAS: u32 = 1 << 12;
const MODIFIERS: u32 = 1 << 13;
const HEADER_FLAGS_OFFSET: usize = 2 * size_of::<u32>();
const MAX_TRACK_SAMPLES: usize = 1_000_000;
pub const POSITION: &str = "a_PositionVec4";
pub const COMBO: &str = "MORPHING";
pub const MODIFIER_COMBO: &str = "MORPHING_MODIFIERS";
pub const TRANSFORMS: &str = "g_MorphBoneTransform[11]";
pub const RULES: &str = "g_MorphBoneRules[11]";
pub const TEXTURE_SLOT: usize = 5;
pub const OFFSETS: &str = "g_MorphOffsets[12]";
pub const WEIGHTS: &str = "g_MorphWeights[12]";
pub const SLOTS: usize = 12;
pub const TARGETS: usize = SLOTS - 1;
const AXIS_RULE: u32 = 2;
const TEXEL_CHANNELS: usize = 4;
const TEXEL_BYTES: usize = TEXEL_CHANNELS * size_of::<i16>();
const DEFAULT_TEXEL: [i16; TEXEL_CHANNELS] = [0, 0, 0, i16::MAX];

#[derive(Debug, PartialEq)]
pub struct Morph {
    pub scale: f32,
    pub samples: usize,
    pub indices: Vec<u32>,
    pub shapes: Vec<Shape>,
}

#[derive(Debug, PartialEq)]
pub struct Shape {
    pub id: u64,
    pub name: String,
    pub deltas: Vec<[i16; DELTA_AXES]>,
    pub alpha: Vec<i16>,
    pub modifier: Option<Modifier>,
}

#[derive(Debug, PartialEq)]
pub struct Modifier {
    pub bone: u32,
    pub mode: u32,
    pub range: [f32; 2],
}

#[derive(Debug, PartialEq)]
pub struct Clip {
    pub scale: f32,
    pub tracks: Vec<Track>,
}

#[derive(Debug, PartialEq)]
pub struct Track {
    pub shape: u16,
    pub samples: Vec<f32>,
}

pub(super) fn closes_file(bytes: &[u8], start: usize) -> bool {
    if !bytes.get(start..).is_some_and(|tail| tail.starts_with(SIGNATURE)) {
        return false;
    }
    let body = start + SIGNATURE.len();
    le_section_end(bytes, body)
        .is_some_and(|end| end >= body && bytes.get(end..) == Some(&SECTION_TERMINATOR))
}

fn le_section_end(bytes: &[u8], at: usize) -> Option<usize> {
    let word = bytes.get(at..at + size_of::<u32>())?;
    Some(u32::from_le_bytes(word.try_into().ok()?) as usize)
}

pub(super) fn parse(
    bytes: &[u8],
    vertices: usize,
    count: usize,
    stride: usize,
) -> Result<Option<Morph>> {
    let Some(section) = find(bytes, SIGNATURE) else {
        if stride == MORPH_STRIDE {
            return Err(anyhow!("puppet morph section missing"));
        }
        return Ok(None);
    };
    let mut header = super::MAGIC_23.len() + 1 + HEADER_FLAGS_OFFSET;
    let meshes = read_u32(bytes, &mut header)?;
    if !(1..=2).contains(&meshes) {
        return Err(anyhow!("puppet morph mesh count unsupported"));
    }
    cstring(bytes, &mut header)?;
    let flags = read_u32(bytes, &mut header)?;
    let mut at = section + SIGNATURE.len();
    let end = read_u32(bytes, &mut at)? as usize;
    if end > bytes.len() || end < at {
        return Err(anyhow!("puppet morph section bounds"));
    }
    let bytes = &bytes[..end];
    let shapes = usize::from(read_u16(bytes, &mut at)?);
    if shapes == 0 || shapes > MAX_SHAPES {
        return Err(anyhow!("puppet morph shape count invalid"));
    }
    let scale = read_f32(bytes, &mut at)?;
    let samples = read_u32(bytes, &mut at)? as usize;
    if !scale.is_finite() || scale <= 0.0 || samples > count || stride != MORPH_STRIDE {
        return Err(anyhow!("puppet morph dimensions invalid"));
    }
    let mut out = Vec::with_capacity(shapes);
    for _ in 0..shapes {
        out.push(shape(bytes, &mut at, samples, flags)?);
    }
    for _ in 1..meshes {
        if read_u16(bytes, &mut at)? != 0 {
            return Err(anyhow!("puppet auxiliary morph shapes unsupported"));
        }
    }
    if at != end {
        return Err(anyhow!("puppet morph trailing data"));
    }
    let indices = (0..count)
        .map(|index| {
            let value =
                le_f32(bytes, vertices + index * stride + (POSITION_COMPONENTS - 1) * FLOAT_BYTES)?;
            if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > samples as f32 {
                return Err(anyhow!("puppet morph vertex index invalid"));
            }
            Ok(value as u32)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(Morph { scale, samples, indices, shapes: out }))
}

fn shape(bytes: &[u8], at: &mut usize, samples: usize, flags: u32) -> Result<Shape> {
    let id = u64::from_le_bytes(take(bytes, at, size_of::<u64>())?.try_into()?);
    let name = cstring(bytes, at)?.to_owned();
    let deltas = packed(bytes, at, samples * DELTA_AXES)?
        .chunks_exact(DELTA_AXES)
        .map(|delta| [delta[0], delta[1], delta[2]])
        .collect();
    for flag in [NORMAL_DELTAS, TANGENT_DELTAS] {
        if flags & flag != 0 {
            packed(bytes, at, samples * DELTA_AXES)?;
        }
    }
    let alpha = if flags & ALPHA_DELTAS != 0 { packed(bytes, at, samples)? } else { Vec::new() };
    let modifier = if flags & MODIFIERS != 0 {
        let bone = read_u32(bytes, at)?;
        let mode = read_u32(bytes, at)?;
        let range = [read_f32(bytes, at)?, read_f32(bytes, at)?];
        if !range.into_iter().all(f32::is_finite) {
            return Err(anyhow!("puppet morph modifier invalid"));
        }
        Some(Modifier { bone, mode, range })
    } else {
        None
    };
    Ok(Shape { id, name, deltas, alpha, modifier })
}

fn packed(bytes: &[u8], at: &mut usize, count: usize) -> Result<Vec<i16>> {
    let length = read_u32(bytes, at)? as usize;
    if length != count * size_of::<i16>() {
        return Err(anyhow!("puppet morph sample size invalid"));
    }
    Ok(take(bytes, at, length)?
        .chunks_exact(size_of::<i16>())
        .map(|word| i16::from_le_bytes(word.try_into().expect("sample word")))
        .collect())
}

pub(super) fn clip(
    bytes: &[u8],
    at: &mut usize,
    duration: u32,
    meshes: usize,
) -> Result<Option<Clip>> {
    if take(bytes, at, 1)? == [0] {
        return Ok(None);
    }
    let flags = read_u32(bytes, at)?;
    if flags == 0 {
        empty_clips(bytes, at, meshes)?;
        return Ok(None);
    }
    if flags != 1 {
        return Err(anyhow!("puppet morph track flags unsupported"));
    }
    let scale = read_f32(bytes, at)?;
    let count = usize::from(read_u16(bytes, at)?);
    let samples = (duration as usize)
        .checked_add(1)
        .filter(|samples| samples.saturating_mul(count) <= MAX_TRACK_SAMPLES)
        .ok_or_else(|| anyhow!("puppet morph track samples exceed limit"))?;
    if !scale.is_finite() || count > MAX_SHAPES {
        return Err(anyhow!("puppet morph clip invalid"));
    }
    let mut tracks = Vec::with_capacity(count);
    for _ in 0..count {
        let shape = read_u16(bytes, at)?;
        if read_u32(bytes, at)? as usize != samples * FLOAT_BYTES {
            return Err(anyhow!("puppet morph track size invalid"));
        }
        let samples = (0..samples).map(|_| read_f32(bytes, at)).collect::<Result<Vec<_>>>()?;
        if !samples.iter().all(|value| value.is_finite()) {
            return Err(anyhow!("puppet morph track not finite"));
        }
        tracks.push(Track { shape, samples });
    }
    empty_clips(bytes, at, meshes)?;
    Ok(Some(Clip { scale, tracks }))
}

fn empty_clips(bytes: &[u8], at: &mut usize, meshes: usize) -> Result<()> {
    for _ in 1..meshes {
        if read_u32(bytes, at)? != 0 {
            return Err(anyhow!("puppet auxiliary morph channels unsupported"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub struct Uniforms {
    pub offsets: [f32; SLOTS],
    pub weights: [f32; SLOTS],
    pub modifiers: Option<Modifiers>,
    targets: [Option<usize>; TARGETS],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modifiers {
    pub transforms: [crate::matrix::Mat4; TARGETS],
    pub rules: [[f32; DELTA_AXES]; TARGETS],
}

impl Uniforms {
    pub fn pose(&mut self, mesh: &super::Mesh, bones: &[Option<super::BonePose>]) -> Result<()> {
        let Some(morph) = mesh.morph.as_ref().filter(|morph| morph.has_modifiers()) else {
            return Ok(());
        };
        let mut modifiers = Modifiers {
            transforms: [crate::matrix::IDENTITY; TARGETS],
            rules: [[0.0, -1.0, 0.0]; TARGETS],
        };
        for (slot, target) in self.targets.iter().enumerate() {
            let Some(target) = target else { continue };
            let Some(modifier) = &morph.shapes[*target].modifier else { continue };
            let index = modifier.bone as usize;
            let Some(Some(super::BonePose::Spatial(skin))) = bones.get(index) else {
                return Err(anyhow!("puppet morph modifier bone unavailable"));
            };
            let bind = mesh
                .native_bind_inverse
                .get(index)
                .and_then(Option::as_ref)
                .ok_or_else(|| anyhow!("puppet morph modifier bind unavailable"))?;
            let inverse = crate::matrix::mat4_inverse(skin)
                .ok_or_else(|| anyhow!("puppet morph modifier pose singular"))?;
            modifiers.transforms[slot] = crate::matrix::mat4_mul(bind, &inverse);
            modifiers.rules[slot] = [
                f32::from(u8::from(modifier.mode & AXIS_RULE != 0)),
                modifier.range[0],
                modifier.range[1],
            ];
        }
        self.modifiers = Some(modifiers);
        Ok(())
    }
}

pub fn weights(
    mesh: &super::Mesh,
    samples: &[(super::AnimationLayer, f32)],
) -> Result<Option<Vec<f32>>> {
    let Some(morph) = &mesh.morph else { return Ok(None) };
    let mut weights = vec![0.0; morph.shapes.len()];
    for (layer, seconds) in samples.iter().filter(|(layer, _)| layer.visible) {
        let Some(animation) = mesh.animations.iter().find(|clip| clip.id == layer.id) else {
            continue;
        };
        let Some(clip) = &animation.morph else { continue };
        let (first, next, amount) = animation.sample_frame(*seconds);
        for track in &clip.tracks {
            let weight =
                track.samples[first] + (track.samples[next] - track.samples[first]) * amount;
            let target = weights
                .get_mut(usize::from(track.shape))
                .ok_or_else(|| anyhow!("puppet morph target absent"))?;
            let weight = weight * clip.scale / morph.scale;
            if layer.additive {
                *target += weight * layer.blend;
            } else {
                *target += (weight - *target) * layer.blend;
            }
        }
    }
    Ok(Some(weights))
}

pub(super) fn targets_present(mesh: &super::Mesh) -> bool {
    let Some(morph) = &mesh.morph else { return true };
    mesh.animations
        .iter()
        .filter_map(|animation| animation.morph.as_ref())
        .flat_map(|clip| &clip.tracks)
        .all(|track| usize::from(track.shape) < morph.shapes.len())
}

pub fn apply(
    mesh: &super::Mesh,
    frame: &mut super::Frame,
    samples: &[(super::AnimationLayer, f32)],
    edited: Option<&[f32]>,
    alphas: &mut Vec<f32>,
) -> Result<()> {
    let Some(morph) = &mesh.morph else { return Ok(()) };
    let weights = selected(mesh, samples, edited)?;
    if weights.iter().all(|weight| *weight == 0.0) {
        return Ok(());
    }
    if alphas.is_empty() {
        alphas.resize(mesh.vertices.len(), 1.0);
    }
    for (index, vertex) in mesh.vertices.iter().enumerate() {
        let Some(sample) = morph.indices[index].checked_sub(1) else { continue };
        let sample = sample as usize;
        let mut offset = [0.0_f32; DELTA_AXES];
        for (shape, weight) in
            morph.shapes.iter().zip(weights.iter().copied()).filter(|(_, weight)| *weight != 0.0)
        {
            for (axis, delta) in offset.iter_mut().zip(shape.deltas[sample]) {
                *axis += snorm(delta) * weight;
            }
            let alpha = shape.alpha.get(sample).map_or(1.0, |alpha| snorm(*alpha));
            alphas[index] *= alpha * weight + 1.0 - weight;
        }
        let mut morphed = *vertex;
        for (axis, delta) in morphed.position.iter_mut().zip(offset) {
            *axis += delta * morph.scale;
        }
        frame.positions[index] = super::skin_vertex(&morphed, &frame.bones);
    }
    Ok(())
}

fn selected<'a>(
    mesh: &super::Mesh,
    samples: &[(super::AnimationLayer, f32)],
    edited: Option<&'a [f32]>,
) -> Result<std::borrow::Cow<'a, [f32]>> {
    Ok(match edited {
        Some(edited) => std::borrow::Cow::Borrowed(edited),
        None => std::borrow::Cow::Owned(weights(mesh, samples)?.expect("morph mesh")),
    })
}

fn snorm(value: i16) -> f32 {
    (f32::from(value) / f32::from(i16::MAX)).max(-1.0)
}

pub fn uniforms(
    mesh: &super::Mesh,
    samples: &[(super::AnimationLayer, f32)],
    edited: Option<&[f32]>,
) -> Result<Option<Uniforms>> {
    let Some(morph) = &mesh.morph else { return Ok(None) };
    if morph
        .shapes
        .iter()
        .filter_map(|shape| shape.modifier.as_ref())
        .any(|modifier| modifier.bone as usize >= mesh.bones.len())
    {
        return Err(anyhow!("puppet morph modifier bone unavailable"));
    }
    let weights = selected(mesh, samples, edited)?;
    let mut out = Uniforms {
        offsets: [0.0; SLOTS],
        weights: [0.0; SLOTS],
        modifiers: None,
        targets: [None; TARGETS],
    };
    out.weights[0] = morph.scale;
    let mut slot = 1;
    for (index, weight) in weights.iter().copied().enumerate().filter(|(_, weight)| *weight != 0.0)
    {
        if slot == SLOTS {
            return Err(anyhow!("puppet active morph targets exceed shader slots"));
        }
        out.offsets[slot] = (index * morph.samples) as f32;
        out.weights[slot] = weight;
        out.targets[slot - 1] = Some(index);
        slot += 1;
    }
    out.offsets[0] = (slot - 1) as f32;
    Ok(Some(out))
}

impl Morph {
    pub fn has_modifiers(&self) -> bool {
        self.shapes.iter().any(|shape| shape.modifier.is_some())
    }

    pub fn texture(&self) -> Result<crate::model::Texture> {
        let texels = self.samples * self.shapes.len() + 1;
        let side = texels.isqrt() + usize::from(texels.isqrt().pow(2) != texels);
        let mut data = Vec::with_capacity(side * side * TEXEL_BYTES);
        data.extend(DEFAULT_TEXEL.into_iter().flat_map(i16::to_le_bytes));
        for shape in &self.shapes {
            for (index, delta) in shape.deltas.iter().enumerate() {
                data.extend(delta.iter().copied().flat_map(i16::to_le_bytes));
                data.extend(shape.alpha.get(index).copied().unwrap_or(i16::MAX).to_le_bytes());
            }
        }
        data.resize(side * side * TEXEL_BYTES, 0);
        let mut texture = crate::model::solid_texture();
        texture.width = side as u32;
        texture.height = side as u32;
        texture.img_width = side as u32;
        texture.img_height = side as u32;
        texture.nearest = true;
        texture.pixels = std::sync::Arc::new(crate::tex::Pixels {
            format: crate::tex::PixelFormat::Rgba16Snorm,
            levels: vec![crate::tex::MipLevel { width: side as u32, height: side as u32, data }],
        });
        Ok(texture)
    }
}
