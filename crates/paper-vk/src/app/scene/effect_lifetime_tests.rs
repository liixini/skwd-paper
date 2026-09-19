use super::*;

fn model(visible: serde_json::Value, fragment: &[u8]) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0",
            "size":"64 64","visible":visible,"effects":[{"file":"effects/test.json"}]}]
    })
    .to_string();
    let files: &[(&str, &[u8])] = &[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
        ("materials/test.json", br#"{"passes":[{"shader":"test"}]}"#),
        (
            "shaders/test.vert",
            b"attribute vec3 a_Position;\nvoid main(){\ngl_Position=vec4(a_Position,1.0);\n}",
        ),
        ("shaders/test.frag", fragment),
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
    let package = paper_scene::pkg::Package::parse(bytes).unwrap();
    paper_scene::model::load_with(&package, &paper_scene::effects::Assets::discover(None)).unwrap()
}

fn center(group: &mut Group) -> [u8; 4] {
    let (width, _, rgba) = group.read_canvas().unwrap();
    let offset = ((32 * width + 32) * 4) as usize;
    rgba[offset..offset + 4].try_into().unwrap()
}

#[test]
#[ignore = "requires Vulkan"]
fn audio_only_effect_updates_without_scripts_or_animation() {
    let mut model = model(
        serde_json::json!(true),
        b"uniform float g_AudioSpectrum16Left[16];\nvoid main(){\nfloat a=g_AudioSpectrum16Left[0];\ngl_FragColor=vec4(a,1.0-a,0.0,1.0);\n}",
    );
    assert!(model.scripts.is_none());
    assert!(model.layers[0].texture.atlas_frames().is_none());
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.audio = None;
    group.configure_scene_targets(true).unwrap();
    assert!(!group.frozen);
    assert_eq!(group.fx.len(), 1);
    assert!(!group.animated());
    for (time, level, expected) in
        [(1.0, 0.0, [0, 255, 0, 255]), (2.0, 1.0, [255, 0, 0, 255]), (3.0, 0.0, [0, 255, 0, 255])]
    {
        group.fx[0].uniforms.insert("g_AudioSpectrum16Left".into(), vec![level; 16]);
        group.compose(time, 1.0 / 60.0).unwrap();
        assert_eq!(center(&mut group), expected);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn initially_hidden_effect_keeps_its_targets_until_first_visible_frame() {
    let mut model = model(
        serde_json::json!({"value":false,"script":"export function update(v){return engine.runtime>=1.0;}"}),
        b"void main(){\ngl_FragColor=vec4(1.0);\n}",
    );
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert_eq!(group.fx.len(), 1);
    assert!(group.fx[0].output.is_none());
    assert!(group.effect_target_allocation_bytes() > 0);
    group.compose(1.5, 1.0 / 60.0).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(group.fx[0].output.is_some());
    group.destroy();
}
