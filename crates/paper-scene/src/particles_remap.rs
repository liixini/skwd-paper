use super::control::{self, PointMotion};
use super::{
    Animation, Initializer, Operator, OperatorBlend, Particle, ParticleSystem, SIZE_TO_NATIVE,
    SPATIAL_AXES, dot3, noise, num_of, reciprocal, reciprocal_sqrt, simd_saturate, simplex_one,
    vec3_of,
};
use crate::effects::FrameClock;
use serde::Deserialize;
use serde_json::Value;
use std::cell::Cell;
use std::f32::consts::{FRAC_PI_2, PI};

pub(super) const REMAP_INITIAL: &str = "remapinitialvalue";
pub(super) const REMAP_OPERATOR: &str = "remapvalue";
const INPUT: &str = "input";
const OUTPUT: &str = "output";
const INPUT_COMPONENT: &str = "inputcomponent";
const OUTPUT_COMPONENT: &str = "outputcomponent";
const OPERATION: &str = "operation";
const TRANSFORM: &str = "transformfunction";
const TRANSFORM_SCALE: &str = "transforminputscale";
const TRANSFORM_OCTAVES: &str = "transformoctaves";
const INPUT_MIN: &str = "inputrangemin";
const INPUT_MAX: &str = "inputrangemax";
const OUTPUT_MIN: &str = "outputrangemin";
const OUTPUT_MAX: &str = "outputrangemax";
const INPUT_POINT: &str = "inputcontrolpoint0";
const OUTPUT_POINT_START: &str = "outputcontrolpoint0";
const OUTPUT_POINT_END: &str = "outputcontrolpoint1";
const FLAGS: &str = "flags";
const CLAMP_INPUT: u32 = 1;
const CLAMP_OUTPUT: u32 = 2;
const DEFAULT_SCALE: f32 = 2.0;
const DEFAULT_OCTAVES: u32 = 3;
const CHANNEL_SEEDS: [u32; SPATIAL_AXES] = [0, 0x0b39_24ad, 0x493a_8e83];
const WAVE_BIAS: f32 = 0.5;
const AVERAGE_WEIGHT: f32 = 0.333_333_34;
const TRIANGLE_RANGE: f32 = 2.0;
const BATCH_LANES: usize = 4;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Field {
    LifetimeFraction,
    MaxLifetime,
    Size,
    Opacity,
    Speed,
    Rotation,
    AngularSpeed,
    DistanceToControlPoint,
    PositionBetweenTwoControlPoints,
    Runtime,
    TimeOfDay,
    ParticleSystemTime,
    LayerTime,
    Color,
    Position,
    Velocity,
    ControlPoint,
    DeltaToControlPoint,
    DirectionToControlPoint,
    LayerOrigin,
}

impl Field {
    fn is_vector(self) -> bool {
        matches!(
            self,
            Self::Color
                | Self::Position
                | Self::Velocity
                | Self::ControlPoint
                | Self::DeltaToControlPoint
                | Self::DirectionToControlPoint
                | Self::LayerOrigin
        )
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Operation {
    Remap,
    Multiply,
    Add,
    Subtract,
}

impl Operation {
    fn apply(self, current: f32, mapped: f32) -> f32 {
        match self {
            Self::Remap => mapped,
            Self::Multiply => current * mapped,
            Self::Add => current + mapped,
            Self::Subtract => current - mapped,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Component {
    All,
    X,
    Y,
    Z,
    Sum,
    Average,
    Max,
    Min,
}

impl Component {
    fn select(self, [x, y, z]: [f32; SPATIAL_AXES]) -> [f32; SPATIAL_AXES] {
        match self {
            Self::All => [x, y, z],
            Self::X => [x; SPATIAL_AXES],
            Self::Y => [y; SPATIAL_AXES],
            Self::Z => [z; SPATIAL_AXES],
            Self::Sum => [x + y + z; SPATIAL_AXES],
            Self::Average => [(x + y + z) * AVERAGE_WEIGHT; SPATIAL_AXES],
            Self::Max => [x.max(y).max(z); SPATIAL_AXES],
            Self::Min => [x.min(y).min(z); SPATIAL_AXES],
        }
    }

    fn axes(self) -> std::ops::Range<usize> {
        match self {
            Self::All => 0..SPATIAL_AXES,
            Self::X => 0..1,
            Self::Y => 1..2,
            Self::Z => 2..SPATIAL_AXES,
            Self::Sum | Self::Average | Self::Max | Self::Min => 0..0,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Transform {
    None,
    Sine,
    Square,
    Saw,
    Triangle,
    SimplexNoise,
    FbmNoise,
}

pub(super) struct Frame<'a> {
    pub(super) system: &'a ParticleSystem,
    pub(super) clock: FrameClock,
    pub(super) system_time: f32,
    pub(super) layer_time: f32,
    pub(super) points: &'a [Cell<PointMotion>; control::CONTROL_POINTS],
    pub(super) written: &'a Cell<u32>,
    pub(super) step: usize,
    pub(super) substeps: usize,
    pub(super) particle: usize,
}

impl Frame<'_> {
    fn point(&self, index: usize) -> [f32; SPATIAL_AXES] {
        self.points[index].get().step(self.step, self.substeps).position
    }

    fn write_point(&self, index: usize, update: impl FnOnce(&mut [f32; SPATIAL_AXES])) {
        let mut motion = self.points[index].get();
        update(&mut motion.position);
        self.points[index].set(motion);
        self.written.set(self.written.get() | 1 << index);
    }
}

pub struct Remap {
    initial: bool,
    input: Field,
    output: Field,
    operation: Operation,
    input_component: Component,
    output_component: Component,
    transform: Transform,
    input_min: [f32; SPATIAL_AXES],
    input_rate: [f32; SPATIAL_AXES],
    output_min: [f32; SPATIAL_AXES],
    output_span: [f32; SPATIAL_AXES],
    input_point: usize,
    output_points: [usize; 2],
    scale: f32,
    fractal: Option<noise::Fractal>,
    flags: u32,
    blend: Option<OperatorBlend>,
}

fn choice<T: for<'de> Deserialize<'de>>(entry: &Value, key: &str, fallback: T) -> Option<T> {
    entry.get(key).map_or(Some(fallback), |value| serde_json::from_value(value.clone()).ok())
}

impl Remap {
    pub(super) fn parse_initial(entry: &Value) -> Option<Self> {
        Self::parse(entry, true)
    }

    pub(super) fn parse_operator(entry: &Value) -> Option<Self> {
        Self::parse(entry, false)
    }

    fn parse(entry: &Value, initial: bool) -> Option<Self> {
        let input_min = vec3_of(entry.get(INPUT_MIN), [0.0; SPATIAL_AXES]);
        let input_max = vec3_of(entry.get(INPUT_MAX), [1.0; SPATIAL_AXES]);
        let output_min = vec3_of(entry.get(OUTPUT_MIN), [0.0; SPATIAL_AXES]);
        let output_max = vec3_of(entry.get(OUTPUT_MAX), [1.0; SPATIAL_AXES]);
        let input_rate = std::array::from_fn(|axis| {
            let span = input_max[axis] - input_min[axis];
            let span = if span == 0.0 { f32::EPSILON } else { span };
            if initial { span.recip() } else { reciprocal(span) }
        });
        let octaves =
            entry.get(TRANSFORM_OCTAVES).map_or(Some(DEFAULT_OCTAVES), noise::octaves_of)?;
        let transform = choice(entry, TRANSFORM, Transform::None)?;
        let default_input = if initial { Field::MaxLifetime } else { Field::LifetimeFraction };
        Some(Self {
            initial,
            input: choice(entry, INPUT, default_input)?,
            output: choice(entry, OUTPUT, Field::Size)?,
            operation: choice(entry, OPERATION, Operation::Multiply)?,
            input_component: choice(entry, INPUT_COMPONENT, Component::All)?,
            output_component: choice(entry, OUTPUT_COMPONENT, Component::All)?,
            transform,
            input_min,
            input_rate,
            output_min,
            output_span: std::array::from_fn(|axis| output_max[axis] - output_min[axis]),
            input_point: control::point_index(entry.get(INPUT_POINT), 0.0),
            output_points: [
                control::point_index(entry.get(OUTPUT_POINT_START), 0.0),
                control::point_index(entry.get(OUTPUT_POINT_END), 1.0),
            ],
            scale: num_of(entry.get(TRANSFORM_SCALE), DEFAULT_SCALE),
            fractal: matches!(transform, Transform::FbmNoise).then(|| noise::Fractal::new(octaves)),
            flags: num_of(entry.get(FLAGS), CLAMP_INPUT as f32) as u32,
            blend: if initial { None } else { OperatorBlend::parse(entry) },
        })
    }

    pub(super) fn apply(&self, particle: &mut Particle, frame: &Frame<'_>) {
        let lanes = self.ranged(self.read(particle, frame));
        let lanes_used = if self.output.is_vector() { SPATIAL_AXES } else { 1 };
        let mut mapped = [0.0; SPATIAL_AXES];
        for axis in 0..lanes_used {
            let value = self.transformed(lanes[axis], particle.noise_seed, axis)
                * self.output_span[axis]
                + self.output_min[axis];
            mapped[axis] =
                if self.flags & CLAMP_OUTPUT == 0 { value } else { simd_saturate(value) };
        }
        self.write(particle, mapped, frame);
        if self.initial {
            particle.base_size = particle.size;
            particle.base_alpha = particle.alpha;
        }
    }

    fn read(&self, particle: &Particle, frame: &Frame<'_>) -> [f32; SPATIAL_AXES] {
        let start = self.output_points[0];
        match self.input {
            Field::LifetimeFraction if self.initial => {
                [spawn_fraction(particle, frame.system); SPATIAL_AXES]
            }
            Field::LifetimeFraction => [particle.age * reciprocal(particle.lifetime); SPATIAL_AXES],
            Field::MaxLifetime => [particle.lifetime; SPATIAL_AXES],
            Field::Size => [particle.size * SIZE_TO_NATIVE; SPATIAL_AXES],
            Field::Opacity => [particle.alpha; SPATIAL_AXES],
            Field::Speed => [dot3(particle.vel, particle.vel).sqrt(); SPATIAL_AXES],
            Field::Rotation => [particle.angle; SPATIAL_AXES],
            Field::AngularSpeed => [particle.angular; SPATIAL_AXES],
            Field::DistanceToControlPoint => {
                let delta = between(frame.point(self.input_point), particle.pos);
                [dot3(delta, delta).sqrt(); SPATIAL_AXES]
            }
            Field::PositionBetweenTwoControlPoints => {
                let origin = frame.point(start);
                let axis = between(origin, frame.point(self.output_points[1]));
                let squared = dot3(axis, axis);
                let along = dot3(between(origin, particle.pos), axis);
                [if squared > 0.0 { along * reciprocal(squared) } else { 0.0 }; SPATIAL_AXES]
            }
            Field::TimeOfDay => [frame.clock.daytime; SPATIAL_AXES],
            Field::ParticleSystemTime if self.initial => [frame.system_time; SPATIAL_AXES],
            Field::Runtime | Field::ParticleSystemTime => [frame.clock.time; SPATIAL_AXES],
            Field::LayerTime => [frame.layer_time; SPATIAL_AXES],
            Field::Color => self.input_component.select(particle.color),
            Field::Position => self.input_component.select(particle.pos),
            Field::Velocity => self.input_component.select(particle.vel),
            Field::ControlPoint => self.input_component.select(frame.point(self.input_point)),
            Field::DeltaToControlPoint => {
                self.input_component.select(between(particle.pos, frame.point(self.input_point)))
            }
            Field::DirectionToControlPoint => {
                let delta = between(particle.pos, frame.point(self.input_point));
                self.input_component.select(unit(delta))
            }
            Field::LayerOrigin => {
                let (x, y, z) = frame.system.origin;
                self.input_component.select([x, y, z])
            }
        }
    }

    fn ranged(&self, value: [f32; SPATIAL_AXES]) -> [f32; SPATIAL_AXES] {
        let mut lanes = std::array::from_fn(|axis| {
            (value[axis] - self.input_min[axis]) * self.input_rate[axis]
        });
        if !self.initial && !self.input.is_vector() {
            lanes = [lanes[0]; SPATIAL_AXES];
        }
        if self.flags & CLAMP_INPUT != 0 {
            lanes = lanes.map(simd_saturate);
        }
        lanes
    }

    fn transformed(&self, value: f32, seed: u32, axis: usize) -> f32 {
        let scaled = value * self.scale;
        match self.transform {
            Transform::None => value,
            Transform::Sine => {
                (value * (self.scale * PI) - FRAC_PI_2).sin() * WAVE_BIAS + WAVE_BIAS
            }
            Transform::Square => (scaled - scaled.trunc()).round_ties_even() + wrap_offset(scaled),
            Transform::Saw => scaled - scaled.trunc() + wrap_offset(value),
            Transform::Triangle => {
                let magnitude = scaled.abs();
                1.0 - ((magnitude - magnitude.trunc()) * TRIANGLE_RANGE - 1.0).abs()
            }
            Transform::SimplexNoise if self.initial => simplex_one(scaled) * WAVE_BIAS + WAVE_BIAS,
            Transform::SimplexNoise => {
                noise::seeded_simplex(seed ^ CHANNEL_SEEDS[axis], scaled) * WAVE_BIAS + WAVE_BIAS
            }
            Transform::FbmNoise => {
                let Some(fractal) = &self.fractal else { return value };
                let sample = if self.initial {
                    fractal.sample_plain(scaled)
                } else {
                    fractal.sample(seed ^ CHANNEL_SEEDS[axis], scaled)
                };
                sample * WAVE_BIAS + WAVE_BIAS
            }
        }
    }

    fn combined(&self, current: f32, mapped: f32, weight: Option<f32>) -> f32 {
        let target = self.operation.apply(current, mapped);
        weight.map_or(target, |weight| (target - current) * weight + current)
    }

    fn combined_vector(
        &self,
        target: &mut [f32; SPATIAL_AXES],
        mapped: [f32; SPATIAL_AXES],
        weight: Option<f32>,
    ) {
        for axis in self.output_component.axes() {
            target[axis] = self.combined(target[axis], mapped[axis], weight);
        }
    }

    fn write(&self, particle: &mut Particle, mapped: [f32; SPATIAL_AXES], frame: &Frame<'_>) {
        let weight = self
            .blend
            .as_ref()
            .map(|blend| blend.weight(particle.age * reciprocal(particle.lifetime)));
        let value = mapped[0];
        let [start, end] = self.output_points;
        match self.output {
            Field::MaxLifetime => {
                particle.lifetime = self.combined(particle.lifetime, value, weight);
            }
            Field::Size => {
                let native = if matches!(self.operation, Operation::Multiply) {
                    value
                } else {
                    value / SIZE_TO_NATIVE
                };
                particle.size = self.combined(particle.size, native, weight);
            }
            Field::Opacity => particle.alpha = self.combined(particle.alpha, value, weight),
            Field::Speed => {
                let speed = dot3(particle.vel, particle.vel).sqrt();
                let mut factor = self.combined(speed, value, weight);
                if speed > 0.0 {
                    factor *= reciprocal(speed);
                }
                particle.vel = particle.vel.map(|velocity| velocity * factor);
            }
            Field::Rotation => particle.angle = self.combined(particle.angle, value, weight),
            Field::AngularSpeed => {
                if frame.system.angular_motion {
                    particle.angular = self.combined(particle.angular, value, weight);
                }
            }
            Field::DistanceToControlPoint => {
                let point = frame.point(start);
                let delta = between(point, particle.pos);
                let (distance, inverse) = nonzero_measure(delta);
                let target = self.combined(distance, value, weight);
                particle.pos =
                    std::array::from_fn(|axis| target * inverse * delta[axis] + point[axis]);
            }
            Field::PositionBetweenTwoControlPoints => {
                let origin = frame.point(start);
                let segment = between(origin, frame.point(end));
                let (length, inverse) = nonzero_measure(segment);
                let axis = segment.map(|component| component * inverse);
                let offset = between(origin, particle.pos);
                let along = dot3(offset, axis);
                let target = self.combined(along * inverse, value, weight) * length;
                particle.pos = std::array::from_fn(|index| {
                    target * axis[index] + (offset[index] - along * axis[index]) + origin[index]
                });
            }
            Field::Color => self.combined_vector(&mut particle.color, mapped, weight),
            Field::Position => self.combined_vector(&mut particle.pos, mapped, weight),
            Field::Velocity => self.combined_vector(&mut particle.vel, mapped, weight),
            Field::ControlPoint => {
                if self.initial || frame.particle.is_multiple_of(BATCH_LANES) {
                    frame.write_point(start, |point| self.combined_vector(point, mapped, weight));
                }
            }
            Field::DeltaToControlPoint => {
                let point = frame.point(start);
                let mut delta = between(particle.pos, point);
                self.combined_vector(&mut delta, mapped, weight);
                particle.pos = between(delta, point);
            }
            Field::DirectionToControlPoint => {
                let point = frame.point(start);
                let delta = between(particle.pos, point);
                let distance = measure(delta).0;
                let mut direction = unit(delta);
                self.combined_vector(&mut direction, mapped, weight);
                let scale = distance * positive_inverse(direction);
                particle.pos = std::array::from_fn(|axis| point[axis] - scale * direction[axis]);
            }
            Field::LifetimeFraction
            | Field::Runtime
            | Field::TimeOfDay
            | Field::ParticleSystemTime
            | Field::LayerTime
            | Field::LayerOrigin => {}
        }
    }
}

pub(super) fn written_points(initializers: &[Initializer], operators: &[Operator]) -> u8 {
    initializers
        .iter()
        .filter_map(|step| match step {
            Initializer::Remap(remap) => Some(remap),
            _ => None,
        })
        .chain(operators.iter().filter_map(|step| match step {
            Operator::Remap(remap) => Some(remap),
            _ => None,
        }))
        .filter(|remap| matches!(remap.output, Field::ControlPoint))
        .fold(0, |mask, remap| mask | 1 << remap.output_points[0])
}

fn wrap_offset(value: f32) -> f32 {
    if value < 0.0 { 1.0 } else { 0.0 }
}

fn spawn_fraction(particle: &Particle, system: &ParticleSystem) -> f32 {
    if matches!(system.animation, Animation::RandomFrame) {
        f32::from_bits(particle.noise_seed)
    } else {
        0.0
    }
}

fn between(from: [f32; SPATIAL_AXES], to: [f32; SPATIAL_AXES]) -> [f32; SPATIAL_AXES] {
    std::array::from_fn(|axis| to[axis] - from[axis])
}

fn measure(vector: [f32; SPATIAL_AXES]) -> (f32, f32) {
    let squared = dot3(vector, vector);
    let root = reciprocal_sqrt(squared);
    (root * squared, root)
}

fn positive_inverse(vector: [f32; SPATIAL_AXES]) -> f32 {
    let (length, root) = measure(vector);
    if length > 0.0 { root } else { 0.0 }
}

fn nonzero_measure(vector: [f32; SPATIAL_AXES]) -> (f32, f32) {
    let (length, root) = measure(vector);
    (length, if length == 0.0 { 0.0 } else { root })
}

fn unit(vector: [f32; SPATIAL_AXES]) -> [f32; SPATIAL_AXES] {
    let inverse = positive_inverse(vector);
    vector.map(|component| component * inverse)
}

#[cfg(test)]
#[path = "particles_remap_tests.rs"]
pub(super) mod tests;
