use super::effect_lifetime_tests::model_files;
use super::effect_space_tests::PASS_VERTEX;
use super::*;

const TINT_FRAGMENT: &[u8] = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){vec4 c=texSample2D(g_Texture0,v_TexCoord);gl_FragColor=vec4(0.0,c.r,0.0,c.a);}";
const COPY_FRAGMENT: &[u8] = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}";

struct Spec<'a> {
    angles: &'a str,
    zoom: f32,
    effects: serde_json::Value,
    extra: &'a [serde_json::Value],
}

fn graph_model(scene: &str) -> SceneModel {
    let particles = super::tests::particle_package();
    let mesh = super::tests::triangle_mdl();
    let mut files: Vec<(&str, &[u8])> = particles
        .entries()
        .iter()
        .filter(|entry| entry.path != "scene.json")
        .map(|entry| (entry.path.as_str(), particles.read(entry)))
        .collect();
    files.extend_from_slice(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        (
            "models/screen.json",
            br#"{"material":"materials/flat.json","fullscreen":true,"passthrough":true}"#,
        ),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("models/compose.json", br#"{"material":"materials/compose.json","passthrough":true}"#),
        (
            "materials/compose.json",
            br#"{"passes":[{"shader":"composelayer","textures":["_rt_FullFrameBuffer"]}]}"#,
        ),
        (
            "models/compose_depth.json",
            br#"{"material":"materials/compose_depth.json","passthrough":true}"#,
        ),
        (
            "materials/compose_depth.json",
            br#"{"passes":[{"shader":"composelayer","depthtest":"enabled","textures":["_rt_FullFrameBuffer"]}]}"#,
        ),
        ("models/solid_depth.json", br#"{"material":"materials/solid_depth.json","solidlayer":true}"#),
        (
            "materials/solid_depth.json",
            br#"{"passes":[{"shader":"flat","depthtest":"enabled","blending":"translucent"}]}"#,
        ),
        ("models/mesh.mdl", &mesh),
        (
            "materials/mesh.json",
            br#"{"passes":[{"shader":"mesh","depthtest":"enabled","depthwrite":"enabled","cullmode":"nocull","blending":"normal"}]}"#,
        ),
        (
            "shaders/mesh.vert",
            b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}",
        ),
        ("shaders/mesh.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ("effects/tint.json", br#"{"passes":[{"material":"materials/tint.json"}]}"#),
        ("materials/tint.json", br#"{"passes":[{"shader":"tint"}]}"#),
        ("shaders/tint.vert", PASS_VERTEX),
        ("shaders/tint.frag", TINT_FRAGMENT),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/copy.vert", PASS_VERTEX),
        ("shaders/copy.frag", COPY_FRAGMENT),
    ]);
    model_files(&files)
}

fn reds(spec: &Spec<'_>) -> [u8; 3] {
    let Spec { angles, zoom, effects, extra } = spec;
    let mut objects = vec![
        serde_json::json!({"id":1,"origin":"0 0 0","angles":angles}),
        serde_json::json!({"id":2,"parent":1,"image":"models/flat.json","size":"2 2","color":"1 0 0","effects":effects}),
    ];
    objects.extend_from_slice(extra);
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0","zoom":zoom},
        "objects":objects
    })
    .to_string();
    let shared = crate::shared::test_device();
    let mut model = graph_model(&scene);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.camera.is_some());
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 1.0 / 60.0).unwrap();
    let (width, _, rgba) = group.read_canvas().unwrap();
    group.destroy();
    let red = |x: u32| rgba[((32 * width + x) as usize) * RGBA_CHANNELS];
    [red(32), red(37), red(42)]
}

#[test]
#[ignore = "requires Vulkan"]
fn parented_planes_turn() {
    for effects in [serde_json::json!([]), serde_json::json!([{"file":"effects/copy.json"}])] {
        let flat = Spec { angles: "0 0 0", zoom: 1.0, effects, extra: &[] };
        assert_eq!(reds(&flat), [255, 255, 0], "{}", flat.effects);
        let turned = Spec { angles: "0 1.4 0", ..flat };
        assert_eq!(reds(&turned), [255, 0, 0], "{}", turned.effects);
        let zoomed = Spec { zoom: 2.0, ..turned };
        assert_eq!(reds(&Spec { angles: "0 0 0", ..zoomed }), [255; 3]);
    }
}

fn shaken_group(
    shared: &crate::shared::SharedDevice,
    effects: &serde_json::Value,
    visible: serde_json::Value,
) -> Group {
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0","camerashake":true,
            "camerashakeamplitude":5,"camerashakespeed":0.5},
        "objects":[{"id":2,"image":"models/flat.json","size":"2 2","color":"1 0 0",
            "effects":effects,"visible":visible}]
    })
    .to_string();
    let mut model = graph_model(&scene);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    build_group(shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap()
}

fn assert_near(actual: [f32; 3], expected: [f32; 3]) {
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-4, "{actual} {expected}");
    }
}

fn red_at(group: &mut Group, x: u32, y: u32) -> u8 {
    let (width, _, rgba) = group.read_canvas().unwrap();
    rgba[((y * width + x) as usize) * RGBA_CHANNELS]
}

#[test]
#[ignore = "requires Vulkan"]
fn shake_moves_camera() {
    let shared = crate::shared::test_device();
    for effects in [serde_json::json!([]), serde_json::json!([{"file":"effects/copy.json"}])] {
        let mut group = shaken_group(&shared, &effects, serde_json::json!(true));
        group.compose(0.0, 1.0 / 60.0).unwrap();
        assert_near(group.camera.unwrap().eye, [0.5, 0.0, 10.0]);
        assert_eq!(
            [red_at(&mut group, 25, 32), red_at(&mut group, 37, 32), red_at(&mut group, 30, 40)],
            [255, 0, 0],
            "{effects}"
        );
        group.compose(4.0, 1.0 / 60.0).unwrap();
        assert_near(group.camera.unwrap().eye, [0.270_151, 0.485_93, 10.420_735]);
        assert_near(group.camera.unwrap().center, [0.270_151, 0.485_93, 0.420_735]);
        assert_eq!([red_at(&mut group, 30, 27), red_at(&mut group, 30, 40)], [0, 255], "{effects}");
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn shake_off_restores() {
    let shared = crate::shared::test_device();
    let script = r"
export function update(value) {
    if (engine.runtime >= 1) thisScene.camerashake = false;
    return value;
}";
    let visible = serde_json::json!({"value":true,"script":script});
    let mut group = shaken_group(&shared, &serde_json::json!([]), visible);
    group.compose(0.5, 1.0 / 60.0).unwrap();
    assert!(group.camera.unwrap().eye[0] > 0.1);
    group.compose(1.5, 1.0 / 60.0).unwrap();
    group.compose(1.6, 1.0 / 60.0).unwrap();
    assert_eq!(group.camera.unwrap().eye, [0.0, 0.0, 10.0]);
    assert_eq!(group.camera.unwrap().center, [0.0, 0.0, 0.0]);
    assert_eq!(group.eye, [0.0, 0.0, 10.0]);
    assert_eq!(red_at(&mut group, 37, 32), 255);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn camera_object_wins() {
    let script = r"
export function update(value){
    const camera = thisScene.getCameraTransforms();
    camera.eye = new Vec3(2, 0, 10);
    camera.center = new Vec3(2, 0, 0);
    camera.up = new Vec3(1, 1, 0);
    thisScene.setCameraTransforms(camera);
    return value;
}";
    let lens = serde_json::json!({"id":3,"camera":"default","origin":"0 0 10","fov":30});
    let cases = [
        (vec![lens], [[0.0, 0.0, 10.0], [0.0, 0.0, 9.0], [0.0, 1.0, 0.0]], 30.0),
        (Vec::new(), [[2.0, 0.0, 10.0], [2.0, 0.0, 0.0], [1.0, 1.0, 0.0]], 50.0),
    ];
    for (extra, [eye, center, up], fov) in cases {
        let mut objects = vec![serde_json::json!({"id":2,"image":"models/flat.json","size":"2 2",
            "origin":{"value":"0 0 0","script":script}})];
        objects.extend(extra);
        let scene = serde_json::json!({
            "camera":{"eye":"-2 0 10","center":"-2 0 0","up":"0 1 0"},
            "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
            "objects":objects
        })
        .to_string();
        let shared = crate::shared::test_device();
        let mut model = graph_model(&scene);
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        for frame in 0..3 {
            group.compose(frame as f32 / 60.0, 1.0 / 60.0).unwrap();
        }
        let camera = group.camera.unwrap();
        group.destroy();
        assert_near(camera.eye, eye);
        assert_near(camera.center, center);
        assert_near(camera.up, up);
        assert_eq!(camera.fov, fov);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn compose_reads_backdrop() {
    let shared = crate::shared::test_device();
    for (copy, inside) in [(true, [0, 255, 0]), (false, [255, 0, 0])] {
        let scene = serde_json::json!({
            "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
            "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
            "objects":[
                {"id":1,"image":"models/flat.json","origin":"0 0 -1","size":"6 6","color":"1 0 0"},
                {"id":2,"image":"models/compose.json","size":"2 2","angles":"0 0.5 0",
                    "copybackground":copy,"effects":[{"file":"effects/tint.json"}]}
            ]
        })
        .to_string();
        let mut model = graph_model(&scene);
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.camera.is_some());
        assert!(model.layers[1].passthrough);
        assert!(model.layers[1].composition_size.is_some());
        assert_eq!(model.layers[1].copy_background, copy);
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        group.compose(0.0, 1.0 / 60.0).unwrap();
        let (width, _, rgba) = group.read_canvas().unwrap();
        group.destroy();
        let pixel = |x: u32| {
            let at = ((32 * width + x) as usize) * RGBA_CHANNELS;
            [rgba[at], rgba[at + 1], rgba[at + 2]]
        };
        assert_eq!(pixel(32), inside, "copy {copy}");
        assert_eq!(pixel(14), [255, 0, 0], "copy {copy}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn depth_survives_effects() {
    let shared = crate::shared::test_device();
    let cases = [
        ("models/compose.json", "1 1 1", [0, 255, 0]),
        ("models/compose_depth.json", "1 1 1", [255, 0, 0]),
        ("models/solid_depth.json", "1 0 0", [255, 0, 0]),
    ];
    for (image, color, over_mesh) in cases {
        let scene = serde_json::json!({
            "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
            "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
            "objects":[
                {"id":1,"image":"models/flat.json","origin":"0 0 -1","size":"6 6","color":"1 0 0"},
                {"id":2,"model":"models/mesh.mdl","origin":"0 0 5"},
                {"id":3,"image":image,"origin":"0 0 1","size":"2 2","color":color,
                    "effects":[{"file":"effects/tint.json"}]}
            ]
        })
        .to_string();
        let mut model = graph_model(&scene);
        assert!(model.skipped.is_empty(), "{image} {:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        group.compose(0.0, 1.0 / 60.0).unwrap();
        let (width, _, rgba) = group.read_canvas().unwrap();
        group.destroy();
        let pixel = |x: u32, y: u32| {
            let at = ((y * width + x) as usize) * RGBA_CHANNELS;
            [rgba[at], rgba[at + 1], rgba[at + 2]]
        };
        assert_eq!(pixel(27, 36), over_mesh, "{image}");
        assert_eq!(pixel(38, 26), [0, 255, 0], "{image}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn degenerate_particles_load() {
    let collapsed = [serde_json::json!(
        {"id":3,"particle":"particles/p.json","origin":"8 0 0","scale":"0 0 0","angles":"0.3 0 0"}
    )];
    let spec =
        Spec { angles: "0 0 0", zoom: 1.0, effects: serde_json::json!([]), extra: &collapsed };
    assert_eq!(reds(&spec), [255, 255, 0]);
}

#[test]
fn tilted_particles_follow_offset() {
    let package = super::tests::particle_package();
    let model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )
    .unwrap();
    let mut system = model.particles.into_iter().next().unwrap().system;
    let tilted = paper_scene::matrix::model_matrix([1.0, 2.0, 3.0], [2.0; 3], [0.4, 0.2, 0.0]);
    system.spatial = Some(tilted);
    system.origin = (1.0, 2.0, 3.0);
    let moved = particle_model(&system, (1.5, 1.0, 3.0));
    let mut expected = tilted;
    expected[12..15].copy_from_slice(&[1.5, 1.0, 3.0]);
    assert_eq!(moved, expected);
    let identity = mat4_mul(&moved, &particle_inverse(&system, (1.5, 1.0, 3.0)));
    for (index, value) in identity.iter().enumerate() {
        let diagonal = index % (MATRIX_COLUMN_COMPONENTS + 1) == 0;
        assert!((value - f32::from(u8::from(diagonal))).abs() < 1e-5, "{identity:?}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn camera_turn_refreshes_basis() {
    let script = "let moved = false;\nexport function update(value) {\n    if (!moved && engine.runtime >= 1) {\n        thisScene.setCameraTransforms({eye:new Vec3(10,0,0),center:new Vec3(0,0,0),up:new Vec3(0,1,0)});\n        moved = true;\n    }\n    return value;\n}";
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
        "objects":[
            {"id":2,"image":"models/flat.json","size":"2 2","color":"1 0 0","alpha":{"value":1.0,"script":script}},
            {"id":3,"particle":"particles/p.json"}
        ]
    })
    .to_string();
    let shared = crate::shared::test_device();
    let mut model = graph_model(&scene);
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let before = group.particles[0].screen_basis.unwrap().forward;
    group.compose(2.0, 2.0).unwrap();
    let after = group.particles[0].screen_basis.unwrap().forward;
    group.destroy();
    assert_ne!(before, after);
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_plane_moves() {
    let origin = serde_json::json!({"value":"0 0 0","animation":{
        "options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":0.0,"step":true},{"frame":10,"value":100.0,"step":true}],
        "c1":[{"frame":0,"value":0.0,"step":true}],
        "c2":[{"frame":0,"value":0.0,"step":true}]}});
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
        "objects":[{"id":2,"image":"models/flat.json","size":"2 2","color":"1 0 0","origin":origin}]
    })
    .to_string();
    let shared = crate::shared::test_device();
    let mut model = graph_model(&scene);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.camera.is_some());
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let mut centre = |time: f32| {
        group.compose(time, 1.0 / 60.0).unwrap();
        let (width, height, rgba) = group.read_canvas().unwrap();
        rgba[((height / 2 * width + width / 2) * 4) as usize]
    };
    assert_eq!([centre(0.0), centre(2.0)], [255, 0]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_particles_load() {
    let alpha = serde_json::json!({"value":1.0,"animation":{
        "options":{"fps":10,"length":20,"mode":"loop"},
        "c0":[{"frame":0,"value":0.0},{"frame":10,"value":1.0}]}});
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
        "objects":[
            {"id":2,"image":"models/flat.json","size":"2 2","color":"1 0 0","alpha":alpha},
            {"id":3,"particle":"particles/p.json"}
        ]
    })
    .to_string();
    let shared = crate::shared::test_device();
    let mut model = graph_model(&scene);
    assert!(model.camera.is_some());
    assert!(!model.particles.is_empty());
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.5, 1.0 / 60.0).unwrap();
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn fullscreen_stays_screenspace() {
    let visible = serde_json::json!({"value":true,"script":"export function update(value) { return value; }"});
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
        "objects":[
            {"id":2,"image":"models/flat.json","size":"2 2","color":"1 0 0"},
            {"id":3,"image":"models/screen.json","visible":visible,"effects":[{"file":"effects/copy.json"}]}
        ]
    })
    .to_string();
    let shared = crate::shared::test_device();
    let mut model = graph_model(&scene);
    assert!(model.camera.is_some(), "{:?}", model.skipped);
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let screen = group.screen_quads[0];
    let built = group.quads[screen].rect;
    group.compose(1.0, 1.0 / 60.0).unwrap();
    let (rect, projection) = (group.quads[screen].rect, group.quads[screen].projection);
    group.destroy();
    assert_eq!(rect, built);
    assert!(projection.is_none());
}
