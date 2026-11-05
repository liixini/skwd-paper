use super::control::SPATIAL_AXES;
use super::{Operator, Particle, num_of, operators, reciprocal, reciprocal_sqrt};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const NAME: &str = "boids";
const SEPARATION_THRESHOLD: &str = "separationthreshold";
const NEIGHBOR_THRESHOLD: &str = "neighborthreshold";
const SEPARATION_FACTOR: &str = "separationfactor";
const ALIGNMENT_FACTOR: &str = "alignmentfactor";
const COHESION_FACTOR: &str = "cohesionfactor";
const MAX_SPEED: &str = "maxspeed";
const FLAGS: &str = "flags";
const LIMIT_SPEED: u32 = 1;
const BATCH: usize = 4;
const SAMPLES: usize = 200;
const PERMUTATIONS: [usize; BATCH] = [0, 3, 2, 1];
const SEPARATION: f32 = 15.0;
const ALIGNMENT: f32 = 1.0;
const COHESION: f32 = 2.0;
const EMPTY: Particle = Particle {
    id: u64::MAX,
    pos: [0.0; SPATIAL_AXES],
    vel: [0.0; SPATIAL_AXES],
    size: 0.0,
    base_size: 0.0,
    color: [0.0; SPATIAL_AXES],
    base_color: [0.0; SPATIAL_AXES],
    alpha: 0.0,
    base_alpha: 0.0,
    angle: 0.0,
    angle_xy: [0.0; 2],
    angular: 0.0,
    angular_xy: [0.0; 2],
    age: 0.0,
    lifetime: 0.0,
    phase: 0.0,
    noise_seed: 0,
    slot: 0,
};

#[derive(Clone, Copy)]
pub(super) struct Defaults {
    separation: f32,
    neighbor: f32,
    speed: f32,
}

impl Defaults {
    pub(super) const ORTHOGRAPHIC: Self = Self { separation: 20.0, neighbor: 50.0, speed: 500.0 };
    pub(super) const OTHER: Self = Self { separation: 0.02, neighbor: 0.2, speed: 1.0 };
}

pub struct Boids {
    separation: f32,
    neighbor: f32,
    factors: [f32; 3],
    speed: f32,
    limit: bool,
}

impl Boids {
    pub(super) fn parse(entry: &Value, defaults: Defaults) -> Self {
        Self {
            separation: num_of(entry.get(SEPARATION_THRESHOLD), defaults.separation),
            neighbor: num_of(entry.get(NEIGHBOR_THRESHOLD), defaults.neighbor),
            factors: [
                num_of(entry.get(SEPARATION_FACTOR), SEPARATION),
                num_of(entry.get(ALIGNMENT_FACTOR), ALIGNMENT),
                num_of(entry.get(COHESION_FACTOR), COHESION),
            ],
            speed: num_of(entry.get(MAX_SPEED), defaults.speed),
            limit: num_of(entry.get(FLAGS), LIMIT_SPEED as f32) as u32 & LIMIT_SPEED != 0,
        }
    }

    pub(super) fn apply(&self, particles: &mut [Particle], count: usize, frame: u32, dt: f32) {
        let stride = count / SAMPLES + 1;
        let factors = self.factors.map(|factor| factor * stride as f32);
        let first = frame as usize % stride * BATCH;
        for target in (first..count).step_by(stride * BATCH) {
            let mut velocities = [[0.0; SPATIAL_AXES]; BATCH];
            for (lane, target_velocity) in velocities.iter_mut().enumerate() {
                let particle = particles[target + lane];
                let mut separation = [0.0; SPATIAL_AXES];
                let mut position = [0.0; SPATIAL_AXES];
                let mut velocity = [0.0; SPATIAL_AXES];
                let mut separated = 0.0;
                let mut neighbors = 0.0;
                let first = frame.wrapping_add(target as u32) as usize % stride * BATCH;
                for block in (first..count).step_by(stride * BATCH) {
                    let alive = particles[block + lane].lifetime != 0.0;
                    for rotation in PERMUTATIONS {
                        let neighbor = particles[block + (lane + rotation) % BATCH];
                        let delta =
                            std::array::from_fn(|axis| particle.pos[axis] - neighbor.pos[axis]);
                        let squared = squared_length(delta);
                        let inverse = reciprocal_sqrt(squared);
                        let distance = inverse * squared;
                        let force = if alive && squared != 0.0 && distance < self.separation {
                            separated += 1.0;
                            inverse * self.separation - 1.0
                        } else {
                            0.0
                        };
                        for axis in 0..SPATIAL_AXES {
                            separation[axis] += force * delta[axis];
                        }
                        if alive && distance < self.neighbor {
                            neighbors += 1.0;
                            for axis in 0..SPATIAL_AXES {
                                position[axis] += neighbor.pos[axis];
                                velocity[axis] += neighbor.vel[axis];
                            }
                        }
                    }
                }
                let separation_scale =
                    if separated == 0.0 { 0.0 } else { reciprocal(separated) * factors[0] * dt };
                let (average, alignment, cohesion) = if neighbors == 0.0 {
                    (0.0, 0.0, 0.0)
                } else {
                    (reciprocal(neighbors), factors[1] * dt, factors[2] * dt)
                };
                let mut result = std::array::from_fn(|axis| {
                    let force = (average * velocity[axis] - particle.vel[axis]) * alignment
                        + separation_scale * separation[axis]
                        + (average * position[axis] - particle.pos[axis]) * cohesion;
                    particle.vel[axis] + force
                });
                let squared = squared_length(result);
                if self.limit && squared > squared_length(particle.vel).max(self.speed * self.speed)
                {
                    let scale = reciprocal_sqrt(squared) * self.speed;
                    result = result.map(|value| scale * value);
                }
                *target_velocity = result;
            }
            for (particle, velocity) in particles[target..target + BATCH].iter_mut().zip(velocities)
            {
                particle.vel = velocity;
            }
        }
    }
}

fn squared_length(vector: [f32; 3]) -> f32 {
    vector[1] * vector[1] + vector[0] * vector[0] + vector[2] * vector[2]
}

pub(super) struct Pool {
    pub(super) particles: Vec<Particle>,
    indices: Vec<Option<usize>>,
    positions: HashMap<u64, usize>,
    previous: Vec<[f32; 3]>,
    count: usize,
    next_id: u64,
}

impl Pool {
    pub(super) fn new() -> Self {
        Self {
            particles: Vec::new(),
            indices: Vec::new(),
            positions: HashMap::new(),
            previous: Vec::new(),
            count: 0,
            next_id: 0,
        }
    }

    pub(super) fn sync(&mut self, live: &[Particle], dt: f32) {
        self.positions.clear();
        self.positions
            .extend(live.iter().enumerate().map(|(index, particle)| (particle.id, index)));
        for (slot, index) in self.particles.iter_mut().zip(&mut self.indices) {
            *index = self.positions.get(&slot.id).copied();
            if let Some(index) = *index {
                *slot = live[index];
            } else {
                slot.age += dt;
                slot.lifetime = 0.0;
            }
        }
        for (index, particle) in
            live.iter().enumerate().filter(|(_, particle)| particle.id >= self.next_id)
        {
            let slot = self.particles[..self.count]
                .iter()
                .position(|particle| particle.lifetime == 0.0)
                .unwrap_or(self.count);
            if slot == self.count {
                self.count += 1;
                let padded = self.count.next_multiple_of(BATCH);
                self.particles.resize(padded, EMPTY);
                self.indices.resize(padded, None);
            }
            self.particles[slot] = *particle;
            self.indices[slot] = Some(index);
        }
        if let Some(newest) = live.iter().map(|particle| particle.id).max() {
            self.next_id = newest.wrapping_add(1);
        }
        self.previous.clear();
        self.previous.extend(self.particles.iter().map(|particle| particle.pos));
    }

    pub(super) fn step(
        &mut self,
        context: &operators::Step<'_>,
        frame: u32,
        live: &mut [Particle],
    ) {
        for step in 0..context.substeps {
            for particle in &mut self.particles {
                particle.alpha = particle.base_alpha;
                particle.size = particle.base_size;
                particle.color = particle.base_color;
            }
            for (slot, operator) in context.system.operators.iter().enumerate() {
                if let Operator::Boids(boids) = operator {
                    boids.apply(
                        &mut self.particles,
                        self.count,
                        frame,
                        context.dt * context.drag_scale,
                    );
                } else {
                    for (index, (particle, previous)) in
                        self.particles.iter_mut().zip(&self.previous).enumerate()
                    {
                        context.apply(particle, slot, *previous, step, index);
                    }
                }
            }
        }
        for (particle, index) in self.particles.iter_mut().zip(&self.indices) {
            if let Some(index) = *index {
                particle.alpha = (particle.alpha * context.system.alpha).clamp(0.0, 1.0);
                live[index] = *particle;
            }
        }
    }
}

#[cfg(test)]
#[path = "particles_boids_tests.rs"]
mod tests;
