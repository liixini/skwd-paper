use super::{BIND_BASIS, BIND_ORIGIN, VECTOR_AXES};
use crate::matrix::{MATRIX_SIDE, Mat4, direction, mat4_inverse, mat4_mul};

mod rotation;

pub use rotation::Rotation;

pub const MAX_STEP_SECONDS: f32 = 0.1;
pub const OWNER_UNSUPPORTED: &str = "spring owner unsupported";
pub const POSE_INVALID: &str = "puppet spring pose invalid";
pub const PALETTE_INVALID: &str = "puppet spring palette invalid";
const GRAVITY_RATE: f32 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Translation {
    pub stiffness: f32,
    pub friction: f32,
    pub inertia: f32,
    pub maximum: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gravity {
    pub direction: [f32; VECTOR_AXES],
    pub mass: f32,
}

#[derive(Clone, Copy, Default)]
pub struct State {
    offset: [f32; VECTOR_AXES],
    velocity: [f32; VECTOR_AXES],
}

impl State {
    fn fall(&mut self, gravity: Gravity, dt: f32) {
        let speed = gravity.mass * dt * GRAVITY_RATE;
        for (velocity, direction) in self.velocity.iter_mut().zip(gravity.direction) {
            *velocity += speed * direction;
        }
    }

    pub fn step(
        &mut self,
        parameters: Translation,
        dt: f32,
        baseline: [f32; VECTOR_AXES],
        previous: [f32; VECTOR_AXES],
        scale: f32,
    ) -> [f32; VECTOR_AXES] {
        let force = dt * parameters.stiffness;
        for axis in 0..self.offset.len() {
            self.offset[axis] -=
                ((self.offset[axis] + baseline[axis]) - previous[axis]) * parameters.inertia;
            self.velocity[axis] -= force * self.offset[axis];
            self.offset[axis] += dt * self.velocity[axis];
        }
        let maximum = scale * parameters.maximum;
        if maximum > 0.0 {
            let [x, y, z] = self.offset;
            let squared = (z * z + 0.0) + (y * y + x * x);
            if maximum * maximum < squared {
                let ratio = maximum / squared.sqrt();
                for value in &mut self.offset {
                    *value *= ratio;
                }
            }
        }
        let damping = (dt * parameters.friction).min(1.0);
        for value in &mut self.velocity {
            *value -= damping * *value;
        }
        std::array::from_fn(|axis| baseline[axis] + self.offset[axis])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Definition {
    pub bone: u16,
    pub translation: Option<Translation>,
    pub rotation: Option<Rotation>,
    pub gravity: Option<Gravity>,
}

#[derive(Default)]
struct Particle {
    state: State,
    swing: rotation::Swing,
    basis: Option<Mat4>,
}

pub struct Simulation {
    particles: Vec<Particle>,
    corrections: Vec<Option<Mat4>>,
}

pub struct Frame<'a> {
    pub dt: f32,
    pub owner: &'a Mat4,
    pub scale: f32,
}

impl Simulation {
    pub fn new(bones: usize, definitions: &[Definition]) -> Self {
        Self {
            particles: (0..definitions.len()).map(|_| Particle::default()).collect(),
            corrections: vec![None; bones],
        }
    }

    pub fn of(mesh: &super::Mesh) -> Option<Box<Self>> {
        (!mesh.springs.is_empty()).then(|| Box::new(Self::new(mesh.bones.len(), &mesh.springs)))
    }

    pub fn palette(&self, bones: &mut [Option<super::BonePose>]) -> bool {
        for (bone, correction) in bones.iter_mut().zip(&self.corrections) {
            let (Some(pose), Some(correction)) = (bone, correction) else { continue };
            let matrix = mat4_mul(correction, &pose.matrix());
            if matrix.iter().any(|value| !value.is_finite()) {
                return false;
            }
            *pose = super::BonePose::Spatial(matrix);
        }
        true
    }

    pub fn skin(&self, mesh: &super::Mesh, pose: &mut super::Frame) -> bool {
        if !self.palette(std::sync::Arc::make_mut(&mut pose.bones)) {
            return false;
        }
        for (position, vertex) in pose.positions.iter_mut().zip(&mesh.vertices) {
            *position = super::skin_vertex(vertex, &pose.bones);
            if position.iter().any(|value| !value.is_finite()) {
                return false;
            }
        }
        true
    }

    pub fn correct(&self, matrices: &mut [Mat4]) -> bool {
        for (matrix, correction) in matrices.iter_mut().zip(&self.corrections) {
            let Some(correction) = correction else { continue };
            *matrix = mat4_mul(correction, matrix);
            if matrix.iter().any(|value| !value.is_finite()) {
                return false;
            }
        }
        true
    }

    pub fn record(&mut self, definitions: &[Definition], bone: u16, world: &Mat4) {
        if let Ok(index) = definitions.binary_search_by_key(&bone, |definition| definition.bone) {
            self.particles[index].basis = Some(*world);
        }
    }

    pub fn step(
        &mut self,
        definitions: &[Definition],
        bones: &[super::Bone],
        world: &mut [Mat4],
        frame: &Frame<'_>,
    ) -> bool {
        if !affine_owner(frame.owner)
            || !frame.scale.is_finite()
            || !frame.dt.is_finite()
            || frame.dt < 0.0
        {
            return false;
        }
        let mut next = 0;
        for (index, (bone, matrix)) in bones.iter().zip(world).enumerate() {
            let inherited = bone.parent.and_then(|parent| self.corrections[usize::from(parent)]);
            let start = inherited.map_or(*matrix, |correction| mat4_mul(&correction, matrix));
            let mut correction = inherited;
            let mut pose = start;
            if let Some(definition) =
                definitions.get(next).filter(|entry| usize::from(entry.bone) == index)
            {
                let Some(simulated) = self.particles[next].settle(definition, &start, frame) else {
                    return false;
                };
                if simulated != start
                    && let Some(unsimulated) = mat4_inverse(matrix)
                {
                    correction = Some(mat4_mul(&simulated, &unsimulated));
                }
                pose = simulated;
                next += 1;
            }
            self.corrections[index] = correction;
            *matrix = pose;
        }
        true
    }
}

impl Particle {
    fn settle(&mut self, definition: &Definition, start: &Mat4, frame: &Frame<'_>) -> Option<Mat4> {
        let current = mat4_mul(frame.owner, start);
        let Some(inverse) = mat4_inverse(&current) else {
            *self = Self::default();
            return Some(*start);
        };
        let Some(last) = self.basis.replace(current) else {
            return Some(*start);
        };
        let origin = BIND_ORIGIN.map(|index| current[index]);
        if let Some(rotation) = &definition.rotation {
            let tip = super::pose::point(&last, rotation.tip);
            let previous =
                direction(&inverse, std::array::from_fn(|axis| tip[axis] - origin[axis]));
            let pull = definition.gravity.map(|gravity| {
                direction(&inverse, gravity.direction)
                    .map(|value| value * (gravity.mass * frame.scale))
            });
            let offset = direction(&inverse, self.state.offset);
            self.swing.step(rotation, frame.dt, previous, offset, pull);
        }
        if let Some(translation) = definition.translation {
            if let Some(gravity) = definition.gravity {
                self.state.fall(gravity, frame.dt);
            }
            self.state.step(
                translation,
                frame.dt,
                origin,
                BIND_ORIGIN.map(|index| last[index]),
                frame.scale,
            );
        }
        let simulated = mat4_mul(
            start,
            &crate::matrix::model_matrix(
                direction(&inverse, self.state.offset),
                [1.0; VECTOR_AXES],
                self.swing.euler,
            ),
        );
        self.basis = Some(mat4_mul(frame.owner, &simulated));
        let finite =
            self.state.velocity.iter().chain(&self.state.offset).all(|value| value.is_finite());
        (finite && self.swing.finite() && simulated.iter().all(|value| value.is_finite()))
            .then_some(simulated)
    }
}

fn affine_owner(owner: &Mat4) -> bool {
    let [_, _, _, xw, _, _, _, yw, _, _, _, zw, _, _, _, tw] = *owner;
    [xw, yw, zw].iter().all(|value| *value == 0.0)
        && tw == 1.0
        && owner.iter().all(|value| value.is_finite())
}

fn planar_owner(owner: &Mat4) -> bool {
    let [_, _, xz, xw, _, _, yz, yw, zx, zy, _, zw, _, _, _, tw] = *owner;
    [xz, xw, yz, yw, zx, zy, zw].iter().all(|value| *value == 0.0)
        && tw == 1.0
        && owner.iter().all(|value| value.is_finite())
}

pub fn owner_supported(owner: &Mat4) -> bool {
    planar_owner(owner) && mat4_inverse(owner).is_some()
}

#[cfg(test)]
mod tests;

const PERCENT: f32 = 100.0;
const ROTATION: &str = "r";
const TRANSLATION: &str = "t";
const SPRING: &str = "se";
const RIGID: &str = "re";
const GRAVITY: &str = "ge";
const TORQUE: &str = "lt";
const LIMITS: &str = "la";
const MOTIONS: [&str; 2] = [ROTATION, TRANSLATION];
const MODES: [&str; 2] = [SPRING, RIGID];
const SOLVERS: [&str; 2] = ["ik", "ikce"];
const TRANSLATION_AXES: [&str; VECTOR_AXES] = ["tax", "tay", "taz"];
const ROTATION_AXES: [&str; VECTOR_AXES] = ["rax", "ray", "raz"];

pub(super) enum Constraint {
    Inactive,
    Spring {
        translation: Option<Translation>,
        rotation: Option<Rotation>,
        gravity: Option<Gravity>,
    },
    Unsupported,
}

#[derive(serde::Deserialize)]
struct Parameters {
    ts: f32,
    tf: f32,
    ti: f32,
    tm: f32,
}

#[derive(serde::Deserialize)]
struct Falling {
    gd: serde_json::Value,
    m: f32,
}

#[derive(serde::Deserialize)]
struct Swinging {
    rs: f32,
    rf: f32,
    ri: f32,
    tp: serde_json::Value,
    lamin: Option<serde_json::Value>,
    lamax: Option<serde_json::Value>,
}

pub(super) fn constraint(metadata: &str) -> Constraint {
    if metadata.is_empty() {
        return Constraint::Inactive;
    }
    let Ok(metadata @ serde_json::Value::Object(_)) = crate::json::parse(metadata.as_bytes())
    else {
        return Constraint::Unsupported;
    };
    let switch = |key: &str| metadata.get(key).and_then(serde_json::Value::as_bool);
    let on = |key: &&str| switch(key) == Some(true);
    if SOLVERS.iter().any(on) {
        return Constraint::Unsupported;
    }
    if !(MOTIONS.iter().any(on) && MODES.iter().any(on)) {
        return Constraint::Inactive;
    }
    if [ROTATION, TORQUE].iter().all(on) {
        return Constraint::Unsupported;
    }
    let rigid = on(&RIGID);
    let locked = |keys: [&str; VECTOR_AXES]| {
        let axes = keys.map(switch);
        axes.map(|axis| axes.iter().all(Option::is_some) && axis == Some(false))
    };
    let translation =
        on(&TRANSLATION).then(|| translation(&metadata, locked(TRANSLATION_AXES), rigid));
    let rotation =
        on(&ROTATION).then(|| rotation(&metadata, locked(ROTATION_AXES), on(&LIMITS), rigid));
    let gravity = on(&GRAVITY).then(|| gravity(&metadata));
    match (translation, rotation, gravity) {
        (Some(None), _, _) | (_, Some(None), _) | (_, _, Some(None)) => Constraint::Unsupported,
        (translation, rotation, gravity) => Constraint::Spring {
            translation: translation.flatten(),
            rotation: rotation.flatten(),
            gravity: gravity.flatten(),
        },
    }
}

fn gravity(metadata: &serde_json::Value) -> Option<Gravity> {
    let parameters = <Falling as serde::Deserialize>::deserialize(metadata).ok()?;
    let direction = crate::effects::json_numbers(&parameters.gd)?;
    let direction = <[f32; VECTOR_AXES]>::try_from(direction).ok()?;
    (direction.iter().all(|value| value.is_finite()) && parameters.m.is_finite())
        .then_some(Gravity { direction, mass: parameters.m })
}

fn translation(
    metadata: &serde_json::Value,
    locked: [bool; VECTOR_AXES],
    rigid: bool,
) -> Option<Translation> {
    let parameters = <Parameters as serde::Deserialize>::deserialize(metadata).ok()?;
    let values = [parameters.ts, parameters.tf, parameters.ti, parameters.tm];
    (!locked.contains(&true) && values.into_iter().all(f32::is_finite)).then(|| Translation {
        stiffness: if rigid { 0.0 } else { parameters.ts },
        friction: parameters.tf,
        inertia: 1.0 - parameters.ti / PERCENT,
        maximum: parameters.tm,
    })
}

fn rotation(
    metadata: &serde_json::Value,
    locked: [bool; VECTOR_AXES],
    limited: bool,
    rigid: bool,
) -> Option<Rotation> {
    let parameters = <Swinging as serde::Deserialize>::deserialize(metadata).ok()?;
    let vector = |value: Option<&serde_json::Value>| {
        let values = crate::effects::json_numbers(value?)?;
        <[f32; VECTOR_AXES]>::try_from(values)
            .ok()
            .filter(|values| values.iter().all(|value| value.is_finite()))
    };
    let limits = if limited {
        Some([vector(parameters.lamin.as_ref())?, vector(parameters.lamax.as_ref())?])
    } else {
        None
    };
    if ![parameters.rs, parameters.rf, parameters.ri].into_iter().all(f32::is_finite) {
        return None;
    }
    Some(Rotation {
        stiffness: parameters.rs,
        friction: parameters.rf,
        inertia: 1.0 - parameters.ri / PERCENT,
        tip: vector(Some(&parameters.tp))?,
        locked,
        limits,
        rigid,
    })
}

const SCALE_AXES: f32 = VECTOR_AXES as f32;

pub fn owner_scale(owner: &Mat4) -> f32 {
    let length = |column: usize| {
        let start = column * MATRIX_SIDE;
        let [x, y, z, w] = std::array::from_fn(|index| owner[start + index]);
        (((w * w + z * z) + y * y) + x * x).sqrt()
    };
    (length(1) + length(0) + length(2)) / SCALE_AXES
}
