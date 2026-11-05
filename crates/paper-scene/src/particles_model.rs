use super::{Collision, Particle, ParticleSystem, ParticleTransform, dependency};
use crate::matrix::{Mat4, mat4_mul, model_matrix};
use crate::model::{MeshLayer, ParticleLayer};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub const NAME: &str = "collisionmodel";
const NATIVE_BATCH: usize = 10_000;
const PARENT: &str = "parent";
const SOURCE_PHYSICS: &str = "physics";
const TRANSLATION: usize = 12;

#[derive(Default)]
pub struct Bindings {
    supported: bool,
    dependencies: Vec<(u32, usize)>,
    sources: Vec<Option<Source>>,
}

impl Bindings {
    pub(super) fn parse(object: &Value, count: usize) -> Self {
        Self {
            supported: object.get(PARENT).is_none() && ParticleTransform::of(object).is_some(),
            dependencies: dependency::parse(object, dependency::Kind::Model)
                .map(|entry| (entry.source, entry.ordinal))
                .collect(),
            sources: vec![None; count],
        }
    }
}

#[derive(Clone, Copy)]
struct Source {
    size: [f32; super::SPATIAL_AXES],
    center: [f32; super::SPATIAL_AXES],
    matrix: Mat4,
}

impl Source {
    fn of(layer: &MeshLayer) -> Option<Self> {
        if layer.mount.is_some() {
            return None;
        }
        let [part] = layer.mesh.parts.as_slice() else {
            return None;
        };
        let (min, max) = part.bounds?;
        let size = std::array::from_fn(|axis| (max[axis] - min[axis]) * 0.5);
        if size.iter().any(|extent| !extent.is_finite() || *extent <= 0.0) {
            return None;
        }
        let center = std::array::from_fn(|axis| (max[axis] + min[axis]) * 0.5);
        let pose = layer.pose;
        let mut matrix = model_matrix(pose.origin, pose.scale, pose.angles);
        for parent in &layer.parents {
            matrix = mat4_mul(&model_matrix(parent.origin, parent.scale, parent.angles), &matrix);
        }
        if center.iter().chain(matrix.iter()).any(|value| !value.is_finite()) {
            return None;
        }
        let source = Self { size, center, matrix };
        source.capsule(None)?;
        Some(source)
    }

    fn capsule(self, pose: Option<ParticleTransform>) -> Option<Capsule> {
        Capsule::prepare(
            self.size,
            self.center,
            |position| {
                let position = self.point(position);
                pose.map_or(position, |pose| pose.inverse_point(position))
            },
            |direction| {
                let direction = self.vector(direction);
                pose.map_or(direction, |pose| pose.inverse_vector(direction))
            },
        )
    }

    fn vector(self, vector: [f32; super::SPATIAL_AXES]) -> [f32; super::SPATIAL_AXES] {
        crate::matrix::direction(&self.matrix, vector)
    }

    fn point(self, point: [f32; super::SPATIAL_AXES]) -> [f32; super::SPATIAL_AXES] {
        let vector = self.vector(point);
        std::array::from_fn(|axis| vector[axis] + self.matrix[TRANSLATION + axis])
    }
}

pub(crate) fn bind(
    particles: &mut [ParticleLayer],
    meshes: &[MeshLayer],
    objects: &HashMap<String, &Value>,
    skipped: &mut Vec<String>,
) {
    let sources: BTreeMap<_, _> = meshes
        .iter()
        .filter_map(|layer| {
            let object = objects.get(&layer.id)?;
            if object.get(SOURCE_PHYSICS).is_some() {
                return None;
            }
            Some((layer.id.parse::<u32>().ok()?, Source::of(layer)?))
        })
        .collect();
    for layer in particles {
        let supported = objects.get(&layer.id).is_some_and(|object| {
            object.get(PARENT).is_none() && ParticleTransform::of(object).is_some()
        });
        bind_system(&mut layer.system, &sources, supported, skipped);
    }
}

fn bind_system(
    system: &mut ParticleSystem,
    sources: &BTreeMap<u32, Source>,
    supported: bool,
    skipped: &mut Vec<String>,
) {
    let event = system.child_event.is_some();
    let supported = supported && system.model_collisions.supported && !event;
    let pose = (!system.world).then(|| ParticleTransform::for_system(system));
    let bindings = &mut system.model_collisions;
    if !bindings.sources.is_empty() {
        if event {
            skipped.push(format!("{NAME}: event children unsupported"));
        } else if !supported || system.max_count >= NATIVE_BATCH {
            skipped.push(format!("{NAME}: particle transform or high count unsupported"));
        } else {
            let mut seen = vec![false; bindings.sources.len()];
            for &(source, ordinal) in &bindings.dependencies {
                let Some(target) = bindings.sources.get_mut(ordinal) else {
                    continue;
                };
                if seen[ordinal] {
                    *target = None;
                    skipped
                        .push(format!("{NAME}: duplicate dependency index {ordinal} unsupported"));
                    continue;
                }
                seen[ordinal] = true;
                *target =
                    sources.get(&source).copied().filter(|source| source.capsule(pose).is_some());
                if target.is_none() {
                    skipped.push(format!("{NAME}: source {source} unsupported or unavailable"));
                }
            }
        }
    }
    for child in &mut system.children {
        bind_system(child, sources, supported, skipped);
    }
}

pub(super) fn prepare(
    system: &ParticleSystem,
    capsules: &mut Vec<Option<Capsule>>,
    previous: &mut Option<(ParticleTransform, bool)>,
    failed: &mut bool,
) -> bool {
    let pose = ParticleTransform::for_system(system);
    if previous.is_some_and(|previous| previous == (pose, system.world)) {
        return false;
    }
    let mut invalid = false;
    capsules.clear();
    capsules.extend(system.model_collisions.sources.iter().map(|source| {
        let source = (*source)?;
        let capsule = source.capsule((!system.world).then_some(pose));
        invalid |= capsule.is_none();
        capsule
    }));
    *previous = Some((pose, system.world));
    let report = invalid && !*failed;
    *failed = invalid;
    report
}

#[derive(Clone, Copy)]
pub struct Capsule {
    origin: [f32; super::SPATIAL_AXES],
    axis: [f32; super::SPATIAL_AXES],
    length: f32,
    radius: f32,
}

impl Capsule {
    fn prepare(
        size: [f32; super::SPATIAL_AXES],
        center: [f32; super::SPATIAL_AXES],
        point: impl Fn([f32; super::SPATIAL_AXES]) -> [f32; super::SPATIAL_AXES],
        vector: impl Fn([f32; super::SPATIAL_AXES]) -> [f32; super::SPATIAL_AXES],
    ) -> Option<Self> {
        if size.iter().any(|extent| !(*extent * *extent).is_finite())
            || center.iter().any(|value| !value.is_finite())
        {
            return None;
        }
        let mut major = 0;
        let mut secondary = 1;
        let mut remaining = 2;
        if size[secondary] * size[secondary] > size[major] * size[major] {
            std::mem::swap(&mut major, &mut secondary);
        }
        if size[remaining] * size[remaining] > size[major] * size[major] {
            std::mem::swap(&mut major, &mut remaining);
        }
        if size[remaining] * size[remaining] > size[secondary] * size[secondary] {
            secondary = remaining;
        }
        let half_length = size[major] - size[secondary];
        let mut start = center;
        let mut end = center;
        start[major] -= half_length;
        end[major] += half_length;
        let origin = point(start);
        let end = point(end);
        let delta = std::array::from_fn(|axis| end[axis] - origin[axis]);
        let mut axis = [0.0; super::SPATIAL_AXES];
        axis[major] = 1.0;
        let axis = vector(axis);
        let norm = super::dot3(axis, axis).sqrt();
        let mut radius = [0.0; super::SPATIAL_AXES];
        radius[secondary] = size[secondary];
        let radius = vector(radius);
        let length = super::dot3(delta, delta).sqrt();
        let radius = super::dot3(radius, radius).sqrt();
        if !norm.is_finite()
            || norm <= 0.0
            || !length.is_finite()
            || !(radius * radius).is_finite()
            || radius <= 0.0
            || origin.iter().chain(axis.iter()).any(|value| !value.is_finite())
        {
            return None;
        }
        Some(Self { origin, axis: axis.map(|value| value / norm), length, radius })
    }

    pub(super) fn collide(self, particle: &mut Particle, collision: Collision) {
        if let Some((normal, depth)) = self.contact(particle.pos) {
            super::collision_response(particle, normal, depth, collision);
        }
    }

    fn contact(
        self,
        position: [f32; super::SPATIAL_AXES],
    ) -> Option<([f32; super::SPATIAL_AXES], f32)> {
        let offset = std::array::from_fn(|axis| position[axis] - self.origin[axis]);
        let along = super::simd_min(super::dot3(offset, self.axis), self.length);
        let along = if 0.0 > along { 0.0 } else { along };
        let offset = std::array::from_fn(|axis| {
            position[axis] - (along * self.axis[axis] + self.origin[axis])
        });
        let squared = super::dot3(offset, offset);
        if squared < self.radius * self.radius {
            let inverse = super::reciprocal_sqrt(squared);
            Some((offset.map(|value| value * inverse), squared * inverse - self.radius))
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "particles_model_tests.rs"]
mod tests;
