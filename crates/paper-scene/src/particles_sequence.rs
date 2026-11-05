use super::{
    MAP_SEQUENCE_BOUNDS, MAP_SEQUENCE_DEFAULT_COUNT, MAP_SEQUENCE_LIMIT, MAP_SEQUENCE_MIN_COUNT,
    MAP_SEQUENCE_MIRROR, Particle, SequencePhase, dot3, num_of, vec3_of,
};
use serde_json::Value;

pub(super) const BETWEEN_SEQUENCE: &str = "mapsequencebetweencontrolpoints";
const COUNT: &str = "count";
const START: &str = "controlpointstart";
const END: &str = "controlpointend";
const FLAGS: &str = "flags";
const ARC_AMOUNT: &str = "arcamount";
const ARC_DIRECTION: &str = "arcdirection";
const SIZE_REDUCTION: &str = "sizereductionamount";
const LAST_CONTROL_POINT: usize = 7;
const TAPER_POSITION: u32 = 1;
const TAPER_VELOCITY: u32 = 2;
const TAPER_SIZE: u32 = 4;
const ARC: u32 = 8;
const DEFAULT_ARC: f32 = 0.3;
const DEFAULT_REDUCTION: f32 = 0.9;
const VECTOR_COMPONENTS: usize = 3;

pub struct BetweenSequence {
    pub(super) step: f32,
    bounds: [f32; 2],
    mirror: bool,
    start: usize,
    end: usize,
    flags: u32,
    arc: f32,
    direction: [f32; 3],
    reduction: f32,
}

impl BetweenSequence {
    pub(super) fn parse(entry: &Value) -> Self {
        let bounds = vec3_of(entry.get(MAP_SEQUENCE_BOUNDS), [0.0, 1.0, 0.0]);
        let count = num_of(entry.get(COUNT), MAP_SEQUENCE_DEFAULT_COUNT);
        Self {
            step: 1.0 / (count - 1.0).max(MAP_SEQUENCE_MIN_COUNT),
            bounds: [bounds[0], bounds[1]],
            mirror: entry.get(MAP_SEQUENCE_LIMIT).and_then(Value::as_str)
                == Some(MAP_SEQUENCE_MIRROR),
            start: (num_of(entry.get(START), 0.0) as i32 as u32 as usize).min(LAST_CONTROL_POINT),
            end: (num_of(entry.get(END), 1.0) as i32 as u32 as usize).min(LAST_CONTROL_POINT),
            flags: num_of(entry.get(FLAGS), 0.0) as u32,
            arc: num_of(entry.get(ARC_AMOUNT), DEFAULT_ARC),
            direction: vec3_of(entry.get(ARC_DIRECTION), [0.0, 1.0, 0.0]),
            reduction: num_of(entry.get(SIZE_REDUCTION), DEFAULT_REDUCTION),
        }
    }

    pub(super) fn apply(
        &self,
        particle: &mut Particle,
        state: &mut SequencePhase,
        points: &[[f32; 3]],
        world: bool,
    ) {
        let start = points[self.start];
        let delta = std::array::from_fn(|axis| points[self.end][axis] - start[axis]);
        let distance = dot3(delta, delta).sqrt().max(f32::MIN_POSITIVE);
        let axis = delta.map(|value| value / distance);
        let relative = std::array::from_fn(|index| {
            particle.pos[index] - if world { start[index] } else { 0.0 }
        });
        let axial = dot3(relative, axis);
        let phase = self.bounds[0] + state.phase * (self.bounds[1] - self.bounds[0]);
        let taper = 1.0 - (state.phase * 2.0 - 1.0).abs().powi(2);
        for index in 0..VECTOR_COMPONENTS {
            let mut radial = relative[index] - axial * axis[index];
            if self.flags & TAPER_POSITION != 0 {
                radial *= taper;
            }
            particle.pos[index] = radial + phase * axis[index] * distance + start[index];
            if self.flags & ARC != 0 {
                particle.pos[index] += self.direction[index] * (taper * distance * self.arc);
            }
            if self.flags & TAPER_VELOCITY != 0 {
                particle.vel[index] *= taper;
            }
        }
        if self.flags & TAPER_SIZE != 0 {
            particle.base_size *= (1.0 - self.reduction) + self.reduction * taper;
            particle.size = particle.base_size;
        }
        if !self.mirror && state.phase + state.step > 1.0 {
            state.phase = 0.0;
        } else {
            state.advance(self.mirror);
        }
    }
}
