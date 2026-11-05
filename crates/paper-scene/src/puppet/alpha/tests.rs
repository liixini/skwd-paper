use super::*;

#[test]
fn alpha_blending() {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    mesh.alpha = true;
    mesh.animations[0].alpha.tracks = vec![Some(vec![0.75, 0.25, 0.75])];
    let first = AnimationLayer {
        visible: true,
        id: mesh.animations[0].id,
        rate: 1.0,
        blend: 1.0,
        additive: false,
    };
    let second = AnimationLayer { blend: 0.5, additive: true, ..first };
    let time = 1.0 / mesh.animations[0].fps;
    assert_eq!(weights(&mesh, &[]), [1.0]);
    assert_eq!(weights(&mesh, &[(first, 0.0), (second, time)]), [0.375]);
    assert_eq!(
        weights(&mesh, &[(first, 0.0), (AnimationLayer { blend: 1.0, ..second }, time)]),
        [0.25]
    );
    assert_eq!(
        weights(&mesh, &[(first, 0.0), (AnimationLayer { additive: false, ..second }, time)]),
        [0.5]
    );
    let hidden = AnimationLayer { visible: false, ..first };
    assert_eq!(weights(&mesh, &[(hidden, time)]), [1.0]);
    mesh.alpha = false;
    assert_eq!(weights(&mesh, &[(first, time)]), [1.0]);
}

#[test]
fn alpha_track_bounds() {
    let mut bytes = vec![1];
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend([0.25_f32, 0.5].into_iter().flat_map(f32::to_le_bytes));
    assert_eq!(clip(&bytes, &mut 0, 1, &[0]).unwrap().tracks, [Some(vec![0.25, 0.5])]);
    assert_eq!(clip(&bytes, &mut 0, 1, &[1]).unwrap().tracks, [None]);
    assert_eq!(clip(&bytes, &mut 0, 1, &[3]).unwrap().tracks, [Some(vec![0.25, 0.5])]);
    assert!(clip(&bytes, &mut 0, 2, &[0]).is_err());
    assert!(clip(&bytes[..bytes.len() - 1], &mut 0, 1, &[0]).is_err());
    bytes[9..13].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(clip(&bytes, &mut 0, 1, &[0]).is_err());
}

#[test]
#[ignore = "requires authored bone alpha fixture"]
fn authored_bone_alpha() {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_BONE_ALPHA_DIR").unwrap());
    let mesh = super::super::parse(&std::fs::read(root.join("circle.mdl")).unwrap()).unwrap();
    assert!(mesh.alpha);
    assert_eq!(mesh.bones.len(), 1);
    let layer = AnimationLayer { visible: true, id: 2480, rate: 1.0, blend: 1.0, additive: false };
    assert_eq!(weights(&mesh, &[(layer, 0.0)]), [1.0]);
    assert_eq!(weights(&mesh, &[(layer, 0.5)]), [0.721_917_3]);
    assert_eq!(weights(&mesh, &[(layer, 1.0)]), [0.0]);
    let add = AnimationLayer { id: 8853, blend: 0.5, additive: true, ..layer };
    assert!((weights(&mesh, &[(layer, 0.5), (add, 0.5)])[0] - 0.402_487_78).abs() < 0.000_001);
}

#[test]
fn vertex_alpha_weighted() {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    let layer = AnimationLayer {
        visible: true,
        id: mesh.animations[0].id,
        rate: 1.0,
        blend: 1.0,
        additive: false,
    };
    let mut alphas = vec![0.5];
    vertex_alphas(&mesh, &[(layer, 0.0)], &mut alphas);
    assert!(alphas.is_empty());
    mesh.alpha = true;
    mesh.animations[0].alpha.tracks = vec![Some(vec![0.75, 0.25, 0.75])];
    mesh.vertices[0].bones = [u16::MAX, 0, 0, 0];
    mesh.vertices[0].weights = [1.0, 0.0, 0.0, 0.0];
    mesh.vertices[1].bones = [0; 4];
    mesh.vertices[1].weights = [0.5, 0.0, 0.0, 0.0];
    mesh.vertices[2].bones = [0; 4];
    mesh.vertices[2].weights = [1.0, 1.0, 0.0, 0.0];
    vertex_alphas(&mesh, &[(layer, 0.0)], &mut alphas);
    assert_eq!(alphas, [0.0, 0.375, 1.0]);
}
