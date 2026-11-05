pub(crate) const MATRIX_SIDE: usize = 4;
pub type Mat4 = [f32; MATRIX_SIDE * MATRIX_SIDE];
pub const IDENTITY: Mat4 =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub(crate) fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub(crate) fn direction(matrix: &Mat4, vector: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        vector[0] * matrix[row]
            + vector[1] * matrix[MATRIX_SIDE + row]
            + vector[2] * matrix[2 * MATRIX_SIDE + row]
    })
}

pub(crate) fn native_inverse_sqrt(squared: f32) -> f32 {
    const SEED: u32 = 0x5f37_5a86;
    const REFINEMENT: f32 = 1.5;
    let inverse = f32::from_bits(SEED - (squared.to_bits() >> 1));
    (REFINEMENT - squared * 0.5 * inverse * inverse) * inverse
}

pub fn model_matrix(
    origin: [f32; crate::particles::SPATIAL_AXES],
    scale: [f32; crate::particles::SPATIAL_AXES],
    angles: [f32; crate::particles::SPATIAL_AXES],
) -> Mat4 {
    let (sx, cx) = angles[0].sin_cos();
    let (sy, cy) = angles[1].sin_cos();
    let (sz, cz) = angles[2].sin_cos();
    [
        scale[0] * cy * cz,
        scale[0] * cy * sz,
        -scale[0] * sy,
        0.0,
        scale[1] * (sx * sy * cz - cx * sz),
        scale[1] * (sx * sy * sz + cx * cz),
        scale[1] * sx * cy,
        0.0,
        scale[2] * (cx * sy * cz + sx * sz),
        scale[2] * (cx * sy * sz - sx * cz),
        scale[2] * cx * cy,
        0.0,
        origin[0],
        origin[1],
        origin[2],
        1.0,
    ]
}

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0; MATRIX_SIDE * MATRIX_SIDE];
    for col in 0..MATRIX_SIDE {
        for row in 0..MATRIX_SIDE {
            out[col * MATRIX_SIDE + row] = (0..MATRIX_SIDE)
                .map(|axis| a[axis * MATRIX_SIDE + row] * b[col * MATRIX_SIDE + axis])
                .sum();
        }
    }
    out
}

pub fn mat4_inverse(matrix: &Mat4) -> Option<Mat4> {
    let mut rows: [[f64; MATRIX_SIDE * 2]; MATRIX_SIDE] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if column < MATRIX_SIDE {
                f64::from(matrix[column * MATRIX_SIDE + row])
            } else {
                f64::from(column - MATRIX_SIDE == row)
            }
        })
    });
    for pivot in 0..MATRIX_SIDE {
        let selected = (pivot..MATRIX_SIDE)
            .max_by(|left, right| rows[*left][pivot].abs().total_cmp(&rows[*right][pivot].abs()))?;
        let scale = rows[selected][pivot];
        if scale == 0.0 || !scale.is_finite() {
            return None;
        }
        rows.swap(pivot, selected);
        for value in &mut rows[pivot] {
            *value /= scale;
        }
        let basis = rows[pivot];
        for (index, row) in rows.iter_mut().enumerate() {
            if index == pivot {
                continue;
            }
            let factor = row[pivot];
            for (value, basis) in row.iter_mut().zip(basis) {
                *value -= factor * basis;
            }
        }
    }
    let inverse = std::array::from_fn(|index| {
        rows[index % MATRIX_SIDE][index / MATRIX_SIDE + MATRIX_SIDE] as f32
    });
    inverse.iter().all(|value| value.is_finite()).then_some(inverse)
}

#[cfg(test)]
mod tests;
