pub mod alpha;
mod bind;
pub mod channels;
pub mod draw_order;
pub mod masks;
pub mod morph;
mod pose;

pub mod playback;
pub mod sampler;
pub mod spring;

#[path = "puppet_regions.rs"]
mod regions;
pub use regions::{Region, assign_regions};

use crate::affine::Affine;
use crate::matrix::Mat4;
use anyhow::{Result, anyhow};

const MAGIC_13: &[u8; 8] = b"MDLV0013";
const MAGIC_14: &[u8; 8] = b"MDLV0014";
const MAGIC_16: &[u8; 8] = b"MDLV0016";
const MAGIC_17: &[u8; 8] = b"MDLV0017";
const SKELETON_SIGNATURE_2: &[u8] = b"MDLS0002\0";
const SKELETON_SIGNATURE_3: &[u8] = b"MDLS0003\0";
const SKELETON_SIGNATURE_4: &[u8] = b"MDLS0004\0";
const OLD_SKELETON_SIGNATURE: &[u8] = b"MDLS0001\0";
const OLD_ANIMATION_SIGNATURE: &[u8] = b"MDLA0001\0";
const EXTENSION_SIGNATURE: &[u8] = b"MDLE0002\0";
const SKELETON_2: &str = "MDLS0002";
const SKELETON_3: &str = "MDLS0003";
const SKELETON_4: &str = "MDLS0004";
const ANIMATION_2: &str = "MDLA0002";
const ANIMATION_3: &str = "MDLA0003";
const ANIMATION_4: &str = "MDLA0004";
const ANIMATION_5: &str = "MDLA0005";
const ANIMATION_6: &str = "MDLA0006";
const MIRROR_MODE: &str = "mirror";
const INFLUENCES: usize = 4;
const VECTOR_AXES: usize = 3;
pub const AUTHORED_BONES: usize = 2;
pub const MAX_ANIMATION_LAYERS: usize = 128;
const MESH_PREFIX_BYTES: usize = 2 * size_of::<u32>();
const OLD_SKELETON: &str = "MDLS0001";
const OLD_ANIMATION: &str = "MDLA0001";
const OLD_MODEL_PADDING: u8 = 0xcd;
const OLD_VERTEX_STRIDE: usize = 52;
const OLD_VERTEX_BONES: usize = 12;
const OLD_VERTEX_WEIGHTS: usize = 28;
const OLD_VERTEX_UV: usize = 44;
const OLD_MATERIAL_AT: usize = 21;
const OLD_BONE_FLAGS: [u32; 2] = [0, 1];
const OLD_MESH_HEADER: [u32; 3] = [0x0180_0009, 1, 1];
const VERTEX_BONES: usize = 40;
const VERTEX_WEIGHTS: usize = 56;
const VERTEX_UV: usize = 72;
const STILL_VERTEX_UV: usize = 40;
const FLOAT_BYTES: usize = size_of::<f32>();
const TRIANGLE_BYTES: usize = 3 * size_of::<u16>();
const ANIMATION_TRAILER_BYTES: usize = 6 * size_of::<u32>();
const MAX_EVENTS: usize = 1024;
const AUTHORED_CLIP: u32 = 0x401;
const MESH_FLAGS_AT: usize = 9 + 3 * size_of::<u32>();
const MAGIC_19: &[u8; 8] = b"MDLV0019";
const MAGIC_21: &[u8; 8] = b"MDLV0021";
const MAGIC_23: &[u8; 8] = b"MDLV0023";
const VERSIONS: [&[u8; 8]; 7] =
    [MAGIC_13, MAGIC_14, MAGIC_16, MAGIC_17, MAGIC_19, MAGIC_21, MAGIC_23];
const OLD_MESH_VERSIONS: [&[u8; 8]; 3] = [MAGIC_13, MAGIC_14, MAGIC_16];
const MORPH_FORMAT: u32 = 0x0181_000e;
const HEADER_AT: usize = 9;
const STILL_VERSIONS: [&[u8; 8]; 2] = [MAGIC_21, MAGIC_23];
const STILL_HEADER: [u32; 3] = [crate::model::mesh::POSITION_UV, 1, 1];
const ROOT_WEIGHTS: [f32; INFLUENCES] = [1.0, 0.0, 0.0, 0.0];
const FLAT_FRAME: [f32; 7] = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0];
pub const MORPH_STRIDE: usize = 84;
pub(crate) const VERTEX_STRIDE: usize = 80;
const STILL_VERTEX_STRIDE: usize = 48;
const MAX_VERTICES: usize = u16::MAX as usize + 1;
const MAX_INDICES: usize = 3_000_000;
const MAX_ANIMATION_SAMPLES: usize = 1_000_000;
const ATTACHMENT_MAGIC: &str = "MDAT0001";
const ATTACHMENT_MATRIX_FLOATS: usize = 16;
const TRANSFORM_BYTES: usize = 36;
const TRACK_HEADER_BYTES: usize = 8;
const BIND_RECONSTRUCTION_TOLERANCE: f32 = 0.0001;
const BIND_SHEAR_TOLERANCE: f32 = 0.001;
const GIMBAL_COSINE: f32 = 0.001;
const BIND_BASIS: [[usize; 3]; 3] = [[0, 1, 2], [4, 5, 6], [8, 9, 10]];
const BIND_ORIGIN: [usize; 3] = [12, 13, 14];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub bones: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Debug, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub raw_vertices: Option<Vec<u8>>,
    pub indices: Vec<u16>,
    pub bones: Vec<Bone>,
    pub animations: Vec<Animation>,
    pub attachments: Vec<Attachment>,
    pub regions: Vec<Region>,
    pub morph: Option<morph::Morph>,
    pub texture_channels: Option<channels::Geometry>,
    pub alpha: bool,
    pub draw_order: Option<draw_order::Layout>,
    pub masks: masks::Masks,
    pub skipped_constraints: usize,
    pub skipped_morphs: bool,
    pub skipped_draw_order: bool,
    pub springs: Vec<spring::Definition>,
    spatial: bool,
    bind_inverse: Vec<Option<Affine>>,
    native_bind_inverse: Vec<Option<Mat4>>,
}

#[derive(Debug, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub bone: u16,
    pub transform: Transform,
    pub matrix: Mat4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<u16>,
    raw_bind: Mat4,
    bind: Transform,
    raw_rest: Mat4,
    rest: Transform,
    native_rest: bind::Bind,
}

impl Bone {
    fn from_matrix(
        name: String,
        parent: Option<u16>,
        raw_bind: Mat4,
        spatial: bool,
    ) -> Result<Self> {
        let bind = local_transform(raw_bind, spatial)?;
        Ok(Self {
            name,
            parent,
            raw_bind,
            bind,
            raw_rest: raw_bind,
            rest: bind,
            native_rest: bind::Bind::new(&raw_bind),
        })
    }

    fn set_rest(&mut self, raw_rest: Mat4, spatial: bool) -> Result<()> {
        self.rest = local_transform(raw_rest, spatial)?;
        self.native_rest = bind::Bind::new(&raw_rest);
        self.raw_rest = raw_rest;
        Ok(())
    }

    pub fn raw_bind(&self) -> &Mat4 {
        &self.raw_bind
    }
}

#[derive(Debug, PartialEq)]
pub struct Animation {
    pub id: u32,
    pub name: String,
    pub fps: f32,
    pub duration: u32,
    pub mirror: bool,
    pub tracks: Vec<Vec<Transform>>,
    spatial: bool,
    pub disabled: Option<Vec<bool>>,
    pub events: Vec<AnimationEvent>,
    pub morph: Option<morph::Clip>,
    pub channels: channels::Clip,
    pub alpha: alpha::Clip,
    pub draw_order: alpha::Clip,
}

impl Animation {
    fn sample_frame(&self, seconds: f32) -> (usize, usize, f32) {
        let duration = self.duration as f32;
        let mut frame =
            (seconds * self.fps).rem_euclid(if self.mirror { duration * 2.0 } else { duration });
        if self.mirror && frame > duration {
            frame = duration * 2.0 - frame;
        }
        let first = frame.floor() as usize;
        (first, (first + 1).min(self.duration as usize), frame.fract())
    }
}

#[derive(Debug, PartialEq)]
pub struct AnimationEvent {
    pub field: u32,
    pub payload: EventPayload,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
pub struct EventPayload {
    pub frame: u32,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationLayer {
    pub id: u32,
    pub rate: f32,
    pub blend: f32,
    pub additive: bool,
    pub visible: bool,
}

impl AnimationLayer {
    pub fn animated(&self) -> bool {
        self.visible && self.rate != 0.0 && self.blend > 0.0
    }
}

pub fn parse(bytes: &[u8]) -> Result<Mesh> {
    let magic = bytes.get(..8).ok_or_else(|| anyhow!("puppet header is truncated"))?;
    if !VERSIONS.iter().any(|version| magic == *version) {
        return Err(anyhow!("unsupported puppet header"));
    }
    let old_mesh = OLD_MESH_VERSIONS.iter().any(|version| magic == *version);
    let (skeleton, start, vertex_bytes, index_bytes, stride) = if old_mesh {
        let (skeleton, start, vertex_bytes, index_bytes) = locate_old(bytes, magic)?;
        (Some(skeleton), start, vertex_bytes, index_bytes, OLD_VERTEX_STRIDE)
    } else {
        let skeleton = find(bytes, SKELETON_SIGNATURE_2)
            .or_else(|| find(bytes, SKELETON_SIGNATURE_3))
            .or_else(|| find(bytes, SKELETON_SIGNATURE_4));
        let still = skeleton.is_none();
        if still
            && !(STILL_VERSIONS.iter().any(|version| magic == *version)
                && header(bytes) == Some(STILL_HEADER))
        {
            return Err(anyhow!("puppet skeleton is missing"));
        }
        let (start, vertex_bytes, index_bytes, stride) =
            locate(bytes, skeleton.unwrap_or(bytes.len()), still)?;
        (skeleton, start, vertex_bytes, index_bytes, stride)
    };
    let mesh_end = skeleton.unwrap_or(bytes.len());
    let vertex_count = vertex_bytes / stride;
    let vertices = parse_vertices(bytes, start + MESH_PREFIX_BYTES, vertex_count, stride)?;
    let indices_at = start + 8 + vertex_bytes + 4;
    let indices = parse_indices(bytes, indices_at, index_bytes / 2, vertex_count)?;
    if skeleton.is_none() && bytes[indices_at + index_bytes..].iter().any(|byte| *byte != 0) {
        return Err(anyhow!("still puppet tail unsupported"));
    }
    let texture_channels = channels::parse(bytes, indices_at + index_bytes, mesh_end)?;
    let meshes = if texture_channels.is_some() { 2 } else { 1 };
    let (mut mesh, draw_rest) = match skeleton {
        Some(skeleton) => rig(bytes, skeleton, meshes, false)?,
        None => (still()?, Vec::new()),
    };
    if old_mesh
        && vertices.iter().any(|vertex| {
            vertex
                .bones
                .iter()
                .zip(vertex.weights)
                .any(|(&bone, weight)| weight > 0.0 && usize::from(bone) >= mesh.bones.len())
        })
    {
        return Err(anyhow!("puppet bone index is outside skeleton"));
    }
    mesh.raw_vertices =
        Some(modern_vertices(bytes, start + MESH_PREFIX_BYTES, vertex_bytes, stride)?);
    mesh.morph = morph::parse(bytes, start + MESH_PREFIX_BYTES, vertex_count, stride)?;
    if !morph::targets_present(&mesh) {
        return Err(anyhow!("puppet morph target absent"));
    }
    mesh.vertices = vertices;
    mesh.indices = indices;
    mesh.texture_channels = texture_channels;
    let flags = mesh_flags(bytes)?;
    mesh.alpha = flags & alpha::ALPHA_FLAG != 0;
    mesh.masks = masks::parse(&bytes[..mesh_end], indices_at + index_bytes, magic)?;
    if flags & draw_order::FLAG != 0 {
        let tail = indices_at + index_bytes;
        mesh.draw_order =
            draw_order::Layout::parse(bytes, tail, magic, draw_rest, mesh.indices.len());
        mesh.skipped_draw_order = mesh.draw_order.is_none();
    }
    if mesh.draw_order.is_none() {
        for animation in &mut mesh.animations {
            animation.draw_order = alpha::Clip::default();
        }
    }
    Ok(mesh)
}

fn mesh_flags(bytes: &[u8]) -> Result<u32> {
    let mut at = MESH_FLAGS_AT;
    cstring(bytes, &mut at)?;
    read_u32(bytes, &mut at)
}

fn header(bytes: &[u8]) -> Option<[u32; 3]> {
    let word = |slot: usize| le_u32(bytes, HEADER_AT + slot * size_of::<u32>());
    Some([word(0)?, word(1)?, word(2)?])
}

fn modern_vertices(bytes: &[u8], start: usize, length: usize, stride: usize) -> Result<Vec<u8>> {
    let raw =
        bytes.get(start..start + length).ok_or_else(|| anyhow!("puppet vertices truncated"))?;
    if stride == VERTEX_STRIDE || stride == MORPH_STRIDE {
        return Ok(raw.to_vec());
    }
    let frame: Vec<u8> = FLAT_FRAME.into_iter().flat_map(f32::to_le_bytes).collect();
    let root: Vec<u8> = [0_u32; INFLUENCES]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .chain(ROOT_WEIGHTS.into_iter().flat_map(f32::to_le_bytes))
        .collect();
    let mut out = Vec::with_capacity(length / stride * VERTEX_STRIDE);
    for vertex in raw.chunks_exact(stride) {
        if stride == STILL_VERTEX_STRIDE {
            out.extend_from_slice(&vertex[..STILL_VERTEX_UV]);
            out.extend_from_slice(&root);
            out.extend_from_slice(&vertex[STILL_VERTEX_UV..]);
        } else {
            out.extend_from_slice(&vertex[..OLD_VERTEX_BONES]);
            out.extend_from_slice(&frame);
            out.extend_from_slice(&vertex[OLD_VERTEX_BONES..]);
        }
    }
    Ok(out)
}

fn still() -> Result<Mesh> {
    let skeleton = Skeleton {
        draw_rest: Vec::new(),
        bones: vec![Bone::from_matrix(String::new(), None, crate::matrix::IDENTITY, false)?],
        regions: Vec::new(),
        skipped_constraints: 0,
        springs: Vec::new(),
    };
    Ok(assemble(skeleton, Vec::new(), Vec::new()))
}

pub(crate) fn rig(
    bytes: &[u8],
    skeleton: usize,
    meshes: usize,
    spatial: bool,
) -> Result<(Mesh, Vec<i32>)> {
    let old = bytes.get(..MAGIC_13.len()) == Some(MAGIC_13);
    let (mut skeleton, animation_at) = parse_bones(bytes, skeleton, spatial)?;
    let bones = &skeleton.bones;
    let (attachments, animation_at) = parse_attachments(bytes, animation_at, bones.len())?;
    let animations = if old && bytes.get(animation_at) == Some(&0) {
        let padding = &bytes[animation_at + 1..];
        let value = padding.first().copied().unwrap_or(0);
        if ![0, OLD_MODEL_PADDING].contains(&value) || padding.iter().any(|byte| *byte != value) {
            return Err(anyhow!("invalid old puppet empty animation padding"));
        }
        Vec::new()
    } else if !old
        && !spatial
        && bytes.get(animation_at..).is_some_and(|tail| {
            tail == [0]
                || (tail.starts_with(EXTENSION_SIGNATURE)
                    && le_u32(bytes, animation_at + EXTENSION_SIGNATURE.len())
                        .is_some_and(|end| bytes.get(end as usize..) == Some(&[0])))
        })
    {
        Vec::new()
    } else {
        if old
            && !bytes
                .get(animation_at..)
                .is_some_and(|tail| tail.starts_with(OLD_ANIMATION_SIGNATURE))
        {
            return Err(anyhow!("unsupported old puppet animation version"));
        }
        if !old {
            let mut at = animation_at;
            let version = cstring(bytes, &mut at)?;
            let supported = match Clips::of(version) {
                Some(Clips::Bounds | Clips::DrawOrder) => true,
                Some(Clips::Plain | Clips::Alpha | Clips::Morph) => !spatial,
                Some(Clips::Legacy) | None => false,
            };
            if !supported {
                return Err(anyhow!("unsupported modern puppet animation version"));
            }
        }
        parse_animations(bytes, animation_at, bones.len(), meshes, spatial)?
    };
    let mut morph_section = false;
    if spatial {
        let end = le_u32(bytes, animation_at + ANIMATION_6.len() + 1)
            .ok_or_else(|| anyhow!("skeletal animation end missing"))? as usize;
        morph_section = morph::closes_file(bytes, end);
        if !morph_section && bytes.get(end..) != Some(&[0]) {
            return Err(anyhow!("skeletal mesh extension unsupported"));
        }
    }
    let draw_rest = std::mem::take(&mut skeleton.draw_rest);
    let mut mesh = assemble(skeleton, attachments, animations);
    mesh.skipped_morphs = morph_section;
    mesh.spatial = spatial;
    Ok((mesh, draw_rest))
}

fn assemble(skeleton: Skeleton, attachments: Vec<Attachment>, animations: Vec<Animation>) -> Mesh {
    let Skeleton { bones, regions, skipped_constraints, springs, .. } = skeleton;
    let bind_inverse = world(bones.iter().map(|bone| bone.bind), &bones)
        .into_iter()
        .map(Affine::inverse)
        .collect();
    let native_bind_inverse = pose::bind_inverse(&bones);
    Mesh {
        raw_vertices: None,
        vertices: Vec::new(),
        indices: Vec::new(),
        bones,
        animations,
        attachments,
        regions,
        skipped_constraints,
        skipped_morphs: false,
        springs,
        morph: None,
        texture_channels: None,
        alpha: false,
        draw_order: None,
        masks: masks::Masks::default(),
        skipped_draw_order: false,
        spatial: false,
        bind_inverse,
        native_bind_inverse,
    }
}

fn locate_old(bytes: &[u8], version: &[u8]) -> Result<(usize, usize, usize, usize)> {
    if bytes.get(MAGIC_13.len()) != Some(&0) {
        return Err(anyhow!("unterminated old puppet header"));
    }
    if header(bytes) != Some(OLD_MESH_HEADER) {
        return Err(anyhow!("unsupported old puppet mesh header"));
    }
    let mut at = OLD_MATERIAL_AT;
    cstring(bytes, &mut at)?;
    let mut start = at;
    let known = if version == MAGIC_16 { alpha::ALPHA_FLAG } else { 0 };
    if read_u32(bytes, &mut at)? & !known != 0 {
        return Err(anyhow!("unknown old puppet mesh field"));
    }
    if version == MAGIC_16 {
        start = at;
        if read_u32(bytes, &mut at)? != OLD_MESH_HEADER[0] {
            return Err(anyhow!("unsupported old puppet vertex layout"));
        }
    }
    let vertex_bytes = read_u32(bytes, &mut at)? as usize;
    if vertex_bytes == 0
        || !vertex_bytes.is_multiple_of(OLD_VERTEX_STRIDE)
        || vertex_bytes / OLD_VERTEX_STRIDE > MAX_VERTICES
    {
        return Err(anyhow!("invalid old puppet vertex length"));
    }
    take(bytes, &mut at, vertex_bytes)?;
    let index_bytes = read_u32(bytes, &mut at)? as usize;
    if index_bytes == 0
        || !index_bytes.is_multiple_of(TRIANGLE_BYTES)
        || index_bytes / 2 > MAX_INDICES
    {
        return Err(anyhow!("invalid old puppet index length"));
    }
    take(bytes, &mut at, index_bytes)?;
    let skeleton = if version == MAGIC_13 { OLD_SKELETON_SIGNATURE } else { SKELETON_SIGNATURE_2 };
    if !bytes.get(at..).is_some_and(|tail| tail.starts_with(skeleton)) {
        return Err(anyhow!("unsupported old puppet skeleton version"));
    }
    Ok((at, start, vertex_bytes, index_bytes))
}

fn parse_events(
    bytes: &[u8],
    at: &mut usize,
    end: usize,
    old: bool,
) -> Result<Vec<AnimationEvent>> {
    let section = bytes.get(..end).ok_or_else(|| anyhow!("truncated puppet events"))?;
    let count = read_u32(section, at)? as usize;
    if count > MAX_EVENTS {
        return Err(anyhow!("too many puppet events"));
    }
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        let field = read_u32(section, at)?;
        if old && field != 0 {
            return Err(anyhow!("unknown old puppet event field {field}"));
        }
        let payload = serde_json::from_str(cstring(section, at)?)?;
        events.push(AnimationEvent { field, payload });
    }
    Ok(events)
}

fn mesh_stride(bytes: &[u8], start: usize, still: bool) -> Result<usize> {
    match le_u32(bytes, start) {
        Some(crate::model::mesh::POSITION_NORMAL_TANGENT_UV) if still => Ok(STILL_VERTEX_STRIDE),
        Some(crate::model::mesh::SKINNED) if !still => Ok(VERTEX_STRIDE),
        Some(MORPH_FORMAT) if !still => Ok(MORPH_STRIDE),
        _ => Err(anyhow!("puppet vertex layout unsupported")),
    }
}

fn locate(bytes: &[u8], end: usize, still: bool) -> Result<(usize, usize, usize, usize)> {
    for start in HEADER_AT..end.saturating_sub(12) {
        let Ok(stride) = mesh_stride(bytes, start, still) else { continue };
        let Some(vertex_bytes) = le_u32(bytes, start + 4).map(|value| value as usize) else {
            continue;
        };
        if vertex_bytes == 0 || vertex_bytes % stride != 0 {
            continue;
        }
        let vertex_count = vertex_bytes / stride;
        if vertex_count > MAX_VERTICES {
            return Err(anyhow!("puppet has {vertex_count} vertices; limit is {MAX_VERTICES}"));
        }
        let Some(index_len_at) = start.checked_add(8).and_then(|at| at.checked_add(vertex_bytes))
        else {
            continue;
        };
        let Some(index_bytes) = le_u32(bytes, index_len_at).map(|value| value as usize) else {
            continue;
        };
        let Some(indices_end) =
            index_len_at.checked_add(4).and_then(|at| at.checked_add(index_bytes))
        else {
            continue;
        };
        if index_bytes == 0 || index_bytes % 6 != 0 || indices_end > end {
            continue;
        }
        let index_count = index_bytes / 2;
        if index_count > MAX_INDICES {
            return Err(anyhow!("puppet has {index_count} indices; limit is {MAX_INDICES}"));
        }
        return Ok((start, vertex_bytes, index_bytes, stride));
    }
    Err(anyhow!("puppet mesh is missing"))
}

fn parse_vertices(bytes: &[u8], start: usize, count: usize, stride: usize) -> Result<Vec<Vertex>> {
    let mut out = Vec::with_capacity(count);
    let old = stride == OLD_VERTEX_STRIDE;
    let extra = if stride == MORPH_STRIDE { FLOAT_BYTES } else { 0 };
    let bones = if old { OLD_VERTEX_BONES } else { VERTEX_BONES + extra };
    let weights = if old { OLD_VERTEX_WEIGHTS } else { VERTEX_WEIGHTS + extra };
    let still = stride == STILL_VERTEX_STRIDE;
    let uv = if old {
        OLD_VERTEX_UV
    } else if still {
        STILL_VERTEX_UV
    } else {
        VERTEX_UV + extra
    };
    for index in 0..count {
        let at = start + index * stride;
        let mut bone_ids = [0; INFLUENCES];
        let mut bone_weights = ROOT_WEIGHTS;
        if !still {
            for (slot, bone) in bone_ids.iter_mut().enumerate() {
                let mut offset = at + bones + slot * size_of::<u32>();
                *bone = u16::try_from(read_u32(bytes, &mut offset)?)?;
            }
            for (slot, weight) in bone_weights.iter_mut().enumerate() {
                *weight = le_f32(bytes, at + weights + slot * FLOAT_BYTES)?;
            }
        }
        let vertex = Vertex {
            position: [le_f32(bytes, at)?, le_f32(bytes, at + 4)?, le_f32(bytes, at + 8)?],
            uv: [le_f32(bytes, at + uv)?, le_f32(bytes, at + uv + FLOAT_BYTES)?],
            bones: bone_ids,
            weights: bone_weights,
        };
        if !vertex.position.into_iter().chain(vertex.uv).chain(vertex.weights).all(f32::is_finite) {
            return Err(anyhow!("puppet vertex {index} is not finite"));
        }
        out.push(vertex);
    }
    Ok(out)
}

struct Skeleton {
    draw_rest: Vec<i32>,
    bones: Vec<Bone>,
    regions: Vec<Region>,
    skipped_constraints: usize,
    springs: Vec<spring::Definition>,
}

fn parse_bones(bytes: &[u8], start: usize, spatial: bool) -> Result<(Skeleton, usize)> {
    let mut at = start;
    let magic = cstring(bytes, &mut at)?;
    if ![OLD_SKELETON, SKELETON_2, SKELETON_3, SKELETON_4].contains(&magic) {
        return Err(anyhow!("unsupported skeleton header {magic}"));
    }
    let animation_at = read_u32(bytes, &mut at)? as usize;
    let count = read_u32(bytes, &mut at)? as usize;
    if count == 0 || count > MAX_VERTICES {
        return Err(anyhow!("invalid puppet bone count {count}"));
    }
    let bytes = bytes.get(..animation_at).ok_or_else(|| anyhow!("truncated puppet skeleton"))?;
    let mut bones = Vec::with_capacity(count);
    let mut skipped_constraints = 0;
    let mut springs = Vec::new();
    for index in 0..count {
        let name = cstring(bytes, &mut at)?.to_owned();
        let flags = read_u32(bytes, &mut at)?;
        if magic == OLD_SKELETON && !OLD_BONE_FLAGS.contains(&flags) {
            return Err(anyhow!("unknown old puppet bone flags {flags}"));
        }
        let parent = read_u32(bytes, &mut at)?;
        let matrix_bytes = read_u32(bytes, &mut at)? as usize;
        if matrix_bytes != 64 {
            return Err(anyhow!("puppet bone {index} matrix uses {matrix_bytes} bytes"));
        }
        let mut matrix = Mat4::default();
        for value in &mut matrix {
            *value = read_f32(bytes, &mut at)?;
        }
        match spring::constraint(cstring(bytes, &mut at)?) {
            spring::Constraint::Inactive => {}
            spring::Constraint::Spring { translation, rotation, gravity } => {
                springs.push(spring::Definition {
                    bone: u16::try_from(index)?,
                    translation,
                    rotation,
                    gravity,
                });
            }
            spring::Constraint::Unsupported => skipped_constraints += 1,
        }
        let parent = if parent == u32::MAX {
            None
        } else {
            let parent = u16::try_from(parent)?;
            if usize::from(parent) >= index {
                return Err(anyhow!("puppet bone {index} has invalid parent {parent}"));
            }
            Some(parent)
        };
        bones.push(Bone::from_matrix(name, parent, matrix, spatial)?);
    }
    if animation_at < at || animation_at > bytes.len() {
        return Err(anyhow!("invalid puppet animation offset {animation_at}"));
    }
    if magic == OLD_SKELETON && at != animation_at {
        return Err(anyhow!("unknown old skeleton tail"));
    }
    let regions::Tail { regions, rest, draw_order: draw_rest } =
        regions::parse(&bytes[at..animation_at], bones.len(), magic)?;
    for (bone, raw_rest) in bones.iter_mut().zip(rest) {
        bone.set_rest(raw_rest, spatial)?;
    }
    Ok((Skeleton { draw_rest, bones, regions, skipped_constraints, springs }, animation_at))
}

fn parse_attachments(bytes: &[u8], start: usize, bones: usize) -> Result<(Vec<Attachment>, usize)> {
    let mut at = start;
    if cstring(bytes, &mut at)? != ATTACHMENT_MAGIC {
        return Ok((Vec::new(), start));
    }
    let end = read_u32(bytes, &mut at)? as usize;
    let section = bytes
        .get(..end)
        .filter(|_| end >= at)
        .ok_or_else(|| anyhow!("invalid puppet attachment offset {end}"))?;
    let count = read_u16(section, &mut at)?;
    let mut attachments = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let bone = read_u16(section, &mut at)?;
        if usize::from(bone) >= bones {
            return Err(anyhow!("puppet attachment has invalid bone {bone}"));
        }
        let name = cstring(section, &mut at)?.to_owned();
        let mut matrix = [0.0; ATTACHMENT_MATRIX_FLOATS];
        for value in &mut matrix {
            *value = read_f32(section, &mut at)?;
        }
        attachments.push(Attachment { name, bone, transform: matrix_transform(matrix)?, matrix });
    }
    if at != end {
        return Err(anyhow!("puppet attachment section has trailing data"));
    }
    Ok((attachments, end))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Clips {
    Legacy,
    Plain,
    Alpha,
    Morph,
    Bounds,
    DrawOrder,
}

impl Clips {
    fn of(magic: &str) -> Option<Self> {
        match magic {
            OLD_ANIMATION => Some(Self::Legacy),
            ANIMATION_2 => Some(Self::Plain),
            ANIMATION_3 => Some(Self::Alpha),
            ANIMATION_4 => Some(Self::Morph),
            ANIMATION_5 => Some(Self::Bounds),
            ANIMATION_6 => Some(Self::DrawOrder),
            _ => None,
        }
    }
}

fn parse_animations(
    bytes: &[u8],
    start: usize,
    bones: usize,
    meshes: usize,
    spatial: bool,
) -> Result<Vec<Animation>> {
    let mut at = start;
    let magic = cstring(bytes, &mut at)?;
    let clips = Clips::of(magic).ok_or_else(|| anyhow!("unsupported animation header {magic}"))?;
    let old = clips == Clips::Legacy;
    let end = read_u32(bytes, &mut at)? as usize;
    let count = read_u32(bytes, &mut at)? as usize;
    if count == 0 || count > 1024 || end > bytes.len() || end <= at {
        return Err(anyhow!("invalid puppet animation directory"));
    }
    let bytes = &bytes[..end];
    let mut out = Vec::with_capacity(count);
    let mut total_samples = 0_usize;
    for _ in 0..count {
        let id = read_u32(bytes, &mut at)?;
        let flags = read_u32(bytes, &mut at)?;
        if old && flags != 0 {
            return Err(anyhow!("unknown old puppet animation flags {flags}"));
        }
        let name = cstring(bytes, &mut at)?.to_owned();
        let mode = cstring(bytes, &mut at)?;
        let fps = read_f32(bytes, &mut at)?;
        let duration = read_u32(bytes, &mut at)?;
        let field = read_u32(bytes, &mut at)?;
        if old && field != 0 {
            return Err(anyhow!("unknown old puppet animation field {field}"));
        }
        let track_count = read_u32(bytes, &mut at)? as usize;
        if !fps.is_finite() || fps <= 0.0 || duration == 0 || track_count != bones {
            return Err(anyhow!("invalid puppet animation {id}"));
        }
        let mut tracks = Vec::with_capacity(track_count);
        let mut track_flags = Vec::with_capacity(track_count);
        let mut disabled = old.then(|| Vec::with_capacity(track_count));
        for _ in 0..track_count {
            let flag = read_u32(bytes, &mut at)?;
            track_flags.push(flag);
            if let Some(disabled) = &mut disabled {
                if flag > 1 {
                    return Err(anyhow!("unknown old puppet track flag {flag}"));
                }
                disabled.push(flag != 0);
            }
            let track_bytes = read_u32(bytes, &mut at)? as usize;
            let samples = usize::try_from(duration)?
                .checked_add(1)
                .ok_or_else(|| anyhow!("puppet animation {id} sample count overflows"))?;
            total_samples = total_samples
                .checked_add(samples)
                .filter(|total| *total <= MAX_ANIMATION_SAMPLES)
                .ok_or_else(|| {
                    anyhow!("puppet animations exceed {MAX_ANIMATION_SAMPLES} samples")
                })?;
            if samples.checked_mul(TRANSFORM_BYTES) != Some(track_bytes) {
                return Err(anyhow!("invalid puppet animation track size {track_bytes}"));
            }
            let mut track = Vec::with_capacity(samples);
            for _ in 0..samples {
                track.push(read_transform(bytes, &mut at)?);
            }
            tracks.push(track);
        }
        if !matches!(clips, Clips::Legacy | Clips::Plain) {
            skip_stale_tracks(bytes, &mut at, duration)?;
        }
        let (morph, channels, alpha, draw_order) =
            parse_channels(bytes, &mut at, duration, clips, meshes, &track_flags, spatial)?;
        if field == AUTHORED_CLIP {
            authored_tail(bytes, &mut at, duration)?;
        }
        let events = parse_events(bytes, &mut at, end, old)?;
        let spatial = tracks
            .iter()
            .flatten()
            .any(|sample| sample.rotation[0] != 0.0 || sample.rotation[1] != 0.0);
        out.push(Animation {
            id,
            name,
            fps,
            duration,
            mirror: mode == MIRROR_MODE,
            tracks,
            spatial,
            disabled,
            events,
            morph,
            channels,
            alpha,
            draw_order,
        });
    }
    let remaining = end.saturating_sub(at);
    let padding = take(bytes, &mut at, remaining)?;
    if at != end || padding.iter().any(|byte| *byte != 0) {
        return Err(anyhow!("puppet animation section has trailing data"));
    }
    Ok(out)
}

fn skip_stale_tracks(bytes: &[u8], at: &mut usize, duration: u32) -> Result<()> {
    let track_bytes = (duration as usize + 1) * TRANSFORM_BYTES;
    while bytes
        .get(*at + FLOAT_BYTES..*at + TRACK_HEADER_BYTES)
        .is_some_and(|size| size == (track_bytes as u32).to_le_bytes())
    {
        take(bytes, at, TRACK_HEADER_BYTES + track_bytes)?;
    }
    Ok(())
}

fn authored_tail(bytes: &[u8], at: &mut usize, duration: u32) -> Result<()> {
    read_u32(bytes, at)?;
    let reserved = read_u16(bytes, at)?;
    let length = read_u32(bytes, at)?;
    let padding = read_u32(bytes, at)?;
    let terminator = read_u32(bytes, at)?;
    if reserved != 0 || length != duration || padding != 0 || terminator != u32::MAX {
        return Err(anyhow!("unsupported authored puppet clip"));
    }
    Ok(())
}

fn parse_channels(
    bytes: &[u8],
    at: &mut usize,
    duration: u32,
    clips: Clips,
    meshes: usize,
    flags: &[u32],
    spatial: bool,
) -> Result<(Option<morph::Clip>, channels::Clip, alpha::Clip, alpha::Clip)> {
    if matches!(clips, Clips::Legacy | Clips::Plain) {
        return Ok((
            None,
            channels::Clip::default(),
            alpha::Clip::default(),
            alpha::Clip::default(),
        ));
    }
    let channels = channels::clip(bytes, at, duration)?;
    let alpha = alpha::clip(bytes, at, duration, flags)?;
    if clips == Clips::Alpha {
        return Ok((None, channels, alpha, alpha::Clip::default()));
    }
    let morph = morph::clip(bytes, at, duration, meshes)?;
    if clips == Clips::Morph {
        return Ok((morph, channels, alpha, alpha::Clip::default()));
    }
    let trailer = take(bytes, at, ANIMATION_TRAILER_BYTES)?;
    if spatial {
        let bounds: Vec<_> = trailer
            .chunks_exact(FLOAT_BYTES)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("bound width")))
            .collect();
        let (minimum, maximum) = bounds.split_at(VECTOR_AXES);
        if !minimum
            .iter()
            .zip(maximum)
            .all(|(min, max)| min.is_finite() && max.is_finite() && min <= max)
        {
            return Err(anyhow!("skeletal animation bounds invalid"));
        }
    } else if trailer.iter().any(|byte| *byte != 0) {
        return Err(anyhow!("puppet animation trailer unsupported"));
    }
    let draw_order = if clips == Clips::DrawOrder {
        alpha::clip(bytes, at, duration, flags)?
    } else {
        alpha::Clip::default()
    };
    Ok((morph, channels, alpha, draw_order))
}

fn read_transform(bytes: &[u8], at: &mut usize) -> Result<Transform> {
    let mut values = [0.0_f32; 9];
    for value in &mut values {
        *value = read_f32(bytes, at)?;
    }
    if !values.into_iter().all(f32::is_finite) {
        return Err(anyhow!("puppet animation transform is not finite"));
    }
    Ok(Transform {
        translation: values[0..3].try_into().unwrap(),
        rotation: values[3..6].try_into().unwrap(),
        scale: values[6..9].try_into().unwrap(),
    })
}

fn matrix_transform(matrix: Mat4) -> Result<Transform> {
    if !matrix.into_iter().all(f32::is_finite) {
        return Err(anyhow!("puppet bind matrix is not finite"));
    }
    let sx = matrix[0].hypot(matrix[1]);
    let sy = matrix[4].hypot(matrix[5]);
    if sx <= f32::EPSILON || sy <= f32::EPSILON {
        return Err(anyhow!("puppet bind matrix is singular"));
    }
    Ok(Transform {
        translation: [matrix[12], matrix[13], matrix[14]],
        rotation: [0.0, 0.0, matrix[1].atan2(matrix[0])],
        scale: [sx, sy, 1.0],
    })
}

#[derive(Debug, PartialEq)]
pub struct Frame {
    pub bones: std::sync::Arc<[Option<BonePose>]>,
    pub positions: Vec<[f32; 3]>,
    pub attachments: Vec<AttachmentPose>,
}

pub fn authored(mesh: &Mesh, layers: &[AnimationLayer], seconds: Option<f32>) -> Result<Frame> {
    if (seconds.is_none()
        && layers.iter().any(|layer| layer.visible && layer.rate != 0.0 && layer.blend > 0.0))
        || mesh.bones.is_empty()
        || !mesh.attachments.is_empty()
        || mesh.skipped_constraints != 0
        || mesh.vertices.iter().any(|vertex| {
            (vertex.bones.contains(&u16::MAX) && vertex.weights.iter().sum::<f32>() != 1.0)
                || !vertex.weights.iter().any(|weight| *weight > 0.0)
                || !vertex.weights.iter().sum::<f32>().is_finite()
                || vertex.weights.iter().any(|weight| !weight.is_finite() || *weight < 0.0)
                || vertex
                    .bones
                    .iter()
                    .any(|bone| *bone != u16::MAX && usize::from(*bone) >= mesh.bones.len())
        })
    {
        return Err(anyhow!("puppet material requires supported frozen weights"));
    }
    let time = seconds.unwrap_or(0.0);
    let samples: Vec<_> = layers.iter().map(|layer| (*layer, time)).collect();
    let bones = palette_layers(mesh, &samples)?;
    let frame = Frame {
        positions: mesh
            .vertices
            .iter()
            .map(|vertex| {
                if vertex.bones.contains(&u16::MAX) {
                    return Some(skin_vertex(vertex, &bones));
                }
                pose::skin_vertex(vertex, |index| match bones.get(usize::from(index))? {
                    Some(BonePose::Spatial(matrix)) => Some(matrix),
                    _ => None,
                })
            })
            .collect::<Option<_>>()
            .ok_or_else(|| anyhow!("puppet material weighted position is invalid"))?,
        bones,
        attachments: Vec::new(),
    };
    for bone in frame.bones.iter() {
        let Some(BonePose::Spatial(matrix)) = bone else {
            return Err(anyhow!("puppet material requires native spatial palette"));
        };
        if matrix.iter().any(|value| !value.is_finite()) {
            return Err(anyhow!("puppet material palette is not finite"));
        }
    }
    Ok(frame)
}

pub fn retain_animation(mesh: &mut Mesh, animation: u32) -> Result<()> {
    retain_animations(mesh, &[animation])
}

pub fn retain_animations(mesh: &mut Mesh, animations: &[u32]) -> Result<()> {
    if animations.iter().any(|id| !mesh.animations.iter().any(|clip| clip.id == *id)) {
        return Err(anyhow!("puppet clip missing"));
    }
    mesh.animations.retain(|clip| animations.contains(&clip.id));
    strip_geometry(mesh);
    Ok(())
}

pub fn strip_geometry(mesh: &mut Mesh) {
    mesh.vertices = Vec::new();
    if mesh.draw_order.is_none() {
        mesh.indices = Vec::new();
    }
    mesh.raw_vertices = None;
    mesh.regions = Vec::new();
    mesh.bind_inverse = Vec::new();
    if let Some(channels) = &mut mesh.texture_channels {
        channels.vertices = Vec::new();
        channels.indices = Vec::new();
    }
    if let Some(morph) = &mut mesh.morph {
        morph.indices = Vec::new();
        for shape in &mut morph.shapes {
            shape.deltas = Vec::new();
            shape.alpha = Vec::new();
        }
    }
}

pub fn rest_palette(mesh: &Mesh) -> Result<std::sync::Arc<[Option<BonePose>]>> {
    palette_layers(mesh, &[])
}

pub fn palette(
    mesh: &Mesh,
    layer: AnimationLayer,
    seconds: f32,
) -> Result<std::sync::Arc<[Option<BonePose>]>> {
    palette_layers(mesh, &[(layer, seconds)])
}

pub fn palette_layers(
    mesh: &Mesh,
    samples: &[(AnimationLayer, f32)],
) -> Result<std::sync::Arc<[Option<BonePose>]>> {
    let mut scratch = sampler::Scratch::default();
    let palette = scratch.sample(mesh, samples)?;
    Ok(palette.iter().map(|matrix| Some(BonePose::Spatial(*matrix))).collect())
}

pub fn palette_world(mesh: &Mesh, world: &[Mat4]) -> Result<Vec<Mat4>> {
    let mut palette = Vec::with_capacity(world.len());
    palette_matrices(mesh, world, &mut palette)?;
    Ok(palette)
}

fn palette_matrices(mesh: &Mesh, world: &[Mat4], palette: &mut Vec<Mat4>) -> Result<()> {
    if world.len() != mesh.bones.len() {
        return Err(anyhow!("puppet world pose size mismatch"));
    }
    if mesh.bones.len() != mesh.native_bind_inverse.len() {
        return Err(anyhow!("puppet native bind palette missing"));
    }
    palette.clear();
    for (pose, inverse) in world.iter().zip(&mesh.native_bind_inverse) {
        let matrix = crate::matrix::mat4_mul(
            pose,
            inverse.as_ref().ok_or_else(|| anyhow!("puppet native bind missing"))?,
        );
        if matrix.iter().any(|value| !value.is_finite()) {
            return Err(anyhow!("puppet palette is not finite"));
        }
        palette.push(matrix);
    }
    Ok(())
}

pub fn skin_edited(
    mesh: &Mesh,
    base: &Frame,
    reference: &[Mat4],
    edited: &[Mat4],
) -> Result<Frame> {
    if base.bones.len() != mesh.bones.len()
        || reference.len() != mesh.bones.len()
        || edited.len() != mesh.bones.len()
    {
        return Err(anyhow!("puppet edited pose size mismatch"));
    }
    let bones: std::sync::Arc<[Option<BonePose>]> = base
        .bones
        .iter()
        .zip(reference)
        .zip(edited)
        .map(|((bone, reference), edited)| {
            let Some(bone) = bone else { return Ok(None) };
            let inverse = crate::matrix::mat4_inverse(reference)
                .ok_or_else(|| anyhow!("puppet reference pose singular"))?;
            let delta = crate::matrix::mat4_mul(edited, &inverse);
            let skin = bone.matrix();
            let matrix = crate::matrix::mat4_mul(&delta, &skin);
            matrix
                .iter()
                .all(|value| value.is_finite())
                .then_some(Some(BonePose::Spatial(matrix)))
                .ok_or_else(|| anyhow!("puppet edited pose invalid"))
        })
        .collect::<Result<Vec<_>>>()?
        .into();
    let positions = mesh.vertices.iter().map(|vertex| skin_vertex(vertex, &bones)).collect();
    let attachments = mesh
        .attachments
        .iter()
        .map(|attachment| {
            attachment_pose_world(mesh, edited, &attachment.name)
                .ok_or_else(|| anyhow!("puppet attachment pose invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Frame { bones, positions, attachments })
}

pub fn refresh_attachments(mesh: &Mesh, world: &[Mat4], pose: &mut Frame) {
    for (attachment, posed) in mesh.attachments.iter().zip(&mut pose.attachments) {
        if let Some(corrected) = attachment_pose_world(mesh, world, &attachment.name) {
            *posed = AttachmentPose { depth: posed.depth, ..corrected };
        }
    }
}

pub fn attachment_pose_world(mesh: &Mesh, world: &[Mat4], name: &str) -> Option<AttachmentPose> {
    let attachment = mesh.attachments.iter().find(|attachment| attachment.name == name)?;
    let bone = world.get(usize::from(attachment.bone))?;
    let local = crate::matrix::model_matrix(
        attachment.transform.translation,
        attachment.transform.scale,
        attachment.transform.rotation,
    );
    let matrix = crate::matrix::mat4_mul(bone, &local);
    matrix.iter().all(|value| value.is_finite()).then_some(AttachmentPose {
        affine: Affine {
            a: matrix[BIND_BASIS[0][0]],
            b: matrix[BIND_BASIS[0][1]],
            c: matrix[BIND_BASIS[1][0]],
            d: matrix[BIND_BASIS[1][1]],
            x: matrix[BIND_ORIGIN[0]],
            y: matrix[BIND_ORIGIN[1]],
        },
        depth: matrix[BIND_ORIGIN[2]],
    })
}

pub fn sampled_world(
    mesh: &Mesh,
    samples: &[(AnimationLayer, f32)],
    world: &mut Vec<Mat4>,
    mut local_matrices: Option<&mut Vec<Mat4>>,
) -> Result<()> {
    world.clear();
    if let Some(locals) = &mut local_matrices {
        locals.clear();
    }
    if samples.is_empty() {
        for bone in &mesh.bones {
            if let Some(locals) = &mut local_matrices {
                locals.push(bone.raw_rest);
            }
            world.push(bone.parent.map_or(bone.raw_rest, |parent| {
                crate::matrix::mat4_mul(&world[usize::from(parent)], &bone.raw_rest)
            }));
        }
        return Ok(());
    }
    if samples.iter().all(|(layer, _)| !layer.visible || layer.blend == 0.0) {
        return pose::rest_world(
            mesh,
            &samples.iter().map(|(layer, _)| layer).filter(|layer| layer.visible),
            world,
            local_matrices,
        )
        .ok_or_else(|| anyhow!("puppet rest world unavailable"));
    }
    let mut visible = samples.iter().filter(|(layer, _)| layer.visible);
    if let (Some((layer, seconds)), None) = (visible.next(), visible.next())
        && layer.blend == 1.0
        && !layer.additive
    {
        let sampled = AnimationLayer { rate: 1.0, ..*layer };
        let mut matrices = pose::Matrices {
            locals: local_matrices.as_mut().map(|locals| std::mem::take(*locals)),
            ..pose::Matrices::default()
        };
        let sampled =
            pose::matrices(mesh, &[sampled], *seconds, pose::Mode::Authored, &mut matrices);
        if let Some(locals) = &mut local_matrices {
            **locals =
                matrices.locals.take().ok_or_else(|| anyhow!("missing puppet local output"))?;
        }
        if sampled.is_some() {
            let complete = matrices.palette.iter().zip(&mesh.native_bind_inverse).try_for_each(
                |(pose, inverse)| {
                    world.push(crate::matrix::mat4_mul(
                        pose,
                        &crate::matrix::mat4_inverse(inverse.as_ref()?)?,
                    ));
                    Some(())
                },
            );
            if complete.is_some() && world.len() == mesh.bones.len() {
                return Ok(());
            }
        }
    }
    let binds = mesh
        .bones
        .iter()
        .map(|bone| native_bind_transform(bone.raw_rest))
        .collect::<Result<Vec<_>>>()?;
    let mut locals = binds.clone();
    blend_world(mesh, samples, &binds, &mut locals, world, local_matrices)
}

fn blend_world(
    mesh: &Mesh,
    samples: &[(AnimationLayer, f32)],
    binds: &[Transform],
    locals: &mut [Transform],
    world: &mut Vec<Mat4>,
    mut local_matrices: Option<&mut Vec<Mat4>>,
) -> Result<()> {
    locals.copy_from_slice(binds);
    for (layer, seconds) in samples.iter().filter(|(layer, _)| layer.visible && layer.blend > 0.0) {
        let animation = mesh
            .animations
            .iter()
            .find(|clip| clip.id == layer.id)
            .ok_or_else(|| anyhow!("puppet clip missing"))?;
        if !animation.fps.is_finite()
            || animation.fps <= 0.0
            || animation.duration == 0
            || animation.tracks.len() != mesh.bones.len()
            || animation.tracks.iter().any(|track| track.len() != animation.duration as usize + 1)
        {
            return Err(anyhow!("puppet clip invalid"));
        }
        for (index, local) in locals.iter_mut().enumerate() {
            if animation.disabled.as_ref().is_some_and(|mask| mask[index]) {
                continue;
            }
            let sampled = sample(animation, index, *seconds);
            *local = if layer.additive {
                add(*local, sampled, binds[index], layer.blend)
            } else {
                interpolate(*local, sampled, layer.blend)
            };
        }
    }
    world.clear();
    if let Some(matrices) = &mut local_matrices {
        matrices.clear();
    }
    for (bone, local) in mesh.bones.iter().zip(locals.iter()) {
        let matrix = crate::matrix::model_matrix(local.translation, local.scale, local.rotation);
        if let Some(matrices) = &mut local_matrices {
            matrices.push(matrix);
        }
        world.push(bone.parent.map_or(matrix, |parent| {
            crate::matrix::mat4_mul(&world[usize::from(parent)], &matrix)
        }));
    }
    Ok(())
}

fn local_transform(matrix: Mat4, spatial: bool) -> Result<Transform> {
    if spatial { native_bind_transform(matrix) } else { matrix_transform(matrix) }
}

fn native_bind_transform(matrix: Mat4) -> Result<Transform> {
    let [right, up, forward] = BIND_BASIS.map(|column| column.map(|index| matrix[index]));
    let mut scale = [
        right[0].hypot(right[1]).hypot(right[2]),
        up[0].hypot(up[1]).hypot(up[2]),
        forward[0].hypot(forward[1]).hypot(forward[2]),
    ];
    if scale.iter().any(|value| !value.is_finite() || *value <= f32::EPSILON) {
        return Err(anyhow!("puppet bind scale invalid"));
    }
    let cross = [
        up[1] * forward[2] - up[2] * forward[1],
        up[2] * forward[0] - up[0] * forward[2],
        up[0] * forward[1] - up[1] * forward[0],
    ];
    if right[0] * cross[0] + right[1] * cross[1] + right[2] * cross[2] < 0.0 {
        scale[0] = -scale[0];
    }
    let pitch_cosine = (right[0] / scale[0]).hypot(right[1] / scale[0]);
    let pitch = (-right[2] / scale[0]).atan2(pitch_cosine);
    let translation = BIND_ORIGIN.map(|index| matrix[index]);
    let rotation = [
        (up[2] / scale[1]).atan2(forward[2] / scale[2]),
        pitch,
        (right[1] / scale[0]).atan2(right[0] / scale[0]),
    ];
    let rebuilt = crate::matrix::model_matrix(translation, scale, rotation);
    if matrix.iter().zip(rebuilt).all(|(value, rebuilt)| {
        (value - rebuilt).abs() <= BIND_RECONSTRUCTION_TOLERANCE * value.abs().max(1.0)
    }) {
        return Ok(Transform { translation, rotation, scale });
    }
    let skew = |left: [f32; 3], left_scale: f32, right: [f32; 3], right_scale: f32| {
        let dot = left[0] * right[0] + left[1] * right[1] + left[2] * right[2];
        (dot / (left_scale.abs() * right_scale.abs())).abs()
    };
    if skew(right, scale[0], up, scale[1]) > BIND_SHEAR_TOLERANCE
        || skew(right, scale[0], forward, scale[2]) > BIND_SHEAR_TOLERANCE
        || skew(up, scale[1], forward, scale[2]) > BIND_SHEAR_TOLERANCE
    {
        return Err(anyhow!("puppet bind transform has shear"));
    }
    let rotation = if pitch_cosine < GIMBAL_COSINE {
        [(-forward[1] / scale[2]).atan2(up[1] / scale[1]), pitch, 0.0]
    } else {
        rotation
    };
    Ok(Transform { translation, rotation, scale })
}

pub fn layer_samples(
    layers: &[AnimationLayer],
    time: f32,
) -> impl Iterator<Item = (AnimationLayer, f32)> + '_ {
    layers.iter().map(move |layer| (AnimationLayer { rate: 1.0, ..*layer }, time * layer.rate))
}

pub fn skin(mesh: &Mesh, layers: &[AnimationLayer], time: f32) -> Frame {
    let samples = layer_samples(layers, time).collect::<Vec<_>>();
    skin_samples(mesh, &samples)
}

pub fn skin_samples(mesh: &Mesh, samples: &[(AnimationLayer, f32)]) -> Frame {
    let mut visible = samples.iter().filter(|(layer, _)| layer.visible);
    if let (Some((layer, seconds)), None) = (visible.next(), visible.next())
        && let Some(frame) = pose::skin(mesh, &[*layer], *seconds, pose::Mode::Legacy)
    {
        return frame;
    }
    let mut locals: Vec<Transform> = mesh.bones.iter().map(|bone| bone.rest).collect();
    for (layer, seconds) in samples.iter().filter(|(layer, _)| layer.visible && layer.blend > 0.0) {
        let Some(animation) = mesh.animations.iter().find(|animation| animation.id == layer.id)
        else {
            continue;
        };
        let blend = layer.blend.clamp(0.0, 1.0);
        for (index, local) in locals.iter_mut().enumerate() {
            if animation.disabled.as_ref().is_some_and(|mask| mask[index]) {
                continue;
            }
            let sampled = sample(animation, index, *seconds);
            *local = if layer.additive {
                add(*local, sampled, mesh.bones[index].rest, blend)
            } else {
                interpolate(*local, sampled, blend)
            };
        }
    }
    let posed = world(locals, &mesh.bones);
    let bones: std::sync::Arc<[Option<BonePose>]> = posed
        .iter()
        .zip(&mesh.bind_inverse)
        .map(|(pose, bind)| bind.map(|bind| BonePose::Planar(skin_transform(*pose, bind))))
        .collect();
    let positions = mesh.vertices.iter().map(|vertex| skin_vertex(vertex, &bones)).collect();
    Frame {
        bones,
        positions,
        attachments: mesh
            .attachments
            .iter()
            .map(|attachment| {
                let transform =
                    posed[usize::from(attachment.bone)].then(affine(attachment.transform));
                AttachmentPose { affine: transform, depth: attachment.transform.translation[2] }
            })
            .collect(),
    }
}

fn world(transforms: impl IntoIterator<Item = Transform>, bones: &[Bone]) -> Vec<Affine> {
    let mut out: Vec<Affine> = Vec::with_capacity(bones.len());
    for (index, transform) in transforms.into_iter().enumerate() {
        let local = affine(transform);
        out.push(bones[index].parent.map_or(local, |parent| out[usize::from(parent)].then(local)));
    }
    out
}

fn skin_transform(pose: Affine, bind: Affine) -> Affine {
    Affine {
        x: pose.x + pose.c * bind.y + pose.a * bind.x,
        y: pose.y + pose.d * bind.y + pose.b * bind.x,
        ..pose.then(bind)
    }
}

fn skin_vertex(vertex: &Vertex, palette: &[Option<BonePose>]) -> [f32; 3] {
    let mut out = [0.0_f32; 3];
    let mut total = 0.0_f32;
    for (bone, weight) in vertex.bones.into_iter().zip(vertex.weights) {
        if bone == u16::MAX || weight <= 0.0 {
            continue;
        }
        let index = usize::from(bone);
        let Some(transform) = palette.get(index).copied().flatten() else {
            continue;
        };
        let point = transform.point(vertex.position);
        for axis in 0..3 {
            out[axis] += point[axis] * weight;
        }
        total += weight;
    }
    if total <= f32::EPSILON {
        return vertex.position;
    }
    for value in &mut out {
        *value /= total;
    }
    out
}

fn sample(animation: &Animation, bone: usize, time: f32) -> Transform {
    let duration = animation.duration as f32;
    let mut frame = (time.max(0.0) * animation.fps).rem_euclid(if animation.mirror {
        duration * 2.0
    } else {
        duration
    });
    if animation.mirror && frame > duration {
        frame = duration * 2.0 - frame;
    }
    let first = frame.floor() as usize;
    let second = (first + 1).min(animation.duration as usize);
    interpolate(animation.tracks[bone][first], animation.tracks[bone][second], frame.fract())
}

fn add(base: Transform, value: Transform, bind: Transform, blend: f32) -> Transform {
    let mut out = base;
    for axis in 0..3 {
        out.translation[axis] += (value.translation[axis] - bind.translation[axis]) * blend;
        out.rotation[axis] += (value.rotation[axis] - bind.rotation[axis]) * blend;
        out.scale[axis] += (value.scale[axis] - bind.scale[axis]) * blend;
    }
    out
}

fn interpolate(first: Transform, second: Transform, amount: f32) -> Transform {
    let mut out = first;
    for axis in 0..3 {
        out.translation[axis] += (second.translation[axis] - first.translation[axis]) * amount;
        out.rotation[axis] += (second.rotation[axis] - first.rotation[axis]) * amount;
        out.scale[axis] += (second.scale[axis] - first.scale[axis]) * amount;
    }
    out
}

fn cstring<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a str> {
    let rest = bytes.get(*at..).ok_or_else(|| anyhow!("puppet string is truncated"))?;
    let len = rest
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| anyhow!("unterminated puppet string"))?;
    let text = std::str::from_utf8(&rest[..len])?;
    *at += len + 1;
    Ok(text)
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = at.checked_add(len).ok_or_else(|| anyhow!("puppet offset overflow"))?;
    let value = bytes.get(*at..end).ok_or_else(|| anyhow!("puppet data is truncated"))?;
    *at = end;
    Ok(value)
}

fn read_u16(bytes: &[u8], at: &mut usize) -> Result<u16> {
    Ok(u16::from_le_bytes(take(bytes, at, size_of::<u16>())?.try_into().unwrap()))
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Result<u32> {
    Ok(u32::from_le_bytes(take(bytes, at, 4)?.try_into().unwrap()))
}

fn read_f32(bytes: &[u8], at: &mut usize) -> Result<f32> {
    Ok(f32::from_le_bytes(take(bytes, at, 4)?.try_into().unwrap()))
}

fn parse_indices(bytes: &[u8], start: usize, count: usize, vertices: usize) -> Result<Vec<u16>> {
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let value = le_u16(bytes, start + index * 2)
            .ok_or_else(|| anyhow!("puppet indices are truncated"))?;
        if usize::from(value) >= vertices {
            return Err(anyhow!("puppet index {value} is outside {vertices} vertices"));
        }
        out.push(value);
    }
    Ok(out)
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|part| part == needle)
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le_f32(bytes: &[u8], at: usize) -> Result<f32> {
    let raw = bytes.get(at..at + 4).ok_or_else(|| anyhow!("puppet vertices are truncated"))?;
    Ok(f32::from_le_bytes(raw.try_into().unwrap()))
}

#[cfg(test)]
#[path = "puppet_tests.rs"]
pub(crate) mod tests;

fn affine(transform: Transform) -> Affine {
    Affine::new(
        [transform.translation[0], transform.translation[1]],
        [transform.scale[0], transform.scale[1]],
        transform.rotation[2],
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttachmentPose {
    pub affine: Affine,
    pub depth: f32,
}

impl From<Transform> for AttachmentPose {
    fn from(transform: Transform) -> Self {
        Self { affine: affine(transform), depth: transform.translation[2] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BonePose {
    Planar(Affine),
    Spatial(Mat4),
}

impl BonePose {
    pub fn matrix(self) -> Mat4 {
        match self {
            Self::Spatial(matrix) => matrix,
            Self::Planar(affine) => {
                let mut matrix = crate::matrix::model_matrix(
                    [affine.x, affine.y, 0.0],
                    [1.0; BIND_BASIS.len()],
                    [0.0; BIND_BASIS.len()],
                );
                matrix[BIND_BASIS[0][0]] = affine.a;
                matrix[BIND_BASIS[0][1]] = affine.b;
                matrix[BIND_BASIS[1][0]] = affine.c;
                matrix[BIND_BASIS[1][1]] = affine.d;
                matrix
            }
        }
    }

    pub fn point(self, point: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Planar(transform) => transform.point(point),
            Self::Spatial(matrix) => pose::point(&matrix, point),
        }
    }
}
