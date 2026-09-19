use super::*;

#[test]
#[ignore = "requires Vulkan"]
fn audio_uniforms_follow_effect_layers_after_scene_target_sorting() {
    let files: &[(&str, &[u8])] = &[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64","visible":{"value":true,"script":"export function update(v){return v;}"},"effects":[{"file":"effects/plain.json"}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64","effects":[{"file":"effects/audio.json"}]}
        ]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/plain.json", br#"{"passes":[{"material":"materials/plain.json"}]}"#),
        ("effects/audio.json", br#"{"passes":[{"material":"materials/audio.json"}]}"#),
        ("materials/plain.json", br#"{"passes":[{"shader":"plain"}]}"#),
        ("materials/audio.json", br#"{"passes":[{"shader":"audio"}]}"#),
        ("shaders/plain.vert", b"attribute vec3 a_Position;\nvoid main(){\ngl_Position=vec4(a_Position,1.0);\n}"),
        ("shaders/audio.vert", b"attribute vec3 a_Position;\nvoid main(){\ngl_Position=vec4(a_Position,1.0);\n}"),
        ("shaders/plain.frag", b"void main(){\ngl_FragColor=vec4(1.0);\n}"),
        ("shaders/audio.frag", b"uniform float g_AudioSpectrum16Left[16];\nvoid main(){\nfloat a=g_AudioSpectrum16Left[0];\ngl_FragColor=vec4(a,1.0-a,0.0,1.0);\n}"),
    ];
    let mut bytes = Vec::new();
    let text = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    text(&mut bytes, "PKGV0007");
    bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    for (name, value) in files {
        text(&mut bytes, name);
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        offset += value.len() as u32;
    }
    for (_, value) in files {
        bytes.extend_from_slice(value);
    }
    let package = paper_scene::pkg::Package::parse(bytes).unwrap();
    let assets = paper_scene::effects::Assets::discover(None);
    let mut model = paper_scene::model::load_with(&package, &assets).unwrap();
    model.layers.reverse();
    assert_eq!(model.layers.iter().map(|layer| layer.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.audio = None;
    assert_eq!(group.fx.iter().map(|fx| fx.layer_id.as_str()).collect::<Vec<_>>(), ["1", "2"]);
    for fx in &mut group.fx {
        fx.uniforms.insert("g_AudioSpectrum16Left".into(), vec![1.0; 16]);
    }
    group.apply_audio_bands();
    assert_eq!(group.fx[0].uniforms["g_AudioSpectrum16Left"], [1.0; 16]);
    assert_eq!(group.fx[1].uniforms["g_AudioSpectrum16Left"], [0.0; 16]);
    group.compose(0.0, 0.0).unwrap();
    let (width, _, rgba) = group.read_canvas().unwrap();
    let offset = (32 * width as usize + 48) * 4;
    assert_eq!(&rgba[offset..offset + 4], &[0, 255, 0, 255]);
    group.fx.reverse();
    group.fx[0].uniforms.insert("g_AudioSpectrum16Left".into(), vec![1.0; 16]);
    group.apply_audio_bands();
    assert_eq!(group.fx[0].uniforms["g_AudioSpectrum16Left"], [0.0; 16]);
    assert_eq!(group.fx[1].uniforms["g_AudioSpectrum16Left"], [1.0; 16]);
    group.destroy();
}
