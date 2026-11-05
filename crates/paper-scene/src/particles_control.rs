use super::{
    OperatorBlend, Particle, ParticleSystem, ParticleTransform, Sim, dot3, num_of, reciprocal,
    reciprocal_sqrt, simd_saturate,
};
use serde_json::Value;
use std::cell::Cell;

const CONTROL_POINT: &str = "controlpoint";
const MINIMUM: &str = "min";
const MAXIMUM: &str = "max";
const DEFAULT_MINIMUM: f32 = 0.1;
const DEFAULT_MAXIMUM: f32 = 0.2;
pub(super) const CONTROL_POINTS: usize = 8;
pub(crate) const SPATIAL_AXES: usize = 3;
pub(super) const ABSOLUTE_CONTROL_FLAGS: u32 = 3;
pub(super) const WORLDSPACE_CONTROL_FLAG: u32 = 2;
const DISTANCE: &str = "distance";
const VARIABLE_STRENGTH: &str = "variablestrength";
const START_POINT: &str = "controlpointstart";
const END_POINT: &str = "controlpointend";
const MIN_SEGMENT_SQUARED: f32 = 1.421_085_5e-14;
const DISTANCE_INNER: &str = "distanceinner";
const DISTANCE_OUTER: &str = "distanceouter";
const REDUCTION_INNER: &str = "reductioninner";
const REDUCTION_OUTER: &str = "reductionouter";
const DEFAULT_REDUCTION: f32 = 100.0;

pub struct ReduceMovement {
    pub(super) point: usize,
    inner: f32,
    inverse_span: f32,
    reduction: f32,
    range: f32,
    blend: Option<OperatorBlend>,
}

impl ReduceMovement {
    pub(super) fn parse(entry: &Value, distances: [f32; 2]) -> Self {
        let inner = num_of(entry.get(DISTANCE_INNER), distances[0]);
        let outer = num_of(entry.get(DISTANCE_OUTER), distances[1]);
        let reduction = num_of(entry.get(REDUCTION_INNER), DEFAULT_REDUCTION);
        let reduction_outer = num_of(entry.get(REDUCTION_OUTER), 0.0);
        Self {
            point: point_index(entry.get(CONTROL_POINT), 0.0),
            inner,
            inverse_span: if inner == outer { 1.0 } else { reciprocal(outer - inner) },
            reduction,
            range: if reduction == reduction_outer { 1.0 } else { reduction_outer - reduction },
            blend: OperatorBlend::parse(entry),
        }
    }

    pub(super) fn apply(&self, particle: &mut Particle, point: PointMotion, dt: f32) {
        let offset = std::array::from_fn(|axis| particle.pos[axis] - point.position[axis]);
        let squared = dot3(offset, offset);
        let distance = reciprocal_sqrt(squared) * squared;
        let fraction = simd_saturate((distance - self.inner) * self.inverse_span);
        let reduction = simd_saturate((fraction * self.range + self.reduction) * dt);
        let weight = self
            .blend
            .as_ref()
            .map_or(1.0, |blend| blend.weight(particle.age * reciprocal(particle.lifetime)));
        for velocity in &mut particle.vel {
            *velocity *= 1.0 - reduction * weight;
        }
    }
}

pub struct MaintainBetween {
    pub(super) start: usize,
    pub(super) end: usize,
    blend: Option<OperatorBlend>,
}

impl MaintainBetween {
    pub(super) fn parse(entry: &Value) -> Self {
        Self {
            start: point_index(entry.get(START_POINT), 0.0),
            end: point_index(entry.get(END_POINT), 1.0),
            blend: OperatorBlend::parse(entry),
        }
    }

    pub(super) fn apply(&self, particle: &mut Particle, start: PointMotion, end: PointMotion) {
        let previous: [f32; 3] =
            std::array::from_fn(|axis| start.position[axis] - start.delta[axis]);
        let old_axis =
            std::array::from_fn(|axis| (end.position[axis] - end.delta[axis]) - previous[axis]);
        let axis: [f32; 3] = std::array::from_fn(|axis| end.position[axis] - start.position[axis]);
        let old_squared = dot3(old_axis, old_axis);
        let squared = dot3(axis, axis);
        let valid = old_squared > MIN_SEGMENT_SQUARED && squared > MIN_SEGMENT_SQUARED;
        if !valid {
            return;
        }
        let old_reciprocal = reciprocal_sqrt(old_squared);
        let inverse = reciprocal_sqrt(squared);
        let projection: f32 = (0..SPATIAL_AXES)
            .map(|axis| (particle.pos[axis] - previous[axis]) * old_reciprocal * old_axis[axis])
            .sum();
        let mapped = simd_saturate(projection * old_reciprocal) * inverse * squared;
        let weight = self
            .blend
            .as_ref()
            .map_or(1.0, |blend| blend.weight(particle.age * reciprocal(particle.lifetime)));
        for (index, position) in particle.pos.iter_mut().enumerate() {
            let correction = (mapped * (axis[index] * inverse)
                - projection * old_reciprocal * old_axis[index])
                + start.delta[index];
            *position += correction * weight;
        }
    }
}

pub(super) fn point_index(value: Option<&Value>, fallback: f32) -> usize {
    (num_of(value, fallback) as i32 as u32 as usize).min(CONTROL_POINTS - 1)
}

pub struct MaintainDistance {
    pub(super) point: usize,
    distance: f32,
    strength: f32,
    blend: Option<OperatorBlend>,
}

impl MaintainDistance {
    pub(super) fn parse(entry: &Value, default_distance: f32) -> Self {
        Self {
            point: point_index(entry.get(CONTROL_POINT), 0.0),
            distance: num_of(entry.get(DISTANCE), default_distance),
            strength: num_of(entry.get(VARIABLE_STRENGTH), 0.0),
            blend: OperatorBlend::parse(entry),
        }
    }

    pub(super) fn apply(&self, particle: &mut Particle, motion: PointMotion, dt: f32) {
        let direction: [f32; 3] = std::array::from_fn(|axis| {
            (particle.pos[axis] + motion.delta[axis]) - motion.position[axis]
        });
        let metric = std::array::from_fn(|axis| direction[axis] * motion.metric[axis]);
        let strength =
            if self.strength == 0.0 { 1.0 } else { (self.strength * dt).clamp(0.0, 1.0) };
        let weight = self
            .blend
            .as_ref()
            .map_or(1.0, |blend| blend.weight(particle.age * reciprocal(particle.lifetime)));
        let correction =
            (self.distance * reciprocal_sqrt(dot3(metric, metric)) - 1.0) * (strength * weight);
        for (axis, position) in particle.pos.iter_mut().enumerate() {
            *position += direction[axis] * correction + motion.delta[axis];
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct PointMotion {
    pub(super) position: [f32; 3],
    delta: [f32; 3],
    metric: [f32; 3],
}

impl PointMotion {
    pub(super) fn step(self, index: usize, count: usize) -> Self {
        let delta = self.delta.map(|value| value / count as f32);
        Self {
            position: std::array::from_fn(|axis| {
                self.position[axis] - delta[axis] * (count - index - 1) as f32
            }),
            delta,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InheritVelocity {
    pub(super) point: usize,
    pub(super) min: f32,
    pub(super) max: f32,
}

impl InheritVelocity {
    pub(super) fn parse(entry: &Value) -> Option<Self> {
        let point = entry.get(CONTROL_POINT).map_or(Some(0), Value::as_u64)?;
        (point < CONTROL_POINTS as u64).then(|| Self {
            point: point as usize,
            min: num_of(entry.get(MINIMUM), DEFAULT_MINIMUM),
            max: num_of(entry.get(MAXIMUM), DEFAULT_MAXIMUM),
        })
    }
}

impl ParticleTransform {
    fn vector(self, vector: [f32; 3]) -> [f32; 3] {
        let (sin, cos) = self.angle.sin_cos();
        let [x, y, z] = std::array::from_fn(|axis| vector[axis] * self.scale[axis]);
        [x * cos - y * sin, x * sin + y * cos, z]
    }

    fn point(self, point: [f32; 3]) -> [f32; 3] {
        let vector = self.vector(point);
        std::array::from_fn(|axis| vector[axis] + self.origin[axis])
    }
}

impl Sim {
    pub(super) fn keep_written_points(
        &mut self,
        system: &ParticleSystem,
        written: u32,
        points: &[Cell<PointMotion>; CONTROL_POINTS],
    ) {
        if written == 0 {
            return;
        }
        for index in (0..CONTROL_POINTS).filter(|index| written & (1 << index) != 0) {
            let flags = system.control_points[index].flags;
            let scaled = system.world && flags & ABSOLUTE_CONTROL_FLAGS == 0;
            let position = points[index].get().position;
            let local = std::array::from_fn(|axis| {
                position[axis] * if scaled { system.scale3[axis].recip() } else { 1.0 }
            });
            self.control_points[index] = local;
        }
    }

    pub(super) fn control_motion(
        &self,
        system: &ParticleSystem,
        dt: f32,
    ) -> [PointMotion; CONTROL_POINTS] {
        std::array::from_fn(|index| {
            let absolute = system.control_points[index].flags & ABSOLUTE_CONTROL_FLAGS != 0;
            PointMotion {
                position: std::array::from_fn(|axis| {
                    self.control_points[index][axis]
                        * if system.world && !absolute { system.scale3[axis] } else { 1.0 }
                }),
                delta: self.control_velocity[index].map(|velocity| velocity * dt),
                metric: std::array::from_fn(|axis| {
                    if system.world && !absolute {
                        system.scale3[axis].recip()
                    } else if !system.world && absolute {
                        system.scale3[axis]
                    } else {
                        1.0
                    }
                }),
            }
        })
    }

    pub(super) fn update_control_points(&mut self, system: &ParticleSystem, dt: f32) {
        let transform = ParticleTransform::for_system(system);
        if system.world
            && let Some(previous) = self.transform
            && (previous.origin != transform.origin
                || previous.angle != transform.angle
                || previous.scale != transform.scale)
        {
            for particle in self
                .particles
                .iter_mut()
                .chain(&mut self.retired)
                .chain(self.boids.iter_mut().flat_map(|pool| &mut pool.particles))
            {
                particle.pos = transform.inverse_point(previous.point(particle.pos));
                particle.vel = transform.inverse_vector(previous.vector(particle.vel));
                particle.angle += previous.angle - transform.angle;
            }
            for history in &mut self.history {
                history.transform(|point| transform.inverse_point(previous.point(point)));
            }
        }
        let control_transform = ParticleTransform { scale: system.scale3, ..transform };
        let mut current = [[0.0; SPATIAL_AXES]; CONTROL_POINTS];
        for (index, point) in system.control_points.iter().enumerate() {
            if system.remapped_points & 1 << index != 0
                && let Some(previous) = self.control_previous
            {
                current[index] = previous[index];
                self.control_velocity[index] = [0.0; SPATIAL_AXES];
                continue;
            }
            let mut position = point.offset;
            if point.flags & 1 != 0 {
                let pointer = self.pointer.unwrap_or([system.origin.0, system.origin.1]);
                position[0] += pointer[0];
                position[1] += pointer[1];
            }
            self.control_points[index] = if point.flags & ABSOLUTE_CONTROL_FLAGS != 0 {
                transform.inverse_point(position)
            } else {
                position
            };
            current[index] = if !system.world {
                self.control_points[index]
            } else if point.flags & ABSOLUTE_CONTROL_FLAGS != 0 {
                position
            } else {
                control_transform.point(position)
            };
            self.control_velocity[index] = self
                .control_previous
                .as_ref()
                .filter(|_| dt > 0.0)
                .map_or([0.0; SPATIAL_AXES], |previous| {
                    let velocity = std::array::from_fn(|axis| {
                        (current[index][axis] - previous[index][axis]) / dt
                    });
                    if system.world { transform.inverse_vector(velocity) } else { velocity }
                });
        }
        self.control_previous = Some(current);
        self.transform = Some(transform);
    }
}
