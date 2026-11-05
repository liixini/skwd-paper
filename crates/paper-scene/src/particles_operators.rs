use super::control::SPATIAL_AXES;
use super::{
    EventValues, Operator, Particle, ParticleSystem, collision_response, control, cross3, dot3,
    normalize3, particle_life_fraction, reciprocal, reciprocal_sqrt, remap, simd_min, simplex,
};
use std::cell::Cell;

const FLAT_BAND_FALLOFF: f32 = 1.0;

pub(super) struct Step<'a> {
    pub(super) model_capsules: &'a [Option<super::model_collision::Capsule>],
    pub(super) system: &'a ParticleSystem,
    pub(super) event: Option<EventValues>,
    pub(super) control_points: &'a [[f32; 3]; control::CONTROL_POINTS],
    pub(super) control_motion: &'a [Cell<control::PointMotion>; control::CONTROL_POINTS],
    pub(super) written_points: &'a Cell<u32>,
    pub(super) clock: crate::effects::FrameClock,
    pub(super) layer_time: f32,
    pub(super) gains: &'a [f32],
    pub(super) time: f32,
    pub(super) dt: f32,
    pub(super) drag_scale: f32,
    pub(super) substeps: usize,
    pub(super) world_turn: Option<(f32, f32)>,
}

impl Step<'_> {
    fn to_frame(&self, vector: [f32; SPATIAL_AXES]) -> [f32; SPATIAL_AXES] {
        self.world_turn.map_or(vector, |turn| super::to_frame(vector, turn))
    }

    #[expect(clippy::too_many_lines, reason = "one particle operator state transition")]
    pub(super) fn apply(
        &self,
        particle: &mut Particle,
        operator: usize,
        previous_position: [f32; 3],
        step: usize,
        index: usize,
    ) {
        let system = self.system;
        let speed = system.instance.speed;
        let control_motion = self.control_motion;
        let op = &system.operators[operator];
        let gain = self.gains[operator];
        let (time, dt, drag_scale) = (self.time, self.dt, self.drag_scale);
        let substeps = self.substeps;
        let seed = particle.phase / std::f32::consts::TAU;
        match op {
            Operator::Remap(remap) => remap.apply(
                particle,
                &remap::Frame {
                    system,
                    clock: self.clock,
                    system_time: time,
                    layer_time: self.layer_time,
                    points: control_motion,
                    written: self.written_points,
                    step,
                    substeps,
                    particle: index,
                },
            ),
            Operator::InheritEvent { inherit, blend } => {
                if let Some(event) = self.event {
                    let weight = blend
                        .as_ref()
                        .map(|blend| blend.weight(particle.age * reciprocal(particle.lifetime)));
                    inherit.apply_operator(particle, event, weight);
                }
            }
            Operator::Boids(_) => unreachable!("boids requires pool"),
            Operator::VortexV2(vortex) => {
                let point = control_motion[vortex.point].get().step(step, substeps).position;
                let mut axis = vortex.axis;
                if system.control_points[vortex.point].flags & control::ABSOLUTE_CONTROL_FLAGS != 0
                {
                    let (sin, cos) = (-system.angle).sin_cos();
                    axis = [axis[0] * cos - axis[1] * sin, axis[0] * sin + axis[1] * cos, axis[2]];
                }
                vortex.apply(
                    particle,
                    super::vortex::Step {
                        center: point,
                        axis,
                        dt,
                        force_dt: dt * drag_scale * speed,
                        gain,
                        speed_scale: speed,
                    },
                );
            }
            Operator::Movement { gravity, drag } => {
                let damp = 1.0 - (drag * dt * drag_scale).min(0.999_999_9);
                let gravity = self.to_frame(*gravity);
                for ((vel, pos), gravity) in
                    particle.vel.iter_mut().zip(particle.pos.iter_mut()).zip(gravity)
                {
                    *vel += gravity * dt * speed;
                    *pos += *vel * dt;
                    *vel *= damp;
                }
            }
            Operator::ReduceMovement(reduction) => {
                reduction.apply(
                    particle,
                    control_motion[reduction.point].get().step(step, substeps),
                    dt,
                );
            }
            Operator::MaintainBetween(between) => {
                between.apply(
                    particle,
                    control_motion[between.start].get().step(step, substeps),
                    control_motion[between.end].get().step(step, substeps),
                );
            }
            Operator::MaintainDistance(distance) => {
                distance.apply(
                    particle,
                    control_motion[distance.point].get().step(step, substeps),
                    dt,
                );
            }
            Operator::CapVelocity { max_speed, blend } => {
                let ratio = max_speed * reciprocal_sqrt(dot3(particle.vel, particle.vel));
                let factor = blend.as_ref().map_or_else(
                    || simd_min(1.0, ratio),
                    |blend| {
                        1.0 + blend.weight(particle.age * reciprocal(particle.lifetime))
                            * simd_min(0.0, ratio - 1.0)
                    },
                );
                for vel in &mut particle.vel {
                    *vel *= factor;
                }
            }
            Operator::CollisionPlane { normal, distance, collision } => {
                let depth = dot3(particle.pos, *normal) - *distance;
                if depth < 0.0 {
                    collision_response(particle, *normal, depth, *collision);
                }
            }
            Operator::CollisionSphere { origin, point, radius, collision } => {
                let origin = point.map_or(*origin, |point| {
                    control_motion[point].get().step(step, substeps).position
                });
                let delta = std::array::from_fn(|axis| particle.pos[axis] - origin[axis]);
                let distance_squared = dot3(delta, delta);
                if distance_squared < *radius * *radius {
                    let inverse_distance = reciprocal_sqrt(distance_squared);
                    let normal = delta.map(|component| component * inverse_distance);
                    let depth = inverse_distance * distance_squared - *radius;
                    collision_response(particle, normal, depth, *collision);
                }
            }
            Operator::CollisionBounds { planes, collision, .. } => {
                let contact = planes
                    .iter()
                    .copied()
                    .rfind(|plane| dot3(particle.pos, plane.normal) < plane.distance);
                if let Some(plane) = contact {
                    let depth = dot3(particle.pos, plane.normal) - plane.distance;
                    collision_response(particle, plane.normal, depth, *collision);
                }
            }
            Operator::CollisionQuad(quad) => quad.apply(particle),
            Operator::CollisionBox => {}
            Operator::CollisionModel { ordinal, collision } => {
                if let Some(Some(capsule)) = self.model_capsules.get(*ordinal) {
                    capsule.collide(particle, *collision);
                }
            }
            Operator::AlphaFade { fade_in, fade_out } => {
                let life = particle_life_fraction(particle);
                let entering = life < *fade_in;
                let leaving = life > *fade_out;
                let incoming = if entering { life / *fade_in } else { 0.0 };
                let outgoing = if leaving { (1.0 - life) / (1.0 - *fade_out) } else { 0.0 };
                particle.alpha *= if entering || leaving {
                    f32::from_bits(incoming.to_bits() | outgoing.to_bits())
                } else {
                    1.0
                };
            }
            Operator::SizeChange { start, end, start_time, end_time } => {
                let life = particle_life_fraction(particle);
                let span = (*end_time - *start_time).max(f32::EPSILON);
                let t = ((life - *start_time) / span).clamp(0.0, 1.0);
                particle.size *= start + (end - start) * t;
            }
            Operator::AlphaChange { start, end, start_time, end_time } => {
                let life = particle_life_fraction(particle);
                let span = (*end_time - *start_time).max(f32::EPSILON);
                let t = ((life - *start_time) / span).clamp(0.0, 1.0);
                particle.alpha *= start + (end - start) * t;
            }
            Operator::ColorChange { start, end, start_time, end_time } => {
                let life = particle_life_fraction(particle);
                let span = (*end_time - *start_time).max(f32::EPSILON);
                let t = ((life - *start_time) / span).clamp(0.0, 1.0);
                for ((channel, start), end) in
                    particle.color.iter_mut().zip(start.iter()).zip(end.iter())
                {
                    *channel *= start + (end - start) * t;
                }
            }
            Operator::OscillateAlpha(osc) => {
                particle.alpha *= osc.blended(particle, osc.modulation(particle.age, seed));
            }
            Operator::OscillateSize(osc) => {
                let modulation = osc.modulation(particle.age, seed);
                particle.size *= if osc.blend.is_some() {
                    osc.blended(particle, modulation)
                } else {
                    modulation.max(0.0)
                };
            }
            Operator::OscillatePosition(osc) => {
                let weight = osc.weight(particle);
                let delta = osc.displacement(particle.age, dt, seed).map(|step| step * weight);
                let push = self.to_frame(std::array::from_fn(|axis| delta[axis] * osc.mask[axis]));
                for (pos, push) in particle.pos.iter_mut().zip(push) {
                    *pos += push;
                }
            }
            Operator::AngularMovement { force, drag, blend } => {
                let weight = blend.as_ref().map_or(1.0, |blend| {
                    blend.weight(particle.age * reciprocal(particle.lifetime))
                });
                let damping = 1.0 - (drag * dt * drag_scale).min(0.999_999_9) * weight;
                let turn = |angle: &mut f32, angular: &mut f32, force: f32| {
                    *angular += force * dt * weight;
                    *angle += *angular * dt * weight;
                    *angular *= damping;
                };
                let [x, y] = &mut particle.angle_xy;
                let [spin_x, spin_y] = &mut particle.angular_xy;
                turn(x, spin_x, force[0]);
                turn(y, spin_y, force[1]);
                turn(&mut particle.angle, &mut particle.angular, force[2]);
            }
            Operator::Turbulence { scale, speed, phase_range, time_scale, mask, blend, .. } => {
                let offset = time * *time_scale + seed * phase_range;
                let pos = particle.pos.map(|value| (value + offset) * scale);
                let weight = blend.as_ref().map_or(1.0, |blend| {
                    blend.weight(particle.age * reciprocal(particle.lifetime))
                });
                let strength = (speed.0 + (speed.1 - speed.0) * seed) * gain * weight;
                for (axis, mask) in mask.iter().enumerate() {
                    if *mask != 0.0 {
                        let first = (SPATIAL_AXES - axis) % SPATIAL_AXES;
                        let sample = [
                            pos[first],
                            pos[(first + 1) % SPATIAL_AXES],
                            pos[(first + 2) % SPATIAL_AXES],
                        ];
                        particle.vel[axis] += simplex(sample)
                            * strength
                            * mask
                            * dt
                            * drag_scale
                            * system.instance.speed;
                    }
                }
            }
            Operator::ControlPointAttract {
                control_point,
                scale,
                threshold,
                flags,
                delete_threshold,
                blend,
            } => {
                let point =
                    self.control_points.get(*control_point).copied().unwrap_or([0.0; SPATIAL_AXES]);
                let to = std::array::from_fn(|axis| point[axis] - particle.pos[axis]);
                let distance = dot3(to, to).sqrt();
                if distance > f32::MIN_POSITIVE && distance < *threshold {
                    let mut pull = scale * (1.0 - distance / threshold) * dt * drag_scale * speed;
                    if flags & 2 != 0 {
                        pull = pull.min(distance);
                    }
                    pull *= blend.as_ref().map_or(1.0, |blend| {
                        blend.weight(particle.age * reciprocal(particle.lifetime))
                    });
                    for (velocity, to) in particle.vel.iter_mut().zip(to) {
                        *velocity += to * pull / distance;
                    }
                }
                if flags & 1 != 0 {
                    let step =
                        std::array::from_fn(|axis| particle.pos[axis] - previous_position[axis]);
                    let target = std::array::from_fn(|axis| point[axis] - previous_position[axis]);
                    let length = dot3(step, step);
                    let along = if length > 0.0 {
                        (dot3(target, step) / length).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let separation = std::array::from_fn(|axis| target[axis] - step[axis] * along);
                    if dot3(separation, separation) <= delete_threshold * delete_threshold {
                        particle.age = particle.lifetime;
                    }
                }
            }
            Operator::ControlPointForce { control_point, offset, scale, threshold, blend } => {
                let point = control_motion[*control_point].get().step(step, substeps).position;
                let away = std::array::from_fn::<_, SPATIAL_AXES, _>(|axis| {
                    particle.pos[axis] - point[axis] - offset[axis]
                });
                let distance = dot3(away, away).sqrt();
                if distance > f32::MIN_POSITIVE && distance < *threshold {
                    let weight = blend.as_ref().map_or(1.0, |blend| {
                        blend.weight(particle.age * reciprocal(particle.lifetime))
                    });
                    let force = scale * weight * dt * speed / distance;
                    for (velocity, direction) in particle.vel.iter_mut().zip(away) {
                        *velocity += direction * force;
                    }
                }
            }
            Operator::Vortex(vortex) => {
                let point = self
                    .control_points
                    .get(vortex.control_point)
                    .copied()
                    .unwrap_or([0.0; SPATIAL_AXES]);
                let axis = normalize3(vortex.axis).unwrap_or([0.0, 0.0, 1.0]);
                let to = [
                    particle.pos[0] - point[0] - vortex.offset[0],
                    particle.pos[1] - point[1] - vortex.offset[1],
                    particle.pos[2] - point[2] - vortex.offset[2],
                ];
                let radial = if vortex.infinite_axis {
                    let along = dot3(to, axis);
                    [to[0] - axis[0] * along, to[1] - axis[1] * along, to[2] - axis[2] * along]
                } else {
                    to
                };
                let squared = dot3(radial, radial);
                if squared <= 0.0 {
                    return;
                }
                let inverse = reciprocal_sqrt(squared);
                let distance = squared * inverse;
                let direction = radial.map(|value| value * inverse);
                let tangent = cross3(direction, axis);
                let span = vortex.distance.1 - vortex.distance.0;
                let falloff = if span == 0.0 { FLAT_BAND_FALLOFF } else { reciprocal(span) };
                let ramp = ((distance - vortex.distance.0) * falloff).clamp(0.0, 1.0);
                let band = vortex.speed.0 + (vortex.speed.1 - vortex.speed.0) * ramp;
                let force = band * gain * dt * drag_scale * system.instance.speed;
                for (velocity, tangent) in particle.vel.iter_mut().zip(tangent) {
                    *velocity += tangent * force;
                }
            }
        }
    }
}
