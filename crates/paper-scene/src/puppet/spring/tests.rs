use super::{State, Translation};
use crate::matrix::{mat4_inverse, mat4_mul, model_matrix};

pub(super) fn close(left: &[f32], right: &[f32]) -> bool {
    left.iter().zip(right).all(|(left, right)| (left - right).abs() < 1e-4)
}

#[test]
fn release_order() {
    let parameters = Translation { stiffness: 8.0, friction: 2.0, inertia: 0.5, maximum: 0.0 };
    let mut state = State::default();
    let first = state.step(parameters, 0.125, [0.0; 3], [8.0, 0.0, 0.0], 1.0);
    assert_eq!(first, [3.5, 0.0, 0.0]);
    assert_eq!(state.velocity, [-3.0, 0.0, 0.0]);
    let second = state.step(parameters, 0.125, [0.0; 3], first, 1.0);
    assert_eq!(second, [2.6875, 0.0, 0.0]);
    assert_eq!(state.velocity, [-4.875, 0.0, 0.0]);
}

#[test]
fn displacement_limit() {
    let parameters = Translation { stiffness: 8.0, friction: 2.0, inertia: 0.5, maximum: 2.0 };
    let mut state = State::default();
    assert_eq!(state.step(parameters, 0.125, [0.0; 3], [8.0, 0.0, 0.0], 1.0), [2.0, 0.0, 0.0]);
    assert_eq!(state.velocity, [-3.0, 0.0, 0.0]);
}

#[test]
fn saturated_damping() {
    let parameters = Translation { stiffness: 8.0, friction: 16.0, inertia: 0.5, maximum: 0.0 };
    let mut state = State::default();
    assert_eq!(state.step(parameters, 0.125, [0.0; 3], [8.0, 0.0, 0.0], 1.0), [3.5, 0.0, 0.0]);
    assert_eq!(state.velocity, [0.0; 3]);
}

#[test]
fn reflected_scale() {
    let matrix = crate::matrix::model_matrix([20.0, 30.0, 40.0], [-2.0, 3.0, 1.0], [0.0; 3]);
    assert_eq!(super::owner_scale(&matrix), 2.0);
    let mut shear = matrix;
    shear[4] = 4.0;
    assert_eq!(super::owner_scale(&shear), 8.0 / 3.0);
}

#[test]
fn resting_hierarchy_storage() {
    let mesh = crate::puppet::tests::hierarchy_fixture();
    let definitions = [super::Definition {
        bone: 0,
        translation: Some(Translation {
            stiffness: 8.0,
            friction: 2.0,
            inertia: 0.5,
            maximum: 2.0,
        }),
        rotation: None,
        gravity: None,
    }];
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let storage = (spring.particles.as_ptr(), spring.corrections.as_ptr());
    let owner = crate::matrix::model_matrix([100.0, 200.0, 7.0], [2.0, 3.0, 1.0], [0.0; 3]);
    let mut world = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut world, None).unwrap();
    let initial = world.clone();
    for _ in 0..100 {
        world.copy_from_slice(&initial);
        spring.step(
            &definitions,
            &mesh.bones,
            &mut world,
            &super::Frame { dt: 0.125, owner: &owner, scale: super::owner_scale(&owner) },
        );
        assert_eq!(world, initial);
        assert_eq!((spring.particles.as_ptr(), spring.corrections.as_ptr()), storage);
    }
}

#[test]
fn damping_overflow_rejected() {
    let mesh = crate::puppet::tests::hierarchy_fixture();
    let definitions = [super::Definition {
        bone: 0,
        translation: Some(Translation {
            stiffness: 8.0,
            friction: -f32::MAX,
            inertia: 0.5,
            maximum: 2.0,
        }),
        rotation: None,
        gravity: None,
    }];
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    spring.particles[0].basis =
        Some(crate::matrix::model_matrix([80.0, 0.0, 0.0], [1.0; 3], [0.0; 3]));
    let owner = crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]);
    let mut world = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut world, None).unwrap();
    assert!(!spring.step(
        &definitions,
        &mesh.bones,
        &mut world,
        &super::Frame { dt: 0.125, owner: &owner, scale: 1.0 }
    ));
    assert!(spring.particles[0].state.offset.iter().all(|value| value.is_finite()));
    assert!(spring.particles[0].state.velocity.iter().any(|value| !value.is_finite()));
}

#[test]
fn animated_spring_rates() {
    use crate::puppet::{AnimationLayer, layer_samples, sampled_world, skin_samples};
    let mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
    let definitions = [super::Definition {
        bone: 0,
        translation: Some(Translation {
            stiffness: 0.0,
            friction: 0.0,
            inertia: 0.5,
            maximum: 0.0,
        }),
        rotation: None,
        gravity: None,
    }];
    let owner = crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]);
    for (rate, expected) in [(0.0, 0.0), (0.5, 2.5), (2.0, 10.0)] {
        let layers = [AnimationLayer { visible: true, id: 1, rate, blend: 1.0, additive: true }];
        let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
        let mut world = Vec::new();
        for time in [0.0, 1.0 / 60.0] {
            let samples: Vec<_> = layer_samples(&layers, time).collect();
            let mut pose = skin_samples(&mesh, &samples);
            sampled_world(&mesh, &samples, &mut world, None).unwrap();
            assert!(spring.step(
                &definitions,
                &mesh.bones,
                &mut world,
                &super::Frame { dt: 1.0 / 60.0, owner: &owner, scale: 1.0 }
            ));
            assert!(spring.skin(&mesh, &mut pose));
            if time > 0.0 {
                assert!((world[0][12] - expected * 0.5).abs() < 0.001);
                assert!((pose.positions[0][0] - (1.0 + expected * 0.5)).abs() < 0.001);
            }
        }
    }
}

#[test]
fn changed_owner_scope() {
    let mesh = crate::puppet::tests::hierarchy_fixture();
    let definitions = [super::Definition {
        bone: 0,
        translation: Some(Translation {
            stiffness: 8.0,
            friction: 2.0,
            inertia: 0.5,
            maximum: 2.0,
        }),
        rotation: None,
        gravity: None,
    }];
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let mut world = Vec::new();
    for (angles, supported) in [
        ([0.0; 3], true),
        ([0.25, 0.0, 0.0], false),
        ([0.0, 0.25, 0.0], false),
        ([0.0, 0.0, 0.25], true),
    ] {
        let owner = crate::matrix::model_matrix([100.0, 200.0, 7.0], [2.0, 3.0, 1.0], angles);
        crate::puppet::sampled_world(&mesh, &[], &mut world, None).unwrap();
        assert_eq!(super::owner_supported(&owner), supported);
        assert!(spring.step(
            &definitions,
            &mesh.bones,
            &mut world,
            &super::Frame { dt: 0.125, owner: &owner, scale: super::owner_scale(&owner) }
        ));
    }
    let mut projective = crate::matrix::IDENTITY;
    projective[3] = 0.5;
    let before = world.clone();
    assert!(!spring.step(
        &definitions,
        &mesh.bones,
        &mut world,
        &super::Frame { dt: 0.125, owner: &projective, scale: 1.0 }
    ));
    assert_eq!(world, before);
}

#[test]
fn invalid_owner_basis() {
    let singular = crate::matrix::model_matrix([0.0; 3], [0.0, 1.0, 1.0], [0.0; 3]);
    assert!(!super::owner_supported(&singular));
    let mut invalid = crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]);
    invalid[12] = f32::INFINITY;
    assert!(!super::owner_supported(&invalid));
}

#[test]
fn engine_switch_rules() {
    let translation = serde_json::json!({"se":true,"t":true,"ts":200,"tf":10,"ti":30,"tm":200});
    let with = |extra: serde_json::Value| {
        let mut metadata = translation.clone();
        metadata.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        super::constraint(&metadata.to_string())
    };
    for inert in [
        r#"{"r":true}"#,
        r#"{"se":true}"#,
        r#"{"ge":true}"#,
        r#"{"ge":true,"gd":"0 -1 0","m":20}"#,
        r#"{"se":true,"ge":true,"gd":"0 -1 0","m":20}"#,
        r#"{"se":false,"t":true,"r":false,"re":false}"#,
        r#"{"re":true,"ge":true,"la":true,"lt":true}"#,
        r#"{"r":true,"ikg":true,"ikr":true,"ikse":true,"raz":true,"tax":true}"#,
    ] {
        assert!(matches!(super::constraint(inert), super::Constraint::Inactive), "{inert}");
    }
    for unsupported in [r#"{"ik":true}"#, r#"{"ikce":true}"#, r#"{"se":true,"t":true}"#] {
        assert!(matches!(super::constraint(unsupported), super::Constraint::Unsupported));
    }
    for extra in [
        serde_json::json!({"future":true,"la":true,"lt":true,"ikg":true}),
        serde_json::json!({"tax":false,"tay":null,"taz":true}),
        serde_json::json!({"tax":false,"tay":true}),
    ] {
        assert!(
            matches!(
                with(extra.clone()),
                super::Constraint::Spring { translation: Some(_), rotation: None, gravity: None }
            ),
            "{extra}"
        );
    }
    for extra in [
        serde_json::json!({"r":true}),
        serde_json::json!({"ge":true}),
        serde_json::json!({"ge":true,"gd":"1 2"}),
        serde_json::json!({"ik":true,"ikse":true}),
        serde_json::json!({"ikce":true}),
        serde_json::json!({"tax":false,"tay":true,"taz":true}),
    ] {
        assert!(matches!(with(extra.clone()), super::Constraint::Unsupported), "{extra}");
    }
    for missing in ["ts", "tf", "ti", "tm"] {
        let mut metadata = translation.clone();
        metadata.as_object_mut().unwrap().remove(missing);
        assert!(matches!(super::constraint(&metadata.to_string()), super::Constraint::Unsupported));
    }
}

fn swing_metadata(extra: &serde_json::Value) -> super::Constraint {
    let mut metadata = serde_json::json!({
        "se":true,"r":true,"rs":50,"rf":40,"ri":9,"tp":"0 1000 0",
        "rax":false,"ray":false,"raz":true,"lamin":"0 0 -1","lamax":"0 0 1"
    });
    metadata.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    super::constraint(&metadata.to_string())
}

#[test]
fn rotation_spring_parse() {
    let rotation = |extra| match swing_metadata(&extra) {
        super::Constraint::Spring { translation: None, rotation: Some(rotation), .. } => rotation,
        _ => panic!("{extra}"),
    };
    assert_eq!(
        rotation(serde_json::json!({})),
        super::Rotation {
            stiffness: 50.0,
            friction: 40.0,
            inertia: 1.0 - 9.0 / 100.0,
            tip: [0.0, 1000.0, 0.0],
            locked: [true, true, false],
            limits: None,
            rigid: false,
        }
    );
    assert_eq!(
        rotation(serde_json::json!({"la":true})).limits,
        Some([[0.0, 0.0, -1.0], [0.0, 0.0, 1.0]])
    );
    assert_eq!(rotation(serde_json::json!({"ray":null})).locked, [false; 3]);
    let both = serde_json::json!({"t":true,"ts":200,"tf":10,"ti":30,"tm":200});
    assert!(matches!(
        swing_metadata(&both),
        super::Constraint::Spring { translation: Some(_), rotation: Some(_), .. }
    ));
}

#[test]
fn rotation_spring_rejects() {
    for extra in [
        serde_json::json!({"lt":true}),
        serde_json::json!({"la":true,"lamin":null}),
        serde_json::json!({"tp":"0 1000"}),
        serde_json::json!({"rs":null}),
    ] {
        assert!(matches!(swing_metadata(&extra), super::Constraint::Unsupported), "{extra}");
    }
}

fn swing_rig(translation: Option<Translation>) -> (crate::puppet::Mesh, [super::Definition; 1]) {
    let definition = super::Definition {
        bone: 0,
        translation,
        rotation: Some(super::Rotation {
            stiffness: 50.0,
            friction: 10.0,
            inertia: 0.9,
            tip: [10.0, 0.0, 0.0],
            locked: [true, true, false],
            limits: None,
            rigid: false,
        }),
        gravity: None,
    };
    (crate::puppet::tests::hierarchy_fixture(), [definition])
}

fn lifted(rest: &[crate::matrix::Mat4]) -> Vec<crate::matrix::Mat4> {
    let lift = model_matrix([0.0, 30.0, 0.0], [1.0; 3], [0.0; 3]);
    rest.iter()
        .enumerate()
        .map(|(index, matrix)| if index < 2 { mat4_mul(&lift, matrix) } else { *matrix })
        .collect()
}

const FRAME: super::Frame<'static> =
    super::Frame { dt: 1.0 / 60.0, owner: &crate::matrix::IDENTITY, scale: 1.0 };

#[test]
fn swinging_hierarchy() {
    let (mesh, definitions) = swing_rig(None);
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let mut rest = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut rest, None).unwrap();
    let mut world = rest.clone();
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    assert_eq!(world, rest);
    let lifted = lifted(&rest);
    world.clone_from(&lifted);
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    assert!(world[0][1] < 0.0);
    assert!(close(&world[0][12..15], &lifted[0][12..15]));
    let local = mat4_mul(&mat4_inverse(&lifted[0]).unwrap(), &lifted[1]);
    assert!(close(&world[1], &mat4_mul(&world[0], &local)));
    assert_eq!(world[2], lifted[2]);
    let mut palette: Vec<_> = lifted
        .iter()
        .zip(&mesh.native_bind_inverse)
        .map(|(pose, bind)| {
            Some(crate::puppet::BonePose::Spatial(mat4_mul(pose, bind.as_ref().unwrap())))
        })
        .collect();
    assert!(spring.palette(&mut palette));
    for ((pose, simulated), bind) in palette.iter().zip(&world).zip(&mesh.native_bind_inverse) {
        let expected = mat4_mul(simulated, bind.as_ref().unwrap());
        assert!(close(&pose.unwrap().matrix(), &expected));
    }
}

#[test]
fn combined_bone_motion() {
    let translation = Translation { stiffness: 8.0, friction: 2.0, inertia: 0.5, maximum: 0.0 };
    let (mesh, definitions) = swing_rig(Some(translation));
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let mut rest = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut rest, None).unwrap();
    let mut world = rest.clone();
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    let lifted = lifted(&rest);
    world.clone_from(&lifted);
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    assert!(world[0][1] < 0.0);
    assert!((0.0..lifted[0][13]).contains(&world[0][13]), "{:?}", &world[0][12..15]);
    let local = mat4_mul(&mat4_inverse(&lifted[0]).unwrap(), &lifted[1]);
    assert!(close(&world[1], &mat4_mul(&world[0], &local)));
}

#[test]
fn recorded_teleport_rests() {
    let (mesh, definitions) = swing_rig(None);
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let mut rest = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut rest, None).unwrap();
    let mut world = rest.clone();
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    let lifted = lifted(&rest);
    spring.record(&definitions, 0, &lifted[0]);
    world.clone_from(&lifted);
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    assert_eq!(world, lifted);
}

#[test]
fn singular_bone_passes() {
    let (mesh, definitions) = swing_rig(None);
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let mut rest = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut rest, None).unwrap();
    let mut world = rest.clone();
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
    let mut hidden = rest.clone();
    hidden[0] = mat4_mul(&hidden[0], &model_matrix([0.0; 3], [1.0, 0.0, 1.0], [0.0; 3]));
    for _ in 0..2 {
        let mut world = hidden.clone();
        assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
        assert_eq!(world[0], hidden[0]);
    }
    let shown = lifted(&rest);
    for _ in 0..2 {
        let mut world = shown.clone();
        assert!(spring.step(&definitions, &mesh.bones, &mut world, &FRAME));
        assert_eq!(world, shown);
    }
}

#[test]
fn correction_reaches_matrices() {
    let mut simulation = super::Simulation::new(2, &[]);
    simulation.corrections[1] =
        Some(crate::matrix::model_matrix([3.0, 0.0, 0.0], [1.0; 3], [0.0; 3]));
    let mut matrices = [crate::matrix::IDENTITY; 2];
    assert!(simulation.correct(&mut matrices));
    assert_eq!(matrices[0], crate::matrix::IDENTITY);
    assert_eq!(matrices[1][12], 3.0);
    simulation.corrections[0] = Some([f32::NAN; 16]);
    assert!(!simulation.correct(&mut matrices));
}

#[test]
fn attachments_follow_correction() {
    const SHIFT: f32 = 5.0;
    let mut mesh = crate::puppet::tests::hierarchy_fixture();
    let identity =
        crate::puppet::Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] };
    mesh.attachments.push(crate::puppet::Attachment {
        name: "probe".into(),
        bone: 0,
        transform: identity,
        matrix: crate::matrix::IDENTITY,
    });
    let mut pose = crate::puppet::skin(&mesh, &[], 0.0);
    pose.attachments.resize(1, identity.into());
    let mut world = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut world, None).unwrap();
    let before = world[0][12];
    world[0][12] += SHIFT;
    crate::puppet::refresh_attachments(&mesh, &world, &mut pose);
    assert!((pose.attachments[0].affine.x - before - SHIFT).abs() < 0.001);
}

#[test]
fn gravity_rigid_metadata() {
    let parse = |extra: &str| {
        let base = r#""t":true,"ts":300,"tf":20,"ti":30,"tm":200,"#;
        super::constraint(&format!("{{{base}{extra}}}"))
    };
    let super::Constraint::Spring { translation: Some(translation), gravity, .. } =
        parse(r#""se":true,"ge":true,"gd":"0 -1 0","m":20"#)
    else {
        panic!("gravity spring");
    };
    assert_eq!(translation.stiffness, 300.0);
    assert_eq!(gravity, Some(super::Gravity { direction: [0.0, -1.0, 0.0], mass: 20.0 }));
    let super::Constraint::Spring { translation: Some(translation), gravity: None, .. } =
        parse(r#""re":true"#)
    else {
        panic!("rigid translation");
    };
    assert_eq!(translation.stiffness, 0.0);
}

#[test]
fn gravity_accelerates_offset() {
    let parameters = Translation { stiffness: 0.0, friction: 0.0, inertia: 0.0, maximum: 0.0 };
    let mut state = State::default();
    let gravity = super::Gravity { direction: [0.0, -1.0, 0.0], mass: 2.0 };
    state.fall(gravity, 0.5);
    assert_eq!(state.velocity, [0.0, -1000.0, 0.0]);
    let moved = state.step(parameters, 0.5, [0.0; 3], [0.0; 3], 1.0);
    assert_eq!(moved, [0.0, -500.0, 0.0]);
}

#[test]
fn gravity_owner_scale() {
    let owner = model_matrix([0.0; 3], [2.0; 3], [0.0; 3]);
    let (mesh, [mut definition]) =
        swing_rig(Some(Translation { stiffness: 0.0, friction: 0.0, inertia: 0.5, maximum: 0.0 }));
    let rotation = definition.rotation.as_mut().unwrap();
    rotation.stiffness = 0.0;
    rotation.friction = 0.0;
    rotation.locked = [false; 3];
    rotation.rigid = true;
    definition.gravity = Some(super::Gravity { direction: [0.0, -1.0, 0.0], mass: 3.0 });
    let definitions = [definition];
    let frame = super::Frame { dt: 1.0 / 60.0, owner: &owner, scale: 2.0 };
    let mut spring = super::Simulation::new(mesh.bones.len(), &definitions);
    let mut rest = Vec::new();
    crate::puppet::sampled_world(&mesh, &[], &mut rest, None).unwrap();
    let mut world = rest.clone();
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &frame));
    let mut world = rest.clone();
    assert!(spring.step(&definitions, &mesh.bones, &mut world, &frame));
    assert!(close(&[world[0][1], world[0][13]], &[-0.013_734_1, -0.416_666_7]), "{:?}", world[0]);
}
