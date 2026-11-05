use crate::matrix::{Mat4, native_inverse_sqrt};

const AXES: usize = 3;
const SIDE: usize = 4;
const Z: usize = 2;
const QUATERNION_FACTOR: f32 = 0.25;

#[derive(Debug, PartialEq)]
pub(super) struct Bind {
    pub rotation: [f32; SIDE],
    pub scale: [f32; AXES],
}

impl Bind {
    pub fn new(matrix: &Mat4) -> Self {
        let columns: [[f32; SIDE]; AXES] =
            std::array::from_fn(|column| std::array::from_fn(|row| matrix[column * SIDE + row]));
        let scale = std::array::from_fn(|column| {
            let [x, y, z, w] = columns[column];
            let squared = if column == 0 {
                ((x * x + y * y) + w * w) + z * z
            } else {
                ((w * w + z * z) + y * y) + x * x
            };
            squared.sqrt()
        });
        let axes = columns.map(|[x, y, z, w]| {
            let inverse = native_inverse_sqrt(((w * w + z * z) + y * y) + x * x);
            [x * inverse, y * inverse, z * inverse]
        });
        Self { rotation: rotation(axes), scale }
    }
}

fn rotation(columns: [[f32; AXES]; AXES]) -> [f32; SIDE] {
    let [x, y, z] = columns;
    let candidates =
        [(x[0] + y[1]) + z[Z], (x[0] - y[1]) - z[Z], (y[1] - x[0]) - z[Z], (z[Z] - x[0]) - y[1]];
    let mut selected = 0;
    for index in 1..SIDE {
        if candidates[index] > candidates[selected] {
            selected = index;
        }
    }
    let largest = (candidates[selected] + 1.0).sqrt() * 0.5;
    let factor = QUATERNION_FACTOR / largest;
    match selected {
        0 => [largest, (y[Z] - z[1]) * factor, (z[0] - x[Z]) * factor, (x[1] - y[0]) * factor],
        1 => [(y[Z] - z[1]) * factor, largest, (x[1] + y[0]) * factor, (x[Z] + z[0]) * factor],
        2 => [(z[0] - x[Z]) * factor, (x[1] + y[0]) * factor, largest, (y[Z] + z[1]) * factor],
        _ => [(x[1] - y[0]) * factor, (x[Z] + z[0]) * factor, (y[Z] + z[1]) * factor, largest],
    }
}
