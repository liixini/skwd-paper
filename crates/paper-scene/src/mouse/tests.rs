use super::*;

#[test]
fn shake_matches_engine() {
    let shake = Shake { amplitude: 2.0, speed: 0.5, roughness: 1.0 };
    let cases = [
        (None, [0.108_060_46, 0.194_371_9, 0.168_294_2]),
        (Some(1000.0), [10.806_046, 19.437_19, 0.0]),
    ];
    for (height, expected) in cases {
        let actual = shake.eye_offset(4.0, height);
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-4, "{height:?} {actual}");
        }
    }
}

#[test]
fn shake_probe_offset() {
    let still = Shake { amplitude: 3.0, speed: 0.0, roughness: 1.0 };
    assert_eq!(still.eye_offset(20.0, Some(1080.0)), [32.4, 0.0, 0.0]);
    let deep = Shake { amplitude: 10.0, speed: 0.0, roughness: 1.0 };
    assert_eq!(deep.eye_offset(20.0, None), [1.0, 0.0, 0.0]);
}

#[test]
fn shake_roughness_bends() {
    let shake = Shake { amplitude: 10.0, speed: 1.0, roughness: 2.0 };
    let rough = shake.eye_offset(1.0, None);
    for (actual, expected) in rough.iter().zip([5.539_531_7, 9.964_137, 8.627_309]) {
        assert!((actual - expected).abs() < 1e-3, "{rough:?}");
    }
    let smooth = Shake { roughness: 0.0, ..shake }.eye_offset(1.0, None);
    for (actual, expected) in smooth.iter().zip([0.540_302_3, 0.971_859_4, 0.841_471]) {
        assert!((actual - expected).abs() < 1e-5, "{smooth:?}");
    }
}

#[test]
fn clock_follows_world_position_and_offsets_shadow_away_from_pointer() {
    let clock = Clock3d { origin: [500.0, 200.0] };
    let canvas = [1000.0, 800.0];
    let neutral = clock.pose([0.5, 0.75], canvas);
    assert_eq!(neutral.angles, [0.0; 3]);
    assert_eq!(neutral.shadow, [0.0; 2]);
    let left = clock.pose([0.1, 0.25], canvas);
    let right = clock.pose([0.9, 0.25], canvas);
    assert!(left.angles[1] < 0.0 && right.angles[1] > 0.0);
    assert!(left.angles[0] < 0.0 && right.angles[0] < 0.0);
    assert_eq!(left.shadow, [4.0, 4.0]);
    assert_eq!(right.shadow, [-4.0, 4.0]);
}

#[test]
fn parallax_settles_and_stops_changing() {
    let config = Parallax { amount: 0.1, influence: 0.5, delay: 0.1, ..Default::default() };
    let mut displacement = [0.0; 2];
    for _ in 0..200 {
        displacement = config.displacement([1.0, 0.0], displacement, 1.0 / 30.0);
    }
    assert_eq!(displacement, [-0.025, 0.025]);
    assert_eq!(config.displacement([1.0, 0.0], displacement, 1.0 / 30.0), displacement);
    let disabled = Parallax { influence: 0.0, ..config };
    assert_eq!(disabled.displacement([1.0, 0.0], [0.0; 2], 1.0), [0.0; 2]);
}

#[test]
fn arbitrary_pointer_script_does_not_become_a_clock() {
    assert!(Clock3d::from_text(&serde_json::json!({"script":"export function update() { return input.cursorWorldPosition.x; }"}), [0.0; 2]).is_none());
}

#[test]
fn depth_parallax_uses_normalized_influence_with_zero_camera_amount() {
    let config = Parallax { amount: 0.0, influence: -0.2, delay: 2.0, ..Default::default() };
    for (pointer, expected) in [([0.25; 2], [0.55, 0.45]), ([0.75; 2], [0.45, 0.55])] {
        let mut position = [0.5; 2];
        for _ in 0..300 {
            position = config.position(pointer, position, 1.0 / 30.0);
        }
        assert_eq!(position, expected);
        assert_eq!(config.position(pointer, position, 1.0 / 30.0), position);
        assert_eq!(config.displacement(pointer, [0.0; 2], 1.0), [0.0; 2]);
    }
    assert_eq!(Parallax { influence: 0.0, ..config }.position([0.0; 2], [0.5; 2], 1.0), [0.5; 2]);
}

#[test]
fn parallax_matches_original_engine_step_response_and_zero_delay() {
    let config = Parallax { amount: 0.5, influence: 0.5, delay: 0.1, ..Default::default() };
    let first = config.position([1.0, 0.0], [0.5; 2], 1.0 / 30.0);
    assert!((first[0] - 0.58055556).abs() < 0.000001);
    assert_eq!(first[0], first[1]);
    let mut position = [0.5; 2];
    for _ in 0..6 {
        position = config.position([1.0, 0.0], position, 1.0 / 30.0);
    }
    assert!(position[0] > 0.725);
    let slow = Parallax { delay: 2.0, ..config }.position([1.0, 0.0], [0.5; 2], 1.0 / 30.0);
    assert!((slow[0] - 0.5277778).abs() < 0.000001);
    let instant = Parallax { delay: 0.0, ..config };
    assert_eq!(instant.position([1.0, 0.0], [0.5; 2], 1.0 / 30.0), [0.75; 2]);
    assert_eq!(instant.displacement([1.0, 0.0], [0.0; 2], 1.0 / 30.0), [-0.125, 0.125]);
}

#[test]
fn parallax_uniform_keeps_camera_offset_and_clamps_cursor() {
    let config = Parallax {
        influence: 0.5,
        camera_offset: [-44.4 / 1920.0, 6.0 / 1080.0],
        ..Default::default()
    };
    let center = config.position([0.5; 2], [0.5; 2], 1.0 / 30.0);
    assert!((center[0] - 0.476875).abs() < 0.000001);
    assert!((center[1] - 0.50555557).abs() < 0.000001);
    assert_eq!(config.position([2.0, -1.0], center, 0.1), config.position([1.0, 0.0], center, 0.1));
}
