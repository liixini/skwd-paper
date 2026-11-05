use crate::read::Reader;
use anyhow::{Result, anyhow, ensure};

const VERSION4: &str = "MDLV0004";
const VERSION14: &str = "MDLV0014";
const VERSION16: &str = "MDLV0016";
const VERSION17: &str = "MDLV0017";
const VERSION21: &str = "MDLV0021";
const VERSION23: &str = "MDLV0023";
const HEADER_BYTES: usize = 9;
pub(crate) const POSITION_UV: u32 = 9;
pub(crate) const SKINNED: u32 = 0x0180_000f;
const SKIN_COMPONENTS: usize = 8;
const INFLUENCES: usize = 4;
const WEIGHT_TOLERANCE: f32 = f32::EPSILON * 4.0;
const MAX_BONES: usize = 128;
const BONE_OFFSET: usize = 10 * size_of::<f32>();
const WEIGHT_OFFSET: usize = BONE_OFFSET + 4 * size_of::<u32>();
const POSITION_NORMAL_UV: u32 = 11;
pub(crate) const POSITION_NORMAL_TANGENT_UV: u32 = 15;
const POSITION_NORMAL_TANGENT_UVS: u32 = 39;
const POSITION_UV_FLOATS: usize = 5;
const POSITION_NORMAL_UV_FLOATS: usize = 8;
const POSITION_NORMAL_TANGENT_UV_FLOATS: usize = 12;
const POSITION_NORMAL_TANGENT_UVS_FLOATS: usize = 14;
const MAX_BYTES: usize = 200 << 20;
const MAX_PARTS: usize = 4096;
const MAX_SKINS: usize = 128;
const MAX_PATH_BYTES: usize = 4096;
const TRIANGLE_INDICES: usize = 3;
const PART_TRAILER_BYTES: usize = 6;
const SHORT_PART_TRAILER_BYTES: usize = 2;
const UNNAMED_PART_FLAG: u32 = 0x400;
const NARROW_INDICES: u32 = 0;
const WIDE_INDICES: u32 = 1;
const VECTOR_COMPONENTS: usize = 3;
const CONSTRAINTS_UNSUPPORTED: &str = "skeletal mesh constraints unsupported";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: Option<[f32; 3]>,
    pub tangent: Option<[f32; 4]>,
    pub uv: [f32; 2],
    pub secondary_uv: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexFormat {
    Standard,
    Tangent,
    TangentLightmap,
}

impl VertexFormat {
    pub const fn floats(self) -> usize {
        match self {
            Self::Standard => POSITION_NORMAL_UV_FLOATS,
            Self::Tangent => POSITION_NORMAL_TANGENT_UV_FLOATS,
            Self::TangentLightmap => POSITION_NORMAL_TANGENT_UVS_FLOATS,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Indices {
    U16(Vec<u16>),
    U32(Vec<u32>),
}

impl Indices {
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::U16(indices) => indices.len(),
            Self::U32(indices) => indices.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn byte_len(&self) -> usize {
        match self {
            Self::U16(indices) => size_of_val(indices.as_slice()),
            Self::U32(indices) => size_of_val(indices.as_slice()),
        }
    }

    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Self::U16(indices) => indices.iter().flat_map(|index| index.to_ne_bytes()).collect(),
            Self::U32(indices) => indices.iter().flat_map(|index| index.to_ne_bytes()).collect(),
        }
    }

    fn max(&self) -> Option<usize> {
        match self {
            Self::U16(indices) => indices.iter().max().map(|index| usize::from(*index)),
            Self::U32(indices) => indices.iter().max().map(|index| *index as usize),
        }
    }
}

#[derive(Clone)]
pub struct Part {
    pub format: VertexFormat,
    pub materials: Vec<String>,
    pub vertices: Vec<Vertex>,
    pub indices: Indices,
    pub bounds: Option<([f32; 3], [f32; 3])>,
    pub skin: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct Mesh {
    pub parts: Vec<Part>,
    pub skins: usize,
    pub rig: Option<std::rc::Rc<crate::puppet::Mesh>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub vertex_buffer: Vec<f32>,
    pub vertex_format: Vec<Attribute>,
    pub material: String,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attribute {
    Position,
    Normal,
    Uv,
}

#[derive(serde::Deserialize)]
pub struct ModelData {
    pub shapes: Vec<Shape>,
}

impl ModelData {
    pub fn mesh(self) -> Result<Mesh> {
        ensure!(!self.shapes.is_empty() && self.shapes.len() <= MAX_PARTS, "invalid model shapes");
        let mut bytes = 0usize;
        let parts = self
            .shapes
            .into_iter()
            .map(|shape| {
                ensure!(
                    shape.vertex_format == [Attribute::Position, Attribute::Normal, Attribute::Uv],
                    "model vertex format unsupported"
                );
                ensure!(!shape.material.is_empty(), "model material missing");
                let stride = VertexFormat::Standard.floats();
                ensure!(
                    !shape.vertex_buffer.is_empty()
                        && shape.vertex_buffer.len().is_multiple_of(stride * TRIANGLE_INDICES),
                    "invalid model vertex buffer"
                );
                let count = shape.vertex_buffer.len() / stride;
                ensure!(count <= usize::from(u16::MAX) + 1, "model vertex count exceeded");
                ensure!(
                    shape.vertex_buffer.iter().all(|value| value.is_finite()),
                    "nonfinite model vertex"
                );
                bytes = bytes
                    .checked_add(
                        shape.vertex_buffer.len() * size_of::<f32>() + count * size_of::<u16>(),
                    )
                    .ok_or_else(|| anyhow!("model byte overflow"))?;
                ensure!(bytes <= MAX_BYTES, "model exceeds byte budget");
                let vertices = shape
                    .vertex_buffer
                    .chunks_exact(stride)
                    .map(|vertex| {
                        let (position, rest) = vertex.split_at(VECTOR_COMPONENTS);
                        let (normal, uv) = rest.split_at(position.len());
                        Vertex {
                            position: position.try_into().expect("position width"),
                            normal: Some(normal.try_into().expect("normal width")),
                            uv: uv.try_into().expect("UV width"),
                            tangent: None,
                            secondary_uv: None,
                        }
                    })
                    .collect();
                Ok(Part {
                    format: VertexFormat::Standard,
                    materials: vec![shape.material],
                    vertices,
                    indices: Indices::U16((0..count).map(|index| index as u16).collect()),
                    bounds: None,
                    skin: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Mesh { parts, skins: 1, rig: None })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Version {
    V4,
    V14,
    V16,
    V17,
    V21,
    V23,
}

impl Version {
    fn bounds(self) -> bool {
        matches!(self, Self::V17 | Self::V21 | Self::V23)
    }

    fn format_word(self) -> bool {
        matches!(self, Self::V16 | Self::V17 | Self::V21 | Self::V23)
    }

    fn part_trailer_bytes(self) -> usize {
        match self {
            Self::V21 => SHORT_PART_TRAILER_BYTES,
            Self::V23 => PART_TRAILER_BYTES,
            _ => 0,
        }
    }

    fn extension(self) -> bool {
        self != Self::V4
    }
}

fn floats<const N: usize>(reader: &mut Reader<'_>) -> Result<[f32; N]> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = reader.f32()?;
        ensure!(value.is_finite(), "nonfinite mesh component");
    }
    Ok(values)
}

fn vertices(reader: &mut Reader<'_>, format: u32) -> Result<Vec<Vertex>> {
    let components = match format {
        POSITION_UV => POSITION_UV_FLOATS,
        POSITION_NORMAL_UV => POSITION_NORMAL_UV_FLOATS,
        POSITION_NORMAL_TANGENT_UV => POSITION_NORMAL_TANGENT_UV_FLOATS,
        SKINNED => POSITION_NORMAL_TANGENT_UV_FLOATS + SKIN_COMPONENTS,
        POSITION_NORMAL_TANGENT_UVS => POSITION_NORMAL_TANGENT_UVS_FLOATS,
        _ => return Err(anyhow!("mesh vertex format {format} unsupported")),
    };
    let stride = components * size_of::<f32>();
    let bytes = reader.u32()? as usize;
    ensure!(bytes != 0 && bytes.is_multiple_of(stride), "invalid mesh vertex bytes {bytes}");
    let data = reader.take(bytes)?;
    let mut reader = Reader::new(data);
    (0..bytes / stride)
        .map(|_| {
            let position = floats(&mut reader)?;
            let normal = (format != POSITION_UV).then(|| floats(&mut reader)).transpose()?;
            let tangent = (format == POSITION_NORMAL_TANGENT_UV
                || format == POSITION_NORMAL_TANGENT_UVS
                || format == SKINNED)
                .then(|| floats(&mut reader))
                .transpose()?;
            if format == SKINNED {
                reader.take(SKIN_COMPONENTS * size_of::<f32>())?;
            }
            let uv = floats(&mut reader)?;
            let secondary_uv =
                (format == POSITION_NORMAL_TANGENT_UVS).then(|| floats(&mut reader)).transpose()?;
            Ok(Vertex { position, normal, tangent, uv, secondary_uv })
        })
        .collect()
}

fn part(
    reader: &mut Reader<'_>,
    data: &[u8],
    version: Version,
    format: u32,
    skins: usize,
) -> Result<Part> {
    let materials =
        (0..skins).map(|_| reader.nul_string(MAX_PATH_BYTES)).collect::<Result<Vec<_>>>()?;
    ensure!(materials.iter().all(|name| !name.is_empty()), "empty mesh material");
    let index_flags = reader.u32()?;
    let ignored = if version == Version::V23 { UNNAMED_PART_FLAG } else { 0 };
    let index_width = match index_flags & !ignored {
        NARROW_INDICES => size_of::<u16>(),
        WIDE_INDICES => size_of::<u32>(),
        _ => return Err(anyhow!("mesh metadata unsupported")),
    };
    let bounds = if version.bounds() {
        let minimum: [f32; 3] = floats(reader)?;
        let maximum: [f32; 3] = floats(reader)?;
        ensure!(minimum.iter().zip(maximum).all(|(min, max)| *min <= max), "invalid mesh bounds");
        Some((minimum, maximum))
    } else {
        None
    };
    if version.format_word() {
        ensure!(reader.u32()? == format, "mixed mesh vertex formats unsupported");
    }
    let vertex_start = reader.pos();
    let vertices = vertices(reader, format)?;
    let skin = if format == SKINNED {
        Some((vertex_start + size_of::<u32>(), reader.pos()))
    } else {
        None
    };
    let bytes = reader.u32()? as usize;
    ensure!(
        bytes != 0 && bytes.is_multiple_of(TRIANGLE_INDICES * index_width),
        "invalid mesh index bytes {bytes}"
    );
    let words = reader.take(bytes)?.chunks_exact(index_width);
    let indices = if index_width == size_of::<u32>() {
        Indices::U32(
            words.map(|word| u32::from_le_bytes(word.try_into().expect("index width"))).collect(),
        )
    } else {
        Indices::U16(
            words.map(|word| u16::from_le_bytes(word.try_into().expect("index width"))).collect(),
        )
    };
    ensure!(indices.max().is_none_or(|max| max < vertices.len()), "mesh index outside vertices");
    ensure!(
        reader.take(version.part_trailer_bytes())?.iter().all(|byte| *byte == 0),
        "mesh part extensions unsupported"
    );
    let format = match format {
        POSITION_NORMAL_TANGENT_UV | SKINNED => VertexFormat::Tangent,
        POSITION_NORMAL_TANGENT_UVS => VertexFormat::TangentLightmap,
        _ => VertexFormat::Standard,
    };
    let skin = skin.map(|(start, end)| data[start..end].to_vec());
    Ok(Part { format, materials, vertices, indices, bounds, skin })
}

pub fn parse(data: &[u8]) -> Result<Mesh> {
    ensure!(data.len() <= MAX_BYTES, "mesh exceeds byte budget");
    let mut reader = Reader::new(data);
    let version = match reader.nul_string(HEADER_BYTES)?.as_str() {
        VERSION4 => Version::V4,
        VERSION14 => Version::V14,
        VERSION16 => Version::V16,
        VERSION17 => Version::V17,
        VERSION21 => Version::V21,
        VERSION23 => Version::V23,
        value => return Err(anyhow!("mesh version {value} unsupported")),
    };
    let format = reader.u32()?;
    let skins = reader.u32()? as usize;
    let count = reader.u32()? as usize;
    ensure!(skins > 0 && skins <= MAX_SKINS, "mesh skin count {skins} unsupported");
    ensure!(count > 0 && count <= MAX_PARTS, "mesh part count {count} unsupported");
    let parts = (0..count)
        .map(|_| part(&mut reader, data, version, format, skins))
        .collect::<Result<Vec<_>>>()?;
    let rig = if format == SKINNED {
        ensure!(
            matches!(version, Version::V21 | Version::V23),
            "skeletal mesh version unsupported"
        );
        let (rig, _) = crate::puppet::rig(data, reader.pos(), count, true)?;
        ensure!(rig.bones.len() <= MAX_BONES, "skeletal mesh bone budget exceeded");
        ensure!(
            rig.animations.iter().all(|clip| clip.morph.is_none()
                && clip.channels.tracks.is_empty()
                && clip.alpha.tracks.iter().all(Option::is_none)),
            "skeletal mesh animation channels unsupported"
        );
        ensure!(rig.skipped_constraints == 0, CONSTRAINTS_UNSUPPORTED);
        for skin in parts.iter().filter_map(|part| part.skin.as_ref()) {
            for vertex in skin.chunks_exact(crate::puppet::VERTEX_STRIDE) {
                let mut total = 0.0;
                for index in 0..INFLUENCES {
                    let bone = u32::from_le_bytes(
                        vertex[BONE_OFFSET + index * size_of::<u32>()..][..size_of::<u32>()]
                            .try_into()
                            .expect("bone width"),
                    );
                    let weight = f32::from_le_bytes(
                        vertex[WEIGHT_OFFSET + index * size_of::<f32>()..][..size_of::<f32>()]
                            .try_into()
                            .expect("weight width"),
                    );
                    ensure!(bone < rig.bones.len() as u32, "mesh bone index outside skeleton");
                    ensure!(weight.is_finite() && weight >= 0.0, "invalid mesh bone weight");
                    total += weight;
                }
                ensure!((total - 1.0).abs() <= WEIGHT_TOLERANCE, "invalid mesh weight sum");
            }
        }
        Some(std::rc::Rc::new(rig))
    } else {
        if version.extension() {
            ensure!(reader.take(1)? == [0], "mesh extension unsupported");
        }
        ensure!(reader.pos() == data.len(), "trailing mesh data unsupported");
        None
    };
    Ok(Mesh { parts, skins, rig })
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
