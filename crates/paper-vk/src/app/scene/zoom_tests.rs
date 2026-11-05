use super::effect_lifetime_tests::model_files;
use super::effect_space_tests::PASS_VERTEX;
use super::*;

const STILL_FRAGMENT: &[u8] = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}";
const COPY_FRAGMENT: &[u8] = b"uniform sampler2D g_Texture0;\nuniform float g_Time;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*step(0.0,g_Time);}";

fn scene(zoom: &serde_json::Value, effects: &serde_json::Value) -> String {
    serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0","zoom":zoom},
        "objects":[{"id":1,"image":"models/flat.json","origin":"16 48 0","size":"8 8","color":"1 0 0","effects":effects}]
    })
    .to_string()
}

fn corners(scene: &str, times: &[f32]) -> Vec<[u8; 2]> {
    let files: &[(&str, &[u8])] = &[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/copy.vert", PASS_VERTEX),
        ("shaders/copy.frag", COPY_FRAGMENT),
        ("effects/still.json", br#"{"passes":[{"material":"materials/still.json"}]}"#),
        ("materials/still.json", br#"{"passes":[{"shader":"still"}]}"#),
        ("shaders/still.vert", PASS_VERTEX),
        ("shaders/still.frag", STILL_FRAGMENT),
    ];
    let shared = crate::shared::test_device();
    let mut model = model_files(files);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let mut corners = Vec::new();
    for &time in times {
        group.compose(time, 1.0 / 60.0).unwrap();
        let (width, _, rgba) = group.read_canvas().unwrap();
        let red = |x: u32, y: u32| rgba[((y * width + x) as usize) * RGBA_CHANNELS];
        corners.push([red(2, 2), red(16, 16)]);
    }
    group.destroy();
    corners
}

#[test]
#[ignore = "requires Vulkan"]
fn static_zoom_scales_layers() {
    let copy = serde_json::json!([{"file":"effects/copy.json"}]);
    for effects in [serde_json::json!([]), copy] {
        assert_eq!(corners(&scene(&serde_json::json!(1.0), &effects), &[0.0]), [[0, 255]]);
        assert_eq!(corners(&scene(&serde_json::json!(2.0), &effects), &[0.0]), [[255, 0]]);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_zoom_animates() {
    let zoom = serde_json::json!({"value":2.0,"animation":{"options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":2.0,"step":true},{"frame":10,"value":1.0,"step":true}]}});
    for effects in [
        serde_json::json!([]),
        serde_json::json!([{"file":"effects/copy.json"}]),
        serde_json::json!([{"file":"effects/still.json"}]),
    ] {
        assert_eq!(
            corners(&scene(&zoom, &effects), &[0.0, 1.5]),
            [[255, 0], [0, 255]],
            "{effects}"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn baked_effect_survives_keyframes() {
    let origin = serde_json::json!({"value":"48 16 0","animation":{
        "options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":48.0,"step":true},{"frame":10,"value":49.0,"step":true}],
        "c1":[{"frame":0,"value":16.0,"step":true}],
        "c2":[{"frame":0,"value":0.0,"step":true}]}});
    let mut scene: serde_json::Value = serde_json::from_str(&scene(
        &serde_json::json!(1.0),
        &serde_json::json!([{"file":"effects/still.json"}]),
    ))
    .unwrap();
    scene["objects"].as_array_mut().unwrap().push(serde_json::json!(
        {"id":2,"image":"models/flat.json","origin":origin,"size":"8 8","color":"0 0 1"}
    ));
    assert_eq!(corners(&scene.to_string(), &[0.0, 1.5]), [[0, 255], [0, 255]]);
}

#[test]
#[ignore = "requires Vulkan"]
fn camera_object_shifts_view() {
    let copy = serde_json::json!([{"file":"effects/copy.json"}]);
    for effects in [serde_json::json!([]), copy] {
        let mut scene: serde_json::Value =
            serde_json::from_str(&scene(&serde_json::json!(1.0), &effects)).unwrap();
        scene["objects"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":9,"camera":"default","origin":"14 -14 0"}));
        assert_eq!(corners(&scene.to_string(), &[0.0]), [[255, 0]], "{effects}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn script_pans_flat() {
    let script = r"
export function update(value) {
    thisScene.setCameraTransforms({eye:new Vec3(14,-14,1),center:new Vec3(14,-14,0)});
    return value;
}";
    let lens = serde_json::json!({"id":9,"camera":"default","origin":"0 0 0"});
    for (extra, expected) in [(None, [255, 0]), (Some(lens), [0, 255])] {
        let mut scene: serde_json::Value =
            serde_json::from_str(&scene(&serde_json::json!(1.0), &serde_json::json!([]))).unwrap();
        let objects = scene["objects"].as_array_mut().unwrap();
        objects[0]["visible"] = serde_json::json!({"value":true,"script":script});
        objects.extend(extra);
        assert_eq!(corners(&scene.to_string(), &[0.1, 0.2])[1], expected);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_camera_pans() {
    let origin = serde_json::json!({"value":"0 0 0","animation":{
        "options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":0.0,"step":true},{"frame":10,"value":14.0,"step":true}],
        "c1":[{"frame":0,"value":0.0,"step":true},{"frame":10,"value":-14.0,"step":true}],
        "c2":[{"frame":0,"value":0.0,"step":true}]}});
    let mut scene: serde_json::Value =
        serde_json::from_str(&scene(&serde_json::json!(1.0), &serde_json::json!([]))).unwrap();
    scene["objects"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":9,"camera":"default","origin":origin}));
    assert_eq!(corners(&scene.to_string(), &[0.0, 1.5]), [[0, 255], [255, 0]]);
}

fn camera_eye_steps(script: &str) -> [f32; 2] {
    let origin = serde_json::json!({"value":"0 0 0","animation":{
        "options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":0.0,"step":true},{"frame":10,"value":5.0,"step":true}],
        "c1":[{"frame":0,"value":0.0,"step":true}],
        "c2":[{"frame":0,"value":0.0,"step":true}]}});
    let scene = serde_json::json!({
        "general":{},
        "camera":{"eye":"0 0 4","center":"0 0 0","up":"0 1 0"},
        "objects":[
            {"id":1,"model":"models/mesh.mdl","alpha":{"value":1.0,"script":script}},
            {"id":8,"origin":"0 0 4"},
            {"id":9,"parent":8,"camera":"default","origin":origin}
        ]
    });
    let mut model = super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    assert!(model.cameras_move, "{:?}", model.skipped);
    let device = crate::shared::test_device();
    let mut group =
        build_group(&device, &mut model, true, &[(32, 32)], paper_geom::FillMode::Fill).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let first = group.camera.unwrap().eye[0];
    group.compose(2.0, 2.0).unwrap();
    let second = group.camera.unwrap().eye[0];
    group.destroy();
    [first, second]
}

#[test]
#[ignore = "requires Vulkan"]
fn parented_camera_follows_keyframes() {
    let [first, second] = camera_eye_steps("export function update(value) { return value; }");
    assert!(first.abs() < 0.001, "{first}");
    assert!((second - 5.0).abs() < 0.001, "{second}");
}

#[test]
#[ignore = "requires Vulkan"]
fn object_beats_script() {
    let script = "export function update(value) {\n    thisScene.setCameraTransforms({eye:new Vec3(3,0,4),center:new Vec3(3,0,0),up:new Vec3(0,1,0)});\n    return value;\n}";
    assert_eq!(camera_eye_steps(script)[1], 5.0);
}
