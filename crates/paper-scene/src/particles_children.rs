use super::image::ImageSource;
use super::{MAX_PARTICLES, Particle, ParticleSystem, Sim, num_of};
use serde_json::Value;
use std::collections::HashSet;

const EVENT_FOLLOW: &str = "eventfollow";
const EVENT_SPAWN: &str = "eventspawn";
const EVENT_DEATH: &str = "eventdeath";
const EVENT_TYPE: &str = "type";
const EVENT_LIMIT: &str = "maxcount";
const EVENT_PROBABILITY: &str = "probability";
const DEFAULT_EVENT_LIMIT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildEventKind {
    Follow,
    Spawn,
    Death,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildEvent {
    pub kind: ChildEventKind,
    pub limit: usize,
    pub probability: f32,
}

impl ChildEvent {
    pub(super) fn parse(object: &Value) -> Option<Self> {
        let kind = match object.get(EVENT_TYPE)?.as_str()? {
            EVENT_FOLLOW => ChildEventKind::Follow,
            EVENT_SPAWN => ChildEventKind::Spawn,
            EVENT_DEATH => ChildEventKind::Death,
            _ => return None,
        };
        Some(Self {
            kind,
            limit: num_of(object.get(EVENT_LIMIT), DEFAULT_EVENT_LIMIT as f32)
                .clamp(0.0, MAX_PARTICLES as f32) as usize,
            probability: num_of(object.get(EVENT_PROBABILITY), 1.0).clamp(0.0, 1.0),
        })
    }
}

struct Follower {
    parent: u64,
    position: [f32; 3],
    sim: Sim,
}

#[derive(Default)]
pub struct Followers {
    instances: Vec<Follower>,
    seen: HashSet<u64>,
    active: HashSet<u64>,
    parent_step: Option<u64>,
}

impl Followers {
    #[must_use]
    pub fn capacity(system: &ParticleSystem) -> usize {
        Sim::capacity(system)
            .saturating_mul(system.child_event.map_or(1, |event| event.limit))
            .min(MAX_PARTICLES)
    }

    pub fn step(
        &mut self,
        output: &mut Sim,
        system: &ParticleSystem,
        parent: (&ParticleSystem, &Sim),
        dt: f32,
        audio: (&[f32], &[f32]),
        query: &mut dyn FnMut(&ImageSource),
    ) {
        let Some(event) = system.child_event else {
            return;
        };
        let limit = event.limit.min(MAX_PARTICLES / Sim::capacity(system).max(1));
        self.instances
            .retain(|follower| follower.sim.emitting || !follower.sim.particles.is_empty());
        if event.kind != ChildEventKind::Death {
            self.active.clear();
            self.active.extend(parent.1.particles.iter().map(|particle| particle.id));
            self.seen.retain(|id| self.active.contains(id));
        }
        let source = if event.kind == ChildEventKind::Death {
            &parent.1.retired
        } else {
            &parent.1.particles
        };
        if event.kind != ChildEventKind::Death || self.parent_step != Some(parent.1.step_id) {
            for particle in source {
                if event.kind != ChildEventKind::Death && parent.1.prewarmed.contains(&particle.id)
                {
                    continue;
                }
                if self.seen.contains(&particle.id) {
                    continue;
                }
                if event.kind != ChildEventKind::Death {
                    self.seen.insert(particle.id);
                }
                if self.instances.len() >= limit || output.rng.unit() > event.probability {
                    continue;
                }
                let position = event_position(particle, parent.0, system);
                let mut sim = Sim::new(output.rng.next_u32());
                sim.event = Some(super::EventValues::from(particle));
                sim.live_event = sim.event;
                sim.image_sources.clone_from(&output.image_sources);
                sim.pointer = output.pointer;
                sim.scene_frame = output.scene_frame;
                sim.clock = output.clock;
                sim.emission_offset =
                    if system.world { position } else { [0.0; super::SPATIAL_AXES] };
                sim.prewarm(system, system.prewarm_step());
                self.instances.push(Follower { parent: particle.id, position, sim });
            }
        }
        self.parent_step = Some(parent.1.step_id);
        output.step_id = output.step_id.wrapping_add(1);
        output.particles.clear();
        output.history.clear();
        output.prewarmed.clear();
        output.retired.clear();
        let capacity = Self::capacity(system);
        for follower in &mut self.instances {
            let live_parent = parent.1.particles.iter().find(|particle| {
                particle.id == follower.parent && particle.age < particle.lifetime
            });
            follower.sim.live_event = live_parent.map(super::EventValues::from);
            if event.kind == ChildEventKind::Follow {
                follower.sim.emitting = live_parent.is_some();
                follower.sim.event = live_parent.map(super::EventValues::from);
                if let Some(particle) = live_parent {
                    follower.position = event_position(particle, parent.0, system);
                }
            }
            follower.sim.image_sources.clone_from(&output.image_sources);
            follower.sim.pointer = output.pointer;
            follower.sim.scene_frame = output.scene_frame;
            follower.sim.clock = output.clock;
            follower.sim.parent_hidden = output.parent_hidden;
            follower.sim.emission_offset = if system.world { follower.position } else { [0.0; 3] };
            follower.sim.step_with_audio(system, dt, audio.0, audio.1, query);
            if event.kind != ChildEventKind::Follow {
                follower.sim.emitting = follower.sim.has_emission(system);
            }
            output.retired.extend(follower.sim.retired.iter().map(|particle| {
                translated(*particle, follower.parent, follower.position, system.world)
            }));
            for (index, particle) in follower.sim.particles.iter().enumerate() {
                if output.particles.len() == capacity {
                    break;
                }
                let prewarmed = follower.sim.prewarmed.contains(&particle.id);
                let particle =
                    translated(*particle, follower.parent, follower.position, system.world);
                if prewarmed {
                    output.prewarmed.insert(particle.id);
                }
                output.particles.push(particle);
                if let Some(history) = follower.sim.history.get(index) {
                    let mut history = history.clone();
                    if !system.world {
                        history.transform(|point| {
                            std::array::from_fn(|axis| point[axis] + follower.position[axis])
                        });
                    }
                    output.history.push(history);
                }
            }
        }
    }
}

fn event_position(
    particle: &Particle,
    parent: &ParticleSystem,
    child: &ParticleSystem,
) -> [f32; 3] {
    let parent_scale = parent.draw_scale3();
    let child_scale = child.draw_scale3();
    let (sin, cos) = (parent.angle - child.angle).sin_cos();
    let [x, y, z] = std::array::from_fn(|axis| particle.pos[axis] * parent_scale[axis]);
    [(x * cos - y * sin) / child_scale[0], (x * sin + y * cos) / child_scale[1], z / child_scale[2]]
}

fn translated(mut particle: Particle, parent: u64, position: [f32; 3], world: bool) -> Particle {
    particle.id = parent.wrapping_mul(0x1_0000_0001).wrapping_add(particle.id);
    if !world {
        for (value, offset) in particle.pos.iter_mut().zip(position) {
            *value += offset;
        }
    }
    particle
}
