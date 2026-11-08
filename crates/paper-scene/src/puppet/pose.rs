#[path = "inverse.rs"]
mod inverse;
#[cfg(target_arch = "x86_64")]
#[path = "trig.rs"]
mod trig;

use super::{AnimationLayer, Bone, Frame, Mesh, Vertex};
#[cfg(target_arch = "x86_64")]
use super::{BonePose, Transform};
use crate::matrix::Mat4;

const AXES: usize = 3;
const SIDE: usize = 4;
const ORIGIN: usize = SIDE * AXES;
const HOMOGENEOUS: usize = SIDE * SIDE - 1;
const Z: usize = 2;
#[cfg(target_arch = "x86_64")]
static FMA: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::is_x86_feature_detected!("fma"));

pub(super) fn bind_inverse(bones: &[Bone]) -> Vec<Option<Mat4>> {
    let mut world: Vec<Option<Mat4>> = Vec::with_capacity(bones.len());
    for bone in bones {
        let raw = bone.raw_bind;
        let affine = (0..AXES).all(|col| raw[col * SIDE + AXES] == 0.0)
            && (raw[HOMOGENEOUS] - 1.0).abs() <= f32::EPSILON;
        let matrix = if affine {
            match bone.parent {
                Some(parent) => world
                    .get(usize::from(parent))
                    .copied()
                    .flatten()
                    .map(|parent| compose(&parent, &raw)),
                None => Some(raw),
            }
        } else {
            None
        };
        world.push(matrix);
    }
    world.into_iter().map(|matrix| matrix.and_then(|matrix| inverse::inverse(&matrix))).collect()
}

#[derive(Default)]
pub(super) struct Matrices {
    pub world: Vec<Mat4>,
    pub locals: Option<Vec<Mat4>>,
    pub palette: Vec<Mat4>,
}

pub(super) fn rest(
    mesh: &Mesh,
    samples: &[(AnimationLayer, f32)],
    out: &mut Matrices,
) -> Option<()> {
    if mesh.bones.len() != mesh.native_bind_inverse.len() {
        return None;
    }
    rest_world(
        mesh,
        &samples.iter().map(|(layer, _)| layer).filter(|layer| layer.visible),
        &mut out.world,
        out.locals.as_mut(),
    )?;
    out.palette.clear();
    for (pose, inverse) in out.world.iter().zip(&mesh.native_bind_inverse) {
        out.palette.push(compose(pose, inverse.as_ref()?));
    }
    Some(())
}

pub(super) fn rest_world<'a>(
    mesh: &Mesh,
    layers: &(impl Iterator<Item = &'a AnimationLayer> + Clone),
    world: &mut Vec<Mat4>,
    mut locals: Option<&mut Vec<Mat4>>,
) -> Option<()> {
    let active = !mesh.spatial
        && layers.clone().next().is_some()
        && layers.clone().all(|layer| {
            mesh.animations.iter().find(|animation| animation.id == layer.id).is_some_and(
                |animation| {
                    animation.disabled.is_none()
                        && animation.duration > 0
                        && animation.tracks.len() == mesh.bones.len()
                        && animation
                            .tracks
                            .iter()
                            .all(|track| track.len() == animation.duration as usize + 1)
                },
            )
        });
    world.clear();
    if let Some(locals) = &mut locals {
        locals.clear();
    }
    for bone in &mesh.bones {
        let local = if active { rest_matrix(bone, layers.clone()) } else { bone.raw_rest };
        if let Some(locals) = &mut locals {
            locals.push(local);
        }
        world.push(match bone.parent {
            Some(parent) => compose(world.get(usize::from(parent))?, &local),
            None => local,
        });
    }
    Some(())
}

#[cfg(target_arch = "x86_64")]
fn rest_matrix<'a>(bone: &Bone, layers: impl Iterator<Item = &'a AnimationLayer>) -> Mat4 {
    let mut identity = [1.0, 0.0, 0.0, 0.0];
    normalize(&mut identity);
    let mut rotation = bone.native_rest.rotation;
    for layer in layers {
        if layer.additive {
            for value in &mut rotation {
                *value *= identity[0];
            }
        } else {
            normalize(&mut rotation);
        }
    }
    let translation = std::array::from_fn(|axis| bone.raw_rest[ORIGIN + axis]);
    matrix(rotation, translation, bone.native_rest.scale)
}

#[cfg(not(target_arch = "x86_64"))]
fn rest_matrix<'a>(bone: &Bone, _layers: impl Iterator<Item = &'a AnimationLayer>) -> Mat4 {
    bone.raw_rest
}

fn compose(left: &Mat4, right: &Mat4) -> Mat4 {
    let mut out = [0.0; SIDE * SIDE];
    for col in 0..SIDE {
        for row in 0..AXES {
            let mut depth = right[col * SIDE + Z] * left[Z * SIDE + row];
            if col == AXES {
                depth += left[ORIGIN + row];
            }
            out[col * SIDE + row] =
                (depth + right[col * SIDE + 1] * left[SIDE + row]) + right[col * SIDE] * left[row];
        }
    }
    out[HOMOGENEOUS] = 1.0;
    out
}

pub(super) fn point(matrix: &Mat4, point: [f32; AXES]) -> [f32; AXES] {
    std::array::from_fn(|row| {
        ((point[0] * matrix[row] + point[1] * matrix[SIDE + row])
            + point[Z] * matrix[Z * SIDE + row])
            + matrix[ORIGIN + row]
    })
}

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Legacy,
    Authored,
}

#[cfg(not(target_arch = "x86_64"))]
pub(super) fn skin(
    _mesh: &Mesh,
    _layers: &[AnimationLayer],
    _time: f32,
    _mode: Mode,
) -> Option<Frame> {
    None
}

#[cfg(target_arch = "x86_64")]
pub(super) fn skin(mesh: &Mesh, layers: &[AnimationLayer], time: f32, mode: Mode) -> Option<Frame> {
    let mut sampled = Matrices::default();
    matrices(mesh, layers, time, mode, &mut sampled)?;
    let matrices = sampled.palette;
    let positions: Option<Vec<[f32; AXES]>> = mesh
        .vertices
        .iter()
        .map(|vertex| skin_vertex(vertex, |index| matrices.get(usize::from(index))))
        .collect();
    Some(Frame {
        positions: positions?,
        bones: matrices.into_iter().map(|matrix| Some(BonePose::Spatial(matrix))).collect(),
        attachments: Vec::new(),
    })
}

#[cfg(not(target_arch = "x86_64"))]
pub(super) fn matrices(
    _mesh: &Mesh,
    _layers: &[AnimationLayer],
    _time: f32,
    _mode: Mode,
    _out: &mut Matrices,
) -> Option<()> {
    None
}

#[cfg(target_arch = "x86_64")]
pub(super) fn matrices(
    mesh: &Mesh,
    layers: &[AnimationLayer],
    time: f32,
    mode: Mode,
    out: &mut Matrices,
) -> Option<()> {
    let [layer] = layers else {
        return None;
    };
    if !layer.visible
        || layer.additive
        || !layer.blend.is_finite()
        || !(0.0..=1.0).contains(&layer.blend)
        || !layer.rate.is_finite()
        || layer.rate < 0.0
        || !time.is_finite()
        || time < 0.0
        || !mesh.attachments.is_empty()
        || mesh.skipped_constraints != 0
        || mesh.native_bind_inverse.len() != mesh.bones.len()
    {
        return None;
    }
    let animation = mesh.animations.iter().find(|animation| animation.id == layer.id)?;
    if animation.duration == 0
        || !animation.fps.is_finite()
        || animation.fps <= 0.0
        || animation.tracks.len() != mesh.bones.len()
        || animation.disabled.as_ref().is_some_and(|mask| mask.iter().any(|disabled| *disabled))
    {
        return None;
    }
    let spatial = animation.spatial
        || mesh.bones.iter().any(|bone| {
            bone.raw_bind[Z] != 0.0
                || bone.raw_bind[SIDE + Z] != 0.0
                || bone.raw_bind[Z * SIDE] != 0.0
                || bone.raw_bind[Z * SIDE + 1] != 0.0
        });
    if matches!(mode, Mode::Legacy) && (!spatial || layer.blend != 1.0) {
        return None;
    }
    let duration = animation.duration as f32;
    let interval = 1.0 / animation.fps;
    let elapsed = (time * layer.rate) / interval;
    if !elapsed.is_finite() {
        return None;
    }
    let mut frame = elapsed.rem_euclid(if animation.mirror { duration * 2.0 } else { duration });
    if animation.mirror && frame > duration {
        frame = duration * 2.0 - frame;
    }
    let first = frame.floor() as usize;
    let second = (first + 1).min(animation.duration as usize);
    let amount = frame.fract();
    let fma = *FMA;
    if let Some(locals) = &mut out.locals {
        locals.clear();
    }
    out.world.clear();
    out.palette.clear();
    for (index, bone) in mesh.bones.iter().enumerate() {
        let track = &animation.tracks[index];
        if track.len() != animation.duration as usize + 1 {
            return None;
        }
        let first = *track.get(first)?;
        let second = *track.get(second)?;
        let local = sample(first, second, amount, fma)?;
        let local = if layer.blend == 1.0 { local } else { local.blend(bone, layer.blend) };
        let local = local.matrix();
        if let Some(locals) = &mut out.locals {
            locals.push(local);
        }
        let matrix = match bone.parent {
            Some(parent) => compose(out.world.get(usize::from(parent))?, &local),
            None => local,
        };
        if matrix.iter().any(|value| !value.is_finite()) {
            return None;
        }
        out.world.push(matrix);
    }
    for (pose, bind) in out.world.iter().zip(&mesh.native_bind_inverse) {
        out.palette.push(compose(pose, bind.as_ref()?));
    }
    if out.palette.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    Some(())
}

#[cfg(target_arch = "x86_64")]
struct Local {
    rotation: [f32; SIDE],
    translation: [f32; AXES],
    scale: [f32; AXES],
}

#[cfg(target_arch = "x86_64")]
impl Local {
    fn matrix(self) -> Mat4 {
        matrix(self.rotation, self.translation, self.scale)
    }

    fn blend(self, bone: &Bone, amount: f32) -> Self {
        let rotation = blend_rotation(bone.native_rest.rotation, self.rotation, amount);
        let inverse = 1.0 - amount;
        Self {
            rotation,
            translation: std::array::from_fn(|axis| {
                inverse * bone.raw_rest[ORIGIN + axis] + amount * self.translation[axis]
            }),
            scale: std::array::from_fn(|axis| {
                inverse * bone.native_rest.scale[axis] + amount * self.scale[axis]
            }),
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn sample(first: Transform, second: Transform, amount: f32, fma: bool) -> Option<Local> {
    if [first, second].iter().any(|transform| {
        transform
            .translation
            .iter()
            .chain(&transform.rotation)
            .chain(&transform.scale)
            .any(|value| !value.is_finite())
    }) {
        return None;
    }
    let rotation =
        blend_rotation(quaternion(first.rotation, fma), quaternion(second.rotation, fma), amount);
    let inverse = 1.0 - amount;
    let translation = std::array::from_fn(|axis| {
        inverse * first.translation[axis] + amount * second.translation[axis]
    });
    let scale =
        std::array::from_fn(|axis| inverse * first.scale[axis] + amount * second.scale[axis]);
    Some(Local { rotation, translation, scale })
}

#[cfg(target_arch = "x86_64")]
fn blend_rotation(first: [f32; SIDE], second: [f32; SIDE], amount: f32) -> [f32; SIDE] {
    const SIGN: u32 = 1 << (u32::BITS - 1);
    let dot = (second[AXES] * first[AXES] + second[Z] * first[Z])
        + (second[1] * first[1] + second[0] * first[0]);
    let signed = f32::from_bits(amount.to_bits() ^ (dot.to_bits() & SIGN));
    let inverse = 1.0 - amount;
    let mut rotation = std::array::from_fn(|axis| inverse * first[axis] + signed * second[axis]);
    normalize(&mut rotation);
    rotation
}

#[cfg(target_arch = "x86_64")]
fn quaternion(angles: [f32; AXES], fma: bool) -> [f32; SIDE] {
    half_turn(
        angles.map(|angle| trig::evaluate(angle * 0.5, false, fma)),
        angles.map(|angle| trig::evaluate(angle * 0.5, true, fma)),
    )
}

pub(super) fn half_turn(sines: [f32; AXES], cosines: [f32; AXES]) -> [f32; SIDE] {
    let [sx, sy, sz] = sines;
    let [cx, cy, cz] = cosines;
    let sy_sx = sy * sx;
    let sy_cx = sy * cx;
    let sx_cy = sx * cy;
    let cy_cx = cy * cx;
    [
        sy_sx * sz + cy_cx * cz,
        sx_cy * cz - sy_cx * sz,
        sx_cy * sz + sy_cx * cz,
        cy_cx * sz - sy_sx * cz,
    ]
}

#[cfg(target_arch = "x86_64")]
fn normalize(rotation: &mut [f32; SIDE]) {
    use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ps, _mm_set1_ps};
    const NEWTON_THREE: f32 = 3.0;
    let norm = (rotation[AXES] * rotation[AXES] + rotation[Z] * rotation[Z])
        + (rotation[1] * rotation[1] + rotation[0] * rotation[0]);
    let seed = unsafe { _mm_cvtss_f32(_mm_rsqrt_ps(_mm_set1_ps(norm))) };
    let square = seed * seed;
    let half = seed * 0.5;
    let factor = (NEWTON_THREE - square * norm) * half;
    for value in rotation {
        *value *= factor;
    }
}

pub(super) fn matrix(rotation: [f32; SIDE], translation: [f32; AXES], scale: [f32; AXES]) -> Mat4 {
    let [w, x, y, z] = rotation;
    let yy = y * y;
    let xy = x * y;
    let yw = y * w;
    let zz = z * z;
    let yz = y * z;
    let xz = x * z;
    let zw = z * w;
    let xx = x * x;
    let xw = x * w;
    let yzsum = zz + yy;
    let xzsum = zz + xx;
    let xysum = yy + xx;
    let xyzw = zw + xy;
    let xz_yw = xz - yw;
    let xy_zw = xy - zw;
    let xwyz = xw + yz;
    let xzyw = yw + xz;
    let yz_xw = yz - xw;
    let mut out = [
        1.0 - (yzsum + yzsum),
        xyzw + xyzw,
        xz_yw + xz_yw,
        0.0,
        xy_zw + xy_zw,
        1.0 - (xzsum + xzsum),
        xwyz + xwyz,
        0.0,
        xzyw + xzyw,
        yz_xw + yz_xw,
        1.0 - (xysum + xysum),
        0.0,
        translation[0],
        translation[1],
        translation[Z],
        1.0,
    ];
    for col in 0..AXES {
        for row in 0..AXES {
            out[col * SIDE + row] *= scale[col];
        }
    }
    out
}

pub(super) fn skin_vertex<'a>(
    vertex: &Vertex,
    palette: impl Fn(u16) -> Option<&'a Mat4>,
) -> Option<[f32; AXES]> {
    if vertex.position.iter().any(|value| !value.is_finite())
        || vertex.weights.iter().any(|weight| !weight.is_finite() || *weight < 0.0)
        || !vertex.weights.iter().any(|weight| *weight > 0.0)
    {
        return None;
    }
    let [first, second, third, fourth] = vertex.bones;
    let matrices = [palette(first)?, palette(second)?, palette(third)?, palette(fourth)?];
    let mixed = std::array::from_fn(|slot| {
        ((matrices[0][slot] * vertex.weights[0] + matrices[1][slot] * vertex.weights[1])
            + matrices[Z][slot] * vertex.weights[Z])
            + matrices[AXES][slot] * vertex.weights[AXES]
    });
    let point = point(&mixed, vertex.position);
    point.iter().all(|value| value.is_finite()).then_some(point)
}

#[cfg(all(test, target_arch = "x86_64"))]
#[path = "pose_tests.rs"]
mod tests;
