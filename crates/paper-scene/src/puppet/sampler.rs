use super::{AnimationLayer, Mesh, Transform, pose};
use crate::matrix::Mat4;
use anyhow::{Result, anyhow, ensure};
use std::rc::Rc;

#[derive(Default)]
pub(super) struct Scratch {
    matrices: pose::Matrices,
    binds: Vec<Transform>,
    locals: Vec<Transform>,
}

impl Scratch {
    pub(super) fn sample(
        &mut self,
        mesh: &Mesh,
        samples: &[(AnimationLayer, f32)],
    ) -> Result<&[Mat4]> {
        ensure!(
            samples.iter().all(|(layer, seconds)| seconds.is_finite()
                && *seconds >= 0.0
                && layer.blend.is_finite()
                && (0.0..=1.0).contains(&layer.blend)),
            "puppet palette sample invalid"
        );
        if samples.iter().all(|(layer, _)| !layer.visible || layer.blend == 0.0) {
            pose::rest(mesh, samples, &mut self.matrices)
                .ok_or_else(|| anyhow!("puppet rest palette unavailable"))?;
            return Ok(&self.matrices.palette);
        }
        let mut visible = samples.iter().filter(|(layer, _)| layer.visible);
        if let (Some((layer, seconds)), None) = (visible.next(), visible.next())
            && !layer.additive
        {
            let sampled = AnimationLayer { rate: 1.0, ..*layer };
            if pose::matrices(mesh, &[sampled], *seconds, pose::Mode::Authored, &mut self.matrices)
                .is_some()
            {
                return Ok(&self.matrices.palette);
            }
        }
        if self.binds.is_empty() {
            self.binds = mesh
                .bones
                .iter()
                .map(|bone| super::native_bind_transform(bone.raw_rest))
                .collect::<Result<_>>()?;
            self.locals.clone_from(&self.binds);
        }
        super::blend_world(
            mesh,
            samples,
            &self.binds,
            &mut self.locals,
            &mut self.matrices.world,
            self.matrices.locals.as_mut(),
        )?;
        super::palette_matrices(mesh, &self.matrices.world, &mut self.matrices.palette)?;
        Ok(&self.matrices.palette)
    }
}

pub struct Sample<'a> {
    pub matrices: &'a [Mat4],
    pub world: &'a [Mat4],
    pub revision: u64,
}

struct Dynamics {
    simulation: Box<super::spring::Simulation>,
    time: Option<f32>,
    world: Vec<Mat4>,
    matrices: Vec<Mat4>,
}

impl Dynamics {
    fn step(
        &mut self,
        mesh: &Mesh,
        rest: (&[Mat4], &[Mat4]),
        owner: &Mat4,
        time: f32,
    ) -> Result<()> {
        let dt = self
            .time
            .map_or(0.0, |previous| (time - previous).clamp(0.0, super::spring::MAX_STEP_SECONDS));
        self.time = Some(time);
        self.world.clear();
        self.world.extend_from_slice(rest.0);
        self.matrices.clear();
        self.matrices.extend_from_slice(rest.1);
        let frame = super::spring::Frame { dt, owner, scale: super::spring::owner_scale(owner) };
        ensure!(
            self.simulation.step(&mesh.springs, &mesh.bones, &mut self.world, &frame),
            super::spring::POSE_INVALID
        );
        ensure!(self.simulation.correct(&mut self.matrices), super::spring::PALETTE_INVALID);
        Ok(())
    }
}

pub struct Sampler {
    mesh: Rc<Mesh>,
    scratch: Scratch,
    samples: Vec<(AnimationLayer, f32)>,
    revision: u64,
    dynamics: Option<Dynamics>,
    owner: Mat4,
}

impl Sampler {
    pub fn new(mesh: Rc<Mesh>) -> Result<Self> {
        let binds = mesh
            .bones
            .iter()
            .map(|bone| super::native_bind_transform(bone.raw_rest))
            .collect::<Result<Vec<_>>>()?;
        let scratch = Scratch {
            matrices: pose::Matrices {
                locals: None,
                world: Vec::with_capacity(mesh.bones.len()),
                palette: Vec::with_capacity(mesh.bones.len()),
            },
            locals: binds.clone(),
            binds,
        };
        let dynamics = super::spring::Simulation::of(&mesh).map(|simulation| Dynamics {
            simulation,
            time: None,
            world: Vec::with_capacity(mesh.bones.len()),
            matrices: Vec::with_capacity(mesh.bones.len()),
        });
        Ok(Self {
            mesh,
            scratch,
            samples: Vec::with_capacity(super::MAX_ANIMATION_LAYERS),
            revision: 0,
            dynamics,
            owner: crate::matrix::IDENTITY,
        })
    }

    pub fn set_owner(&mut self, owner: Mat4) {
        self.owner = owner;
    }

    pub fn sample(&mut self, samples: &[(AnimationLayer, f32)], time: f32) -> Result<Sample<'_>> {
        ensure!(samples.len() <= super::MAX_ANIMATION_LAYERS, "puppet layer budget exceeded");
        let resampled = self.revision == 0 || self.samples != samples;
        if resampled {
            self.scratch.sample(&self.mesh, samples)?;
            self.samples.clear();
            self.samples.extend_from_slice(samples);
            self.revision = self.revision.wrapping_add(1);
        }
        if let Some(dynamics) = &mut self.dynamics {
            if resampled || dynamics.time != Some(time) {
                let rest = (&self.scratch.matrices.world[..], &self.scratch.matrices.palette[..]);
                dynamics.step(&self.mesh, rest, &self.owner, time)?;
                self.revision = self.revision.wrapping_add(1);
            }
            return Ok(Sample {
                matrices: &dynamics.matrices,
                world: &dynamics.world,
                revision: self.revision,
            });
        }
        Ok(Sample {
            matrices: &self.scratch.matrices.palette,
            world: &self.scratch.matrices.world,
            revision: self.revision,
        })
    }
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
