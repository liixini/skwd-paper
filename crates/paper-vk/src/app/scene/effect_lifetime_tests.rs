use super::*;

fn model(visible: serde_json::Value, fragment: &[u8]) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0",
            "size":"64 64","visible":visible,"effects":[{"file":"effects/test.json"}]}]
    });
    model_scene(scene, fragment)
}

fn model_scene(scene: serde_json::Value, fragment: &[u8]) -> SceneModel {
    let scene = scene.to_string();
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
fn packed_audio_spectrum_reads_the_requested_band_and_updates_pixels() {
    let mut model = model(
        serde_json::json!(true),
        b"uniform float g_AudioSpectrum16Left[16];\nvoid main(){\nfloat i=5.0;\nfloat a=g_AudioSpectrum16Left[i/4][i%4];\ngl_FragColor=vec4(a,1.0-a,0.0,1.0);\n}",
    );
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.audio = None;
    group.configure_scene_targets(true).unwrap();
    for (time, band, expected) in
        [(1.0, 1, [0, 255, 0, 255]), (2.0, 5, [255, 0, 0, 255]), (3.0, 6, [0, 255, 0, 255])]
    {
        let mut values = vec![0.0; 16];
        values[band] = 1.0;
        group.fx[0].uniforms.insert("g_AudioSpectrum16Left".into(), values);
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

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_effect_opacity_renders_without_a_script_runtime_and_settles() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{"alpha":{
                "value":0,"animation":{"options":{"fps":30,"length":150,"mode":"single"},
                "c0":[{"frame":0,"value":0},{"frame":63,"value":0},{"frame":150,"value":1}]}
            }}}]}]}]
    });
    let mut model = model_scene(scene, b"uniform float g_UserAlpha; // {\"material\":\"alpha\",\"default\":1.0}\nvoid main(){gl_FragColor=vec4(g_UserAlpha,g_UserAlpha,g_UserAlpha,1.0);}");
    assert!(model.scripts.is_none());
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert!(group.animated());
    group.compose(3.55, 0.1).unwrap();
    let mid = center(&mut group);
    assert!((127..=128).contains(&mid[0]), "{mid:?}");
    group.compose(5.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(!group.animated());
    group.compose(8.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_parent_origin_moves_the_rendered_child() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":2,"origin":{"value":"0 0 0","animation":{"relative":true,
            "options":{"fps":30,"length":30,"mode":"single"},
            "c0":[{"frame":0,"value":0},{"frame":30,"value":32}],"c1":[],"c2":[]}}},
            {"id":1,"parent":2,"image":"models/flat.json","origin":"16 32 0","size":"16 16",
            "effects":[{"file":"effects/test.json"}]}]
    });
    let mut model = model_scene(scene, b"void main(){\ngl_FragColor=vec4(1.0);\n}");
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    assert_eq!(before[(32 * 64 + 16) * 4], 255);
    assert_eq!(before[(32 * 64 + 48) * 4], 0);
    group.compose(1.0, 0.1).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(after[(32 * 64 + 16) * 4], 0);
    assert_eq!(after[(32 * 64 + 48) * 4], 255);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_visibility_retains_an_initially_hidden_layer() {
    let visible = serde_json::json!({"value":false,"animation":{
        "options":{"fps":30,"length":30,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":30,"value":1,"step":true}]
    }});
    let mut model = model(visible, b"void main(){\ngl_FragColor=vec4(1.0);\n}");
    assert!(model.scripts.is_none());
    assert_eq!(model.layers.len(), 1);
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(!group.animated());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_parent_vector_moves_the_rendered_child() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":2,"origin":{"value":"0 0 0",
            "script":"export function update(){return new Vec3(engine.runtime*32,0,0);}"}},
            {"id":1,"parent":2,"image":"models/flat.json","origin":"16 32 0","size":"16 16",
            "effects":[{"file":"effects/test.json"}]}]
    });
    let mut model = model_scene(scene, b"void main(){\ngl_FragColor=vec4(1.0);\n}");
    assert!(model.scripts.is_some());
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    assert_eq!(before[(32 * 64 + 16) * 4], 255);
    assert_eq!(before[(32 * 64 + 48) * 4], 0);
    group.compose(1.0, 0.1).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(after[(32 * 64 + 16) * 4], 0);
    assert_eq!(after[(32 * 64 + 48) * 4], 255);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan and Wallpaper Engine font assets"]
fn scripted_text_relayouts_after_replacement_and_keeps_unchanged_geometry() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"origin":"60 32 0","pointsize":6,
            "font":"fonts/NotoSans-Regular.ttf","horizontalalign":"right","verticalalign":"center",
            "text":{"value":"A","script":"export function update(){return engine.runtime<1?'A':'AAAA';}"}},
            {"id":2,"alpha":{"value":0,"script":"export function update(){return engine.runtime;}"}}]
    });
    let mut model = model_scene(scene, b"void main(){gl_FragColor=vec4(1.0);}");
    assert!(model.layers[0].script_text.is_some());
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let before = group.quads[0].rect;
    let (_, _, before_pixels) = group.read_canvas().unwrap();
    group.compose(1.0, 0.1).unwrap();
    let after = group.quads[0].rect;
    let (_, _, after_pixels) = group.read_canvas().unwrap();
    assert!(after[2] > before[2]);
    assert!(after[0] < before[0]);
    assert_ne!(before_pixels, after_pixels);
    group.compose(2.0, 0.1).unwrap();
    assert_eq!(group.quads[0].rect, after);
    assert_eq!(group.read_canvas().unwrap().2, after_pixels);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_effect_values_reach_the_matching_layer() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64",
                "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{
                    "g_UserAlpha":{"value":0,"script":"export function update(){return engine.runtime<1?0:1;}"}
                }}]}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64",
                "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{"g_UserAlpha":0.25}}]}]}
        ]
    });
    let mut model = model_scene(scene, b"uniform float g_UserAlpha;\nvoid main(){gl_FragColor=vec4(g_UserAlpha,g_UserAlpha,g_UserAlpha,1.0);}");
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    assert_eq!(before[(32 * 64 + 16) * 4], 0);
    assert!((63..=64).contains(&before[(32 * 64 + 48) * 4]));
    group.compose(1.0, 0.1).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(after[(32 * 64 + 16) * 4], 255);
    assert!((63..=64).contains(&after[(32 * 64 + 48) * 4]));
    group.destroy();
}
