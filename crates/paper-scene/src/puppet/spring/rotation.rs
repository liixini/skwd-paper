use super::{BIND_BASIS, VECTOR_AXES};
use crate::matrix::{Mat4, cross3, dot3};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const QUATERNION_SIDE: usize = 4;
const IDENTITY: Quaternion = [1.0, 0.0, 0.0, 0.0];
const DEGREE: f32 = PI / 180.0;
const FRAME: f32 = 1.0 / 60.0;
const TIP_SPEED: f32 = 900.0;
const PARALLEL: f32 = 0.999_999_9;
const X_AXIS: [f32; VECTOR_AXES] = [1.0, 0.0, 0.0];
const Z_AXIS: [f32; VECTOR_AXES] = [0.0, 0.0, 1.0];

type Quaternion = [f32; QUATERNION_SIDE];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    pub stiffness: f32,
    pub friction: f32,
    pub inertia: f32,
    pub tip: [f32; VECTOR_AXES],
    pub locked: [bool; VECTOR_AXES],
    pub limits: Option<[[f32; VECTOR_AXES]; 2]>,
    pub rigid: bool,
}

pub(super) struct Swing {
    velocity: Quaternion,
    pub(super) euler: [f32; VECTOR_AXES],
}

impl Default for Swing {
    fn default() -> Self {
        Self { velocity: IDENTITY, euler: [0.0; VECTOR_AXES] }
    }
}

impl Swing {
    pub(super) fn step(
        &mut self,
        rotation: &Rotation,
        dt: f32,
        previous: [f32; VECTOR_AXES],
        offset: [f32; VECTOR_AXES],
        pull: Option<[f32; VECTOR_AXES]>,
    ) {
        let arm = super::super::pose::point(
            &crate::matrix::model_matrix([0.0; VECTOR_AXES], [1.0; VECTOR_AXES], self.euler),
            rotation.tip,
        );
        let mut target: [f32; VECTOR_AXES] = std::array::from_fn(|axis| arm[axis] + offset[axis]);
        if let Some(pull) = pull {
            let arm = normalize(arm);
            let along = dot3(arm, pull);
            target = std::array::from_fn(|axis| target[axis] - (pull[axis] - along * arm[axis]));
        }
        let distance =
            length(std::array::from_fn(|axis| target[axis] - previous[axis])).min(TIP_SPEED * dt);
        let pull = between(normalize(target), normalize(previous));
        self.velocity = multiply(
            self.velocity,
            slerp(IDENTITY, pull, (distance * rotation.inertia * DEGREE).min(1.0)),
        );
        if !rotation.rigid {
            let rest = half_angles(self.euler.map(|angle| -angle));
            self.velocity = multiply(
                self.velocity,
                slerp(IDENTITY, rest, (rotation.stiffness * DEGREE * dt).min(1.0)),
            );
        }
        let turn = slerp(IDENTITY, self.velocity, (dt / FRAME).min(1.0));
        let turned = crate::matrix::mat4_mul(
            &super::super::pose::matrix(turn, [0.0; VECTOR_AXES], [1.0; VECTOR_AXES]),
            &crate::matrix::model_matrix([0.0; VECTOR_AXES], [1.0; VECTOR_AXES], self.euler),
        );
        self.euler = angles(&turned);
        if !rotation.rigid {
            self.euler = self.euler.map(wrap);
            self.lock(rotation);
        }
        if let Some([low, high]) = rotation.limits {
            let free = self.euler;
            self.euler = std::array::from_fn(|axis| free[axis].max(low[axis]).min(high[axis]));
            let excess = half_angles(std::array::from_fn(|axis| free[axis] - self.euler[axis]));
            self.velocity = multiply(conjugate(excess), self.velocity);
        }
        if rotation.rigid {
            self.lock(rotation);
        }
        self.velocity = slerp(self.velocity, IDENTITY, (dt * rotation.friction).min(1.0));
    }

    fn lock(&mut self, rotation: &Rotation) {
        for (angle, locked) in self.euler.iter_mut().zip(rotation.locked) {
            if locked {
                *angle = 0.0;
            }
        }
    }

    pub(super) fn finite(&self) -> bool {
        self.velocity.iter().chain(&self.euler).all(|value| value.is_finite())
    }
}

fn angles(matrix: &Mat4) -> [f32; VECTOR_AXES] {
    let at = |column: usize, row: usize| matrix[BIND_BASIS[column][row]];
    let z = at(0, 1).atan2(at(0, 0));
    let y = (-at(0, 2)).atan2((at(2, 2) * at(2, 2) + at(1, 2) * at(1, 2)).sqrt());
    let (sz, cz) = z.sin_cos();
    let x = (sz * at(2, 0) - cz * at(2, 1)).atan2(cz * at(1, 1) - sz * at(1, 0));
    [x, y, z]
}

fn wrap(angle: f32) -> f32 {
    if angle >= 0.0 { (angle + PI) % TAU - PI } else { (angle - PI) % TAU + PI }
}

fn half_angles(angles: [f32; VECTOR_AXES]) -> Quaternion {
    super::super::pose::half_turn(
        angles.map(|angle| (angle * 0.5).sin()),
        angles.map(|angle| (angle * 0.5).cos()),
    )
}

fn multiply(left: Quaternion, right: Quaternion) -> Quaternion {
    let [aw, ax, ay, az] = left;
    let [bw, bx, by, bz] = right;
    [
        ((bw * aw - bx * ax) - by * ay) - bz * az,
        (bx * aw + ax * bw + bz * ay) - az * by,
        (by * aw + ay * bw + az * bx) - bz * ax,
        (bz * aw + az * bw + by * ax) - ay * bx,
    ]
}

fn conjugate([w, x, y, z]: Quaternion) -> Quaternion {
    [w, -x, -y, -z]
}

fn slerp(from: Quaternion, mut to: Quaternion, amount: f32) -> Quaternion {
    let [fw, fx, fy, fz] = from;
    let [tw, tx, ty, tz] = to;
    let mut cosine = fy * ty + fw * tw + fz * tz + fx * tx;
    if cosine < 0.0 {
        to = to.map(|value| -value);
        cosine = -cosine;
    }
    let remaining = 1.0 - amount;
    if cosine > PARALLEL {
        return std::array::from_fn(|index| remaining * from[index] + to[index] * amount);
    }
    let angle = cosine.acos();
    let sine = angle.sin();
    let near = (angle * amount).sin();
    let far = (remaining * angle).sin();
    std::array::from_fn(|index| (from[index] * far + to[index] * near) / sine)
}

fn between(from: [f32; VECTOR_AXES], to: [f32; VECTOR_AXES]) -> Quaternion {
    let cosine = dot3(from, to);
    if cosine >= PARALLEL || cosine.is_nan() {
        return IDENTITY;
    }
    if cosine >= -PARALLEL {
        let root = (cosine + 1.0 + cosine + 1.0).sqrt();
        let [x, y, z] = cross3(from, to).map(|value| value * (1.0 / root));
        return [root * 0.5, x, y, z];
    }
    let mut axis = cross3(Z_AXIS, from);
    if dot3(axis, axis) < f32::EPSILON {
        axis = cross3(X_AXIS, from);
    }
    let scale = FRAC_PI_2.sin() / length(axis);
    let [x, y, z] = axis.map(|value| value * scale);
    [FRAC_PI_2.cos(), x, y, z]
}

fn length(vector: [f32; VECTOR_AXES]) -> f32 {
    dot3(vector, vector).sqrt()
}

fn normalize(vector: [f32; VECTOR_AXES]) -> [f32; VECTOR_AXES] {
    let scale = 1.0 / length(vector);
    vector.map(|value| value * scale)
}

#[cfg(test)]
#[path = "rotation_tests.rs"]
mod tests;
