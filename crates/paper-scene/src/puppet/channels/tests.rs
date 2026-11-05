use super::*;

#[test]
fn texture_channel_blending() {
    let mut mesh = super::super::parse(&super::super::tests::fixture()).unwrap();
    mesh.texture_channels = Some(Geometry {
        material: String::new(),
        rows: 1,
        vertices: Vec::new(),
        indices: Vec::new(),
    });
    mesh.animations[0].channels.tracks = vec![vec![0.0, 1.0, 0.0]];
    let layer = AnimationLayer {
        visible: true,
        id: mesh.animations[0].id,
        rate: 1.0,
        blend: 1.0,
        additive: false,
    };
    let add = AnimationLayer { blend: 0.5, additive: true, ..layer };
    let time = 0.5 / mesh.animations[0].fps;
    let hidden = AnimationLayer { visible: false, ..layer };
    assert_eq!(weights(&mesh, &[(hidden, time)]).unwrap(), [0.0; COMPONENTS]);
    assert_eq!(weights(&mesh, &[(layer, time), (add, time)]).unwrap(), [0.75, 0.0, 0.0, 0.0]);
}

#[test]
fn bad_channel_sizes() {
    let mut bytes = Vec::new();
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend([0.0_f32, 1.0].into_iter().flat_map(f32::to_le_bytes));
    assert_eq!(clip(&bytes, &mut 0, 1).unwrap().tracks, [vec![0.0, 1.0]]);
    assert!(clip(&bytes, &mut 0, 2).is_err());
    assert!(clip(&bytes[..bytes.len() - 1], &mut 0, 1).is_err());
}

#[test]
#[ignore = "requires native-qualified puppet texture channel fixtures"]
fn authored_texture_channels() {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_CHANNEL_DIR").unwrap());
    for (name, weight) in [("zero", 0.0), ("full", 1.0)] {
        let bytes = std::fs::read(root.join(name).join("models/1_puppet.mdl")).unwrap();
        let mesh = super::super::parse(&bytes).unwrap();
        let geometry = mesh.texture_channels.as_ref().unwrap();
        assert_eq!(
            (geometry.rows, geometry.vertices.len(), geometry.indices.len()),
            (1, 4 * STRIDE, 6)
        );
        assert_eq!(mesh.animations[0].channels.tracks[0], vec![weight; 451]);
        assert_eq!(mesh.morph.as_ref().unwrap().shapes.len(), 1);
    }
}
