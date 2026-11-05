use super::*;

fn channel() -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(12.5_f32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(12_u32.to_le_bytes());
    bytes.extend([0.0_f32, 1.0, 0.5].into_iter().flat_map(f32::to_le_bytes));
    bytes
}

#[test]
fn morph_channel_samples() {
    let bytes = channel();
    let mut at = 0;
    let clip = clip(&bytes, &mut at, 2, 1).unwrap().unwrap();
    assert_eq!(at, bytes.len());
    assert_eq!(clip.scale, 12.5);
    assert_eq!(clip.tracks[0].shape, 2);
    assert_eq!(clip.tracks[0].samples, [0.0, 1.0, 0.5]);
}

#[test]
fn rejects_truncated_channels() {
    let bytes = channel();
    for end in 0..bytes.len() {
        assert!(clip(&bytes[..end], &mut 0, 2, 1).is_err());
    }
    assert!(clip(&bytes, &mut 0, 1, 1).is_err());
}

#[test]
#[ignore = "requires authored morph fixtures"]
fn authored_morph_assets() {
    let root = std::env::var("SKWD_MORPH_FIXTURE_DIR").unwrap();
    for (name, vertices, samples, shape) in
        [("1_puppet.mdl", 81, 13, 408), ("2_puppet.mdl", 37, 18, 318)]
    {
        let bytes = std::fs::read(std::path::Path::new(&root).join(name)).unwrap();
        let mesh = crate::puppet::parse(&bytes).unwrap();
        assert_eq!(mesh.vertices.len(), vertices);
        let morph = mesh.morph.unwrap();
        assert_eq!(morph.shapes[0].id, shape);
        assert_eq!(morph.shapes[0].deltas.len(), samples);
        assert_eq!(morph.shapes[0].alpha, vec![i16::MAX; samples]);
        assert!(morph.indices.iter().any(|index| *index > 0));
        assert_eq!(mesh.animations[0].morph.as_ref().unwrap().tracks[0].samples.len(), 451);
    }
}

#[test]
fn signed_morph_texture() {
    let morph = Morph {
        scale: 1.0,
        samples: 1,
        indices: vec![1],
        shapes: vec![Shape {
            id: 1,
            name: "shape".into(),
            deltas: vec![[i16::MIN, 0, i16::MAX]],
            alpha: vec![i16::MAX / 2],
            modifier: None,
        }],
    };
    let texture = morph.texture().unwrap();
    assert_eq!(texture.pixels.format, crate::tex::PixelFormat::Rgba16Snorm);
    let data = &texture.pixels.levels[0].data;
    assert_eq!(&data[..8], &[0, 0, 0, 0, 0, 0, 255, 127]);
    assert_eq!(&data[8..16], &[0, 128, 0, 0, 255, 127, 255, 63]);
}

#[test]
fn modifier_follows_posed_bone() {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    let bind = crate::matrix::model_matrix([3.0, 0.0, 0.0], [1.0; 3], [0.0; 3]);
    mesh.native_bind_inverse[0] = crate::matrix::mat4_inverse(&bind);
    mesh.morph = Some(Morph {
        scale: 1.0,
        samples: 1,
        indices: vec![1],
        shapes: vec![Shape {
            id: 1,
            name: "shape".into(),
            deltas: vec![[1, 0, 0]],
            alpha: Vec::new(),
            modifier: Some(Modifier { bone: 0, mode: AXIS_RULE, range: [-2.0, 4.0] }),
        }],
    });
    let mut uniforms = uniforms(&mesh, &[], Some(&[1.0])).unwrap().unwrap();
    let world = crate::matrix::model_matrix(
        [8.0, 0.0, 0.0],
        [1.0; 3],
        [0.0, 0.0, std::f32::consts::FRAC_PI_2],
    );
    let skin = crate::matrix::mat4_mul(&world, &mesh.native_bind_inverse[0].unwrap());
    uniforms.pose(&mesh, &[Some(super::super::BonePose::Spatial(skin))]).unwrap();
    let modifier = uniforms.modifiers.unwrap();
    assert_eq!(modifier.rules[0], [1.0, -2.0, 4.0]);
    let local = super::super::pose::point(&modifier.transforms[0], [8.0, 2.0, 0.0]);
    assert!(
        local
            .iter()
            .zip([2.0, 0.0, 0.0])
            .all(|(actual, expected)| (actual - expected).abs() < 0.00001)
    );
}

#[test]
fn hidden_morph_sample() {
    let mut mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
    mesh.morph = Some(Morph {
        scale: 1.0,
        samples: 0,
        indices: Vec::new(),
        shapes: vec![Shape {
            id: 2,
            name: "shape".into(),
            deltas: Vec::new(),
            alpha: Vec::new(),
            modifier: None,
        }],
    });
    let mut sampled = clip(&channel(), &mut 0, 2, 1).unwrap().unwrap();
    sampled.scale = 1.0;
    sampled.tracks[0].shape = 0;
    mesh.animations[0].morph = Some(sampled);
    let layer = crate::puppet::AnimationLayer {
        id: mesh.animations[0].id,
        rate: 1.0,
        blend: 1.0,
        additive: false,
        visible: true,
    };
    let time = 1.0 / mesh.animations[0].fps;
    assert_eq!(weights(&mesh, &[(layer, time)]).unwrap().unwrap(), [1.0]);
    let hidden = crate::puppet::AnimationLayer { visible: false, ..layer };
    assert_eq!(weights(&mesh, &[(hidden, time)]).unwrap().unwrap(), [0.0]);
}

#[test]
fn morph_moves_vertices() {
    let mut mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
    mesh.morph = Some(Morph {
        scale: 2.0,
        samples: 1,
        indices: vec![1, 0, 1],
        shapes: vec![Shape {
            id: 2,
            name: "shape".into(),
            deltas: vec![[i16::MAX, 0, 0]],
            alpha: vec![i16::MAX / 2],
            modifier: None,
        }],
    });
    let mut sampled = clip(&channel(), &mut 0, 2, 1).unwrap().unwrap();
    sampled.scale = 2.0;
    sampled.tracks[0].shape = 0;
    mesh.animations[0].morph = Some(sampled);
    let layer = crate::puppet::AnimationLayer {
        id: mesh.animations[0].id,
        rate: 1.0,
        blend: 1.0,
        additive: false,
        visible: true,
    };
    assert!(targets_present(&mesh));
    mesh.animations[0].morph.as_mut().unwrap().tracks[0].shape = 1;
    assert!(!targets_present(&mesh));
    mesh.animations[0].morph.as_mut().unwrap().tracks[0].shape = 0;
    let samples = [(layer, 1.0 / mesh.animations[0].fps)];
    let mut frame = crate::puppet::skin_samples(&mesh, &samples);
    let rest = frame.positions.clone();
    let mut alphas = Vec::new();
    apply(&mesh, &mut frame, &samples, None, &mut alphas).unwrap();
    assert_ne!(frame.positions[0], rest[0]);
    assert_eq!(frame.positions[1], rest[1]);
    assert!((alphas[0] - 0.5).abs() < 0.001, "{alphas:?}");
    assert_eq!(alphas[1], 1.0);
    let mut edited = crate::puppet::skin_samples(&mesh, &samples);
    alphas.clear();
    apply(&mesh, &mut edited, &samples, Some(&[0.0]), &mut alphas).unwrap();
    assert_eq!(edited.positions, rest);
    assert!(alphas.is_empty());
}
