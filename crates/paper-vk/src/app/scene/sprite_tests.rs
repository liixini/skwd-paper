use super::*;

fn fixture(playing: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":10,"name":"Group","alpha":{"user":"frame","value":1.0}},
            {"id":20,"image":"models/sprite.json","size":"64 64","origin":"32 32 0",
                "visible":{"value":true,"script":"export function update(value) { const a=thisLayer.getTextureAnimation(); if(engine.userProperties.playing) a.play(); else a.pause(); a.setFrame(engine.userProperties.frame); return value; }"}}
        ]
    })
    .to_string();
    let mut texture = Vec::new();
    texture.extend_from_slice(b"TEXV0005\0TEXI0001\0");
    for value in [0i32, paper_scene::tex::FLAG_IS_GIF as i32, 4, 4, 4, 2, 0] {
        texture.extend_from_slice(&value.to_le_bytes());
    }
    texture.extend_from_slice(b"TEXB0003\0");
    for value in [1i32, -1, 1, 4, 4, 0, 64, 64] {
        texture.extend_from_slice(&value.to_le_bytes());
    }
    for pixel in 0..16 {
        texture.extend_from_slice(if pixel < 8 { &[255, 0, 0, 255] } else { &[0, 255, 0, 255] });
    }
    texture.extend_from_slice(b"TEXS0003\0");
    for value in [2i32, 4, 2] {
        texture.extend_from_slice(&value.to_le_bytes());
    }
    for y in [0.0f32, 2.0] {
        texture.extend_from_slice(&0i32.to_le_bytes());
        for value in [1.0f32, 0.0, y, 4.0, 0.0, 0.0, 2.0] {
            texture.extend_from_slice(&value.to_le_bytes());
        }
    }
    let files: [(&str, &[u8]); 4] = [
        ("scene.json", scene.as_bytes()),
        ("models/sprite.json", br#"{"material":"materials/sprite.json"}"#),
        ("materials/sprite.json", br#"{"passes":[{"textures":["sprite"]}]}"#),
        ("materials/sprite.tex", &texture),
    ];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(b"PKGV0007");
    bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    for (name, data) in files {
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in files {
        bytes.extend_from_slice(data);
    }
    std::fs::write(root.path().join("scene.pkg"), bytes).unwrap();
    std::fs::write(
        root.path().join("project.json"),
        serde_json::json!({"general":{"properties":{
            "frame":{"type":"slider","value":1.0},
            "playing":{"type":"bool","value":playing}
        }}})
        .to_string(),
    )
    .unwrap();
    root
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_frames_present_immediately_with_original_object_identity() {
    for playing in [true, false] {
        let root = fixture(playing);
        let package = paper_scene::pkg::Package::open(&root.path().join("scene.pkg")).unwrap();
        let mut model = paper_scene::model::load_from_dir(&package, root.path()).unwrap();
        let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
        let mut group =
            build_group(&shared, &mut model, true, &[(64, 64)], paper_geom::FillMode::Fit).unwrap();
        assert_eq!(group.animations[0].object, Some(1));
        let pixel = |group: &mut Group| {
            let (width, _, bytes) = group.read_canvas().unwrap();
            let offset = ((32 * width + 32) * 4) as usize;
            [bytes[offset], bytes[offset + 1], bytes[offset + 2]]
        };
        assert_eq!(pixel(&mut group), [0, 255, 0]);
        let directory = root.path().to_string_lossy();
        let mut source = properties::PropertySource::new(&directory, &Default::default());
        for (frame, expected) in [(0.0, [255, 0, 0]), (1.0, [0, 255, 0])] {
            assert!(group.update_properties(
                &mut source,
                &directory,
                &paper_scene::model::Properties::from([("frame".into(), vec![frame])]),
                |_, _| {},
            ));
            assert_eq!(pixel(&mut group), expected, "playing={playing}, frame={frame}");
        }
        group.destroy();
    }
}
