use super::{mat4_inverse, mat4_mul, model_matrix};

#[test]
fn inverse_affine_roundtrip() {
    let matrix = model_matrix([12.0, -8.0, 3.0], [2.0, -3.0, 0.5], [0.3, -0.7, 1.2]);
    let inverse = mat4_inverse(&matrix).unwrap();
    for result in [mat4_mul(&matrix, &inverse), mat4_mul(&inverse, &matrix)] {
        for (index, value) in result.into_iter().enumerate() {
            assert!((value - f32::from(index % 5 == 0)).abs() < 0.00001);
        }
    }
}

#[test]
fn rejects_singular_matrix() {
    let matrix = model_matrix([0.0; 3], [1.0, 0.0, 1.0], [0.0; 3]);
    assert!(mat4_inverse(&matrix).is_none());
}
