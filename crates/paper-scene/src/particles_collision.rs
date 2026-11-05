use super::{
    Collision, Particle, collision_response, control::SPATIAL_AXES, parse_collision, vec3_of,
};
use serde_json::Value;

const ORIGIN: &str = "origin";
const SIZE: &str = "size";
const HALF: f32 = 0.5;
const UP: [f32; SPATIAL_AXES] = [0.0, 1.0, 0.0];

#[derive(Clone, Copy)]
pub struct Quad {
    origin: [f32; SPATIAL_AXES],
    half_size: [f32; 2],
    collision: Collision,
}

impl Quad {
    pub(super) fn parse(entry: &Value) -> Option<Self> {
        let origin = vec3_of(entry.get(ORIGIN), [f32::NAN; SPATIAL_AXES]);
        let mut size = entry.get(SIZE)?.as_str()?.split_whitespace();
        let width = size.next()?.parse::<f32>().ok()?;
        let depth = size.next()?.parse::<f32>().ok()?;
        if size.next().is_some()
            || origin.iter().any(|component| !component.is_finite())
            || !width.is_finite()
            || !depth.is_finite()
            || width <= 0.0
            || depth <= 0.0
        {
            return None;
        }
        Some(Self {
            origin,
            half_size: [width * HALF, depth * HALF],
            collision: parse_collision(entry)?,
        })
    }

    pub(super) fn apply(self, particle: &mut Particle) {
        let within_width = (particle.pos[0] - self.origin[0]).abs() < self.half_size[0];
        let within_depth = (particle.pos[2] - self.origin[2]).abs() < self.half_size[1];
        let penetration = particle.pos[1] - self.origin[1];
        if within_width && within_depth && penetration <= 0.0 {
            collision_response(particle, UP, penetration, self.collision);
        }
    }
}
