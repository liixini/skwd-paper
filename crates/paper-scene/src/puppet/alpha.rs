use super::{AnimationLayer, Mesh, channels, take};
use crate::shader::{Uniform, UniformKind};
use anyhow::{Result, ensure};

pub const COMBO: &str = "SKINNING_ALPHA";
pub const UNIFORM: &str = "g_BonesAlpha";
pub(super) const ALPHA_FLAG: u32 = 4;
const TRACK_MASK: u32 = 3;
const DISABLED: u32 = 1;

#[derive(Debug, Default, PartialEq)]
pub struct Clip {
    pub tracks: Vec<Option<Vec<f32>>>,
}

pub struct Uniforms {
    pub name: String,
    pub weights: Vec<f32>,
}

pub(super) fn clip(bytes: &[u8], at: &mut usize, duration: u32, flags: &[u32]) -> Result<Clip> {
    if take(bytes, at, 1)? == [0] {
        return Ok(Clip::default());
    }
    let tracks = channels::tracks(bytes, at, duration, flags.len())?
        .into_iter()
        .zip(flags)
        .map(|(track, flag)| (flag & TRACK_MASK != DISABLED).then_some(track))
        .collect();
    Ok(Clip { tracks })
}

pub fn weights(mesh: &Mesh, samples: &[(AnimationLayer, f32)]) -> Vec<f32> {
    let mut weights = vec![1.0; mesh.bones.len()];
    if !mesh.alpha {
        return weights;
    }
    for (layer, seconds) in samples.iter().filter(|(layer, _)| layer.visible) {
        let Some(animation) = mesh.animations.iter().find(|clip| clip.id == layer.id) else {
            continue;
        };
        let (first, next, amount) = animation.sample_frame(*seconds);
        for (target, track) in weights.iter_mut().zip(&animation.alpha.tracks) {
            let Some(track) = track else { continue };
            let value = track[first] + (track[next] - track[first]) * amount;
            *target = if layer.additive {
                (*target + (value - 1.0) * layer.blend).clamp(target.min(value), target.max(value))
            } else {
                *target + (value - *target) * layer.blend
            };
        }
    }
    weights
}

pub fn vertex_alphas(mesh: &Mesh, samples: &[(AnimationLayer, f32)], alphas: &mut Vec<f32>) {
    alphas.clear();
    if !mesh.alpha {
        return;
    }
    let bones = weights(mesh, samples);
    alphas.extend(mesh.vertices.iter().map(|vertex| {
        vertex
            .bones
            .iter()
            .zip(vertex.weights)
            .map(|(bone, weight)| bones.get(usize::from(*bone)).map_or(0.0, |alpha| alpha * weight))
            .sum::<f32>()
            .clamp(0.0, 1.0)
    }));
}

pub fn uniforms(
    mesh: &Mesh,
    uniforms: &[Uniform],
    samples: &[(AnimationLayer, f32)],
) -> Result<Option<Uniforms>> {
    let Some(uniform) =
        uniforms.iter().find(|uniform| uniform.name.split('[').next() == Some(UNIFORM))
    else {
        ensure!(!mesh.alpha, "puppet alpha palette missing");
        return Ok(None);
    };
    ensure!(
        uniform.kind == UniformKind::Float && uniform.count == mesh.bones.len(),
        "puppet alpha palette invalid"
    );
    Ok(Some(Uniforms { name: uniform.name.clone(), weights: weights(mesh, samples) }))
}

#[cfg(test)]
mod tests;
