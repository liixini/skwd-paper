use super::{Rng, SPATIAL_AXES, num_of, vec3_of};
use crate::matrix::dot3;
use serde_json::Value;
use std::f32::consts::{PI, TAU};

pub const NAME: &str = "sphererandom";
const DIRECTIONS: &str = "directions";
const SIGN: &str = "sign";
const DISTANCE_MIN: &str = "distancemin";
const DISTANCE_MAX: &str = "distancemax";
const SPEED_MIN: &str = "speedmin";
const SPEED_MAX: &str = "speedmax";
const CONE: &str = "cone";
const DEFAULT_DIRECTIONS: [f32; SPATIAL_AXES] = [1.0, 1.0, 0.0];
const MIN_LAUNCH_LENGTH_SQUARED: f32 = 1e-4;

#[derive(Clone, Copy)]
pub struct SphereShape {
    pub directions: [f32; SPATIAL_AXES],
    pub sign: [f32; SPATIAL_AXES],
    pub distance: [f32; 2],
    pub speed: [f32; 2],
    pub polar_floor: f32,
}

impl SphereShape {
    pub fn parse(entry: &Value) -> Self {
        Self {
            directions: vec3_of(entry.get(DIRECTIONS), DEFAULT_DIRECTIONS),
            sign: vec3_of(entry.get(SIGN), [0.0; SPATIAL_AXES]),
            distance: [num_of(entry.get(DISTANCE_MIN), 0.0), num_of(entry.get(DISTANCE_MAX), 0.0)],
            speed: [num_of(entry.get(SPEED_MIN), 0.0), num_of(entry.get(SPEED_MAX), 0.0)],
            polar_floor: -(num_of(entry.get(CONE), 0.0) * PI).cos(),
        }
    }

    pub(super) fn offset(&self, rng: &mut Rng) -> [f32; SPATIAL_AXES] {
        let azimuth = rng.range(0.0, TAU);
        let polar = rng.range(self.polar_floor, 1.0);
        let depth = rng.unit().cbrt();
        let ring = (1.0 - polar * polar).max(0.0).sqrt() * depth;
        let (sin, cos) = azimuth.sin_cos();
        let ball = [polar * depth, ring * sin, ring * cos];
        let ball: [f32; SPATIAL_AXES] =
            std::array::from_fn(|axis| ball[axis] * self.directions[axis]);
        let length = dot3(ball, ball).sqrt();
        if length == 0.0 {
            return [0.0; SPATIAL_AXES];
        }
        let distance = self.distance[0] + (self.distance[1] - self.distance[0]) * length;
        std::array::from_fn(|axis| {
            let unit = ball[axis] / length;
            let sign = self.sign[axis];
            let unit = if sign == 0.0 { unit } else { unit.abs().copysign(sign) };
            unit * distance
        })
    }

    pub(super) fn launch(
        &self,
        offset: [f32; SPATIAL_AXES],
        rng: &mut Rng,
        frame: impl Fn([f32; SPATIAL_AXES]) -> [f32; SPATIAL_AXES],
    ) -> Option<[f32; SPATIAL_AXES]> {
        if self.speed == [0.0; 2] {
            return None;
        }
        let heading = if dot3(offset, offset) < MIN_LAUNCH_LENGTH_SQUARED {
            frame(std::array::from_fn(|axis| rng.range(-1.0, 1.0) * self.directions[axis]))
        } else {
            offset
        };
        let length = dot3(heading, heading).sqrt();
        let speed = rng.range(self.speed[0], self.speed[1]);
        (length > 0.0).then(|| heading.map(|value| value * speed / length))
    }
}

#[cfg(test)]
#[path = "particles_sphere_tests.rs"]
mod tests;
