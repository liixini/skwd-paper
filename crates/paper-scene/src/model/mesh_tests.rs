use super::*;

fn fixture(version: &str, format: u32, index: u16, x: f32) -> Vec<u8> {
    let mut data = version.as_bytes().to_vec();
    data.push(0);
    for value in [format, 2, 1] {
        data.extend(value.to_le_bytes());
    }
    data.extend(b"materials/a.json\0materials/b.json\0");
    data.extend(0_u32.to_le_bytes());
    if version == VERSION17 || version == VERSION23 {
        for value in [0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0] {
            data.extend(value.to_le_bytes());
        }
    }
    if version != VERSION4 && version != VERSION14 {
        data.extend(format.to_le_bytes());
    }
    let mut vertices = Vec::new();
    for position in [[x, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position {
            vertices.extend(value.to_le_bytes());
        }
        if format != POSITION_UV {
            for value in [0.0_f32, 0.0, 1.0] {
                vertices.extend(value.to_le_bytes());
            }
        }
        if format == POSITION_NORMAL_TANGENT_UV || format == POSITION_NORMAL_TANGENT_UVS {
            for value in [1.0_f32, 0.0, 0.0, 1.0] {
                vertices.extend(value.to_le_bytes());
            }
        }
        for value in [position[0], position[1]] {
            vertices.extend(value.to_le_bytes());
        }
        if format == POSITION_NORMAL_TANGENT_UVS {
            for value in [position[1], position[0]] {
                vertices.extend(value.to_le_bytes());
            }
        }
    }
    data.extend(u32::try_from(vertices.len()).unwrap().to_le_bytes());
    data.extend(vertices);
    data.extend(6_u32.to_le_bytes());
    for value in [0_u16, 1, index] {
        data.extend(value.to_le_bytes());
    }
    if version == VERSION23 {
        data.extend([0; PART_TRAILER_BYTES]);
    }
    if version != VERSION4 {
        data.push(0);
    }
    data
}

#[test]
fn versions_and_layouts() {
    for version in [VERSION4, VERSION14, VERSION16, VERSION17, VERSION23] {
        for format in [
            POSITION_UV,
            POSITION_NORMAL_UV,
            POSITION_NORMAL_TANGENT_UV,
            POSITION_NORMAL_TANGENT_UVS,
        ] {
            let mesh = parse(&fixture(version, format, 2, 0.0)).unwrap();
            assert_eq!(mesh.skins, 2);
            let part = &mesh.parts[0];
            assert_eq!(part.materials, ["materials/a.json", "materials/b.json"]);
            assert_eq!(part.indices, Indices::U16(vec![0, 1, 2]));
            assert_eq!(part.vertices[1].position, [1.0, 0.0, 0.0]);
            assert_eq!(part.vertices[1].uv, [1.0, 0.0]);
            assert_eq!(part.vertices[1].normal.is_some(), format != POSITION_UV);
            assert_eq!(
                part.vertices[1].tangent.is_some(),
                format == POSITION_NORMAL_TANGENT_UV || format == POSITION_NORMAL_TANGENT_UVS
            );
            assert_eq!(
                part.vertices[1].secondary_uv.is_some(),
                format == POSITION_NORMAL_TANGENT_UVS
            );
            if format == POSITION_UV || format == POSITION_NORMAL_UV {
                assert_eq!(part.format, VertexFormat::Standard);
            }
            if format == POSITION_NORMAL_TANGENT_UV {
                assert_eq!(part.format, VertexFormat::Tangent);
                assert_eq!(part.vertices[1].tangent, Some([1.0, 0.0, 0.0, 1.0]));
                assert_eq!(part.vertices[1].secondary_uv, None);
            }
            if format == POSITION_NORMAL_TANGENT_UVS {
                assert_eq!(part.format, VertexFormat::TangentLightmap);
                assert_eq!(part.vertices[1].secondary_uv, Some([0.0, 1.0]));
            }
            assert_eq!(part.bounds.is_some(), version == VERSION17 || version == VERSION23);
        }
    }
}

#[test]
fn distinct_version_layouts() {
    for (version, layout) in [
        (VERSION14, VERSION16),
        (VERSION16, VERSION14),
        (VERSION16, VERSION17),
        (VERSION17, VERSION16),
        (VERSION17, VERSION23),
        (VERSION23, VERSION17),
    ] {
        let mut data = fixture(layout, POSITION_UV, 2, 0.0);
        data[..version.len()].copy_from_slice(version.as_bytes());
        assert!(parse(&data).is_err(), "{version} as {layout}");
    }
}

#[test]
fn truncated_tangent_mesh() {
    let bytes = fixture(VERSION23, POSITION_NORMAL_TANGENT_UV, 2, 0.0);
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_err(), "prefix {end}");
    }
}

#[test]
fn invalid_mesh_data() {
    let bytes = fixture(VERSION23, POSITION_UV, 2, 0.0);
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_err(), "prefix {end}");
    }
    assert!(parse(&fixture(VERSION23, POSITION_UV, 3, 0.0)).is_err());
    assert!(parse(&fixture(VERSION23, POSITION_UV, 2, f32::NAN)).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(parse(&trailing).is_err());
}

fn shipped_root() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap())
        .join(".local/share/Steam/steamapps/common/wallpaper_engine")
}

#[test]
#[ignore = "requires original Wallpaper Engine shipped projects"]
fn shipped_meshes() {
    let root = shipped_root().join("projects/defaultprojects");
    for path in [
        "techno/models/techno/glow.mdl",
        "techno/models/techno/orbitsmall.mdl",
        "techno/models/techno/rays.mdl",
        "techno/models/techno/sphere.mdl",
        "audiophile/models/audiophile/bars.mdl",
        "audiophile/models/audiophile/flow.mdl",
        "audiophile/models/audiophile/glow.mdl",
        "audiophile/models/grid/grid.mdl",
        "dna_fragment/models/bg/bg.mdl",
        "dna_fragment/models/bg/bgfade.mdl",
        "dna_fragment/models/curve/curve.mdl",
        "dna_fragment/models/dna/dna.mdl",
    ] {
        let mesh = parse(&std::fs::read(root.join(path)).unwrap()).unwrap();
        assert!(!mesh.parts.is_empty(), "{path}");
        assert!(mesh.parts.iter().all(|part| !part.vertices.is_empty()), "{path}");
    }
    let arsenal = std::fs::read(root.join("arsenal/models/pistols/pistols.mdl")).unwrap();
    let mesh = parse(&arsenal).unwrap();
    assert_eq!(mesh.parts.len(), 6);
    assert!(mesh.parts.iter().all(|part| part.format == VertexFormat::TangentLightmap));
}

#[test]
#[ignore = "requires original Wallpaper Engine shipped projects"]
fn shipped_tangent_sphere() {
    let sphere = std::fs::read(
        shipped_root()
            .join("assets/scenes/particleelementpreviews/collisionmodel/models/sphere/sphere.mdl"),
    )
    .unwrap();
    let mesh = parse(&sphere).unwrap();
    assert_eq!(mesh.skins, 1);
    assert_eq!(mesh.parts.len(), 1);
    let part = &mesh.parts[0];
    assert_eq!(part.format, VertexFormat::Tangent);
    assert_eq!(part.vertices.len(), 559);
    assert_eq!(part.indices.len(), 2880);
    let Indices::U16(indices) = &part.indices else { panic!("narrow indices") };
    assert_eq!(&indices[2877..], [552, 1, 0]);
    let (min, max) = part.bounds.unwrap();
    assert_eq!(min.map(f32::to_bits), [0xc2c7_fff4, 0xc2c8_0000, 0xc2c8_0000]);
    assert_eq!(max.map(f32::to_bits), [0x42c7_fffc, 0x42c8_0000, 0x42c7_fff7]);
    for (index, expected) in [
        (
            0,
            [
                0x0000_0000,
                0x42a6_4b3d,
                0xc25e_3a69,
                0xb597_8e46,
                0x3f54_2227,
                0xbf0f_4d46,
                0xbf7f_f746,
                0xbc15_b2f8,
                0xbc5d_9204,
                0xbf80_0000,
                0x3f40_0000,
                0x3e40_0000,
            ],
        ),
        (
            278,
            [
                0xb548_0000,
                0x42c4_2834,
                0x419c_1271,
                0x30a3_fef9,
                0x3f7a_c6f4,
                0x3e4d_ca0c,
                0x3f7f_f3cf,
                0x3b7e_17cc,
                0xbc9a_d20c,
                0xbf80_0000,
                0x3e80_0000,
                0x3d7f_fff0,
            ],
        ),
        (
            558,
            [
                0x0000_0000,
                0xc2c8_0000,
                0x3788_aaf3,
                0x3370_2658,
                0xbf80_0000,
                0x3424_0115,
                0xbf7f_fb55,
                0xb377_f6e6,
                0xbc43_995a,
                0xbf80_0000,
                0x3f44_0000,
                0x3f80_0000,
            ],
        ),
    ] {
        let vertex = part.vertices[index];
        let bits = vertex
            .position
            .into_iter()
            .chain(vertex.normal.unwrap())
            .chain(vertex.tangent.unwrap())
            .chain(vertex.uv)
            .map(f32::to_bits)
            .collect::<Vec<_>>();
        assert_eq!(bits, expected);
        assert_eq!(vertex.secondary_uv, None);
    }
}

#[test]
fn procedural_triangle() {
    let shape: ModelData = serde_json::from_value(serde_json::json!({"shapes":[{"vertexBuffer":[0,60,0,0,0,1,0.5,1,-120,-60,0,0,0,1,0,0,120,-60,0,0,0,1,1,0],"vertexFormat":["position","normal","uv"],"material":"materials/red.json"}]})).unwrap();
    let mesh = shape.mesh().unwrap();
    assert_eq!(mesh.parts[0].vertices[0].position, [0.0, 60.0, 0.0]);
    assert_eq!(mesh.parts[0].vertices[0].uv, [0.5, 1.0]);
    assert_eq!(mesh.parts[0].indices, Indices::U16(vec![0, 1, 2]));
}

#[test]
fn procedural_shape_validation() {
    for count in [0, 8, 23, 25] {
        let shape = ModelData {
            shapes: vec![Shape {
                vertex_buffer: vec![0.0; count],
                vertex_format: vec![Attribute::Position, Attribute::Normal, Attribute::Uv],
                material: "materials/red.json".into(),
            }],
        };
        assert!(shape.mesh().is_err());
    }
}

#[test]
#[ignore = "requires authored skeletal model fixture"]
fn authored_skeletal_model() {
    let path = std::env::var("SKWD_SKELETAL_MODEL").unwrap();
    let bytes = std::fs::read(path).unwrap();
    let mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.parts.len(), 2);
    assert_eq!(mesh.parts.iter().map(|part| part.vertices.len()).sum::<usize>(), 1141);
    assert!(
        mesh.parts.iter().all(|part| part.skin.as_ref().unwrap().len() == part.vertices.len() * 80)
    );
    let rig = mesh.rig.unwrap();
    assert_eq!(rig.bones.len(), 5);
    assert_eq!(rig.animations[0].id, 1380);
    let rest = crate::puppet::rest_palette(&rig).unwrap();
    let pose = crate::puppet::palette(
        &rig,
        crate::puppet::AnimationLayer {
            visible: true,
            id: 1380,
            rate: 0.0,
            blend: 1.0,
            additive: false,
        },
        0.0,
    )
    .unwrap();
    assert_ne!(rest, pose);
    let mut corrupt = bytes.clone();
    corrupt[160..164].copy_from_slice(&99_u32.to_le_bytes());
    assert!(parse(&corrupt).is_err());
    for end in [0, 21, 97231, 98061] {
        assert!(parse(&bytes[..end]).is_err());
    }
}

#[test]
fn wide_indices_parse() {
    let narrow = fixture(VERSION23, POSITION_UV, 2, 0.0);
    let flag = narrow.windows(4).position(|window| window == b"b.js").unwrap() + "b.json\0".len();
    let tail = narrow.len() - 1 - PART_TRAILER_BYTES - 3 * size_of::<u16>() - size_of::<u32>();
    let mut wide = narrow[..tail].to_vec();
    wide[flag..flag + 4].copy_from_slice(&WIDE_INDICES.to_le_bytes());
    wide.extend(12_u32.to_le_bytes());
    for value in [0_u32, 1, 2] {
        wide.extend(value.to_le_bytes());
    }
    wide.extend(&narrow[narrow.len() - 1 - PART_TRAILER_BYTES..]);
    let mesh = parse(&wide).unwrap();
    assert_eq!(mesh.parts[0].indices, Indices::U32(vec![0, 1, 2]));
    wide[flag..flag + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(parse(&wide).err().unwrap().to_string(), "mesh metadata unsupported");
    let mut short = narrow.clone();
    short[flag..flag + 4].copy_from_slice(&WIDE_INDICES.to_le_bytes());
    assert_eq!(parse(&short).err().unwrap().to_string(), "invalid mesh index bytes 6");
}

const SPRING: &str = r#"{"se":true,"t":true,"ts":200,"tf":10,"ti":30,"tm":200}"#;

fn skeletal(constraint: &str) -> Vec<u8> {
    let mut data = VERSION23.as_bytes().to_vec();
    data.push(0);
    for value in [SKINNED, 1, 1] {
        data.extend(value.to_le_bytes());
    }
    data.extend(b"materials/a.json\0");
    data.extend(0_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 1.0, 0.0] {
        data.extend(value.to_le_bytes());
    }
    data.extend(SKINNED.to_le_bytes());
    let stride = (POSITION_NORMAL_TANGENT_UV_FLOATS + SKIN_COMPONENTS) * size_of::<f32>();
    data.extend(u32::try_from(3 * stride).unwrap().to_le_bytes());
    for position in [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position.into_iter().chain([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0]) {
            data.extend(value.to_le_bytes());
        }
        data.extend([0_u32; 4].into_iter().flat_map(u32::to_le_bytes));
        for value in [1.0_f32, 0.0, 0.0, 0.0, position[0], position[1]] {
            data.extend(value.to_le_bytes());
        }
    }
    data.extend(6_u32.to_le_bytes());
    data.extend([0_u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
    data.extend([0; PART_TRAILER_BYTES]);
    data.extend(b"MDLS0004\0");
    let animation_offset = data.len();
    data.extend(0_u32.to_le_bytes());
    data.extend(1_u32.to_le_bytes());
    data.push(0);
    data.extend(1_u32.to_le_bytes());
    data.extend(u32::MAX.to_le_bytes());
    data.extend(64_u32.to_le_bytes());
    for value in crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]) {
        data.extend(value.to_le_bytes());
    }
    data.extend(constraint.as_bytes());
    data.push(0);
    let animation = u32::try_from(data.len()).unwrap();
    data[animation_offset..animation_offset + 4].copy_from_slice(&animation.to_le_bytes());
    data.extend(b"MDLA0006\0");
    let animation_end = data.len();
    for value in [0_u32, 1, 1, 0] {
        data.extend(value.to_le_bytes());
    }
    data.extend(b"move\0loop\0");
    data.extend(30.0_f32.to_le_bytes());
    for value in [2_u32, 0, 1, 0, 108] {
        data.extend(value.to_le_bytes());
    }
    for x in [0.0_f32, 10.0, 0.0] {
        for value in [x, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            data.extend(value.to_le_bytes());
        }
    }
    data.resize(data.len() + 35, 0);
    let end = u32::try_from(data.len()).unwrap();
    data[animation_end..animation_end + 4].copy_from_slice(&end.to_le_bytes());
    data.push(0);
    data
}

#[test]
fn spring_bones_parse() {
    let mesh = parse(&skeletal(SPRING)).unwrap();
    assert_eq!(mesh.rig.unwrap().springs.len(), 1);
    assert_eq!(
        parse(&skeletal(r#"{"ik":true}"#)).err().unwrap().to_string(),
        CONSTRAINTS_UNSUPPORTED
    );
}

fn skinned_model(mesh: &[u8], objects: &str) -> crate::model::SceneModel {
    let scene = format!(
        r#"{{"camera":{{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"}},"general":{{"fov":60,"nearz":0.1,"farz":100}},"objects":{objects}}}"#
    );
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene.as_bytes()),
        ("models/mesh.mdl", mesh),
        ("materials/a.json", br#"{"passes":[{"shader":"skin"}]}"#),
        ("shaders/skin.vert", b"#if SKINNING\nuniform mat4x3 g_Bones[BONECOUNT];\n#endif\nuniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nvoid main(){gl_Position=mul(vec4(mul(vec4(a_Position,1.0),g_Bones[a_BlendIndices.x])*a_BlendWeights.x,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/skin.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
    ]))
    .unwrap();
    crate::model::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap()
}

#[test]
fn spring_bones_animate() {
    let model =
        skinned_model(&skeletal(SPRING), r#"[{"id":1,"name":"rig","model":"models/mesh.mdl"}]"#);
    assert_eq!(model.meshes.len(), 1, "{:?}", model.skipped);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.meshes[0].animated());
}

#[test]
fn attachment_mounts_resolve() {
    let model = skinned_model(
        include_bytes!("../fixtures/mounted.mdl"),
        r#"[{"id":10,"name":"rig","model":"models/mesh.mdl"},{"id":1,"name":"hat","model":"models/mesh.mdl","parent":10,"attachment":"Grip"},{"id":2,"name":"lost","model":"models/mesh.mdl","parent":10,"attachment":"Missing"}]"#,
    );
    assert_eq!(model.skipped, ["lost: mesh attachment owner unavailable"]);
    let hat = model.meshes.iter().find(|mesh| mesh.id == "1").unwrap();
    assert_eq!(
        hat.mount,
        Some(crate::model::MeshMount { owner: "10".into(), attachment: "Grip".into(), depth: 0 })
    );
    assert_eq!(hat.parents.len(), 1);
}

#[test]
fn authored_clip_parses() {
    let authored = |length: u32| {
        let mut data = skeletal("");
        let header: Vec<u8> = [2_u32, 0, 1].into_iter().flat_map(u32::to_le_bytes).collect();
        let field = data.windows(header.len()).position(|window| window == header).unwrap() + 4;
        data[field..field + 4].copy_from_slice(&0x401_u32.to_le_bytes());
        let mut tail = 0_u32.to_le_bytes().to_vec();
        tail.extend(0_u16.to_le_bytes());
        for value in [length, 0, u32::MAX] {
            tail.extend(value.to_le_bytes());
        }
        let section = data.windows(9).position(|window| window == b"MDLA0006\0").unwrap() + 9;
        let end = u32::from_le_bytes(data[section..section + 4].try_into().unwrap());
        data[section..section + 4]
            .copy_from_slice(&(end + u32::try_from(tail.len()).unwrap()).to_le_bytes());
        let events = end as usize - 4;
        data.splice(events..events, tail);
        parse(&data)
    };
    assert!(authored(2).is_ok());
    assert_eq!(authored(3).err().unwrap().to_string(), "unsupported authored puppet clip");
}

fn swinging_rig() -> crate::puppet::sampler::Sampler {
    let rig = parse(&skeletal(SPRING)).unwrap().rig.unwrap();
    crate::puppet::sampler::Sampler::new(rig).unwrap()
}

fn yawed_owner(x: f32) -> crate::matrix::Mat4 {
    crate::matrix::model_matrix([x, 0.0, 0.0], [1.0; 3], [0.0, 0.9, 0.0])
}

#[test]
fn rotated_owner_swings_spring() {
    const STEP: f32 = 1.0 / 60.0;
    let mut rig = swinging_rig();
    rig.set_owner(yawed_owner(0.0));
    let rest = rig.sample(&[], 0.0).unwrap().matrices[0];
    rig.set_owner(yawed_owner(5.0));
    let moved = rig.sample(&[], STEP).unwrap().matrices[0];
    assert!(moved.iter().all(|value| value.is_finite()));
    assert_ne!(moved, rest);
    let revision = rig.sample(&[], STEP).unwrap().revision;
    assert_eq!(rig.sample(&[], STEP).unwrap().revision, revision);
    assert_ne!(rig.sample(&[], STEP * 2.0).unwrap().revision, revision);
}

#[test]
fn still_owner_settles_spring() {
    const STEP: f32 = 1.0 / 60.0;
    const SETTLE_STEPS: u16 = 1200;
    let mut rig = swinging_rig();
    rig.set_owner(yawed_owner(0.0));
    let rest = rig.sample(&[], 0.0).unwrap().matrices[0];
    rig.set_owner(yawed_owner(5.0));
    rig.sample(&[], STEP).unwrap();
    let swung = rig.sample(&[], STEP * 2.0).unwrap().matrices[0];
    let last = (0..SETTLE_STEPS)
        .map(|index| rig.sample(&[], STEP * (f32::from(index) + 3.0)).unwrap().matrices[0])
        .last()
        .unwrap();
    let distance = |matrix: &crate::matrix::Mat4| {
        matrix.iter().zip(&rest).map(|(left, right)| (left - right).abs()).sum::<f32>()
    };
    assert!(distance(&swung) > 0.0);
    assert!(distance(&last) < distance(&swung) * 0.01, "{} {}", distance(&last), distance(&swung));
}

#[test]
fn layer_change_restepped() {
    let mut rig = swinging_rig();
    let layer = crate::puppet::AnimationLayer {
        visible: false,
        id: 1,
        rate: 1.0,
        blend: 1.0,
        additive: false,
    };
    let first = rig.sample(&[], 0.0).unwrap().revision;
    assert_ne!(rig.sample(&[(layer, 0.0)], 0.0).unwrap().revision, first);
}

#[test]
fn long_gap_step_clamped() {
    let step = crate::puppet::spring::MAX_STEP_SECONDS;
    let swing = |gap: f32| {
        let mut rig = swinging_rig();
        rig.set_owner(yawed_owner(0.0));
        rig.sample(&[], 0.0).unwrap();
        rig.set_owner(yawed_owner(5.0));
        rig.sample(&[], gap).unwrap().matrices[0]
    };
    assert_eq!(swing(step), swing(600.0));
}

const NO_CONSTRAINT: &str = "";
const PART_FLAG_OFFSET: usize = 38;

#[test]
fn part_flag_keeps_width() {
    let mut data = skeletal(NO_CONSTRAINT);
    data[PART_FLAG_OFFSET..PART_FLAG_OFFSET + 4].copy_from_slice(&UNNAMED_PART_FLAG.to_le_bytes());
    let parts = parse(&data).unwrap().parts;
    assert!(matches!(parts[0].indices, Indices::U16(_)));
    data[PART_FLAG_OFFSET..PART_FLAG_OFFSET + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(parse(&data).is_err());
}

#[test]
fn version_21_short_trailer() {
    let mut data = skeletal(NO_CONSTRAINT);
    data[..VERSION23.len()].copy_from_slice(VERSION21.as_bytes());
    let skeleton = data.windows(4).position(|window| window == b"MDLS").unwrap();
    let removed = PART_TRAILER_BYTES - SHORT_PART_TRAILER_BYTES;
    data.drain(skeleton - removed..skeleton);
    let moved = removed as u32;
    for at in
        [skeleton - removed + 9, data.windows(4).position(|window| window == b"MDLA").unwrap() + 9]
    {
        let value = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) - moved;
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    assert!(parse(&data).unwrap().rig.is_some());
}

#[test]
fn morph_section_reported() {
    let mut data = skeletal(NO_CONSTRAINT);
    assert!(!parse(&data).unwrap().rig.unwrap().skipped_morphs);
    data.pop();
    let end = u32::try_from(data.len() + 9 + 4).unwrap();
    data.extend(b"MDMP0001\0");
    data.extend(end.to_le_bytes());
    data.push(0);
    assert!(parse(&data).unwrap().rig.unwrap().skipped_morphs);
    let model = skinned_model(&data, r#"[{"id":1,"name":"rig","model":"models/mesh.mdl"}]"#);
    assert_eq!(model.meshes.len(), 1);
    assert_eq!(model.skipped, [format!("rig: {}", crate::model::MESH_MORPH_GAP)]);
    let mut other = skeletal(NO_CONSTRAINT);
    other.pop();
    other.extend(b"OTHER");
    assert!(parse(&other).is_err());
    let mut trailing = data.clone();
    trailing.extend(b"MDLE0002");
    assert!(parse(&trailing).is_err());
    let mut bare = skeletal(NO_CONSTRAINT);
    bare.pop();
    bare.extend(b"MDMP");
    assert!(parse(&bare).is_err());
}
