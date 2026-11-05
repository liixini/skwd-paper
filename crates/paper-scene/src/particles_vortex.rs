use super::{
    AudioResponse, OperatorBlend, Particle, num_of, reciprocal, reciprocal_sqrt, simd_saturate,
    vec3_of,
};
use serde_json::Value;

pub(super) const NAME: &str = "vortex_v2";
const AXIS: &str = "axis";
const FLAGS: &str = "flags";
const POINT: &str = "controlpoint";
const DISTANCE_INNER: &str = "distanceinner";
const DISTANCE_OUTER: &str = "distanceouter";
const SPEED_INNER: &str = "speedinner";
const SPEED_OUTER: &str = "speedouter";
const CENTER_FORCE: &str = "centerforce";
const RING_RADIUS: &str = "ringradius";
const RING_WIDTH: &str = "ringwidth";
const RING_DISTANCE: &str = "ringpulldistance";
const RING_FORCE: &str = "ringpullforce";
const CYLINDER_FLAG: u32 = 1;
const CENTER_FLAG: u32 = 2;
const RING_FLAG: u32 = 4;
const MIN_AXIS_LENGTH: f32 = 0.001;
const LAST_POINT: u32 = 7;

#[derive(Clone, Copy)]
pub(super) struct Defaults {
    distance: [f32; 2],
    speed: f32,
    radius: f32,
    width: f32,
    pull_distance: f32,
    pull_force: f32,
}

impl Defaults {
    pub(super) const ORTHOGRAPHIC: Self = Self {
        distance: [500.0, 650.0],
        speed: 2500.0,
        radius: 300.0,
        width: 50.0,
        pull_distance: 50.0,
        pull_force: 10.0,
    };
    pub(super) const OTHER: Self = Self {
        distance: [1.0, 2.0],
        speed: 1.0,
        radius: 1.0,
        width: 0.2,
        pull_distance: 0.25,
        pull_force: 0.05,
    };
}

pub struct Vortex {
    pub(super) point: usize,
    pub(super) axis: [f32; 3],
    pub(super) audio: Option<AudioResponse>,
    cylinder: bool,
    ring: bool,
    inner: f32,
    inverse_range: f32,
    speed: f32,
    speed_range: f32,
    center_force: f32,
    radius: f32,
    pull_force: f32,
    blend: Option<OperatorBlend>,
}

#[derive(Clone, Copy)]
pub(super) struct Step {
    pub(super) center: [f32; 3],
    pub(super) axis: [f32; 3],
    pub(super) dt: f32,
    pub(super) force_dt: f32,
    pub(super) gain: f32,
    pub(super) speed_scale: f32,
}

impl Vortex {
    pub(super) fn parse(entry: &Value, defaults: Defaults) -> Self {
        let raw_axis = vec3_of(entry.get(AXIS), [0.0, 0.0, 1.0]);
        let length = super::dot3(raw_axis, raw_axis).sqrt();
        let flags = num_of(entry.get(FLAGS), 0.0) as u32;
        let ring = flags & RING_FLAG != 0;
        let inner = num_of(entry.get(DISTANCE_INNER), defaults.distance[0]);
        let outer = num_of(entry.get(DISTANCE_OUTER), defaults.distance[1]);
        let speed = num_of(entry.get(SPEED_INNER), defaults.speed);
        let range = if ring {
            num_of(entry.get(RING_DISTANCE), defaults.pull_distance)
        } else {
            outer - inner
        };
        Self {
            point: (num_of(entry.get(POINT), 0.0) as i32 as u32).min(LAST_POINT) as usize,
            axis: if length < MIN_AXIS_LENGTH {
                [0.0, 0.0, 1.0]
            } else {
                raw_axis.map(|value| value * length.recip())
            },
            audio: AudioResponse::parse(entry),
            cylinder: flags & CYLINDER_FLAG != 0,
            ring,
            inner: if ring { num_of(entry.get(RING_WIDTH), defaults.width) } else { inner },
            inverse_range: if range == 0.0 { 1.0 } else { reciprocal(range) },
            speed,
            speed_range: num_of(entry.get(SPEED_OUTER), 0.0) - speed,
            center_force: if flags & CENTER_FLAG != 0 {
                num_of(entry.get(CENTER_FORCE), 1.0)
            } else {
                0.0
            },
            radius: num_of(entry.get(RING_RADIUS), defaults.radius),
            pull_force: num_of(entry.get(RING_FORCE), defaults.pull_force),
            blend: OperatorBlend::parse(entry),
        }
    }

    pub(super) fn apply(&self, particle: &mut Particle, step: Step) {
        let Step { center, axis, dt, force_dt, gain, speed_scale } = step;
        let offset: [f32; 3] = std::array::from_fn(|index| particle.pos[index] - center[index]);
        let projection = if self.cylinder {
            offset[1] * axis[1] + offset[0] * axis[0] + offset[2] * axis[2]
        } else {
            0.0
        };
        let radial = std::array::from_fn(|index| offset[index] - projection * axis[index]);
        let squared = squared_length(radial);
        let inverse = reciprocal_sqrt(squared);
        let normal = radial.map(|value| inverse * value);
        let distance = inverse * squared;
        let ring_delta = self.radius - distance;
        let distance = if self.ring { ring_delta.abs() } else { distance };
        let fraction = simd_saturate((distance - self.inner) * self.inverse_range);
        let weight = self
            .blend
            .as_ref()
            .map_or(1.0, |blend| blend.weight(particle.age * reciprocal(particle.lifetime)));
        let speed = (fraction * (self.speed_range * gain * force_dt)
            + self.speed * gain * force_dt)
            * weight;
        let predicted = std::array::from_fn(|index| {
            ((particle.vel[index] * dt + particle.pos[index]) - center[index])
                - projection * axis[index]
        });
        let mut correction = (reciprocal_sqrt(squared_length(predicted)) * inverse * squared - 1.0)
            * (dt.recip() * self.center_force * speed_scale);
        if self.ring {
            let pull = 1.0 - fraction;
            let pull = if pull == 1.0 { 0.0 } else { pull };
            correction += pull.copysign(ring_delta) * (dt * self.pull_force * speed_scale);
        }
        correction *= weight;
        let tangent = [
            normal[1] * axis[2] - normal[2] * axis[1],
            normal[2] * axis[0] - normal[0] * axis[2],
            normal[0] * axis[1] - normal[1] * axis[0],
        ];
        for (index, velocity) in particle.vel.iter_mut().enumerate() {
            *velocity += tangent[index] * speed + predicted[index] * correction;
        }
    }
}

fn squared_length(value: [f32; 3]) -> f32 {
    value[1] * value[1] + value[0] * value[0] + value[2] * value[2]
}
