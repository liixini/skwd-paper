use super::*;

fn fixture() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC_23);
    out.resize(32, 0);
    out.extend_from_slice(MESH_SIGNATURE);
    out.extend_from_slice(&(VERTEX_STRIDE as u32 * 3).to_le_bytes());
    for (position, uv) in [
        ([1.0_f32, 2.0, 0.0], [0.1_f32, 0.2]),
        ([3.0, 4.0, 0.0], [0.3, 0.4]),
        ([5.0, 6.0, 0.0], [0.5, 0.6]),
    ] {
        let start = out.len();
        out.extend(position.into_iter().flat_map(f32::to_le_bytes));
        out.resize(start + 72, 0);
        out[start + 56..start + 60].copy_from_slice(&1.0_f32.to_le_bytes());
        out.extend(uv.into_iter().flat_map(f32::to_le_bytes));
    }
    out.extend_from_slice(&6_u32.to_le_bytes());
    out.extend([0_u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
    out.extend_from_slice(b"MDLS0004\0");
    let animation_offset = out.len();
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&u32::MAX.to_le_bytes());
    out.extend_from_slice(&64_u32.to_le_bytes());
    for value in
        [1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
    {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.push(0);
    let animation = out.len();
    out[animation_offset..animation_offset + 4].copy_from_slice(&(animation as u32).to_le_bytes());
    out.extend_from_slice(b"MDLA0006\0");
    let animation_end = out.len();
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(b"move\0loop\0");
    out.extend_from_slice(&30.0_f32.to_le_bytes());
    out.extend_from_slice(&2_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&108_u32.to_le_bytes());
    for x in [0.0_f32, 10.0, 0.0] {
        for value in [x, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.resize(out.len() + 35, 0);
    let end = out.len();
    out[animation_end..animation_end + 4].copy_from_slice(&(end as u32).to_le_bytes());
    out
}

#[test]
fn parse_mesh_fields() {
    let mesh = parse(&fixture()).unwrap();
    assert_eq!(mesh.indices, [0, 1, 2]);
    assert_eq!(mesh.vertices[1].position, [3.0, 4.0, 0.0]);
    assert_eq!(mesh.vertices[2].uv, [0.5, 0.6]);
    assert_eq!(mesh.bones.len(), 1);
    assert_eq!(mesh.animations[0].id, 1);

    let mut version_21 = fixture();
    version_21[..8].copy_from_slice(MAGIC_21);
    assert_eq!(parse(&version_21).unwrap().vertices.len(), 3);
}

#[test]
fn rejects_bad_headers_indices() {
    let mut bad_header = fixture();
    bad_header[..8].copy_from_slice(b"MDLV0013");
    assert!(parse(&bad_header).unwrap_err().to_string().contains("header"));

    let mut bad_index = fixture();
    let index = 32 + 8 + VERTEX_STRIDE * 3 + 4;
    bad_index[index..index + 2].copy_from_slice(&3_u16.to_le_bytes());
    assert!(parse(&bad_index).unwrap_err().to_string().contains("outside"));

    let mut oversized_animation = fixture();
    let mode = find(&oversized_animation, b"move\0loop\0").unwrap();
    oversized_animation[mode + 14..mode + 18].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&oversized_animation).unwrap_err().to_string().contains("samples"));
}

#[test]
fn additive_skin_weights() {
    let mesh = parse(&fixture()).unwrap();
    let positions =
        skin(&mesh, &[AnimationLayer { id: 1, rate: 1.0, blend: 1.0, additive: true }], 1.0 / 30.0);
    assert!((positions[0][0] - 11.0).abs() < 0.001);
    assert!((positions[0][1] - 2.0).abs() < 0.001);
}

#[test]
fn mirrored_animation_reverses() {
    let mut mesh = parse(&fixture()).unwrap();
    mesh.animations[0].mirror = true;
    let layers = [AnimationLayer { id: 1, rate: 1.0, blend: 1.0, additive: true }];
    let forward = skin(&mesh, &layers, 1.0 / 30.0);
    let reverse = skin(&mesh, &layers, 3.0 / 30.0);
    assert_eq!(forward, reverse);
}

#[test]
fn degenerate_bind_pose() {
    let zero = Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [0.0, 0.0, 1.0] };
    let bones = vec![Bone { parent: None, bind: zero }];
    let bind_inverse: Vec<Option<Affine>> = world(bones.iter().map(|bone| bone.bind), &bones)
        .into_iter()
        .map(Affine::inverse)
        .collect();
    assert_eq!(bind_inverse, vec![None]);
    let mesh = Mesh {
        vertices: vec![Vertex {
            position: [1.0, 2.0, 0.0],
            uv: [0.0, 0.0],
            bones: [0, u16::MAX],
            weights: [1.0, 0.0],
        }],
        indices: vec![0],
        bones,
        animations: Vec::new(),
        bind_inverse,
    };
    let out = skin(&mesh, &[], 0.0);
    assert_eq!(out, vec![[1.0, 2.0, 0.0]]);
}

#[test]
fn bind_inverse_cached() {
    let mesh = parse(&fixture()).unwrap();
    assert_eq!(mesh.bind_inverse.len(), mesh.bones.len());
    assert!(mesh.bind_inverse.iter().all(Option::is_some));
}
