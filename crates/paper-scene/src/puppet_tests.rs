use super::*;

fn bone(parent: Option<u16>, bind: Transform) -> Bone {
    let raw_bind = crate::matrix::model_matrix(bind.translation, bind.scale, bind.rotation);
    Bone {
        name: String::new(),
        parent,
        raw_bind,
        bind,
        raw_rest: raw_bind,
        rest: bind,
        native_rest: bind::Bind::new(&raw_bind),
    }
}

pub(crate) fn fixture() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC_23);
    out.resize(32, 0);
    out[17..21].copy_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&crate::model::mesh::SKINNED.to_le_bytes());
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
    assert_eq!(mesh.animations[0].name, "move");

    let mut version_21 = fixture();
    version_21[..8].copy_from_slice(MAGIC_21);
    assert_eq!(parse(&version_21).unwrap().vertices.len(), 3);
}

#[test]
fn skeleton_three() {
    let original = fixture();
    let start = find(&original, SKELETON_SIGNATURE_4).unwrap();
    let mut bytes = original.clone();
    bytes[start..start + SKELETON_SIGNATURE_3.len()].copy_from_slice(SKELETON_SIGNATURE_3);
    assert_eq!(parse(&bytes).unwrap(), parse(&original).unwrap());

    let animation = start + SKELETON_SIGNATURE_3.len();
    bytes[animation..animation + 4].copy_from_slice(&((animation + 4) as u32).to_le_bytes());
    assert!(parse(&bytes).is_err());
    let mut bytes = original;
    bytes[start..start + SKELETON_SIGNATURE_3.len()].copy_from_slice(b"MDLS0005\0");
    assert!(parse(&bytes).is_err());
}

#[test]
fn skeleton_rest_matrices() {
    let rest = crate::matrix::model_matrix([56.0, 17.0, 0.0], [1.0; 3], [0.0; 3]);
    let attached = crate::matrix::model_matrix([1.0, 2.0, 0.0], [1.0; 3], [0.0; 3]);
    let block = |matrices: &[Mat4]| -> Vec<u8> {
        matrices.iter().flatten().flat_map(|value| value.to_le_bytes()).collect()
    };
    let mut bytes = vec![0, 0, 1];
    bytes.extend(block(&[rest]));
    bytes.extend([0; 8]);
    let tail = regions::parse(&bytes, 1, SKELETON_3).unwrap();
    assert_eq!(tail.rest, [rest]);
    assert!(tail.regions.is_empty());
    let mut attachments = 1_u16.to_le_bytes().to_vec();
    attachments.extend(b"tip\0");
    attachments.extend([0; 8]);
    attachments.extend(block(&[attached]));
    attachments.push(1);
    attachments.extend(block(&[rest, attached]));
    attachments.extend([0; 8]);
    assert_eq!(regions::parse(&attachments, 1, SKELETON_3).unwrap().rest, [rest]);
    let mut broken = vec![0, 0, 1];
    broken.extend(block(&[[f32::NAN; 16]]));
    broken.extend([0; 8]);
    assert!(regions::parse(&broken, 1, SKELETON_3).is_err());
}

#[test]
fn skeleton_three_regions() {
    let mut bytes = vec![0; 3];
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(
        [0_u32, 1.0_f32.to_bits(), 2.0_f32.to_bits()].into_iter().flat_map(u32::to_le_bytes),
    );
    bytes.extend([0; 4]);
    bytes.push(1);
    let values: Vec<f32> = (0..19).map(|value| value as f32).collect();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    let regions = regions::parse(&bytes, 1, SKELETON_3).unwrap().regions;
    assert_eq!(regions[0].size, [0.0, 1.0, 2.0]);
    assert_eq!(regions[0].transform, values[3..]);
    bytes.pop();
    assert!(regions::parse(&bytes, 1, SKELETON_3).is_err());
}

#[test]
#[ignore = "requires the native-qualified extrusion puppet"]
fn authored_skeleton_three() {
    let path = std::env::var_os("SKWD_PUPPET_EXTRUSION").expect("extrusion fixture path");
    let bytes = std::fs::read(path).expect("read extrusion fixture");
    assert_eq!(&bytes[..8], MAGIC_21);
    assert_eq!(&bytes[101_585..101_594], SKELETON_SIGNATURE_3);
    let mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.vertices.len(), 989);
    assert_eq!(mesh.indices.len(), 5055);
    assert_eq!(mesh.bones.len(), 24);
    assert_eq!(mesh.regions.len(), 24);
    assert_eq!(mesh.attachments.len(), 3);
    assert_eq!(mesh.animations.len(), 8);
    assert_eq!(mesh.skipped_constraints, 1);
    let depth = mesh.vertices.iter().map(|vertex| vertex.position[2]).fold(0.0_f32, f32::max);
    assert_eq!(depth.to_bits(), 0x4255_6202);
}

#[test]
fn rejects_bad_headers_indices() {
    let mut bad_header = fixture();
    bad_header[..8].copy_from_slice(b"MDLV9999");
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
    let positions = skin(
        &mesh,
        &[AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: true }],
        1.0 / 30.0,
    )
    .positions;
    assert!((positions[0][0] - 11.0).abs() < 0.001);
    assert!((positions[0][1] - 2.0).abs() < 0.001);
}

#[test]
fn mirrored_animation_reverses() {
    let mut mesh = parse(&fixture()).unwrap();
    mesh.animations[0].mirror = true;
    let layers = [AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: true }];
    let forward = skin(&mesh, &layers, 1.0 / 30.0).positions;
    let reverse = skin(&mesh, &layers, 3.0 / 30.0).positions;
    assert_eq!(forward, reverse);
}

#[test]
fn degenerate_bind_pose() {
    let zero = Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [0.0, 0.0, 1.0] };
    let bones = vec![bone(None, zero)];
    let bind_inverse: Vec<Option<Affine>> = world(bones.iter().map(|bone| bone.bind), &bones)
        .into_iter()
        .map(Affine::inverse)
        .collect();
    assert_eq!(bind_inverse, vec![None]);
    let mesh = Mesh {
        raw_vertices: None,
        vertices: vec![Vertex {
            position: [1.0, 2.0, 0.0],
            uv: [0.0, 0.0],
            bones: [0, u16::MAX, u16::MAX, u16::MAX],
            weights: [1.0, 0.0, 0.0, 0.0],
        }],
        indices: vec![0],
        bones,
        animations: Vec::new(),
        attachments: Vec::new(),
        regions: Vec::new(),
        morph: None,
        texture_channels: None,
        alpha: false,
        draw_order: None,
        masks: masks::Masks::default(),
        skipped_draw_order: false,
        skipped_constraints: 0,
        skipped_morphs: false,
        springs: Vec::new(),
        native_bind_inverse: vec![None],
        bind_inverse,
    };
    let out = skin(&mesh, &[], 0.0).positions;
    assert_eq!(out, vec![[1.0, 2.0, 0.0]]);
}

#[test]
fn bind_inverse_cached() {
    let mesh = parse(&fixture()).unwrap();
    assert_eq!(mesh.bind_inverse.len(), mesh.bones.len());
    assert!(mesh.bind_inverse.iter().all(Option::is_some));
}

#[test]
fn legacy_puppet_decodes_multiple_animations_and_named_bones() {
    let current = fixture();
    let skeleton = find(&current, b"MDLS0004\0").unwrap();
    let animation = find(&current, b"MDLA0006\0").unwrap();
    let mut legacy = current[..animation].to_vec();
    legacy[..8].copy_from_slice(MAGIC_19);
    legacy[skeleton..skeleton + 8].copy_from_slice(b"MDLS0002");
    let name = b"root";
    legacy.splice(skeleton + 17..skeleton + 17, name.iter().copied());
    let animation = legacy.len();
    legacy[skeleton + 9..skeleton + 13].copy_from_slice(&(animation as u32).to_le_bytes());
    legacy.extend_from_slice(b"MDLA0005\0");
    legacy.extend_from_slice(&0_u32.to_le_bytes());
    legacy.extend_from_slice(&2_u32.to_le_bytes());
    let current_animation = find(&current, b"MDLA0006\0").unwrap();
    let record = &current[current_animation + 17..current.len() - 35];
    for id in [1_u32, 2] {
        legacy.extend_from_slice(&id.to_le_bytes());
        legacy.extend_from_slice(&record[4..]);
        legacy.resize(legacy.len() + 34, 0);
    }
    let end = legacy.len() as u32;
    legacy[animation + 9..animation + 13].copy_from_slice(&end.to_le_bytes());
    let mesh = parse(&legacy).unwrap();
    assert_eq!(mesh.bones[0].name, "root");
    assert_eq!(mesh.vertices, parse(&current).unwrap().vertices);
    assert_eq!(mesh.animations.len(), 2);
    for id in [1, 2] {
        let positions = skin(
            &mesh,
            &[AnimationLayer { visible: true, id, rate: 1.0, blend: 1.0, additive: true }],
            1.0 / 30.0,
        )
        .positions;
        assert!((positions[0][0] - 11.0).abs() < 0.001);
    }
    legacy.truncate(legacy.len() - 1);
    assert!(parse(&legacy).is_err());
}

#[test]
fn all_four_vertex_influences_participate_in_skinning() {
    let mut bytes = fixture();
    let first_vertex = 32 + 8;
    for (index, weight) in [0.0_f32, 0.0, 0.25, 0.75].into_iter().enumerate() {
        let at = first_vertex + 56 + index * 4;
        bytes[at..at + 4].copy_from_slice(&weight.to_le_bytes());
    }
    let mut mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.vertices[0].weights, [0.0, 0.0, 0.25, 0.75]);
    mesh.vertices[0].bones = [u16::MAX, u16::MAX, 0, 1];
    let identity = affine(mesh.bones[0].bind);
    let mut right = identity;
    right.x = 4.0;
    let mut up = identity;
    up.y = 8.0;
    let actual = skin_vertex(
        &mesh.vertices[0],
        &[Some(BonePose::Planar(right)), Some(BonePose::Planar(up))],
    );
    assert_eq!(actual, [2.0, 8.0, 0.0]);
}

fn attachment_fixture() -> Vec<u8> {
    let mut bytes = fixture();
    let animation = find(&bytes, b"MDLA0006\0").unwrap();
    let mut attachment = b"MDAT0001\0".to_vec();
    attachment.extend_from_slice(&0_u32.to_le_bytes());
    attachment.extend_from_slice(&1_u16.to_le_bytes());
    attachment.extend_from_slice(&0_u16.to_le_bytes());
    attachment.extend_from_slice(b"Attachment\0");
    for value in
        [1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 654.09, -278.139, 0.0, 1.0]
    {
        attachment.extend_from_slice(&value.to_le_bytes());
    }
    let next = animation + attachment.len();
    attachment[9..13].copy_from_slice(&(next as u32).to_le_bytes());
    let end = bytes.len() + attachment.len();
    bytes[animation + 9..animation + 13].copy_from_slice(&(end as u32).to_le_bytes());
    bytes.splice(animation..animation, attachment);
    bytes
}

#[test]
fn attachments_preserve_animation() {
    let original = parse(&fixture()).unwrap();
    let mesh = parse(&attachment_fixture()).unwrap();
    assert_eq!(mesh.animations, original.animations);
    assert_eq!(mesh.attachments.len(), 1);
    let layers = [AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: true }];
    assert!((skin(&mesh, &layers, 1.0 / 30.0).positions[0][0] - 11.0).abs() < 0.001);
}

#[test]
fn attachment_offsets_bounded() {
    let original = attachment_fixture();
    let start = find(&original, b"MDAT0001\0").unwrap();
    let animation = find(&original, b"MDLA0006\0").unwrap();
    for end in [0, start, start + 13, animation - 1, animation + 1, original.len() + 1] {
        let mut bytes = original.clone();
        bytes[start + 9..start + 13].copy_from_slice(&(end as u32).to_le_bytes());
        assert!(parse(&bytes).is_err(), "offset {end}");
    }
    for end in start..animation {
        assert!(parse(&original[..end]).is_err(), "length {end}");
    }
}

#[test]
fn attachment_records_validated() {
    let original = attachment_fixture();
    let start = find(&original, b"MDAT0001\0").unwrap();
    for (offset, replacement) in [(13, 0_u16), (13, u16::MAX), (15, 1), (15, u16::MAX)] {
        let mut bytes = original.clone();
        bytes[start + offset..start + offset + 2].copy_from_slice(&replacement.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
    let mut invalid_matrix = original.clone();
    invalid_matrix[start + 28..start + 32].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(parse(&invalid_matrix).unwrap_err().to_string().contains("not finite"));
    let mut invalid_name = original;
    let end = find(&invalid_name, b"MDLA0006\0").unwrap();
    invalid_name[start + 17..end].fill(b'A');
    assert!(parse(&invalid_name).unwrap_err().to_string().contains("unterminated"));
}

#[test]
fn attachment_counts_parse() {
    let original = attachment_fixture();
    let start = find(&original, b"MDAT0001\0").unwrap();
    let animation = find(&original, b"MDLA0006\0").unwrap();
    let record = &original[start + 15..animation];
    for count in [0_u16, 2] {
        let mut bytes = original[..start + 13].to_vec();
        bytes.extend_from_slice(&count.to_le_bytes());
        for _ in 0..count {
            bytes.extend_from_slice(record);
        }
        let next = bytes.len();
        bytes[start + 9..start + 13].copy_from_slice(&(next as u32).to_le_bytes());
        bytes.extend_from_slice(&original[animation..]);
        let end = bytes.len() as u32;
        bytes[next + 9..next + 13].copy_from_slice(&end.to_le_bytes());
        let mesh = parse(&bytes).unwrap();
        assert_eq!(mesh.animations, parse(&fixture()).unwrap().animations);
        assert_eq!(mesh.attachments.len(), usize::from(count));
    }
}

#[test]
fn attachment_tracks_pose() {
    let mut mesh = parse(&attachment_fixture()).unwrap();
    mesh.attachments[0].transform.translation = [3.0, 4.0, 0.0];
    mesh.attachments[0].bone = 1;
    let child = Transform { translation: [5.0, 0.0, 0.0], ..mesh.bones[0].bind };
    mesh.bones.push(bone(Some(0), child));
    mesh.animations[0].tracks.push(vec![child; 3]);
    let track = &mut mesh.animations[0].tracks[0][1];
    track.rotation[2] = std::f32::consts::FRAC_PI_2;
    track.scale = [2.0, 2.0, 1.0];
    let layers = [AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: false }];
    let initial = skin(&mesh, &layers, 0.0);
    let moved = skin(&mesh, &layers, 1.0 / 30.0);
    assert_eq!(
        [
            initial.attachments[0].affine.x,
            initial.attachments[0].affine.y,
            initial.attachments[0].depth
        ],
        [8.0, 4.0, 0.0]
    );
    let attachment = moved.attachments[0];
    assert!((attachment.affine.x - 2.0).abs() < 0.001);
    assert!((attachment.affine.y - 16.0).abs() < 0.001);
    assert!(
        (attachment.affine.b.atan2(attachment.affine.a) - std::f32::consts::FRAC_PI_2).abs()
            < 0.001
    );
    assert_eq!(
        [
            attachment.affine.a.hypot(attachment.affine.b),
            attachment.affine.c.hypot(attachment.affine.d)
        ],
        [2.0, 2.0]
    );
}

#[test]
fn attachment_follows_scripted_bone() {
    let mesh = parse(&attachment_fixture()).unwrap();
    let mut world = Vec::new();
    sampled_world(&mesh, &[], &mut world, None).unwrap();
    let before = attachment_pose_world(&mesh, &world, "Attachment").unwrap();
    world[0][BIND_ORIGIN[0]] += 2.0;
    let after = attachment_pose_world(&mesh, &world, "Attachment").unwrap();
    assert_eq!(after.affine.x - before.affine.x, 2.0);
    assert_eq!(after.affine.y, before.affine.y);
    assert!(attachment_pose_world(&mesh, &world, "missing").is_none());
}

#[test]
fn frame_shares_bone_pose() {
    let mesh = parse(&fixture()).unwrap();
    let frame = skin(&mesh, &[], 0.0);
    for (vertex, position) in mesh.vertices.iter().zip(&frame.positions) {
        assert_eq!(frame.bones[0].unwrap().point(vertex.position), *position);
    }
}

#[test]
fn parses_region_metadata() {
    let mut bytes = vec![0; 11];
    bytes.push(1);
    let values: Vec<f32> = (0..19).map(|value| value as f32).collect();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    let regions = regions::parse(&bytes, 1, "MDLS0002").unwrap().regions;
    assert_eq!(regions[0].size, [0.0, 1.0, 2.0]);
    assert_eq!(regions[0].transform, values[3..]);
    bytes.pop();
    assert!(regions::parse(&bytes, 1, "MDLS0002").is_err());
}

#[test]
fn parses_authored_region() {
    let block: [u8; 93] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 52, 66, 175, 66, 215, 101, 131, 66, 215, 101, 131, 66,
        248, 169, 147, 190, 65, 31, 117, 191, 0, 0, 0, 0, 0, 0, 0, 0, 65, 31, 117, 63, 248, 169,
        147, 190, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 72,
        151, 60, 65, 0, 134, 133, 64, 215, 101, 131, 66, 0, 0, 128, 63, 1, 0, 0, 0, 0,
    ];
    let regions = regions::parse(&block, 1, "MDLS0002").unwrap().regions;
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].size.map(f32::to_bits), [1_118_782_004, 1_115_907_543, 1_115_907_543]);
    assert_eq!(
        regions[0].transform.map(f32::to_bits),
        [
            3_197_348_344,
            3_212_123_969,
            0,
            0,
            1_064_640_321,
            3_197_348_344,
            0,
            0,
            0,
            0,
            1_065_353_216,
            0,
            1_094_489_928,
            1_082_492_416,
            1_115_907_543,
            1_065_353_216
        ]
    );
}

#[test]
fn rejects_nonfinite_regions() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for offset in [12, 24] {
            let mut bytes = vec![0; 88];
            bytes[11] = 1;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(regions::parse(&bytes, 1, "MDLS0002").is_err());
        }
    }
}

fn region_at(x: f32) -> Region {
    Region {
        size: [4.0, 4.0, 0.0],
        transform: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, 0.0, 0.0, 1.0],
    }
}

#[test]
fn assigns_overlapping_regions() {
    use crate::particles::image::{SampleCell, UNBOUND_BONE};
    let mut mesh = parse(&fixture()).unwrap();
    mesh.bones = vec![
        bone(None, Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] }),
        bone(Some(0), Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] }),
    ];
    mesh.regions = vec![region_at(-2.0), region_at(2.0)];
    let mut cells: Vec<_> = [-5, 5, 0, 10]
        .map(|x| SampleCell { color: [0; 3], bone: UNBOUND_BONE, position: [x, 0] })
        .into();
    assign_regions(&mesh, &mut cells);
    assert_eq!(cells.iter().map(|cell| cell.bone).collect::<Vec<_>>(), [0, 1, 0, 1]);
    let mut overlap = [SampleCell { color: [0; 3], bone: UNBOUND_BONE, position: [0, 0] }];
    assign_regions(&mesh, &mut overlap);
    assert_eq!(overlap[0].bone, UNBOUND_BONE);
}

#[test]
fn regions_follow_bind_hierarchy() {
    use crate::particles::image::{SampleCell, UNBOUND_BONE};
    let mut mesh = parse(&fixture()).unwrap();
    let mut parent = Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] };
    parent.translation = [20.0, 30.0, 0.0];
    let mut child = Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] };
    child.translation = [10.0, 0.0, 0.0];
    mesh.bones = vec![bone(None, parent), bone(Some(0), child)];
    mesh.regions = vec![region_at(0.0), region_at(0.0)];
    let mut cells = [
        SampleCell { color: [0; 3], bone: UNBOUND_BONE, position: [20, -30] },
        SampleCell { color: [0; 3], bone: UNBOUND_BONE, position: [30, -30] },
    ];
    assign_regions(&mesh, &mut cells);
    assert_eq!(cells.map(|cell| cell.bone), [0, 1]);
}

#[test]
fn assigns_full_sample_grid() {
    use crate::particles::image::{SampleCell, UNBOUND_BONE};
    let mut mesh = parse(&fixture()).unwrap();
    let bind = Transform { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] };
    mesh.bones = vec![bone(None, bind), bone(None, bind)];
    mesh.regions =
        [-160.0, 160.0].map(|x| Region { size: [320.0, 270.0, 0.0], ..region_at(x) }).into();
    let mut cells: Vec<_> = (-480..480)
        .flat_map(|x| {
            (-270..270).map(move |y| SampleCell {
                color: [0; 3],
                bone: UNBOUND_BONE,
                position: [x, y],
            })
        })
        .collect();
    let started = std::time::Instant::now();
    assign_regions(&mesh, &mut cells);
    eprintln!("region assignment: {} cells in {:?}", cells.len(), started.elapsed());
    assert_eq!(cells.len(), 518_400);
    assert!(cells.iter().all(|cell| cell.bone == u8::from(cell.position[0] > 0)));
}

#[test]
fn old_event_directory() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(b"{\"frame\":0,\"name\":\"sword\"}\0");
    let mut at = 0;
    let events = parse_events(&bytes, &mut at, bytes.len(), true).unwrap();
    assert_eq!(at, bytes.len());
    assert_eq!(events[0].payload.frame, 0);
    assert_eq!(events[0].payload.name, "sword");
    assert!(parse_events(&bytes, &mut 0, bytes.len() - 1, true).is_err());
}

#[test]
fn old_vertex_layout() {
    let mut bytes = Vec::new();
    bytes.extend([1.0_f32, 2.0, 3.0].into_iter().flat_map(f32::to_le_bytes));
    bytes.extend([4_u32, 3, 2, 1].into_iter().flat_map(u32::to_le_bytes));
    bytes.extend([0.5_f32, 0.25, 0.25, 0.0, 0.2, 0.8].into_iter().flat_map(f32::to_le_bytes));
    let vertex = parse_vertices(&bytes, 0, 1, OLD_VERTEX_STRIDE).unwrap()[0];
    assert_eq!(vertex.position, [1.0, 2.0, 3.0]);
    assert_eq!(vertex.bones, [4, 3, 2, 1]);
    assert_eq!(vertex.weights, [0.5, 0.25, 0.25, 0.0]);
    assert_eq!(vertex.uv, [0.2, 0.8]);
}

#[test]
fn modern_mask_unchanged() {
    let mesh = parse(&fixture()).unwrap();
    assert!(mesh.animations[0].disabled.is_none());
    assert!(mesh.animations[0].events.is_empty());
}

#[test]
fn modern_animation_event() {
    let mut bytes = fixture();
    let animation = find(&bytes, b"MDLA0006\0").unwrap();
    let event_count = bytes.len() - size_of::<u32>();
    bytes[event_count..].copy_from_slice(&1_u32.to_le_bytes());
    bytes.extend(1.0_f32.to_le_bytes());
    bytes.extend(b"{\"frame\":1,\"name\":\"step\"}\0");
    let end = bytes.len() as u32;
    bytes[animation + 9..animation + 13].copy_from_slice(&end.to_le_bytes());
    let mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.animations[0].events[0].payload.name, "step");
    assert_eq!(mesh.animations[0].events[0].payload.frame, 1);
    assert_eq!(mesh.animations[0].events[0].field, 1.0_f32.to_bits());
}

#[test]
#[ignore = "requires an installed authored modern puppet model"]
fn authored_modern_animation_event() {
    let path = std::env::var("SKWD_MODERN_EVENT_MDL").unwrap();
    let mesh = parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(mesh.animations.iter().map(|clip| clip.events.len()).sum::<usize>(), 1);
    assert_eq!(mesh.animations[0].events[0].payload.frame, 30);
}

#[test]
fn old_track_mask() {
    let original = fixture();
    let start = find(&original, b"MDLA0006\0").unwrap();
    let mut bytes = original[start..original.len() - 35].to_vec();
    bytes[..8].copy_from_slice(b"MDLA0001");
    bytes.extend(0_u32.to_le_bytes());
    let end = bytes.len() as u32;
    bytes[9..13].copy_from_slice(&end.to_le_bytes());
    let track = find(&bytes, b"move\0loop\0").unwrap() + 10 + 16;
    bytes[track..track + 4].copy_from_slice(&1_u32.to_le_bytes());
    let clips = parse_animations(&bytes, 0, 1, 1, false).unwrap();
    assert_eq!(clips[0].disabled, Some(vec![true]));
    bytes[track..track + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(parse_animations(&bytes, 0, 1, 1, false).is_err());
}

#[test]
fn disabled_track_keeps_pose() {
    let mut mesh = parse(&fixture()).unwrap();
    mesh.animations[0].disabled = Some(vec![true]);
    let layer = AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive: false };
    assert_eq!(skin(&mesh, &[layer], 1.0 / 30.0).positions, skin(&mesh, &[], 0.0).positions);
    mesh.animations[0].disabled = Some(vec![false]);
    assert_ne!(skin(&mesh, &[layer], 1.0 / 30.0).positions, skin(&mesh, &[], 0.0).positions);
}

#[test]
fn official_old_models() {
    for (bytes, vertices, indices, bones, constraints, clips, events) in [
        (&include_bytes!("fixtures/samurai.mdl")[..], 1041, 5400, 17, 0, 4, 1),
        (&include_bytes!("fixtures/wires.mdl")[..], 114, 492, 2, 0, 1, 0),
    ] {
        let mesh = parse(bytes).unwrap();
        assert_eq!(mesh.vertices.len(), vertices);
        assert_eq!(mesh.indices.len(), indices);
        assert_eq!(mesh.bones.len(), bones);
        assert_eq!(mesh.skipped_constraints, constraints);
        assert_eq!(mesh.animations.len(), clips);
        assert_eq!(mesh.animations.iter().map(|clip| clip.events.len()).sum::<usize>(), events);
        assert!(mesh.animations.iter().all(|clip| clip.disabled.is_some()));
        for length in [8, 49, bytes.len() - 1] {
            assert!(parse(&bytes[..length]).is_err());
        }
    }
}

#[test]
fn section_bounds_checked() {
    let mut bytes = include_bytes!("fixtures/wires.mdl").to_vec();
    bytes[7507..7511].copy_from_slice(&7515_u32.to_le_bytes());
    assert!(parse(&bytes).is_err());
    let mut bytes = include_bytes!("fixtures/wires.mdl").to_vec();
    bytes[6975..6979].copy_from_slice(&6983_u32.to_le_bytes());
    assert!(parse(&bytes).is_err());
    let mut bytes = include_bytes!("fixtures/wires.mdl").to_vec();
    bytes[7511..7515].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&bytes).is_err());
    let mut bytes = 1025_u32.to_le_bytes().to_vec();
    assert!(parse_events(&bytes, &mut 0, bytes.len(), true).is_err());
    bytes[..4].copy_from_slice(&1_u32.to_le_bytes());
    assert!(parse_events(&bytes, &mut 0, bytes.len(), true).is_err());
}

#[test]
fn skeleton_requires_terminator() {
    let mut bytes = fixture();
    bytes[9..17].copy_from_slice(b"MDLS0002");
    assert_eq!(parse(&bytes).unwrap(), parse(&fixture()).unwrap());
}

#[test]
fn old_path_signature() {
    let mut bytes = include_bytes!("fixtures/wires.mdl").to_vec();
    let material = b"MDLS0001s/wires.json";
    assert_eq!(material.len(), b"materials/wires.json".len());
    bytes[21..21 + material.len()].copy_from_slice(material);
    assert_eq!(parse(&bytes).unwrap().vertices.len(), 114);
}

#[test]
fn old_contract_rejections() {
    let original = include_bytes!("fixtures/wires.mdl");
    for (offset, replacement) in [
        (8, vec![1]),
        (62, 2_u32.to_le_bytes().to_vec()),
        (6984, 2_u32.to_le_bytes().to_vec()),
        (7519, 1_u32.to_le_bytes().to_vec()),
        (7548, 1_u32.to_le_bytes().to_vec()),
        (7498, b"MDLA0005".to_vec()),
    ] {
        let mut bytes = original.to_vec();
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert!(parse(&bytes).is_err(), "offset {offset}");
    }
    let mut events = 1_u32.to_le_bytes().to_vec();
    events.extend(1_u32.to_le_bytes());
    events.extend(b"{\"frame\":0,\"name\":\"sword\"}\0");
    assert!(parse_events(&events, &mut 0, events.len(), true).is_err());
}

#[test]
fn disabled_layer_preserves_previous() {
    for additive in [false, true] {
        let mut mesh = parse(&fixture()).unwrap();
        let mut clip = parse(&fixture()).unwrap().animations.remove(0);
        clip.id = 2;
        clip.disabled = Some(vec![true]);
        clip.tracks[0][1].translation[0] = 20.0;
        mesh.animations.push(clip);
        let layers = [
            AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 1.0, additive },
            AnimationLayer { visible: true, id: 2, rate: 1.0, blend: 1.0, additive },
        ];
        let first = skin(&mesh, &layers[..1], 1.0 / 30.0).positions;
        assert_eq!(skin(&mesh, &layers, 1.0 / 30.0).positions, first);
        mesh.animations[1].disabled = Some(vec![false]);
        assert_ne!(skin(&mesh, &layers, 1.0 / 30.0).positions, first);
    }
}

#[test]
fn modern_rejects_old_animation() {
    for version in [MAGIC_19, MAGIC_21, MAGIC_23] {
        let mut bytes = fixture();
        bytes[..8].copy_from_slice(version);
        let animation = find(&bytes, b"MDLA0006\0").unwrap();
        bytes[animation..animation + 8].copy_from_slice(b"MDLA0001");
        let track = find(&bytes, b"move\0loop\0").unwrap() + 10 + 16;
        bytes[track..track + 4].copy_from_slice(&1_u32.to_le_bytes());
        assert!(parse(&bytes).unwrap_err().to_string().contains("modern puppet animation"));
    }
}

#[test]
fn constraints_each_version() {
    for version in [b"MDLS0001", b"MDLS0002", b"MDLS0003", b"MDLS0004"] {
        for (metadata, expected) in [
            ("", 0),
            ("{}", 0),
            (r#"{"se":false}"#, 0),
            (r#"{"se":"true","ge":true}"#, 0),
            (r#"{"se":true,"r":true}"#, 1),
            (r#"{"ik":true}"#, 1),
            ("{", 1),
        ] {
            let mut bytes = fixture();
            let start = find(&bytes, b"MDLS0004\0").unwrap();
            bytes[start..start + version.len()].copy_from_slice(version);
            let offset = start + version.len() + 1;
            let end = le_u32(&bytes, offset).unwrap() as usize;
            bytes.splice(end - 1..end - 1, metadata.bytes());
            let end = end + metadata.len();
            bytes[offset..offset + size_of::<u32>()].copy_from_slice(&(end as u32).to_le_bytes());
            let (skeleton, _) = parse_bones(&bytes, start, false).unwrap();
            assert_eq!(skeleton.bones.len(), 1);
            assert_eq!(skeleton.skipped_constraints, expected);
            if version != b"MDLS0001" {
                let offset = end + ANIMATION_6.len() + 1;
                let animation_end = bytes.len() as u32;
                bytes[offset..offset + size_of::<u32>()]
                    .copy_from_slice(&animation_end.to_le_bytes());
                assert_eq!(parse(&bytes).unwrap().skipped_constraints, expected);
            }
        }
    }
}

#[test]
fn frozen_palette_bits() {
    const BIND_ORIGIN: [f32; 3] = [-5.438_96, -3.131_62, 0.0];
    const POSE: Transform = Transform {
        translation: [614.762_3, -331.471_1, 0.0],
        rotation: [0.0, 0.0, -0.828_267_8],
        scale: [1.000_000_1; 3],
    };
    const SAMPLES: [([f32; 3], [u32; 2]); 5] = [
        ([0.0, 0.0, 0.0], [0x441b_2fd1, 0xc3a6_ae30]),
        ([1.0, 0.0, 0.0], [0x441b_5b17, 0xc3a7_0c7e]),
        ([0.0, 1.0, 0.0], [0x441b_5ef8, 0xc3a6_57a4]),
        ([0.0, 0.0, 1.0], [0x441b_2fd1, 0xc3a6_ae30]),
        ([36.158_905, -15.828_461, 0.0], [0x441e_6232, 0xc3b9_5a16]),
    ];
    let mut mesh = parse(&fixture()).unwrap();
    let mut raw_bind = *mesh.bones[0].raw_bind();
    raw_bind[12..15].copy_from_slice(&BIND_ORIGIN);
    mesh.bones[0] = Bone::from_matrix(mesh.bones[0].name.clone(), None, raw_bind, false).unwrap();
    mesh.bind_inverse = world(mesh.bones.iter().map(|bone| bone.bind), &mesh.bones)
        .into_iter()
        .map(Affine::inverse)
        .collect();
    mesh.animations[0].tracks[0].fill(POSE);
    let layer = AnimationLayer {
        visible: true,
        id: mesh.animations[0].id,
        rate: 0.0,
        blend: 1.0,
        additive: false,
    };
    for (position, expected) in SAMPLES {
        mesh.vertices[0].position = position;
        for time in [0.0, 12.0] {
            let frame = skin(&mesh, &[layer], time);
            let actual = frame.positions[0];
            assert_eq!([actual[0].to_bits(), actual[1].to_bits()], expected);
            let palette = frame.bones[0].unwrap().point(position);
            assert_eq!([palette[0].to_bits(), palette[1].to_bits()], expected);
        }
    }
}

#[test]
fn palette_composition_order() {
    let pose = Affine { a: 2.0, b: 1.0, c: 3.0, d: 4.0, x: 5.0, y: 6.0 };
    let bind = Affine { a: 1.0, b: 2.0, c: 0.0, d: 1.0, x: 7.0, y: 8.0 };
    assert_eq!(skin_transform(pose, Affine::default()), pose);
    assert_eq!(skin_transform(Affine::default(), bind), bind);
    assert_eq!(
        skin_transform(pose, bind),
        Affine { a: 8.0, b: 9.0, c: 3.0, d: 4.0, x: 43.0, y: 45.0 }
    );
    assert_ne!(skin_transform(pose, bind), skin_transform(bind, pose));
}

#[test]
fn modern_index_width() {
    let original = fixture();
    assert_eq!(parse(&original).unwrap().vertices[0].bones, [0; 4]);
    for lane in 0..4 {
        let mut bytes = original.clone();
        let offset = 32 + 8 + 40 + lane * 4;
        bytes[offset..offset + 4].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
}

#[test]
#[ignore = "requires the frozen native-qualified Plana model"]
fn authored_index_width() {
    let path = std::env::var_os("SKWD_PUPPET_MODEL_FIXTURE").expect("modern fixture path");
    let original = std::fs::read(path).expect("read modern fixture");
    assert_eq!(&original[..8], b"MDLV0019");
    assert_eq!(&original[73..77], &0x0180_000f_u32.to_le_bytes());
    assert_eq!(&original[11191..11200], b"MDLS0002\0");
    let mesh = parse(&original).unwrap();
    assert_eq!(mesh.vertices.len(), 123);
    assert!(mesh.vertices.iter().all(|vertex| vertex.bones == [0; 4]));
    for lane in 0..4 {
        let mut bytes = original.clone();
        let offset = 81 + 40 + lane * 4;
        bytes[offset..offset + 4].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
    assert_eq!(parse(&original).unwrap(), mesh);
}

#[test]
fn raw_bind_bits() {
    let mut bytes = fixture();
    let skeleton = find(&bytes, b"MDLS0004\0").unwrap();
    let offset = skeleton + 30;
    let raw: Mat4 =
        [1.0, -0.0, 0.25, 0.5, 0.75, 2.0, 1.25, -0.5, 3.5, -4.5, 2.5, 0.125, 5.0, -6.0, 7.0, 1.0];
    for (lane, value) in raw.into_iter().enumerate() {
        bytes[offset + lane * 4..offset + (lane + 1) * 4].copy_from_slice(&value.to_le_bytes());
    }
    let mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.bones[0].raw_bind().map(f32::to_bits), raw.map(f32::to_bits));
    assert_eq!(mesh.bones[0].bind.translation, [5.0, -6.0, 7.0]);
    assert_eq!(mesh.bones[0].bind.rotation.map(f32::to_bits), [0, 0, 0x8000_0000]);
    assert_eq!(mesh.bones[0].bind.scale, [1.0, 0.75_f32.hypot(2.0), 1.0]);
    let mut projected = raw;
    for lane in [2, 3, 6, 7, 8, 9, 10, 11, 15] {
        projected[lane] = 0.0;
    }
    assert_eq!(mesh.bones[0].bind, matrix_transform(projected).unwrap());
    assert_ne!(mesh.bones[0].raw_bind().map(f32::to_bits), projected.map(f32::to_bits));
}

#[test]
fn invalid_bind_payloads() {
    let original = fixture();
    let skeleton = find(&original, b"MDLS0004\0").unwrap();
    let offset = skeleton + 30;
    for lane in 0..16 {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bytes = original.clone();
            bytes[offset + lane * 4..offset + (lane + 1) * 4].copy_from_slice(&value.to_le_bytes());
            assert!(parse(&bytes).unwrap_err().to_string().contains("not finite"));
        }
    }
    for present in 0..64 {
        let mut bytes = original.clone();
        let boundary = offset + present;
        bytes[skeleton + 9..skeleton + 13].copy_from_slice(&(boundary as u32).to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
}

fn authored_package(
    scene: &serde_json::Value,
    combos: &serde_json::Value,
    vertex: &str,
) -> crate::pkg::Package {
    let mut model = fixture();
    let samples = model.len() - 35 - 108;
    for frame in 0..3 {
        let rotation = samples + frame * 36 + 12;
        model[rotation..rotation + 4].copy_from_slice(&0.25_f32.to_le_bytes());
    }
    packaged(&model, scene, combos, vertex)
}

fn packaged(
    model: &[u8],
    scene: &serde_json::Value,
    combos: &serde_json::Value,
    vertex: &str,
) -> crate::pkg::Package {
    let material =
        serde_json::json!({"passes":[{"shader":"authored","textures":["a"],"combos":combos}]})
            .to_string();
    let texture = crate::tests::build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene.to_string().as_bytes()),
        ("models/image.json", br#"{"material":"materials/a.json","puppet":"models/a.mdl"}"#),
        (AUTHORED_MODEL, model),
        ("materials/a.json", material.as_bytes()),
        ("materials/a.tex", &texture),
        ("shaders/authored.vert", vertex.as_bytes()),
        ("shaders/authored.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
    ]))
    .unwrap()
}

fn authored_scene() -> serde_json::Value {
    serde_json::json!({"general":{"orthogonalprojection":{"width":3840,"height":2160}},
        "objects":[{"id":1,"image":"models/image.json","size":"32 32","origin":"1920 1080 0",
        "animationlayers":[{"animation":1,"rate":0,"blend":1,"visible":true,"additive":false}]}]})
}

const AUTHORED_MODEL: &str = "models/a.mdl";
const AUTHORED_VERTEX: &str = "#if SKINNING\nuniform mat4x3 g_Bones[BONECOUNT];\n#endif\nuniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nvoid main(){gl_Position=mul(vec4(mul(vec4(a_Position,1.0),g_Bones[a_BlendIndices.x])*a_BlendWeights.x,1.0),g_ModelViewProjectionMatrix);}";

#[test]
fn authored_loader_ownership() {
    let package = authored_package(&authored_scene(), &serde_json::json!({}), AUTHORED_VERTEX);
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    let puppet = layer.puppet.as_ref().unwrap();
    let skin = layer.material.as_ref().unwrap().skin.as_ref().unwrap();
    assert!(puppet.mesh.raw_vertices.is_none());
    assert!(puppet.mesh.indices.is_empty());
    assert_eq!(skin.vertices.len(), puppet.mesh.vertices.len() * 80);
    assert_eq!(skin.indices, [0, 1, 2]);
    assert_eq!(skin.palette.uniform, "g_Bones[BONECOUNT]");
    let [Some(BonePose::Spatial(matrix))] = skin.palette.bones.as_ref() else {
        panic!("spatial palette");
    };
    let point = puppet.mesh.vertices[0].position;
    assert_eq!(
        &skin.vertices[..12],
        point.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>()
    );
    assert_ne!(BonePose::Spatial(*matrix).point(point), point);
    let shared = skin.palette.bones.clone();
    assert!(std::sync::Arc::ptr_eq(&shared, &skin.palette.bones));
}

#[test]
fn authored_material_keyframes() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for parented in [false, true] {
        let mut scene = authored_scene();
        let origin = serde_json::json!({"value":"1920 1080 0","animation":{
            "options":{"fps":4,"length":8,"mode":"single"},
            "c0":[{"frame":0,"value":1920},{"frame":8,"value":2000}],
            "c1":[{"frame":0,"value":1080},{"frame":8,"value":1080}],
            "c2":[{"frame":0,"value":0},{"frame":8,"value":0}]
        }});
        if parented {
            scene["objects"][0]["parent"] = serde_json::json!(2);
            scene["objects"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"id":2,"origin":origin}));
        } else {
            scene["objects"][0]["origin"] = origin;
        }
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.scripts.is_none());
        assert!(!model.keyframes.is_empty());
        assert!(model.skipped.is_empty(), "{parented}: {:?}", model.skipped);
        assert_eq!(model.layers.len(), 1);
        assert!(model.layers[0].puppet.is_some());
        assert!(model.layers[0].material.as_ref().unwrap().skin.is_some());
    }
}

#[test]
fn authored_loader_rejections() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let original = authored_scene();
    for (pointer, value) in [
        ("/objects/0/animationlayers/0/rate", serde_json::json!(-1)),
        ("/objects/0/animationlayers/0/blend", serde_json::json!(2)),
        ("/objects/0/animationlayers/0/animation", serde_json::json!(99)),
        ("/objects/0/origin", serde_json::json!("nan 0 0")),
    ] {
        let mut scene = original.clone();
        *scene.pointer_mut(pointer).unwrap() = value;
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(!model.skipped.is_empty(), "{pointer}");
    }
    for (pointer, value) in [
        ("/objects/0/animationlayers/0/rate", serde_json::json!(1)),
        ("/objects/0/animationlayers/0/blend", serde_json::json!(0.5)),
        ("/objects/0/animationlayers/0/additive", serde_json::json!(true)),
    ] {
        let mut scene = original.clone();
        *scene.pointer_mut(pointer).unwrap() = value;
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{pointer}: {:?}", model.skipped);
    }
    for combos in [
        serde_json::json!({"SKINNING":0}),
        serde_json::json!({"BONECOUNT":2}),
        serde_json::json!({"MORPHING":1}),
        serde_json::json!({"UNKNOWN_FEATURE":1}),
    ] {
        let package = authored_package(&original, &combos, AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(!model.skipped.is_empty(), "{combos}");
    }
    for vertex in [
        AUTHORED_VERTEX.replace("uvec4", "vec4"),
        AUTHORED_VERTEX.replace("a_Position", "a_Unknown"),
    ] {
        let package = authored_package(&original, &serde_json::json!({}), &vertex);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(!model.skipped.is_empty());
    }
}

#[test]
fn frozen_palette_rejections() {
    let mut bytes = fixture();
    let samples = bytes.len() - 35 - 108;
    for frame in 0..3 {
        let rotation = samples + frame * 36 + 12;
        bytes[rotation..rotation + 4].copy_from_slice(&0.25_f32.to_le_bytes());
    }
    let mut mesh = parse(&bytes).unwrap();
    let layers = [AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 1.0, additive: false }];
    assert!(authored(&mesh, &layers, None).is_ok());
    mesh.vertices[0].bones[1] = 1;
    assert!(authored(&mesh, &layers, None).is_err());
    mesh.vertices[0].bones[1] = 0;
    mesh.animations[0].disabled = Some(vec![true]);
    assert_eq!(authored(&mesh, &layers, None).unwrap().bones, rest_palette(&mesh).unwrap());
    mesh.animations[0].disabled = None;
    mesh.animations[0].fps = f32::NAN;
    assert!(authored(&mesh, &layers, None).is_err());
}

#[test]
fn dormant_fog_selector() {
    let vertex = format!("// [COMBO] {{\"combo\":\"FOG\",\"default\":1}}\n{AUTHORED_VERTEX}");
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for enabled in [false, true] {
        let mut scene = authored_scene();
        scene["general"]["fogdistance"] = serde_json::json!(enabled);
        let package = authored_package(&scene, &serde_json::json!({}), &vertex);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert_eq!(model.skipped.is_empty(), !enabled, "{:?}", model.skipped);
    }
}

#[test]
#[ignore = "requires exact R245 spatial authored fixture and assets"]
fn shipped_puppet_loader() {
    let path = std::env::var_os("SKWD_PUPPET_SCENE_FIXTURE").expect("authored fixture path");
    let assets = std::env::var("SKWD_PUPPET_ASSET_ROOT").expect("authored assets path");
    let package = crate::pkg::Package::parse(std::fs::read(path).unwrap()).unwrap();
    let model = crate::model::load_with(&package, &crate::effects::Assets::discover(Some(&assets)))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers.len(), 1);
    assert_eq!(
        model.layers[0].material.as_ref().unwrap().skin.as_ref().unwrap().vertices.len(),
        9840
    );
    assert!(model.layers[0].puppet.as_ref().unwrap().mesh.raw_vertices.is_none());
}

#[test]
fn authored_palette_keys() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for count in ["1", "BONECOUNT"] {
        let source = AUTHORED_VERTEX.replace("[BONECOUNT]", &format!("[{count}]"));
        let package = authored_package(&authored_scene(), &serde_json::json!({}), &source);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let material = model.layers[0].material.as_ref().unwrap();
        let palette = &material.skin.as_ref().unwrap().palette;
        assert_eq!(palette.uniform, format!("g_Bones[{count}]"));
        let [Some(BonePose::Spatial(matrix))] = palette.bones.as_ref() else {
            panic!("spatial palette");
        };
        let meta = crate::effects::PassMeta::of(&material.pass);
        let values = std::collections::BTreeMap::from([(palette.uniform.clone(), matrix.to_vec())]);
        let bytes = meta.uniform_bytes_with(
            crate::effects::FrameClock::default(),
            None,
            32,
            32,
            (32, 32),
            &[],
            &values,
            true,
        );
        let uniform = material
            .pass
            .vertex
            .uniforms
            .iter()
            .find(|uniform| uniform.name == palette.uniform)
            .unwrap();
        let expected: Vec<_> = matrix.iter().flat_map(|value| value.to_ne_bytes()).collect();
        assert_eq!(&bytes[uniform.offset..uniform.offset + 64], expected);
    }
}

#[test]
fn authored_interface_rejections() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for declaration in [
        "attribute vec3 a_Position;",
        "attribute uvec4 a_BlendIndices;",
        "attribute vec4 a_BlendWeights;",
    ] {
        let source = AUTHORED_VERTEX.replace(declaration, "");
        let package = authored_package(&authored_scene(), &serde_json::json!({}), &source);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(!model.skipped.is_empty(), "{declaration}");
    }
    for declaration in [
        "uniform mat4 g_ModelMatrixInverse;",
        "uniform mat4 g_ModelViewProjectionMatrixInverse;",
        "uniform mat4 g_NormalModelMatrix;",
        "uniform mat3 g_AltModelMatrix;",
        "uniform mat4 g_EffectModelMatrix;",
        "uniform mat4 g_EffectTextureProjectionMatrix;",
        "uniform mat4 g_ViewMatrix;",
    ] {
        let source = format!("{declaration}\n{AUTHORED_VERTEX}");
        let package = authored_package(&authored_scene(), &serde_json::json!({}), &source);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(!model.skipped.is_empty(), "{declaration}");
    }
}

#[test]
fn authored_bound_parameters() {
    let mut assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut scene = authored_scene();
    scene["objects"][0]["animationlayers"][0]["rate"] =
        serde_json::json!({"user":"rate","value":0});
    scene["objects"][0]["animationlayers"][0]["blend"] =
        serde_json::json!({"user":"blend","value":1});
    for (rate, blend, admitted) in [(0.0, 1.0, true), (-1.0, 1.0, false), (0.0, 2.0, false)] {
        assets.properties.insert("rate".into(), vec![rate]);
        assets.properties.insert("blend".into(), vec![blend]);
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert_eq!(model.skipped.is_empty(), admitted, "{:?}", model.skipped);
    }
}

#[test]
fn authored_animation_ids() {
    let mut assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut scene = authored_scene();
    for raw in [
        serde_json::json!(1.5),
        serde_json::json!(-0.5),
        serde_json::json!("NaN"),
        serde_json::json!({"value":1.5}),
        serde_json::json!({"user":"missing","value":1.5}),
        serde_json::json!({"value":"invalid 1"}),
    ] {
        scene["objects"][0]["animationlayers"][0]["animation"] = raw;
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(!model.skipped.is_empty());
    }
    scene["objects"][0]["animationlayers"][0]["animation"] =
        serde_json::json!({"user":"clip","value":1});
    for value in [1.0, 1.5, -0.5, f32::NAN, f32::INFINITY] {
        assets.properties.insert("clip".into(), vec![value]);
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert_eq!(model.skipped.is_empty(), value == 1.0, "{value}");
    }
}

#[test]
fn frozen_planar_palette() {
    let layers = [AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 1.0, additive: false }];
    let mut mesh = parse(&fixture()).unwrap();
    mesh.animations[0].tracks[0].rotate_left(1);
    let legacy = skin(&mesh, &layers, 0.0);
    assert!(matches!(legacy.bones[0], Some(BonePose::Planar(_))));
    let planar = authored(&mesh, &layers, None).unwrap();
    assert_eq!(
        planar.bones.as_ref(),
        [Some(BonePose::Spatial([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 0.0, 0.0, 1.0,
        ]))]
    );
    assert_eq!(planar.positions[0], [11.0, 2.0, 0.0]);
    assert_ne!(planar.positions[0], mesh.vertices[0].position);
    assert_eq!(skin(&mesh, &layers, 0.0), legacy);
    mesh.animations[0].spatial = true;
    assert_eq!(authored(&mesh, &layers, None).unwrap(), planar);
    mesh.animations[0].spatial = false;
    mesh.animations[0].disabled = Some(vec![true]);
    assert_eq!(authored(&mesh, &layers, None).unwrap().bones, rest_palette(&mesh).unwrap());
    mesh.animations[0].disabled = None;
    mesh.vertices[0].weights[0] = 0.5;
    assert_eq!(authored(&mesh, &layers, None).unwrap().positions[0], [5.5, 1.0, 0.0]);
    assert_eq!(skin(&mesh, &layers, 0.0), legacy);
}

#[test]
fn authored_planar_loader() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for rate in [0.0, 1.0] {
        let mut scene = authored_scene();
        scene["objects"][0]["animationlayers"][0]["rate"] = serde_json::json!(rate);
        let original = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let planar = fixture();
        let entries: Vec<_> = original
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.path.as_str(),
                    if entry.path == AUTHORED_MODEL {
                        planar.as_slice()
                    } else {
                        original.read(entry)
                    },
                )
            })
            .collect();
        let package = crate::pkg::Package::parse(crate::tests::build_pkg(&entries)).unwrap();
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let layer = &model.layers[0];
        let puppet = layer.puppet.as_ref().unwrap();
        assert_eq!(puppet.layers[0].rate, rate);
        assert!(!puppet.mesh.animations[0].spatial);
        let palette = &layer.material.as_ref().unwrap().skin.as_ref().unwrap().palette;
        assert!(matches!(palette.bones[0], Some(BonePose::Spatial(_))));
        assert!(puppet.mesh.raw_vertices.is_none());
        assert!(puppet.mesh.indices.is_empty());
    }
}

#[test]
fn authored_playback_palette() {
    let mesh = parse(&fixture()).unwrap();
    let layers = [AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 1.0, additive: false }];
    let initial = authored(&mesh, &layers, None).unwrap();
    let moved = authored(&mesh, &layers, Some(1.0 / 30.0)).unwrap();
    assert_ne!(moved.bones, initial.bones);
    assert_eq!(moved.positions[0], [11.0, 2.0, 0.0]);
    assert_eq!(authored(&mesh, &layers, Some(0.0)).unwrap(), initial);
}

#[test]
fn layered_spatial_palette() {
    let mut mesh = parse(&fixture()).unwrap();
    let child = Transform { translation: [0.0, 5.0, 0.0], rotation: [0.0; 3], scale: [1.0; 3] };
    mesh.bones.push(bone(Some(0), child));
    mesh.native_bind_inverse = pose::bind_inverse(&mesh.bones);
    mesh.animations[0].tracks.push(vec![child; 3]);
    let mut vertical = mesh.animations[0].tracks.clone();
    for (frame, track) in vertical[0].iter_mut().enumerate() {
        track.translation = [0.0, frame as f32 * 5.0, 0.0];
    }
    mesh.animations.push(Animation {
        id: 2,
        name: "second".into(),
        fps: 30.0,
        duration: 2,
        mirror: false,
        tracks: vertical,
        spatial: true,
        disabled: None,
        events: Vec::new(),
        morph: None,
        channels: channels::Clip::default(),
        alpha: alpha::Clip::default(),
        draw_order: alpha::Clip::default(),
    });
    let layers = [
        (
            AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 0.5, additive: false },
            1.0 / 30.0,
        ),
        (
            AnimationLayer { visible: true, id: 2, rate: 1.0, blend: 0.5, additive: false },
            1.0 / 30.0,
        ),
    ];
    let rest = rest_palette(&mesh).unwrap();
    assert_eq!(rest.len(), 2);
    let palette = palette_layers(&mesh, &layers).unwrap();
    let root = palette[0].unwrap().point([0.0; 3]);
    assert!((root[0] - 2.5).abs() < 0.001);
    assert!((root[1] - 2.5).abs() < 0.001);
    assert_ne!(palette, rest);
    let hidden = [(AnimationLayer { blend: 0.0, ..layers[0].0 }, layers[0].1)];
    assert_eq!(palette_layers(&mesh, &hidden).unwrap(), rest);
}

#[test]
fn authored_script_playback() {
    let mut scene = authored_scene();
    scene["objects"][0]["animationlayers"][0]["name"] = serde_json::json!("move");
    scene["objects"][0]["alpha"] = serde_json::json!({"value":1,"script":
        "export function init(){thisLayer.getAnimationLayer('move').setFrame(1);}"});
    let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let host = model.scripts.as_ref().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let puppet = model.layers[0].puppet.as_ref().unwrap();
    let Some(playback::Sample::Frame(seconds)) = host.puppet_sample("1", 1) else {
        panic!("live sample");
    };
    let actual = authored(&puppet.mesh, &puppet.layers, Some(seconds)).unwrap();
    let initial = authored(&puppet.mesh, &puppet.layers, None).unwrap();
    assert_ne!(actual.bones, initial.bones);
}

#[test]
fn retained_native_palette() {
    let mut mesh = parse(include_bytes!("fixtures/a02/clock-step.mdl")).unwrap();
    let layer = AnimationLayer { visible: true, id: 220, rate: 0.0, blend: 1.0, additive: false };
    let seconds = [0.0, 36.0, 120.0, 840.0, 1378.0].map(|frame| frame * (1.0_f32 / 60.0));
    let expected = seconds.map(|time| authored(&mesh, &[layer], Some(time)).unwrap().bones);
    assert!(mesh.animations.len() > 1);
    assert!(!mesh.vertices.is_empty());
    retain_animation(&mut mesh, layer.id).unwrap();
    assert_eq!(mesh.animations.len(), 1);
    assert_eq!(mesh.animations[0].id, layer.id);
    assert!(mesh.vertices.is_empty());
    assert!(mesh.raw_vertices.is_none());
    assert!(mesh.indices.is_empty());
    assert!(mesh.bind_inverse.is_empty());
    assert_ne!(expected[0], expected[2]);
    for (time, expected) in seconds.into_iter().zip(expected) {
        assert_eq!(palette(&mesh, layer, time).unwrap(), expected);
    }
}

#[test]
fn native_rest_palette() {
    let mut mesh = parse(include_bytes!("fixtures/a02/clock-step.mdl")).unwrap();
    retain_animation(&mut mesh, 220).unwrap();
    assert_eq!(
        rest_palette(&mesh).unwrap().as_ref(),
        &[Some(BonePose::Spatial([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]))]
    );
}

#[test]
#[ignore = "requires exact native C05 authored package"]
fn weighted_native_package() {
    use sha2::{Digest, Sha256};
    const PACKAGE_ENV: &str = "SKWD_WEIGHTED_PUPPET_PACKAGE";
    const PACKAGE_SHA: &str = "f12706b717a6fc801408592d3b41afa0208c3b12dedf1e0e1adc2bc5ac646c3d";
    let bytes = std::fs::read(std::env::var_os(PACKAGE_ENV).expect("weighted package")).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), PACKAGE_SHA);
    let package = crate::pkg::Package::parse(bytes).unwrap();
    let model = crate::model::load(&package).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers.len(), 1);
    let puppet = model.layers[0].puppet.as_ref().unwrap();
    let skin = model.layers[0].material.as_ref().unwrap().skin.as_ref().unwrap();
    assert_eq!(skin.palette.bones.len(), 2);
    assert_eq!(puppet.layers[0].rate, 0.0);
    assert!(puppet.mesh.animations.iter().any(|clip| clip.id == 341 && clip.mirror));
    assert_eq!(skin.vertices.len(), 185 * VERTEX_STRIDE);
    assert_eq!(skin.indices.len(), 342 * 3);
    assert!(puppet.mesh.raw_vertices.is_none());
    assert!(puppet.mesh.indices.is_empty());
    assert!(model.scripts.as_ref().unwrap().puppet_sample("182", 341).is_some());
    let vertex = puppet.mesh.vertices.iter().find(|vertex| vertex.weights[1] > 0.0).unwrap();
    assert_ne!(skin.palette.bones[0], skin.palette.bones[1]);
    assert_ne!(skin.palette.bones[1].unwrap().point(vertex.position), vertex.position);
}

pub(crate) fn empty_old_fixture() -> Vec<u8> {
    let modern = fixture();
    let start = find(&modern, SKELETON_SIGNATURE_4).unwrap();
    let end = find(&modern, b"MDLA0006\0").unwrap();
    let mut skeleton = modern[start..end].to_vec();
    skeleton[..OLD_SKELETON_SIGNATURE.len()].copy_from_slice(OLD_SKELETON_SIGNATURE);
    let mut bytes = MAGIC_13.to_vec();
    bytes.push(0);
    bytes.extend(OLD_MESH_HEADER.into_iter().flat_map(u32::to_le_bytes));
    bytes.extend_from_slice(b"materials/test.json\0");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&(OLD_VERTEX_STRIDE as u32 * 3).to_le_bytes());
    for x in [0.0_f32, 1.0, 2.0] {
        bytes.extend([x, 0.0, 0.0].into_iter().flat_map(f32::to_le_bytes));
        bytes.extend([0_u32; INFLUENCES].into_iter().flat_map(u32::to_le_bytes));
        bytes.extend([1.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0].into_iter().flat_map(f32::to_le_bytes));
    }
    bytes.extend_from_slice(&6_u32.to_le_bytes());
    bytes.extend([0_u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
    let animation = bytes.len() + skeleton.len();
    let offset = OLD_SKELETON_SIGNATURE.len();
    skeleton[offset..offset + size_of::<u32>()].copy_from_slice(&(animation as u32).to_le_bytes());
    bytes.extend(skeleton);
    bytes.resize(animation + 8, 0);
    bytes
}

#[test]
fn empty_old_animation() {
    let original = empty_old_fixture();
    let skeleton = find(&original, OLD_SKELETON_SIGNATURE).unwrap();
    let animation = le_u32(&original, skeleton + OLD_SKELETON_SIGNATURE.len()).unwrap() as usize;
    assert!(parse(&original).unwrap().animations.is_empty());
    for padding in [0, OLD_MODEL_PADDING] {
        let mut bytes = original.clone();
        bytes[animation + 1..].fill(padding);
        assert!(parse(&bytes).unwrap().animations.is_empty());
        bytes[animation + 1] = 1;
        assert!(parse(&bytes).is_err());
    }
    assert!(parse(&original[..animation]).is_err());
    assert!(parse(&original[..=animation]).unwrap().animations.is_empty());
    let mut modern = fixture();
    let animation = find(&modern, b"MDLA0006\0").unwrap();
    modern[animation..].fill(0);
    assert!(parse(&modern).is_err());
}

#[test]
#[ignore = "requires original Wallpaper Engine jelly sample"]
fn native_empty_animation() {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_JELLY").unwrap());
    let bytes = std::fs::read(root.join("models/jelly_simple_puppet.mdl")).unwrap();
    let mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.bones.len(), 2);
    assert_eq!(mesh.skipped_constraints, 0);
    assert_eq!(mesh.springs.len(), 1);
    assert!(mesh.animations.is_empty());
    assert!(skin(&mesh, &[], 0.0).positions.iter().flatten().all(|value| value.is_finite()));
}

pub(crate) fn hierarchy_fixture() -> Mesh {
    let mut mesh = parse(&empty_old_fixture()).unwrap();
    for (name, parent, x) in [("child", Some(0), 5.0), ("sibling", None, 20.0)] {
        let matrix = crate::matrix::model_matrix([x, 0.0, 0.0], [1.0; 3], [0.0; 3]);
        mesh.bones
            .push(crate::puppet::Bone::from_matrix(name.into(), parent, matrix, false).unwrap());
    }
    mesh.native_bind_inverse = pose::bind_inverse(&mesh.bones);
    mesh
}

#[test]
fn spatial_image_admission() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for angles in ["0.5 0 0", "0 0.5 0", "0.25 -0.5 0.125"] {
        let mut scene = authored_scene();
        scene["objects"][0]["angles"] = serde_json::json!(angles);
        scene["objects"][0]["scale"] = serde_json::json!("2 3 4");
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.layers[0].geometry.unwrap().spatial.is_some());
        assert!(model.layers[0].material.as_ref().unwrap().skin.is_some());
        for (field, value) in [
            ("parent", serde_json::json!(2)),
            ("attachment", serde_json::json!("Attachment1")),
            ("alignment", serde_json::json!("topleft")),
        ] {
            let mut restricted = scene.clone();
            restricted["objects"][0][field] = value;
            let package = authored_package(&restricted, &serde_json::json!({}), AUTHORED_VERTEX);
            let model = crate::model::load_with(&package, &assets).unwrap();
            assert!(model.layers.is_empty(), "{field}");
            assert!(
                model
                    .skipped
                    .iter()
                    .any(|reason| reason.contains("puppet material layer state unsupported"))
            );
        }
    }
}

fn constrained_fixture(metadata: &serde_json::Value) -> Vec<u8> {
    let mut bytes = empty_old_fixture();
    let skeleton = find(&bytes, OLD_SKELETON_SIGNATURE).unwrap();
    let offset = skeleton + OLD_SKELETON_SIGNATURE.len();
    let animation = le_u32(&bytes, offset).unwrap() as usize;
    let mut metadata = serde_json::to_vec(metadata).unwrap();
    metadata.push(0);
    bytes.splice(animation - 1..animation, metadata.iter().copied());
    bytes[offset..offset + 4]
        .copy_from_slice(&((animation + metadata.len() - 1) as u32).to_le_bytes());
    bytes
}

#[test]
fn translation_constraint_parser() {
    let accepted = serde_json::json!({"se":true,"t":true,"ts":8,"tf":2,"ti":50,"tm":0});
    for (extra, active) in [
        (None, true),
        (Some("r"), false),
        (Some("re"), true),
        (Some("ik"), false),
        (Some("ikce"), false),
        (Some("ge"), false),
        (Some("lt"), true),
        (Some("la"), true),
    ] {
        let mut metadata = accepted.clone();
        if let Some(key) = extra {
            metadata[key] = serde_json::json!(true);
        }
        let mesh = parse(&constrained_fixture(&metadata)).unwrap();
        assert_eq!(mesh.springs.len(), usize::from(active), "{extra:?}");
        assert_eq!(mesh.skipped_constraints, usize::from(!active), "{extra:?}");
        assert!(mesh.animations.is_empty());
    }
}

#[test]
fn authored_rest_material() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for hidden in [false, true] {
        let mut scene = authored_scene();
        if hidden {
            scene["objects"][0]["animationlayers"][0]["visible"] = serde_json::json!(false);
        } else {
            scene["objects"][0].as_object_mut().unwrap().remove("animationlayers");
        }
        let package = authored_package(&scene, &serde_json::json!({}), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let layer = &model.layers[0];
        let puppet = layer.puppet.as_ref().unwrap();
        let palette = &layer.material.as_ref().unwrap().skin.as_ref().unwrap().palette;
        let raw = palette_layers(&puppet.mesh, &[]).unwrap();
        assert_eq!(palette.bones, raw);
        if hidden {
            assert!(!puppet.layers[0].visible);
            assert_eq!(puppet.layers[0].blend, 1.0);
        } else {
            assert!(puppet.layers.is_empty());
        }
    }
}

fn unanimated(tail: &[u8]) -> Vec<u8> {
    let mut bytes = fixture();
    let animation = find(&bytes, b"MDLA0006\0").unwrap();
    bytes.truncate(animation);
    bytes.extend_from_slice(tail);
    bytes
}

fn unanimated_extension() -> Vec<u8> {
    let mut bytes = unanimated(b"");
    let matrix: Vec<u8> = crate::matrix::IDENTITY.into_iter().flat_map(f32::to_le_bytes).collect();
    let end = bytes.len() + EXTENSION_SIGNATURE.len() + 2 * size_of::<u32>() + matrix.len();
    bytes.extend_from_slice(EXTENSION_SIGNATURE);
    bytes.extend_from_slice(&(end as u32).to_le_bytes());
    bytes.extend_from_slice(&(matrix.len() as u32).to_le_bytes());
    bytes.extend(matrix);
    bytes.push(0);
    bytes
}

#[test]
fn unanimated_puppets_parse() {
    for bytes in [unanimated(b"\0"), unanimated_extension()] {
        let mesh = parse(&bytes).unwrap();
        assert!(mesh.animations.is_empty());
        assert_eq!(mesh.bones.len(), 1);
    }
}

#[test]
fn unanimated_tails_rejected() {
    let mut truncated = unanimated_extension();
    truncated.pop();
    for bytes in [unanimated(b"\0\0"), unanimated(b"MDLE0003\0"), truncated] {
        assert!(parse(&bytes).is_err());
    }
}

#[test]
fn version_17_parses() {
    let mut bytes = unanimated(b"\0");
    bytes[..MAGIC_17.len()].copy_from_slice(MAGIC_17);
    assert_eq!(parse(&bytes).unwrap(), parse(&unanimated(b"\0")).unwrap());
}

fn still_fixture() -> Vec<u8> {
    let mut bytes = b"MDLV0021\0".to_vec();
    for value in STILL_HEADER {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"materials/a.json\0");
    bytes.extend_from_slice(&[0; 28]);
    bytes.extend_from_slice(&crate::model::mesh::POSITION_NORMAL_TANGENT_UV.to_le_bytes());
    bytes.extend_from_slice(&(48_u32 * 3).to_le_bytes());
    for (x, y) in [(0.0_f32, 0.0_f32), (4.0, 0.0), (0.0, 4.0)] {
        let mut vertex = vec![x, y, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0];
        vertex.extend([x / 4.0, y / 4.0]);
        bytes.extend(vertex.into_iter().flat_map(f32::to_le_bytes));
    }
    bytes.extend_from_slice(&6_u32.to_le_bytes());
    bytes.extend([0_u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
    bytes.extend_from_slice(&[0; 3]);
    bytes
}

fn word(bytes: &[u8], vertex: usize, offset: usize) -> f32 {
    le_f32(bytes, vertex * VERTEX_STRIDE + offset).unwrap()
}

#[test]
fn still_puppet_parses() {
    let mesh = parse(&still_fixture()).unwrap();
    assert_eq!(mesh.bones.len(), 1);
    assert_eq!(mesh.indices, [0, 1, 2]);
    assert_eq!(mesh.vertices[1].position, [4.0, 0.0, 0.0]);
    assert_eq!(mesh.vertices[2].uv, [0.0, 1.0]);
    assert!(mesh.vertices.iter().all(|vertex| vertex.weights == ROOT_WEIGHTS));
    let raw = mesh.raw_vertices.unwrap();
    assert_eq!(raw.len(), 3 * VERTEX_STRIDE);
    assert_eq!(word(&raw, 1, VERTEX_WEIGHTS), 1.0);
    assert_eq!(word(&raw, 1, VERTEX_UV), 1.0);
}

#[test]
fn still_checks_enforced() {
    let mut two_materials = still_fixture();
    two_materials[HEADER_AT + 4] = 2;
    let mut tail = still_fixture();
    *tail.last_mut().unwrap() = 1;
    let mut version = still_fixture();
    version[..MAGIC_19.len()].copy_from_slice(MAGIC_19);
    for bytes in [two_materials, tail, version] {
        assert!(parse(&bytes).is_err());
    }
}

fn old_layout_fixture(magic: &[u8], flags: u32) -> Vec<u8> {
    let old = empty_old_fixture();
    let field = find(&old, b"materials/test.json\0").unwrap() + b"materials/test.json\0".len();
    let skeleton = find(&old, OLD_SKELETON_SIGNATURE).unwrap();
    let mut bytes = magic.to_vec();
    bytes.extend_from_slice(&old[magic.len()..field]);
    bytes.extend_from_slice(&flags.to_le_bytes());
    let mut shift = 0;
    if magic == MAGIC_16 {
        bytes.extend_from_slice(&OLD_MESH_HEADER[0].to_le_bytes());
        shift = size_of::<u32>() as u32;
    }
    bytes.extend_from_slice(&old[field + size_of::<u32>()..skeleton]);
    let at = bytes.len();
    bytes.extend_from_slice(&old[skeleton..]);
    bytes[at..at + SKELETON_SIGNATURE_2.len()].copy_from_slice(SKELETON_SIGNATURE_2);
    let offset = at + SKELETON_SIGNATURE_2.len();
    let animation = le_u32(&bytes, offset).unwrap() + shift;
    bytes[offset..offset + size_of::<u32>()].copy_from_slice(&animation.to_le_bytes());
    bytes.truncate(animation as usize + 1);
    bytes
}

#[test]
fn old_layouts_parse() {
    let reference = parse(&empty_old_fixture()).unwrap();
    for (magic, flags) in [(MAGIC_14, 0), (MAGIC_16, alpha::ALPHA_FLAG)] {
        let mesh = parse(&old_layout_fixture(magic, flags)).unwrap();
        assert_eq!(mesh.vertices, reference.vertices);
        assert_eq!(mesh.indices, reference.indices);
        assert_eq!(mesh.alpha, flags != 0);
        assert!(mesh.animations.is_empty());
        let raw = mesh.raw_vertices.unwrap();
        assert_eq!(raw.len(), 3 * VERTEX_STRIDE);
        assert_eq!(word(&raw, 2, 0), 2.0);
        assert_eq!(word(&raw, 2, 5 * FLOAT_BYTES), 1.0);
        assert_eq!(word(&raw, 2, 9 * FLOAT_BYTES), 1.0);
        assert_eq!(word(&raw, 2, VERTEX_WEIGHTS), 1.0);
    }
}

#[test]
fn version_14_flags_rejected() {
    assert!(parse(&old_layout_fixture(MAGIC_14, alpha::ALPHA_FLAG)).is_err());
}

#[test]
fn old_root_flags_zero() {
    let mut bytes = empty_old_fixture();
    let skeleton = find(&bytes, OLD_SKELETON_SIGNATURE).unwrap();
    let name = skeleton + OLD_SKELETON_SIGNATURE.len() + 2 * size_of::<u32>();
    let flags = name + bytes[name..].iter().position(|byte| *byte == 0).unwrap() + 1;
    bytes[flags..flags + size_of::<u32>()].copy_from_slice(&0_u32.to_le_bytes());
    assert_eq!(parse(&bytes).unwrap().bones.len(), 1);
}

fn animation_section(magic: &str, tail: usize) -> Vec<u8> {
    let mut bytes = format!("{magic}\0").into_bytes();
    let end = bytes.len();
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(b"wave\0loop\0");
    bytes.extend_from_slice(&30.0_f32.to_le_bytes());
    for value in [1_u32, 0, 1, 0, 72] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for _ in 0..2 {
        let sample = [0.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        bytes.extend(sample.into_iter().flat_map(f32::to_le_bytes));
    }
    bytes.resize(bytes.len() + tail, 0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(b"{\"frame\":0,\"name\":\"step\"}\0");
    let total = bytes.len() as u32;
    bytes[end..end + size_of::<u32>()].copy_from_slice(&total.to_le_bytes());
    bytes
}

#[test]
fn clip_versions_parse() {
    for (magic, tail) in
        [("MDLA0002", 0), ("MDLA0003", 5), ("MDLA0004", 6), ("MDLA0005", 30), ("MDLA0006", 31)]
    {
        let bytes = animation_section(magic, tail);
        let animations = parse_animations(&bytes, 0, 1, 1, false).unwrap();
        assert_eq!(animations.len(), 1, "{magic}");
        assert_eq!(animations[0].events.len(), 1, "{magic}");
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Reject {
    None,
    Layer,
    Scene,
}

fn plain_package(
    fields: &serde_json::Value,
    pass: &serde_json::Value,
    mdl: &[u8],
    reject: Reject,
) -> crate::pkg::Package {
    let material = serde_json::json!({ "passes": [pass] }).to_string();
    let mut object = serde_json::json!({"id":1,"image":"models/image.json","size":"32 32",
        "origin":"32 32 0"});
    object.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
    if reject == Reject::Layer {
        object["alignment"] = serde_json::json!("center");
    }
    let mut scene = serde_json::json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[object]});
    if reject == Reject::Scene {
        scene["general"]["camerashake"] = serde_json::json!(true);
    }
    let scene = scene.to_string();
    let texture = crate::tests::build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene.as_bytes()),
        ("models/image.json", br#"{"material":"materials/a.json","puppet":"models/a.mdl"}"#),
        (AUTHORED_MODEL, mdl),
        ("materials/a.json", material.as_bytes()),
        ("materials/a.tex", &texture),
    ]))
    .unwrap()
}

#[test]
#[ignore = "requires Wallpaper Engine assets"]
fn fallback_cull_carried() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for (scale, cull) in [("1 1 1", "normal"), ("-1 1 1", "normal"), ("1 1 1", "nocull")] {
        let pass = serde_json::json!({"shader":"genericimage4","textures":["a"],"cullmode":cull});
        for reject in [Reject::None, Reject::Layer, Reject::Scene] {
            let package =
                plain_package(&serde_json::json!({"scale":scale}), &pass, &fixture(), reject);
            let model = crate::model::load_with(&package, &assets).unwrap();
            assert!(model.skipped.is_empty(), "{:?}", model.skipped);
            let layer = &model.layers[0];
            let authored = reject == Reject::None && cull == "normal";
            assert_eq!(layer.material.is_some(), authored, "{scale}");
            let puppet = layer.puppet.as_ref().unwrap();
            assert!(puppet.cpu);
            assert_eq!(puppet.cull, cull == "normal", "{scale}");
            assert_eq!(puppet.mesh.indices.is_empty(), authored);
        }
    }
}

#[test]
#[ignore = "requires Wallpaper Engine assets"]
fn puppet_shaders_plain() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for pass in [
        serde_json::json!({"shader":"genericimage3","textures":["a"],"combos":{"VERSION":2}}),
        serde_json::json!({"shader":"genericimage4","textures":["a"],"combos":{"LIGHTING":0}}),
    ] {
        let package = plain_package(&serde_json::json!({}), &pass, &fixture(), Reject::None);
        let model = crate::model::load_with(&package, &assets).unwrap();
        let layer = &model.layers[0];
        assert!(layer.material.as_ref().unwrap().skin.is_some(), "{pass}");
        assert!(layer.puppet.as_ref().unwrap().cpu);
    }
    for combos in [json_combo("LIGHTING"), json_combo("REFLECTION")] {
        let lit = serde_json::json!({"shader":"genericimage4","textures":["a"],"combos":combos});
        let package = plain_package(&serde_json::json!({}), &lit, &fixture(), Reject::Layer);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.layers[0].material.as_ref().unwrap().skin.is_some());
        assert!(!model.layers[0].puppet.as_ref().unwrap().cpu);
    }
}

fn json_combo(name: &str) -> serde_json::Value {
    serde_json::json!({ (name): 1 })
}

#[test]
fn lit_matrices_admitted() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for declaration in [
        "uniform mat3 g_NormalModelMatrix;",
        "uniform mat3 g_AltNormalModelMatrix;",
        "uniform mat4 g_AltModelMatrix;",
        "uniform mat4 g_AltViewProjectionMatrix;",
    ] {
        let source = format!("{declaration}\n{AUTHORED_VERTEX}");
        let package = authored_package(&authored_scene(), &serde_json::json!({}), &source);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{declaration}: {:?}", model.skipped);
    }
}

#[test]
fn lit_combos_admitted() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for name in ["LIGHTING", "REFLECTION", "NORMALMAP", "PBRMASKS", "TEX1FORMAT"] {
        let package = authored_package(&authored_scene(), &json_combo(name), AUTHORED_VERTEX);
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{name}: {:?}", model.skipped);
    }
}

#[test]
fn lit_puppet_names_combo() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let package =
        authored_package(&authored_scene(), &json_combo("UNKNOWN_FEATURE"), AUTHORED_VERTEX);
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped[0].contains("UNKNOWN_FEATURE=1"), "{:?}", model.skipped);
}

#[test]
#[ignore = "requires Wallpaper Engine assets"]
fn skinned_material_uncopyable() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut mdl = fixture();
    mdl[22..26].copy_from_slice(&alpha::ALPHA_FLAG.to_le_bytes());
    let pass = serde_json::json!({"shader":"genericimage4","textures":["a"]});
    let package = plain_package(&serde_json::json!({}), &pass, &mdl, Reject::None);
    let model = crate::model::load_with(&package, &assets).unwrap();
    let material = model.layers[0].material.as_ref().unwrap();
    assert!(material.texture_copy && material.skin.is_some());
    assert!(!material.copyable());
}

#[test]
#[ignore = "requires Wallpaper Engine assets"]
fn alpha_puppet_fallback() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut mdl = fixture();
    mdl[22..26].copy_from_slice(&alpha::ALPHA_FLAG.to_le_bytes());
    let pass = serde_json::json!({"shader":"genericimage4","textures":["a"],"cullmode":"normal"});
    let package = plain_package(&serde_json::json!({}), &pass, &mdl, Reject::None);
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.layers[0].material.as_ref().unwrap().skin.is_some());
    let package = plain_package(&serde_json::json!({}), &pass, &mdl, Reject::Layer);
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.layers[0].material.is_none());
    let puppet = model.layers[0].puppet.as_ref().unwrap();
    assert!(puppet.mesh.alpha && puppet.cull);
}

#[test]
fn spring_owner_unsimulated() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let spring = serde_json::json!({"se":true,"t":true,"ts":8,"tf":2,"ti":50,"tm":0});
    let pass = serde_json::json!({"shader":"genericimage4","textures":["a"]});
    let tilted = serde_json::json!({"angles":"0.5 0 0"});
    let package = plain_package(&tilted, &pass, &constrained_fixture(&spring), Reject::None);
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert_eq!(model.skipped, ["object: puppet spring owner unsupported"]);
    assert!(model.layers[0].puppet.as_ref().unwrap().mesh.springs.is_empty());
}

fn masked_mdl(texture: &str) -> Vec<u8> {
    let mut bytes = include_bytes!("fixtures/draw-order.mdl").to_vec();
    let skeleton = find(&bytes, SKELETON_SIGNATURE_4).unwrap();
    let mut records = 1_u32.to_le_bytes().to_vec();
    records.extend(masks::tests::record(texture, 0, &[1], &[0]));
    bytes.splice(skeleton..skeleton, records.iter().copied());
    let shift = records.len() as u32;
    for (signature, word) in [(SKELETON_SIGNATURE_4, 0), (b"MDLA0006\0".as_slice(), 0)] {
        let at = find(&bytes, signature).unwrap() + signature.len() + word;
        let value = le_u32(&bytes, at).unwrap() + shift;
        bytes[at..at + size_of::<u32>()].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn masked(texture: &str) -> crate::model::SceneModel {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let pass = serde_json::json!({"shader":"genericimage4","textures":["a"]});
    let package = plain_package(&serde_json::json!({}), &pass, &masked_mdl(texture), Reject::None);
    crate::model::load_with(&package, &assets).unwrap()
}

#[test]
fn mask_reduction() {
    let model = masked("masks/eye");
    assert_eq!(model.skipped, ["object: 1 puppet clipping masks unsupported"]);
    assert_eq!(model.layers[0].puppet.as_ref().unwrap().mesh.masks.records[0].base, [0]);
}

#[test]
fn mask_textures_load() {
    let model = masked("a");
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers[0].puppet.as_ref().unwrap().mask_textures.len(), 1);
}

#[test]
fn invalid_draw_reduction() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let pass = serde_json::json!({"shader":"genericimage4","textures":["a"]});
    let object = serde_json::json!({});
    let mut bytes = include_bytes!("fixtures/draw-order.mdl").to_vec();
    let skeleton = find(&bytes, SKELETON_SIGNATURE_4).unwrap();
    let overlapped = skeleton - size_of::<[u32; 2]>();
    bytes[overlapped..overlapped + size_of::<u32>()].copy_from_slice(&0_u32.to_le_bytes());
    let package = plain_package(&object, &pass, &bytes, Reject::None);
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert_eq!(model.skipped, ["object: puppet draw sections invalid"]);
    assert!(model.layers[0].puppet.as_ref().unwrap().mesh.draw_order.is_none());
}

#[test]
fn skinned_order_keeps_indices() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mdl = include_bytes!("fixtures/draw-order.mdl");
    let package = packaged(mdl, &authored_scene(), &serde_json::json!({}), AUTHORED_VERTEX);
    let mut model = crate::model::load_with(&package, &assets).unwrap();
    let layer = &mut model.layers[0];
    assert_eq!(layer.material.as_ref().unwrap().skin.as_ref().unwrap().indices, [0, 1, 2]);
    let puppet = layer.puppet.as_mut().unwrap();
    let ids: Vec<_> = puppet.mesh.animations.iter().map(|clip| clip.id).collect();
    retain_animations(&mut puppet.mesh, &ids).unwrap();
    assert!(draw_order::Sorter::of(&puppet.mesh).is_some());
}

#[test]
fn unflagged_tracks_dropped() {
    let samples = parse(&fixture()).unwrap().animations[0].duration as usize + 1;
    let events_count = size_of::<u32>();
    let mut bytes = fixture();
    let animation = find(&bytes, b"MDLA0006\0").unwrap() + b"MDLA0006\0".len();
    let marker = bytes.len() - events_count - 1;
    bytes[marker] = 1;
    let mut record = Vec::new();
    record.extend_from_slice(&0_u32.to_le_bytes());
    record.extend_from_slice(&((samples * size_of::<f32>()) as u32).to_le_bytes());
    record.resize(record.len() + samples * size_of::<f32>(), 0);
    let insert = marker + 1;
    bytes.splice(insert..insert, record.iter().copied());
    let end = bytes.len() as u32;
    bytes[animation..animation + size_of::<u32>()].copy_from_slice(&end.to_le_bytes());
    assert!(parse(&bytes).unwrap().animations[0].draw_order.tracks.is_empty());
}

#[test]
fn cpu_rest_additive() {
    let mut mesh = parse(&fixture()).unwrap();
    let raw = crate::matrix::model_matrix([3.0, 4.0, 0.0], [1.0; 3], [0.0; 3]);
    mesh.bones[0] = Bone::from_matrix("root".into(), None, raw, false).unwrap();
    mesh.bones[0]
        .set_rest(crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]), false)
        .unwrap();
    let base = AnimationLayer { visible: true, id: 1, rate: 1.0, blend: 0.5, additive: false };
    let additive = AnimationLayer { additive: true, blend: 1.0, ..base };
    let alone = skin_samples(&mesh, &[(base, 1.0 / 30.0), (base, 0.0)]);
    let both = skin_samples(&mesh, &[(base, 1.0 / 30.0), (base, 0.0), (additive, 0.0)]);
    for (expected, actual) in alone.positions.iter().flatten().zip(both.positions.iter().flatten())
    {
        assert!((expected - actual).abs() < 1e-4, "{expected} != {actual}");
    }
}

#[test]
fn bind_survives_gimbal_lock() {
    let matrices = [
        [
            -4.525_532e-9,
            -1.982_847_2e-6,
            1.0,
            0.0,
            1.0,
            1.673_458_3e-6,
            5.684_353e-13,
            0.0,
            -1.673_458_3e-6,
            1.0,
            1.982_847_2e-6,
            0.0,
            0.129_933_49,
            0.166_189_61,
            -7.450_581e-9,
            1.0,
        ],
        [
            -8.569_714e-10,
            1.701_140_5e-7,
            -2.0,
            0.0,
            0.999_999_9,
            -3.687_437e-6,
            -7.193_421e-9,
            0.0,
            3.687_437e-6,
            1.0,
            -1.701_140_2e-7,
            0.0,
            0.200_809_88,
            0.307_121_87,
            0.0,
            1.0,
        ],
    ];
    for matrix in matrices {
        let transform = super::native_bind_transform(matrix).unwrap();
        let rebuilt =
            crate::matrix::model_matrix(transform.translation, transform.scale, transform.rotation);
        for (value, expected) in rebuilt.iter().zip(matrix) {
            assert!((value - expected).abs() < 0.001, "{rebuilt:?}");
        }
    }
}

#[test]
fn bind_shear_boundary() {
    for (shear, sheared) in [(0.25_f32, true), (0.002, true), (0.0005, false)] {
        let mut matrix = crate::matrix::IDENTITY;
        matrix[4] = shear;
        assert_eq!(super::native_bind_transform(matrix).is_err(), sheared, "{shear}");
    }
}

#[test]
fn near_gimbal_keeps_decomposition() {
    for pitch in [1.5704_f32, 1.5698, -1.5704, 1.5] {
        let angles = [0.25, pitch, 0.5];
        let matrix = crate::matrix::model_matrix([0.0; 3], [1.5, 1.5, 0.8], angles);
        let transform = super::native_bind_transform(matrix).unwrap();
        assert!((transform.rotation[2] - angles[2]).abs() < 0.01, "{pitch}: {transform:?}");
        assert!((transform.rotation[0] - angles[0]).abs() < 0.01, "{pitch}: {transform:?}");
    }
}

#[test]
fn stale_tracks_skipped() {
    const AFTER_TRACKS: usize = 35;
    let clean = parse(&fixture()).unwrap();
    let samples = clean.animations[0].duration as usize + 1;
    let mut bytes = fixture();
    let animation = find(&bytes, b"MDLA0006\0").unwrap() + b"MDLA0006\0".len();
    let insert = bytes.len() - AFTER_TRACKS;
    let mut record = Vec::new();
    for _ in 0..2 {
        record.extend_from_slice(&1_u32.to_le_bytes());
        record.extend_from_slice(&((samples * 36) as u32).to_le_bytes());
        record.resize(record.len() + samples * 36, 0);
    }
    bytes.splice(insert..insert, record.iter().copied());
    let end = bytes.len() as u32;
    bytes[animation..animation + size_of::<u32>()].copy_from_slice(&end.to_le_bytes());
    let parsed = parse(&bytes).unwrap();
    assert_eq!(parsed.animations[0].tracks, clean.animations[0].tracks);
    assert_eq!(parsed.animations[0].duration, clean.animations[0].duration);
}

#[test]
fn skinned_masks_reduced() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let scene = authored_scene();
    let package = packaged(&masked_mdl("a"), &scene, &serde_json::json!({}), AUTHORED_VERTEX);
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert_eq!(model.skipped, ["object: 1 puppet clipping masks unsupported"]);
    let layer = &model.layers[0];
    assert!(layer.material.as_ref().unwrap().skin.is_some());
    assert!(layer.puppet.as_ref().unwrap().mask_textures.is_empty());
}
