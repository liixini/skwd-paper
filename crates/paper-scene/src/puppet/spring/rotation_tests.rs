use super::super::tests::close;
use super::{IDENTITY, Rotation, Swing, angles, between, slerp, wrap};

const SWING: Rotation = Rotation {
    stiffness: 50.0,
    friction: 50.0,
    inertia: 0.91,
    tip: [0.0, 100.0, 0.0],
    locked: [true, true, false],
    limits: None,
    rigid: false,
};

#[test]
fn euler_round_trip() {
    for euler in [[0.0, 0.0, 0.0], [0.3, -0.2, 1.1], [-1.0, 0.7, -2.9], [0.0, 0.0, 3.1]] {
        let matrix = crate::matrix::model_matrix([0.0; 3], [1.0; 3], euler);
        assert!(close(&angles(&matrix), &euler), "{euler:?}");
    }
}

#[test]
fn between_turns_vectors() {
    let turn = |from: [f32; 3], to: [f32; 3]| {
        let matrix = crate::puppet::pose::matrix(between(from, to), [0.0; 3], [1.0; 3]);
        crate::puppet::pose::point(&matrix, from)
    };
    let tilted = [0.6, 0.8, 0.0];
    assert!(close(&turn([1.0, 0.0, 0.0], tilted), &tilted));
    assert_eq!(between(tilted, tilted), IDENTITY);
    assert!(close(&turn([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]), &[-1.0, 0.0, 0.0]));
}

#[test]
fn slerp_short_path() {
    let quarter = [std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2];
    assert!(close(&slerp(IDENTITY, quarter, 0.0), &IDENTITY));
    assert!(close(&slerp(IDENTITY, quarter, 1.0), &quarter));
    let flipped = quarter.map(|value| -value);
    assert!(close(&slerp(IDENTITY, flipped, 1.0), &quarter));
}

#[test]
fn lagging_tip_swings() {
    let mut swing = Swing::default();
    let lag = 0.2_f32;
    let previous = [-100.0 * lag.sin(), 100.0 * lag.cos(), 0.0];
    swing.step(&SWING, 1.0 / 60.0, previous, [0.0; 3], None);
    assert!(swing.euler[2] > 0.0 && swing.euler[2] < lag);
    assert_eq!(swing.euler[..2], [0.0, 0.0]);
}

#[test]
fn stiffness_restores_rest() {
    let mut swing = Swing { velocity: IDENTITY, euler: [0.0, 0.0, 0.5] };
    for _ in 0..600 {
        let [x, y, z] = crate::puppet::pose::point(
            &crate::matrix::model_matrix([0.0; 3], [1.0; 3], swing.euler),
            SWING.tip,
        );
        swing.step(&SWING, 1.0 / 60.0, [x, y, z], [0.0; 3], None);
    }
    assert!(swing.euler[2].abs() < 1e-3, "{:?}", swing.euler);
    assert!(swing.finite());
}

#[test]
fn limits_clamp_swing() {
    let limited = Rotation { limits: Some([[0.0, 0.0, -0.05], [0.0, 0.0, 0.05]]), ..SWING };
    let mut swing = Swing::default();
    for _ in 0..30 {
        swing.step(&limited, 1.0 / 60.0, [-100.0, 0.0, 0.0], [0.0; 3], None);
        assert!(swing.euler[2].abs() <= 0.05, "{:?}", swing.euler);
    }
    assert!((swing.euler[2] - 0.05).abs() < 1e-6);
}

#[test]
fn wrap_past_pi() {
    let turn = std::f32::consts::TAU;
    assert!(close(&[wrap(3.5), wrap(-3.5), wrap(0.25)], &[3.5 - turn, turn - 3.5, 0.25]));
}

fn free_swing(limits: Option<[[f32; 3]; 2]>) -> Swing {
    let rotation = Rotation {
        stiffness: 120.0,
        friction: 8.0,
        inertia: 0.85,
        tip: [30.0, 80.0, 20.0],
        locked: [false; 3],
        limits,
        rigid: false,
    };
    let mut swing = Swing::default();
    for frame in 0..12_u8 {
        let phase = f32::from(frame);
        let previous = [60.0 * (phase * 0.3).cos(), 70.0, 40.0 * (phase * 0.5).sin()];
        swing.step(&rotation, 1.0 / 60.0, previous, [1.0, -2.0, 0.5], None);
    }
    swing
}

#[test]
fn free_swing_golden() {
    let swing = free_swing(None);
    assert!(close(&swing.euler, &[-1.178_572_9, 0.029_084_92, 1.511_797_7]), "{:?}", swing.euler);
    assert!(
        close(&swing.velocity, &[0.997_955_44, -0.010_784_549, -0.062_081_5, 0.010_692_583]),
        "{:?}",
        swing.velocity
    );
}

#[test]
fn limited_swing_golden() {
    let swing = free_swing(Some([[-0.1, -0.05, -0.2], [0.1, 0.05, 0.2]]));
    assert!(close(&swing.euler, &[-0.1, 0.048_437_12, 0.2]), "{:?}", swing.euler);
    assert!(
        close(&swing.velocity, &[0.999_950_6, 0.001_087_237, -0.009_731_959, 0.001_718_103_3]),
        "{:?}",
        swing.velocity
    );
}

#[test]
fn rigid_ignores_stiffness() {
    let rigid = Rotation { rigid: true, stiffness: 5000.0, ..SWING };
    let free = Rotation { rigid: true, stiffness: 0.0, ..SWING };
    let mut swings = [Swing::default(), Swing::default()];
    for _ in 0..4 {
        swings[0].step(&rigid, 1.0 / 60.0, [-100.0, 40.0, 0.0], [0.0; 3], None);
        swings[1].step(&free, 1.0 / 60.0, [-100.0, 40.0, 0.0], [0.0; 3], None);
    }
    assert_eq!(swings[0].euler, swings[1].euler);
    assert_ne!(swings[0].euler[2], 0.0);
}

#[test]
fn rigid_locks_after_limits() {
    let limits = Some([[-0.01, 0.0, 0.0], [0.01, 0.0, 0.0]]);
    let tip = [0.0, 0.0, 100.0];
    let free = Rotation { locked: [true, false, false], limits, tip, ..SWING };
    let rigid = Rotation { rigid: true, ..free };
    let mut swing = Swing::default();
    swing.step(&free, 1.0 / 60.0, [0.0, 100.0, 100.0], [0.0; 3], None);
    let mut locked = Swing::default();
    locked.step(&rigid, 1.0 / 60.0, [0.0, 100.0, 100.0], [0.0; 3], None);
    assert_eq!((swing.euler[0], locked.euler[0]), (0.0, 0.0));
    assert_ne!(swing.velocity, locked.velocity);
}

#[test]
fn gravity_bends_target() {
    let tip = [100.0, 0.0, 0.0];
    let rotation = Rotation { tip, locked: [false; 3], rigid: true, ..SWING };
    let mut swing = Swing::default();
    let pull = Some([0.0, -5.0, 0.0]);
    swing.step(&rotation, 1.0 / 60.0, tip, [0.0; 3], pull);
    assert!(swing.euler[2] < 0.0, "{:?}", swing.euler);
    let mut along = Swing::default();
    along.step(&rotation, 1.0 / 60.0, tip, [0.0; 3], Some([-5.0, 0.0, 0.0]));
    assert_eq!(along.euler, [0.0; 3]);
}
