use super::Affine;

#[test]
fn tiny_scale_roundtrips() {
    for scale in [[1e-7, 2e-7], [-1e-6, 3e-6]] {
        let transform = Affine::new([0.0; 2], scale, 0.6);
        let inverse = transform.inverse().unwrap();
        let point = inverse.point(transform.point([0.25, -0.5, 0.0]));
        assert!((point[0] - 0.25).abs() < 1e-5);
        assert!((point[1] + 0.5).abs() < 1e-5);
    }
}

#[test]
fn singular_basis_rejected() {
    let mut transform = Affine { a: 1.0, b: 1.0, c: 2.0, d: 2.0, x: 0.0, y: 0.0 };
    assert!(transform.inverse().is_none());
    transform.c = f32::NAN;
    assert!(transform.inverse().is_none());
    transform = Affine::new([0.0; 2], [f32::MIN_POSITIVE, 1.0], 0.0);
    assert!(transform.inverse().is_some());
}
