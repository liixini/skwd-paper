use super::*;

fn offsets(shape: &SphereShape) -> Vec<[f32; 3]> {
    let mut rng = Rng::new(7);
    (0..2000).map(|_| shape.offset(&mut rng)).collect()
}

#[test]
fn defaults_to_xy_ring() {
    let ring = SphereShape::parse(&serde_json::json!({"distancemin": 300, "distancemax": 300}));
    assert_eq!(ring.directions, [1.0, 1.0, 0.0]);
    let points = offsets(&ring);
    assert!(points.iter().all(|p| p[2] == 0.0 && (p[0].hypot(p[1]) - 300.0).abs() < 1e-2));
    assert!(points.iter().any(|p| p[0] < -290.0) && points.iter().any(|p| p[1] > 290.0));
}

#[test]
fn cone_narrows_around_x() {
    let spike = SphereShape::parse(
        &serde_json::json!({"directions": "1 1 1", "distancemax": 300, "distancemin": 300, "cone": 1}),
    );
    assert!(offsets(&spike).iter().all(|p| (p[0] - 300.0).abs() < 1e-2 && p[1].abs() < 1e-2));
    let half = SphereShape::parse(
        &serde_json::json!({"directions": "1 1 1", "distancemax": 300, "cone": 0.5}),
    );
    let points = offsets(&half);
    assert!(points.iter().all(|p| p[0] > -1e-3));
    assert!(points.iter().any(|p| p[2] < -100.0) && points.iter().any(|p| p[2] > 100.0));
}

#[test]
fn launches_along_offset() {
    let burst = SphereShape::parse(
        &serde_json::json!({"distancemin": 300, "distancemax": 300, "speedmin": 50, "speedmax": 50}),
    );
    let mut rng = Rng::new(3);
    let velocity = burst.launch([180.0, -240.0, 0.0], &mut rng, |vector| vector).unwrap();
    assert!((velocity[0] - 30.0).abs() < 1e-4 && (velocity[1] + 40.0).abs() < 1e-4);
    assert_eq!(
        SphereShape::parse(&serde_json::json!({"distancemax": 300})).launch(
            [1.0; 3],
            &mut rng,
            |vector| vector
        ),
        None
    );
}

#[test]
fn point_launch_stays_planar() {
    let burst = SphereShape::parse(&serde_json::json!({"speedmin": 50, "speedmax": 50}));
    let mut rng = Rng::new(5);
    for _ in 0..100 {
        let velocity = burst.launch([0.0; 3], &mut rng, |vector| vector).unwrap();
        assert_eq!(velocity[2], 0.0);
        assert!((velocity[0].hypot(velocity[1]) - 50.0).abs() < 1e-3);
    }
}
