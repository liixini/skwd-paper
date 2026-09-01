use anyhow::{Result, anyhow};

const MAGIC_21: &[u8; 8] = b"MDLV0021";
const MAGIC_23: &[u8; 8] = b"MDLV0023";
const MESH_SIGNATURE: &[u8; 4] = &[0x0f, 0x00, 0x80, 0x01];
const VERTEX_STRIDE: usize = 80;
const MAX_VERTICES: usize = u16::MAX as usize + 1;
const MAX_INDICES: usize = 3_000_000;
const MAX_ANIMATION_SAMPLES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub bones: [u16; 2],
    pub weights: [f32; 2],
}

#[derive(Debug, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub bones: Vec<Bone>,
    pub animations: Vec<Animation>,
    bind_inverse: Vec<Option<Affine>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, PartialEq)]
pub struct Bone {
    pub parent: Option<u16>,
    pub bind: Transform,
}

#[derive(Debug, PartialEq)]
pub struct Animation {
    pub id: u32,
    pub fps: f32,
    pub duration: u32,
    pub mirror: bool,
    pub tracks: Vec<Vec<Transform>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationLayer {
    pub id: u32,
    pub rate: f32,
    pub blend: f32,
    pub additive: bool,
}

pub fn parse(bytes: &[u8]) -> Result<Mesh> {
    let magic = bytes.get(..8).ok_or_else(|| anyhow!("puppet header is truncated"))?;
    if magic != MAGIC_21 && magic != MAGIC_23 {
        return Err(anyhow!("unsupported puppet header"));
    }
    let skeleton =
        find(bytes, b"MDLS0004\0").ok_or_else(|| anyhow!("puppet skeleton is missing"))?;
    let (start, vertex_bytes, index_bytes) = locate(bytes, skeleton)?;
    let vertex_count = vertex_bytes / VERTEX_STRIDE;
    let vertices = parse_vertices(bytes, start + 8, vertex_count)?;
    let indices_at = start + 8 + vertex_bytes + 4;
    let indices = parse_indices(bytes, indices_at, index_bytes / 2, vertex_count)?;
    let (bones, animation_at) = parse_bones(bytes, skeleton)?;
    let animations = parse_animations(bytes, animation_at, bones.len())?;
    let bind_inverse = world(bones.iter().map(|bone| bone.bind), &bones)
        .into_iter()
        .map(Affine::inverse)
        .collect();
    Ok(Mesh { vertices, indices, bones, animations, bind_inverse })
}

fn locate(bytes: &[u8], end: usize) -> Result<(usize, usize, usize)> {
    for start in 9..end.saturating_sub(12) {
        if bytes.get(start..start + 4) != Some(MESH_SIGNATURE) {
            continue;
        }
        let Some(vertex_bytes) = le_u32(bytes, start + 4).map(|value| value as usize) else {
            continue;
        };
        if vertex_bytes == 0 || vertex_bytes % VERTEX_STRIDE != 0 {
            continue;
        }
        let vertex_count = vertex_bytes / VERTEX_STRIDE;
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
        return Ok((start, vertex_bytes, index_bytes));
    }
    Err(anyhow!("puppet mesh is missing"))
}

fn parse_vertices(bytes: &[u8], start: usize, count: usize) -> Result<Vec<Vertex>> {
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let at = start + index * VERTEX_STRIDE;
        let vertex = Vertex {
            position: [le_f32(bytes, at)?, le_f32(bytes, at + 4)?, le_f32(bytes, at + 8)?],
            uv: [le_f32(bytes, at + 72)?, le_f32(bytes, at + 76)?],
            bones: [le_u16_at(bytes, at + 40)?, le_u16_at(bytes, at + 44)?],
            weights: [le_f32(bytes, at + 56)?, le_f32(bytes, at + 60)?],
        };
        if !vertex.position.into_iter().chain(vertex.uv).chain(vertex.weights).all(f32::is_finite) {
            return Err(anyhow!("puppet vertex {index} is not finite"));
        }
        out.push(vertex);
    }
    Ok(out)
}

fn parse_bones(bytes: &[u8], start: usize) -> Result<(Vec<Bone>, usize)> {
    let mut at = start;
    let magic = cstring(bytes, &mut at)?;
    if magic != "MDLS0004" {
        return Err(anyhow!("unsupported skeleton header {magic}"));
    }
    let animation_at = read_u32(bytes, &mut at)? as usize;
    let count = read_u32(bytes, &mut at)? as usize;
    if count == 0 || count > MAX_VERTICES {
        return Err(anyhow!("invalid puppet bone count {count}"));
    }
    let mut bones = Vec::with_capacity(count);
    for index in 0..count {
        take(bytes, &mut at, 1)?;
        read_u32(bytes, &mut at)?;
        let parent = read_u32(bytes, &mut at)?;
        let matrix_bytes = read_u32(bytes, &mut at)? as usize;
        if matrix_bytes != 64 {
            return Err(anyhow!("puppet bone {index} matrix uses {matrix_bytes} bytes"));
        }
        let mut matrix = [0.0_f32; 16];
        for value in &mut matrix {
            *value = read_f32(bytes, &mut at)?;
        }
        cstring(bytes, &mut at)?;
        let parent = if parent == u32::MAX {
            None
        } else {
            let parent = u16::try_from(parent)?;
            if usize::from(parent) >= index {
                return Err(anyhow!("puppet bone {index} has invalid parent {parent}"));
            }
            Some(parent)
        };
        bones.push(Bone { parent, bind: matrix_transform(matrix)? });
    }
    if animation_at < at || animation_at >= bytes.len() {
        return Err(anyhow!("invalid puppet animation offset {animation_at}"));
    }
    Ok((bones, animation_at))
}

fn parse_animations(bytes: &[u8], start: usize, bones: usize) -> Result<Vec<Animation>> {
    let mut at = start;
    let magic = cstring(bytes, &mut at)?;
    if magic != "MDLA0006" {
        return Err(anyhow!("unsupported animation header {magic}"));
    }
    let end = read_u32(bytes, &mut at)? as usize;
    let count = read_u32(bytes, &mut at)? as usize;
    if count == 0 || count > 1024 || end > bytes.len() || end <= at {
        return Err(anyhow!("invalid puppet animation directory"));
    }
    let mut out = Vec::with_capacity(count);
    let mut total_samples = 0_usize;
    for index in 0..count {
        if index != 0 {
            let padding = take(bytes, &mut at, 35)?;
            if padding.iter().any(|byte| *byte != 0) {
                return Err(anyhow!("puppet animation padding is not empty"));
            }
        }
        let id = read_u32(bytes, &mut at)?;
        read_u32(bytes, &mut at)?;
        cstring(bytes, &mut at)?;
        let mode = cstring(bytes, &mut at)?;
        let fps = read_f32(bytes, &mut at)?;
        let duration = read_u32(bytes, &mut at)?;
        read_u32(bytes, &mut at)?;
        let track_count = read_u32(bytes, &mut at)? as usize;
        if !fps.is_finite() || fps <= 0.0 || duration == 0 || track_count != bones {
            return Err(anyhow!("invalid puppet animation {id}"));
        }
        let mut tracks = Vec::with_capacity(track_count);
        for _ in 0..track_count {
            read_u32(bytes, &mut at)?;
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
            if samples.checked_mul(36) != Some(track_bytes) {
                return Err(anyhow!("invalid puppet animation track size {track_bytes}"));
            }
            let mut track = Vec::with_capacity(samples);
            for _ in 0..samples {
                track.push(read_transform(bytes, &mut at)?);
            }
            tracks.push(track);
        }
        out.push(Animation { id, fps, duration, mirror: mode == "mirror", tracks });
    }
    let remaining = end.saturating_sub(at);
    let padding = take(bytes, &mut at, remaining)?;
    if at != end || padding.iter().any(|byte| *byte != 0) {
        return Err(anyhow!("puppet animation section has trailing data"));
    }
    Ok(out)
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

fn matrix_transform(matrix: [f32; 16]) -> Result<Transform> {
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    x: f32,
    y: f32,
}

impl Affine {
    fn of(transform: Transform) -> Self {
        let (sin, cos) = transform.rotation[2].sin_cos();
        Self {
            a: cos * transform.scale[0],
            b: sin * transform.scale[0],
            c: -sin * transform.scale[1],
            d: cos * transform.scale[1],
            x: transform.translation[0],
            y: transform.translation[1],
        }
    }

    fn then(self, child: Self) -> Self {
        Self {
            a: self.a * child.a + self.c * child.b,
            b: self.b * child.a + self.d * child.b,
            c: self.a * child.c + self.c * child.d,
            d: self.b * child.c + self.d * child.d,
            x: self.a * child.x + self.c * child.y + self.x,
            y: self.b * child.x + self.d * child.y + self.y,
        }
    }

    fn inverse(self) -> Option<Self> {
        let determinant = self.a * self.d - self.b * self.c;
        if determinant.abs() <= f32::EPSILON {
            return None;
        }
        let inverse = determinant.recip();
        let (a, b, c, d) =
            (self.d * inverse, -self.b * inverse, -self.c * inverse, self.a * inverse);
        Some(Self { a, b, c, d, x: -(a * self.x + c * self.y), y: -(b * self.x + d * self.y) })
    }

    fn point(self, point: [f32; 3]) -> [f32; 3] {
        [
            self.a * point[0] + self.c * point[1] + self.x,
            self.b * point[0] + self.d * point[1] + self.y,
            point[2],
        ]
    }
}

pub fn skin(mesh: &Mesh, layers: &[AnimationLayer], time: f32) -> Vec<[f32; 3]> {
    let mut locals: Vec<Transform> = mesh.bones.iter().map(|bone| bone.bind).collect();
    for layer in layers.iter().filter(|layer| layer.blend > 0.0) {
        let Some(animation) = mesh.animations.iter().find(|animation| animation.id == layer.id)
        else {
            continue;
        };
        let blend = layer.blend.clamp(0.0, 1.0);
        for (index, local) in locals.iter_mut().enumerate() {
            let sampled = sample(animation, index, time * layer.rate);
            *local = if layer.additive {
                add(*local, sampled, mesh.bones[index].bind, blend)
            } else {
                interpolate(*local, sampled, blend)
            };
        }
    }
    let posed = world(locals, &mesh.bones);
    mesh.vertices.iter().map(|vertex| skin_vertex(vertex, &mesh.bind_inverse, &posed)).collect()
}

fn world(transforms: impl IntoIterator<Item = Transform>, bones: &[Bone]) -> Vec<Affine> {
    let mut out: Vec<Affine> = Vec::with_capacity(bones.len());
    for (index, transform) in transforms.into_iter().enumerate() {
        let local = Affine::of(transform);
        out.push(bones[index].parent.map_or(local, |parent| out[usize::from(parent)].then(local)));
    }
    out
}

fn skin_vertex(vertex: &Vertex, bind: &[Option<Affine>], posed: &[Affine]) -> [f32; 3] {
    let mut out = [0.0_f32; 3];
    let mut total = 0.0_f32;
    for (bone, weight) in vertex.bones.into_iter().zip(vertex.weights) {
        if bone == u16::MAX || weight <= 0.0 {
            continue;
        }
        let index = usize::from(bone);
        let Some(transform) = bind
            .get(index)
            .copied()
            .flatten()
            .and_then(|inverse| posed.get(index).map(|pose| pose.then(inverse)))
        else {
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

fn read_u32(bytes: &[u8], at: &mut usize) -> Result<u32> {
    Ok(u32::from_le_bytes(take(bytes, at, 4)?.try_into().unwrap()))
}

fn read_f32(bytes: &[u8], at: &mut usize) -> Result<f32> {
    Ok(f32::from_le_bytes(take(bytes, at, 4)?.try_into().unwrap()))
}

fn le_u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    le_u16(bytes, at).ok_or_else(|| anyhow!("puppet vertices are truncated"))
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
mod tests;
