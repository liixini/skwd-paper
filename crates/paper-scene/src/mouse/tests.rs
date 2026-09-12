use super::*;

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
