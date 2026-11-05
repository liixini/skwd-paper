use super::{AnimationLayer, FLOAT_BYTES, Mesh, cstring, read_f32, read_u32, take};
use anyhow::{Result, anyhow, ensure};

pub const COMBO: &str = "BLENDROWCOUNT";
pub const UNIFORM: &str = "g_BlendMap";
pub const COMPONENTS: usize = 4;
const MAX_ROWS: usize = 4;
const MESH_COUNT_OFFSET: usize = 9 + 2 * size_of::<u32>();
const CHANNEL_MESH: u32 = 2;
const VERTEX_FLAGS: u32 = 0x0080_0021;
pub const POSITION_BYTES: usize = size_of::<[f32; 3]>();
pub const INDEX_BYTES: usize = size_of::<[u32; COMPONENTS]>();
const UV_BYTES: usize = size_of::<[f32; COMPONENTS]>();
pub const STRIDE: usize = POSITION_BYTES + INDEX_BYTES + UV_BYTES;
const BOUNDS_BYTES: usize = size_of::<[f32; 6]>();

#[derive(Debug, PartialEq)]
pub struct Geometry {
    pub material: String,
    pub rows: usize,
    pub vertices: Vec<u8>,
    pub indices: Vec<u16>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Clip {
    pub tracks: Vec<Vec<f32>>,
}

pub(super) fn blobs<'a>(bytes: &'a [u8], at: &mut usize, version: &[u8]) -> Result<&'a [u8]> {
    if version != super::MAGIC_21 && version != super::MAGIC_23 {
        return Ok(&[]);
    }
    if take(bytes, at, 1)? != [0] {
        read_u32(bytes, at)?;
        let length = read_u32(bytes, at)? as usize;
        take(bytes, at, length)?;
    }
    if take(bytes, at, 1)? == [0] {
        return Ok(&[]);
    }
    let length = read_u32(bytes, at)? as usize;
    take(bytes, at, length)
}

fn tail(bytes: &[u8], at: &mut usize, version: &[u8]) -> Result<()> {
    blobs(bytes, at, version)?;
    if version == super::MAGIC_23 {
        ensure!(read_u32(bytes, at)? == 0, "puppet channel mesh regions unsupported");
    }
    Ok(())
}

pub(super) fn parse(bytes: &[u8], start: usize, end: usize) -> Result<Option<Geometry>> {
    let mut header = MESH_COUNT_OFFSET;
    let count = read_u32(bytes, &mut header)?;
    if count == 1 {
        return Ok(None);
    }
    ensure!(count == 2, "puppet auxiliary mesh count unsupported");
    let version = match bytes.get(..super::MAGIC_23.len()) {
        Some(version)
            if version == super::MAGIC_23
                || version == super::MAGIC_21
                || version == super::MAGIC_19 =>
        {
            version
        }
        _ => return Err(anyhow!("puppet channel mesh version unsupported")),
    };
    let bytes = bytes.get(..end).ok_or_else(|| anyhow!("puppet channel mesh bounds"))?;
    let mut at = start;
    tail(bytes, &mut at, version)?;
    let material = cstring(bytes, &mut at)?.to_owned();
    ensure!(read_u32(bytes, &mut at)? == CHANNEL_MESH, "puppet auxiliary mesh unsupported");
    let rows = read_u32(bytes, &mut at)? as usize;
    ensure!((1..=MAX_ROWS).contains(&rows), "puppet texture channel rows invalid");
    take(bytes, &mut at, BOUNDS_BYTES)?;
    ensure!(read_u32(bytes, &mut at)? == VERTEX_FLAGS, "puppet channel vertex layout unsupported");
    let length = read_u32(bytes, &mut at)? as usize;
    ensure!(length != 0 && length.is_multiple_of(STRIDE), "puppet channel vertex length invalid");
    ensure!(length / STRIDE <= super::MAX_VERTICES, "puppet channel vertex count exceeds limit");
    let vertices = take(bytes, &mut at, length)?.to_vec();
    validate(&vertices, rows)?;
    let length = read_u32(bytes, &mut at)? as usize;
    ensure!(
        length != 0 && length.is_multiple_of(super::TRIANGLE_BYTES),
        "puppet channel index length invalid"
    );
    ensure!(
        length / size_of::<u16>() <= super::MAX_INDICES,
        "puppet channel index count exceeds limit"
    );
    let indices =
        super::parse_indices(bytes, at, length / size_of::<u16>(), vertices.len() / STRIDE)?;
    take(bytes, &mut at, length)?;
    tail(bytes, &mut at, version)?;
    ensure!(at == end, "puppet channel mesh trailing data");
    Ok(Some(Geometry { material, rows, vertices, indices }))
}

fn validate(vertices: &[u8], rows: usize) -> Result<()> {
    for vertex in vertices.chunks_exact(STRIDE) {
        let target = super::le_u32(vertex, POSITION_BYTES).expect("channel vertex index");
        ensure!((target as usize) < rows * COMPONENTS, "puppet texture channel index invalid");
        for field in [&vertex[..POSITION_BYTES], &vertex[POSITION_BYTES + INDEX_BYTES..]] {
            ensure!(
                field
                    .chunks_exact(FLOAT_BYTES)
                    .all(|word| f32::from_le_bytes(word.try_into().expect("channel vertex float"))
                        .is_finite()),
                "puppet texture channel vertex not finite"
            );
        }
    }
    Ok(())
}

pub(super) fn clip(bytes: &[u8], at: &mut usize, duration: u32) -> Result<Clip> {
    let count = read_u32(bytes, at)? as usize;
    ensure!(count <= MAX_ROWS * COMPONENTS, "puppet texture channel count exceeds limit");
    Ok(Clip { tracks: tracks(bytes, at, duration, count)? })
}

pub(super) fn tracks(
    bytes: &[u8],
    at: &mut usize,
    duration: u32,
    count: usize,
) -> Result<Vec<Vec<f32>>> {
    let samples = duration as usize + 1;
    ensure!(
        count.saturating_mul(samples) <= super::MAX_ANIMATION_SAMPLES,
        "puppet scalar samples exceed limit"
    );
    let mut tracks = Vec::with_capacity(count);
    for _ in 0..count {
        read_u32(bytes, at)?;
        ensure!(
            read_u32(bytes, at)? as usize == samples * FLOAT_BYTES,
            "puppet scalar sample size invalid"
        );
        let track = (0..samples).map(|_| read_f32(bytes, at)).collect::<Result<Vec<_>>>()?;
        ensure!(track.iter().all(|value| value.is_finite()), "puppet scalar sample not finite");
        tracks.push(track);
    }
    Ok(tracks)
}

pub fn weights(mesh: &Mesh, samples: &[(AnimationLayer, f32)]) -> Result<Vec<f32>> {
    let Some(geometry) = &mesh.texture_channels else { return Ok(Vec::new()) };
    let mut weights = vec![0.0; geometry.rows * COMPONENTS];
    for (layer, seconds) in samples.iter().filter(|(layer, _)| layer.visible) {
        let Some(animation) = mesh.animations.iter().find(|clip| clip.id == layer.id) else {
            continue;
        };
        let (first, next, amount) = animation.sample_frame(*seconds);
        ensure!(
            animation.channels.tracks.len() <= weights.len(),
            "puppet texture channel palette exceeds mesh"
        );
        for (target, track) in weights.iter_mut().zip(&animation.channels.tracks) {
            let weight = track[first] + (track[next] - track[first]) * amount;
            if layer.additive {
                *target += weight * layer.blend;
            } else {
                *target += (weight - *target) * layer.blend;
            }
        }
    }
    Ok(weights)
}

#[cfg(test)]
mod tests;
