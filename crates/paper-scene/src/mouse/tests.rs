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
    let config = Parallax { amount: 0.1, influence: 0.5, delay: 10.0 };
    let mut displacement = [0.0; 2];
    for _ in 0..200 {
        displacement = config.displacement([1.0, 0.0], displacement, 1.0 / 30.0);
    }
    assert_eq!(displacement, [0.025, -0.025]);
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
    let config = Parallax { amount: 0.0, influence: -0.2, delay: 2.0 };
    for (pointer, expected) in [([0.25; 2], [0.45; 2]), ([0.75; 2], [0.55; 2])] {
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
