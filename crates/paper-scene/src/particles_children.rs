use super::{MAX_PARTICLES, ParticleSystem, Sim};

struct Follower {
    parent: u64,
    position: [f32; 3],
    sim: Sim,
}

#[derive(Default)]
pub struct Followers {
    instances: Vec<Follower>,
}

impl Followers {
    #[must_use]
    pub fn capacity(system: &ParticleSystem) -> usize {
        Sim::capacity(system).saturating_mul(system.follow_limit.unwrap_or(1)).min(MAX_PARTICLES)
    }

    pub fn step(
        &mut self,
        output: &mut Sim,
        system: &ParticleSystem,
        parent: (&ParticleSystem, &Sim),
        dt: f32,
        audio: (&[f32], &[f32]),
    ) {
        let limit =
            system.follow_limit.unwrap_or(0).min(MAX_PARTICLES / Sim::capacity(system).max(1));
        self.instances
            .retain(|follower| follower.sim.emitting || !follower.sim.particles.is_empty());
        for particle in &parent.1.particles {
            if self.instances.len() >= limit {
                break;
            }
            if !self.instances.iter().any(|follower| follower.parent == particle.id) {
                self.instances.push(Follower {
                    parent: particle.id,
                    position: [0.0; 3],
                    sim: Sim::new(output.rng.next_u32()),
                });
            }
        }
        output.particles.clear();
        output.history.clear();
        let parent_scale = parent.0.draw_scale3();
        let child_scale = system.draw_scale3();
        let (sin, cos) = (parent.0.angle - system.angle).sin_cos();
        let capacity = Self::capacity(system);
        for follower in &mut self.instances {
            let particle =
                parent.1.particles.iter().find(|particle| particle.id == follower.parent);
            follower.sim.emitting = particle.is_some();
            if let Some(particle) = particle {
                let [x, y, z] = std::array::from_fn(|axis| particle.pos[axis] * parent_scale[axis]);
                follower.position = [
                    (x * cos - y * sin) / child_scale[0],
                    (x * sin + y * cos) / child_scale[1],
                    z / child_scale[2],
                ];
            }
            follower.sim.pointer = output.pointer;
            follower.sim.emission_offset = if system.world { follower.position } else { [0.0; 3] };
            follower.sim.step_with_audio(system, dt, audio.0, audio.1);
            for (index, particle) in follower.sim.particles.iter().enumerate() {
                if output.particles.len() == capacity {
                    break;
                }
                let mut particle = *particle;
                particle.id = follower.parent.wrapping_mul(0x1_0000_0001).wrapping_add(particle.id);
                if !system.world {
                    for (position, offset) in particle.pos.iter_mut().zip(follower.position) {
                        *position += offset;
                    }
                }
                output.particles.push(particle);
                if let Some(history) = follower.sim.history.get(index) {
                    let mut history = *history;
                    if !system.world {
                        for point in &mut history {
                            point[0] += follower.position[0];
                            point[1] += follower.position[1];
                        }
                    }
                    output.history.push(history);
                }
            }
        }
    }
}
