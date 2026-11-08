use super::*;

fn sampler() -> Sampler {
    let mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    Sampler::new(Rc::new(mesh)).unwrap()
}

#[test]
fn same_time_state() {
    let mut sampler = sampler();
    let mut samples = [(
        AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: false },
        1.0 / 30.0,
    )];
    let first = sampler.sample(&samples, 0.0).unwrap();
    assert_eq!(first.matrices[0][12], 10.0);
    let revision = first.revision;
    assert_eq!(sampler.sample(&samples, 0.0).unwrap().revision, revision);
    samples[0].0.blend = 0.5;
    let changed = sampler.sample(&samples, 0.0).unwrap();
    assert_eq!(changed.matrices[0][12], 5.0);
    assert_ne!(changed.revision, revision);
}

#[test]
fn retained_sample_storage() {
    let mut sampler = sampler();
    let pointers = (
        sampler.scratch.matrices.world.as_ptr(),
        sampler.scratch.matrices.palette.as_ptr(),
        sampler.scratch.binds.as_ptr(),
        sampler.scratch.locals.as_ptr(),
        sampler.samples.as_ptr(),
    );
    for step in 0..1024 {
        let layer = AnimationLayer {
            visible: true,
            id: 1,
            rate: 1.0,
            blend: [0.0, 0.5, 1.0][step % 3],
            additive: false,
        };
        let samples = [(layer, step as f32 / 120.0)];
        sampler.sample(&samples, 0.0).unwrap();
        assert_eq!(
            pointers,
            (
                sampler.scratch.matrices.world.as_ptr(),
                sampler.scratch.matrices.palette.as_ptr(),
                sampler.scratch.binds.as_ptr(),
                sampler.scratch.locals.as_ptr(),
                sampler.samples.as_ptr(),
            )
        );
    }
}

#[test]
fn sampled_loop_palette() {
    let mut sampler = sampler();
    let layer = AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: false };
    for time in [1.0 / 30.0, 3.0 / 30.0, 5.0 / 30.0] {
        assert!(
            (sampler.sample(&[(layer, time)], 0.0).unwrap().matrices[0][12] - 10.0).abs() < 0.0001
        );
    }
    assert!(sampler.sample(&[(layer, f32::NAN)], 0.0).is_err());
    assert!(
        sampler.sample(&vec![(layer, 0.0); super::super::MAX_ANIMATION_LAYERS + 1], 0.0).is_err()
    );
}
#[test]
fn sampled_quaternion_blend() {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    for transform in &mut mesh.animations[0].tracks[0] {
        transform.rotation[2] = std::f32::consts::FRAC_PI_2;
    }
    let mut sampler = Sampler::new(Rc::new(mesh)).unwrap();
    let layer = AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 0.25, additive: false };
    let bind_rotation = sampler.mesh.bones[0].native_rest.rotation[0];
    let sampled = sampler.sample(&[(layer, 0.0)], 0.0).unwrap();
    let half = std::f64::consts::FRAC_PI_4;
    let w = 0.75 * f64::from(bind_rotation) + 0.25 * half.cos();
    let z = 0.25 * half.sin();
    let angle = 2.0 * z.atan2(w);
    assert!((f64::from(sampled.matrices[0][0]) - angle.cos()).abs() < 0.000_001);
    assert!((f64::from(sampled.matrices[0][1]) - angle.sin()).abs() < 0.000_001);
    assert!((f64::from(sampled.matrices[0][0]) - (half * 0.5).cos()).abs() > 0.001);
}
#[test]
#[ignore = "requires genuine Windmill native matrix fixture"]
fn windmill_bone_matrices() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_SKELETAL_PROBES")?);
    let mesh =
        crate::model::mesh::parse(&std::fs::read(root.join("windmill-source/windmill.mdl"))?)?;
    let rig = mesh.rig.expect("Windmill rig");
    let animation = rig.animations.iter().find(|animation| animation.id == 881).unwrap();
    let mut sampler = Sampler::new(Rc::clone(&rig))?;
    let frame: f32 =
        std::env::var("SKWD_SKELETAL_FRAME").map_or(Ok(50.0), |frame| frame.parse())?;
    let mut samples = Vec::new();
    for blend in [0.0, 0.25, 0.5, 1.0] {
        let layer =
            AnimationLayer { visible: true, id: animation.id, rate: 0.0, blend, additive: false };
        sampler.sample(&[(layer, frame / animation.fps)], 0.0)?;
        samples.push(serde_json::json!({
            "blend": blend,
            "frame": frame,
            "palette": sampler.scratch.matrices.palette,
            "world": sampler.scratch.matrices.world,
        }));
    }
    let bones: Vec<_> = rig
        .bones
        .iter()
        .map(|bone| {
            serde_json::json!({
                "name": bone.name,
                "parent": bone.parent,
                "bind": bone.raw_bind,
                "translation": bone.bind.translation,
                "rotation": bone.bind.rotation,
                "scale": bone.bind.scale,
            })
        })
        .collect();
    std::fs::write(
        root.join("windmill-bones-paper.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"bones":bones,"samples":samples,"bone_bytes":std::mem::size_of::<super::super::Bone>()}),
        )?,
    )?;
    Ok(())
}

#[test]
fn native_bind_blend() {
    use super::super::{Bone, Transform};
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    let raw = [
        100.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -0.000_016_292_068,
        -100.0,
        0.0,
        0.0,
        100.0,
        -0.000_016_292_068,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    mesh.bones[0] = Bone::from_matrix("Windmill".into(), None, raw, true).unwrap();
    mesh.native_bind_inverse = pose::bind_inverse(&mesh.bones);
    mesh.animations[0].tracks[0].fill(Transform {
        translation: [0.0; 3],
        rotation: [f32::from_bits(0xbfc9_0fdc), -0.0, 0.0],
        scale: [100.0; 3],
    });
    let mut sampler = Sampler::new(Rc::new(mesh)).unwrap();
    for (blend, expected) in
        [(0.25, -0.000_615_791_5_f32), (0.5, -0.000_410_393_87), (1.0, 0.000_000_222_525_3)]
    {
        let layer = AnimationLayer { visible: true, id: 1, rate: 0.0, blend, additive: false };
        let sampled = sampler.sample(&[(layer, 0.0)], 0.0).unwrap();
        assert!((sampled.matrices[0][6] - expected).abs() < 0.000_000_5, "blend {blend}");
    }
}

fn rotated_sampler() -> Sampler {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    let raw = crate::matrix::model_matrix([3.0, 4.0, 0.0], [1.0; 3], [0.0, 0.0, 0.25]);
    mesh.bones[0] = super::super::Bone::from_matrix("root".into(), None, raw, false).unwrap();
    mesh.bones[0].native_rest.rotation = [1.0, 0.0, 0.0, 1.0];
    mesh.native_bind_inverse = pose::bind_inverse(&mesh.bones);
    Sampler::new(Rc::new(mesh)).unwrap()
}

#[test]
fn spatial_zero_rest() {
    let mut sampler = rotated_sampler();
    Rc::get_mut(&mut sampler.mesh).unwrap().spatial = true;
    let rest = sampler.sample(&[], 0.0).unwrap().matrices.to_vec();
    let layer = AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 0.0, additive: false };
    assert_eq!(sampler.sample(&[(layer, 0.0)], 0.0).unwrap().matrices, rest);
}

#[test]
fn visible_zero_samples() {
    let mut sampler = rotated_sampler();
    let rest = sampler.sample(&[], 0.0).unwrap().matrices.to_vec();
    let layer = AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 0.0, additive: false };
    let nonadditive = sampler.sample(&[(layer, 0.0)], 0.0).unwrap().matrices.to_vec();
    #[cfg(target_arch = "x86_64")]
    assert_ne!(rest, nonadditive);
    #[cfg(not(target_arch = "x86_64"))]
    assert_eq!(rest, nonadditive);
    let layer = AnimationLayer { additive: true, ..layer };
    let one = sampler.sample(&[(layer, 0.0)], 0.0).unwrap().matrices.to_vec();
    #[cfg(target_arch = "x86_64")]
    assert_ne!(one, nonadditive);
    #[cfg(not(target_arch = "x86_64"))]
    assert_eq!(one, nonadditive);
    let previous = sampler.revision;
    let samples = [(layer, 0.0); 2];
    let two = sampler.sample(&samples, 0.0).unwrap().matrices.to_vec();
    assert_eq!(sampler.revision, previous.wrapping_add(1));
    assert!(one.iter().chain(&two).flatten().all(|value| value.is_finite()));
    let revision = sampler.revision;
    assert_eq!(sampler.sample(&samples, 0.0).unwrap().revision, revision);
    let mut world = Vec::new();
    let mut locals = Vec::new();
    super::super::sampled_world(&sampler.mesh, &samples, &mut world, Some(&mut locals)).unwrap();
    assert_eq!(world, sampler.scratch.matrices.world);
    assert_eq!(locals, world);
    assert_eq!(sampler.sample(&[], 0.0).unwrap().matrices, rest);
}

#[cfg(target_arch = "x86_64")]
const CROSS_ZERO_PALETTES: &[(&[bool], [u32; 12])] = &[
    (
        &[],
        [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0xb580_0000,
            0x35c0_0000,
            0x0000_0000,
        ],
    ),
    (
        &[false],
        [
            0x3f80_0000,
            0xb894_f800,
            0x0000_0000,
            0x3894_f800,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0xbb44_5000,
            0xb580_0000,
            0x0000_0000,
        ],
    ),
    (
        &[true],
        [
            0x3f7f_fed5,
            0xb994_4400,
            0x0000_0000,
            0x3994_4400,
            0x3f7f_fed5,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0xbc43_6000,
            0x3a42_f800,
            0x0000_0000,
        ],
    ),
    (
        &[true, true],
        [
            0x3f7f_fed5,
            0xb994_4800,
            0x0000_0000,
            0x3994_4800,
            0x3f7f_fed5,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0xbc43_6400,
            0x3a43_0800,
            0x0000_0000,
        ],
    ),
    (
        &[true, true, true],
        [
            0x3f7f_fed5,
            0xb994_4a00,
            0x0000_0000,
            0x3994_4a00,
            0x3f7f_fed5,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0xbc43_6800,
            0x3a43_1000,
            0x0000_0000,
        ],
    ),
    (
        &[false, true],
        [
            0x3f80_0000,
            0xb895_0800,
            0x0000_0000,
            0x3895_0800,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0xbb44_6800,
            0xb540_0000,
            0x0000_0000,
        ],
    ),
];

#[test]
#[cfg(target_arch = "x86_64")]
#[ignore = "requires genuine cross native matrix fixture"]
fn cross_zero_blend_palette() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_CROSS_NATIVE")?);
    let mesh = super::super::parse(&std::fs::read(root.join("models/cross_puppet.mdl"))?)?;
    let mut sampler = Sampler::new(Rc::new(mesh))?;
    for (additive, expected) in CROSS_ZERO_PALETTES {
        let samples: Vec<_> = additive
            .iter()
            .map(|additive| {
                (
                    AnimationLayer {
                        visible: true,
                        id: 441,
                        rate: 0.0,
                        blend: 0.0,
                        additive: *additive,
                    },
                    0.0,
                )
            })
            .collect();
        let sampled = sampler.sample(&samples, 0.0)?;
        let actual: Vec<_> = sampled.matrices[0]
            .chunks_exact(4)
            .flat_map(|column| column[..3].iter().map(|value| value.to_bits()))
            .collect();
        assert_eq!(actual.as_slice(), expected.as_slice(), "layers {additive:?}");
    }
    Ok(())
}

#[test]
fn zero_blend_unqualified_tracks() {
    for missing in [false, true] {
        let mut sampler = rotated_sampler();
        let rest = sampler.sample(&[], 0.0).unwrap().matrices.to_vec();
        let mesh = Rc::get_mut(&mut sampler.mesh).unwrap();
        if missing {
            mesh.animations[0].tracks.clear();
        } else {
            mesh.animations[0].disabled = Some(vec![true]);
        }
        let layer = AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 0.0, additive: false };
        assert_eq!(sampler.sample(&[(layer, 0.0)], 0.0).unwrap().matrices, rest);
    }
}

#[test]
fn hidden_authored_samples() {
    let mut sampler = rotated_sampler();
    let rest = sampler.sample(&[], 0.0).unwrap().matrices.to_vec();
    let layer = AnimationLayer { id: 1, rate: 0.0, blend: 1.0, additive: false, visible: false };
    assert_eq!(sampler.sample(&[(layer, 0.0)], 0.0).unwrap().matrices, rest);
    let shown = AnimationLayer { visible: true, ..layer };
    let active = sampler.sample(&[(shown, 0.0)], 0.0).unwrap().matrices.to_vec();
    assert_eq!(sampler.sample(&[(layer, 0.0), (shown, 0.0)], 0.0).unwrap().matrices, active);
    assert_eq!(sampler.sample(&[(shown, 0.0), (layer, 0.0)], 0.0).unwrap().matrices, active);
    let playback =
        crate::puppet::playback::Playback::authored("rig", &sampler.mesh, &[layer]).unwrap();
    assert_eq!(playback.states("rig")[0].sample, crate::puppet::playback::Sample::Rest);
    assert!(!playback.states("rig")[0].layer.visible);
}

fn rested_sampler() -> Sampler {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    let raw = crate::matrix::model_matrix([3.0, 4.0, 0.0], [1.0; 3], [0.0; 3]);
    mesh.bones[0] = super::super::Bone::from_matrix("root".into(), None, raw, false).unwrap();
    mesh.native_bind_inverse = pose::bind_inverse(&mesh.bones);
    let rest = crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]);
    mesh.bones[0].set_rest(rest, false).unwrap();
    Sampler::new(Rc::new(mesh)).unwrap()
}

fn assert_close(expected: &[Mat4], actual: &[Mat4]) {
    for (expected, actual) in expected.iter().flatten().zip(actual.iter().flatten()) {
        assert!((expected - actual).abs() < 1e-4, "{expected} != {actual}");
    }
}

#[test]
fn rest_pose_layers() {
    let mut sampler = rested_sampler();
    let base = AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: false };
    let additive = AnimationLayer { additive: true, ..base };
    let first = sampler.sample(&[(base, 0.0)], 0.0).unwrap().matrices.to_vec();
    let rest = sampler.sample(&[], 0.0).unwrap().matrices.to_vec();
    assert_close(&first, &rest);
    let posed = sampler.sample(&[(base, 1.0 / 30.0)], 0.0).unwrap().matrices.to_vec();
    let both =
        sampler.sample(&[(base, 1.0 / 30.0), (additive, 0.0)], 0.0).unwrap().matrices.to_vec();
    assert_close(&posed, &both);
    let sole = sampler.sample(&[(additive, 1.0 / 30.0)], 0.0).unwrap().matrices.to_vec();
    assert_close(&posed, &sole);
}
