use super::effect_lifetime_tests::{model_files, package_files, texture_levels};
use super::effect_space_tests::PASS_VERTEX;
use super::*;

const SOLID: &[u8] = b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}";
const MASK: &[u8] = b"varying vec2 v_TexCoord;\nvoid main(){gl_FragColor=v_TexCoord.x<0.5?vec4(1.0,0.0,0.0,1.0):vec4(0.0,1.0,0.0,1.0);}";
const COMPOSITE: &[u8] = b"uniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=texSample2D(g_Texture1,vec2(0.5,0.5));}";
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const ADD_MODE: u32 = 9;
const ADD_BLEND: &[u8] = b"vec3 ApplyBlending(const int mode, in vec3 A, in vec3 B, in float opacity) { return mix(A, min(A + B, vec3(1.0)), opacity); }\n";
const MISSING: &[u8] = b"void main(){gl_FragColor=missing;}";
const TIMED: &[u8] =
    b"uniform float g_Time;\nvoid main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0+g_Time*0.0);}";
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const SOFT_OVER_BLUE: [u8; 4] = [64, 0, 255, 255];
const CLOCK_ANIMATION: u32 = 220;
const CUSTOM: &[u8] = b"uniform sampler2D g_Texture0;\nuniform vec4 g_Color4;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*g_Color4;}";
const COPY: &[u8] = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}";

fn puppet(
    left: f32,
    right: f32,
    layers: Vec<paper_scene::puppet::AnimationLayer>,
) -> paper_scene::model::Puppet {
    let mut mesh = paper_scene::puppet::parse(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    )))
    .unwrap();
    mesh.vertices = [
        [left, 16.0, 0.0, 0.0],
        [right, 16.0, 1.0, 0.0],
        [right, -16.0, 1.0, 1.0],
        [left, -16.0, 0.0, 1.0],
    ]
    .map(|[x, y, u, v]| paper_scene::puppet::Vertex {
        position: [x, y, 0.0],
        uv: [u, v],
        bones: [0; 4],
        weights: [1.0, 0.0, 0.0, 0.0],
    })
    .to_vec();
    mesh.indices = vec![0, 1, 2, 2, 3, 0];
    paper_scene::model::Puppet {
        cull: false,
        cpu: false,
        planar_owner: true,
        world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
        mesh,
        layers,
        size: (32.0, 32.0),
        mask_textures: Vec::new(),
    }
}

fn scene(objects: &serde_json::Value, shader: &[u8]) -> SceneModel {
    let mut model = with_materials(objects, shader);
    for layer in &mut model.layers {
        layer.material = None;
    }
    model
}

fn with_materials(objects: &serde_json::Value, shader: &[u8]) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":objects,
    })
    .to_string();
    let solid = |rgba: [u8; 4]| {
        texture_levels(&[paper_scene::tex::MipLevel { width: 1, height: 1, data: rgba.to_vec() }])
    };
    let (texture, green, blue) = (solid(RED), solid(GREEN), solid(BLUE));
    let soft = solid([255, 0, 0, 128]);
    let edge = texture_levels(&[paper_scene::tex::MipLevel {
        width: 2,
        height: 1,
        data: [RED, [0; 4]].concat(),
    }]);
    model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/custom.json", br#"{"material":"materials/custom.json"}"#),
        ("materials/custom.json", br#"{"passes":[{"shader":"custom","textures":["probe"]}]}"#),
        ("models/added.json", br#"{"material":"materials/added.json"}"#),
        (
            "materials/added.json",
            br#"{"passes":[{"shader":"custom","textures":["probe"],"blending":"additive"}]}"#,
        ),
        ("shaders/custom.vert", PASS_VERTEX),
        ("shaders/custom.frag", CUSTOM),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat","textures":["probe"]}]}"#),
        ("materials/probe.tex", &texture),
        ("models/green.json", br#"{"material":"materials/green.json"}"#),
        ("materials/green.json", br#"{"passes":[{"shader":"flat","textures":["green"]}]}"#),
        ("materials/green.tex", &green),
        ("models/blue.json", br#"{"material":"materials/blue.json"}"#),
        ("materials/blue.json", br#"{"passes":[{"shader":"flat","textures":["blue"]}]}"#),
        ("materials/blue.tex", &blue),
        ("models/soft.json", br#"{"material":"materials/soft.json"}"#),
        ("materials/soft.json", br#"{"passes":[{"shader":"flat","textures":["soft"]}]}"#),
        ("materials/soft.tex", &soft),
        ("models/edge.json", br#"{"material":"materials/edge.json"}"#),
        ("materials/edge.json", br#"{"passes":[{"shader":"flat","textures":["edge"]}]}"#),
        ("materials/edge.tex", &edge),
        ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
        (
            "effects/two.json",
            br#"{"passes":[{"material":"materials/green_pass.json"},{"material":"materials/red_pass.json"}]}"#,
        ),
        ("effects/one.json", br#"{"passes":[{"material":"materials/green_pass.json"}]}"#),
        ("materials/green_pass.json", br#"{"passes":[{"shader":"green_pass"}]}"#),
        ("materials/red_pass.json", br#"{"passes":[{"shader":"red_pass"}]}"#),
        ("shaders/green_pass.vert", PASS_VERTEX),
        ("shaders/green_pass.frag", b"void main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}"),
        ("shaders/red_pass.vert", PASS_VERTEX),
        ("shaders/red_pass.frag", SOLID),
        (
            "materials/test.json",
            br#"{"passes":[{"shader":"test","textures":[null,"_rt_imageLayerComposite_1_a"]}]}"#,
        ),
        ("shaders/test.vert", PASS_VERTEX),
        ("shaders/test.frag", shader),
    ])
}

fn effect_scene(shader: &[u8], puppet: paper_scene::model::Puppet) -> SceneModel {
    let objects = serde_json::json!([{"id":1,"image":"models/flat.json","origin":"32 32 0",
        "size":"32 32","effects":[{"file":"effects/test.json"}]}]);
    let mut model = scene(&objects, shader);
    model.layers[0].puppet = Some(puppet);
    model
}

fn blended_scene(
    shader: &[u8],
    image: &str,
    effects: bool,
    puppet: paper_scene::model::Puppet,
) -> SceneModel {
    let mut layer = serde_json::json!({"id":3,"image":image,"origin":"16 32 0","size":"32 32"});
    if effects {
        layer["effects"] = serde_json::json!([{"file":"effects/test.json"}]);
    }
    let objects = serde_json::json!([
        {"id":1,"image":"models/green.json","origin":"16 32 0","size":"32 64"},
        {"id":2,"image":"models/blue.json","origin":"48 32 0","size":"32 64"},
        layer,
    ]);
    blended_objects(&objects, shader, Some(puppet))
}

fn blended_objects(
    objects: &serde_json::Value,
    shader: &[u8],
    puppet: Option<paper_scene::model::Puppet>,
) -> SceneModel {
    let mut model = scene(objects, shader);
    let blend = blend_effect(puppet.is_some());
    let layer = model.layers.last_mut().unwrap();
    layer.puppet = puppet;
    layer.effects.push(blend);
    model
}

fn blend_effect(mesh: bool) -> paper_scene::effects::Effect {
    paper_scene::effects::color_blend_effect(
        &package_files(&[("shaders/common_blending.h", ADD_BLEND)]),
        &paper_scene::effects::Assets::discover(None),
        ADD_MODE,
        None,
        mesh,
    )
    .unwrap()
}

fn live() -> ([usize; 2], [usize; 3]) {
    (
        [
            vk::Renderer::scene_texture_live_allocations(),
            vk::Renderer::scene_target_live_allocations(),
        ],
        vk::Renderer::effect_live_allocations(),
    )
}

fn row(group: &mut Group) -> Vec<[u8; 4]> {
    let (width, height, rgba) = group.read_canvas().unwrap();
    let stride = width as usize * RGBA_CHANNELS;
    let start = (height / 2) as usize * stride;
    rgba[start..start + stride]
        .chunks_exact(RGBA_CHANNELS)
        .map(|pixel| pixel.try_into().unwrap())
        .collect()
}

fn built(model: &mut SceneModel) -> Group {
    let shared = crate::shared::test_device();
    build_group(&shared, model, true, &[(64, 64)], FillMode::Fit).unwrap()
}

fn clock_layer() -> paper_scene::puppet::AnimationLayer {
    paper_scene::puppet::AnimationLayer {
        id: CLOCK_ANIMATION,
        rate: 1.0,
        blend: 1.0,
        additive: false,
        visible: true,
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_mesh_unclipped() {
    let mut group = built(&mut effect_scene(SOLID, puppet(-16.0, 28.0, Vec::new())));
    let row = row(&mut group);
    assert_eq!(row[56], RED, "past the size box");
    assert_eq!(row[62], BLACK, "past the mesh");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn puppet_never_presented() {
    let group = built(&mut effect_scene(TIMED, puppet(0.0, 32.0, Vec::new())));
    assert!(!group.fx.is_empty());
    assert!(group.fx.iter().flat_map(|fx| &fx.pipelines).all(|pipe| pipe.scene.is_none()));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_mask_follows() {
    let before = live();
    let mut group = built(&mut effect_scene(MASK, puppet(0.0, 32.0, Vec::new())));
    group.compose(1.0, 0.1).unwrap();
    let row = row(&mut group);
    assert_eq!(row[24], BLACK, "left the mesh");
    assert_eq!(row[40], RED, "masked half");
    assert_eq!(row[56], GREEN, "other half");
    group.destroy();
    assert_eq!(live(), before);
}

#[test]
#[ignore = "requires Vulkan"]
fn pose_keeps_bake() {
    let before = live();
    let mut group = built(&mut effect_scene(SOLID, puppet(-16.0, 16.0, vec![clock_layer()])));
    assert!(group.animated());
    assert!(group.fx.is_empty(), "static chain");
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(row(&mut group)[32], RED);
    group.destroy();
    assert_eq!(live(), before);
}

#[test]
#[ignore = "requires Vulkan"]
fn revealed_puppet_skinned() {
    let objects = serde_json::json!([{"id":1,"image":"models/flat.json","origin":"32 32 0",
        "size":"32 32","visible":{"value":false,
            "script":"export function update(){return engine.runtime>0.5;}"}}]);
    let mut model = scene(&objects, SOLID);
    assert!(model.scripts.is_some() && !model.layers[0].visible);
    model.layers[0].puppet = Some(puppet(-16.0, 28.0, Vec::new()));
    let meshes = vk::Renderer::scene_mesh_live_allocations();
    let mut group = built(&mut model);
    assert_eq!(vk::Renderer::scene_mesh_live_allocations(), meshes + 1);
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(row(&mut group)[56], RED, "past the size box");
    group.destroy();
    assert_eq!(vk::Renderer::scene_mesh_live_allocations(), meshes);
}

fn keyed_puppet(alpha: serde_json::Value, above: bool) -> SceneModel {
    let mut objects = vec![serde_json::json!({"id":1,"image":"models/flat.json",
        "origin":"32 32 0","size":"32 32","alpha":alpha})];
    if above {
        objects.push(
            serde_json::json!({"id":2,"image":"models/green.json","origin":"32 32 0","size":"16 16"}),
        );
    }
    let mut model = scene(&serde_json::Value::Array(objects), SOLID);
    model.layers[0].puppet = Some(puppet(-16.0, 16.0, vec![clock_layer()]));
    if above {
        model.layers[1].effects.push(blend_effect(false));
    }
    model
}

fn fade_in() -> serde_json::Value {
    serde_json::json!({"value":0,"animation":{"options":{"fps":10,"length":10,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":5,"value":1}]}})
}

#[test]
#[ignore = "requires Vulkan"]
fn keyed_reveal_sampled() {
    let mut model = keyed_puppet(fade_in(), false);
    assert!(model.scripts.is_none() && !model.keyframes.is_empty());
    let mut group = built(&mut model);
    assert!(group.puppets[0].samples.is_empty());
    group.compose(0.8, 0.1).unwrap();
    assert!(!group.puppets[0].samples.is_empty(), "sampled on reveal");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn revealable_chain_live() {
    for (alpha, chains) in [(fade_in(), 1), (serde_json::json!(0), 0)] {
        let group = built(&mut keyed_puppet(alpha, true));
        assert_eq!(group.fx.len(), chains, "{:?}", group.puppets[0].revealable);
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn composite_a_final() {
    for (effects, want) in [
        (serde_json::json!([]), RED),
        (serde_json::json!([{"file":"effects/one.json"}]), GREEN),
        (serde_json::json!([{"file":"effects/two.json"}]), RED),
        (serde_json::json!([{"file":"effects/two.json"},{"file":"effects/one.json"}]), GREEN),
    ] {
        let objects = serde_json::json!([
            {"id":1,"image":"models/flat.json","origin":"32 32 0","size":"32 32","effects":effects},
            {"id":2,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
                "dependencies":[1],"effects":[{"file":"effects/test.json"}]},
        ]);
        let mut model = scene(&objects, COMPOSITE);
        let mut group = built(&mut model);
        group.compose(1.0, 0.1).unwrap();
        assert_eq!(row(&mut group)[32], want, "{effects}");
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn sampled_puppet_flat() {
    let objects = serde_json::json!([
        {"id":1,"image":"models/flat.json","origin":"32 32 0","size":"32 32"},
        {"id":2,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[{"file":"effects/test.json"}]},
    ]);
    let mut model = scene(&objects, COMPOSITE);
    model.layers[0].puppet = Some(puppet(64.0, 96.0, Vec::new()));
    let mut group = built(&mut model);
    assert_eq!(row(&mut group)[32], RED, "flat composite");
    group.destroy();
}

fn sheet(layer: &mut paper_scene::model::Layer, axes: [f32; 4], images: i32) {
    layer.spritesheet = true;
    layer.texture.image_count = images as usize;
    layer.texture.frames = (0..images)
        .map(|image| paper_scene::model::SpriteFrame {
            uv: [0.0, 0.0, axes[0], axes[3]],
            axes,
            rotated: false,
            time: 1.0,
            image,
        })
        .collect();
}

#[test]
#[ignore = "requires Vulkan"]
fn sprite_mesh_frame() {
    let objects = serde_json::json!([{"id":1,"image":"models/flat.json","origin":"32 32 0",
        "size":"32 32"}]);
    let mut model = scene(&objects, SOLID);
    let layer = &mut model.layers[0];
    layer.puppet = Some(puppet(-16.0, 28.0, Vec::new()));
    sheet(layer, [1.0, 0.0, 0.0, 1.0], 1);
    let mut group = built(&mut model);
    assert_eq!(row(&mut group)[56], RED, "past the size box");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn sprite_effect_frame() {
    let mut model = effect_scene(MASK, puppet(-16.0, 16.0, Vec::new()));
    sheet(&mut model.layers[0], [0.5, 0.0, 0.0, 1.0], 1);
    let mut group = built(&mut model);
    let row = row(&mut group);
    assert_eq!(row[24], RED, "masked half");
    assert_eq!(row[40], GREEN, "other half");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn sprite_chain_dropped() {
    let mut model = effect_scene(MISSING, puppet(-16.0, 16.0, Vec::new()));
    let layer = &mut model.layers[0];
    sheet(layer, [1.0, 0.0, 0.0, 1.0], 2);
    layer.texture.pages =
        vec![std::sync::Arc::new(paper_scene::tex::Pixels::rgba(1, 1, GREEN.to_vec()))];
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.fx.is_empty(), "dropped chain");
    assert_eq!(row(&mut group)[32], RED, "first page");
    group.compose(1.5, 0.1).unwrap();
    assert_eq!(row(&mut group)[32], GREEN, "second page");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn blend_reads_behind() {
    let before = live();
    let mut model = blended_scene(SOLID, "models/edge.json", false, puppet(16.0, 48.0, Vec::new()));
    let mut group = built(&mut model);
    group.compose(1.0, 0.1).unwrap();
    let row = row(&mut group);
    assert_eq!(row[16], GREEN, "left its box");
    assert_eq!(row[36], MAGENTA, "blends with behind");
    assert_eq!(row[60], BLUE, "transparent texel");
    group.destroy();
    assert_eq!(live(), before);
}

#[test]
#[ignore = "requires Vulkan"]
fn blend_after_effect() {
    let mut model = blended_scene(MASK, "models/flat.json", true, puppet(16.0, 48.0, Vec::new()));
    let mut group = built(&mut model);
    let row = row(&mut group);
    assert_eq!(row[40], MAGENTA, "masked half");
    assert_eq!(row[56], CYAN, "other half");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn blend_chain_bakes() {
    let before = live();
    let mut model =
        blended_scene(SOLID, "models/flat.json", true, puppet(16.0, 48.0, vec![clock_layer()]));
    let mut group = built(&mut model);
    assert!(group.animated());
    assert!(group.fx.is_empty(), "static chain");
    group.compose(0.0, 0.0).unwrap();
    assert_eq!(row(&mut group)[40], MAGENTA);
    group.destroy();
    assert_eq!(live(), before);
}

#[test]
#[ignore = "requires Vulkan"]
fn replan_keeps_grab() {
    let before = live();
    let mut model = blended_scene(TIMED, "models/flat.json", true, puppet(16.0, 48.0, Vec::new()));
    let mut group = built(&mut model);
    assert_eq!(group.fx.len(), 1, "live chain");
    let grab = group.scene_grab.as_ref().unwrap().view;
    let targets = vk::Renderer::scene_target_live_allocations();
    for _ in 0..2 {
        let plan = group.scene_target_plan(true).unwrap();
        group.apply_scene_target_plan(&plan).unwrap();
        assert_eq!(group.scene_grab.as_ref().unwrap().view, grab, "same grab");
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
    }
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(row(&mut group)[40], MAGENTA);
    group.destroy();
    assert_eq!(live(), before);
}

#[test]
#[ignore = "requires Vulkan"]
fn blend_viewport_crop() {
    let objects = serde_json::json!([
        {"id":1,"image":"models/green.json","origin":"12 32 0","size":"24 64"},
        {"id":2,"image":"models/blue.json","origin":"44 32 0","size":"40 64"},
        {"id":3,"image":"models/flat.json","origin":"16 32 0","size":"32 32"},
    ]);
    let mut model = blended_objects(&objects, SOLID, Some(puppet(0.0, 16.0, vec![clock_layer()])));
    let mut group = built(&mut model);
    group.target.set_viewport(Some(vk::SceneViewport {
        canvas: [64.0, 64.0],
        source: [16.0, 16.0, 32.0, 32.0],
        raster: [64, 64],
    }));
    group.compose(0.0, 0.0).unwrap();
    assert_eq!(row(&mut group)[12], YELLOW, "backdrop under pixel");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn blend_partial_culled() {
    let objects = |origin: &str| {
        serde_json::json!([
            {"id":1,"image":"models/blue.json","origin":"32 32 0","size":"64 64"},
            {"id":3,"image":"models/soft.json","origin":origin,"size":"32 32","alpha":0.5},
        ])
    };
    let mut flat = blended_objects(&objects("16 32 0"), SOLID, None);
    let mut group = built(&mut flat);
    let expected = row(&mut group)[16];
    group.destroy();
    assert!(
        expected.iter().zip(SOFT_OVER_BLUE).all(|(pixel, want)| pixel.abs_diff(want) <= 2),
        "{expected:?}"
    );
    for (reversed, want) in [(false, BLUE), (true, expected)] {
        let mut culled = puppet(16.0, 48.0, Vec::new());
        culled.cull = true;
        if reversed {
            culled.mesh.indices.reverse();
        }
        let mut model = blended_objects(&objects("16 32 0"), SOLID, Some(culled));
        let mut group = built(&mut model);
        let pixel = row(&mut group)[48];
        group.destroy();
        assert!(
            pixel.iter().zip(want).all(|(pixel, want)| pixel.abs_diff(want) <= 1),
            "{pixel:?} {want:?}"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn additive_quad_adds() {
    let objects = serde_json::json!([
        {"id":1,"image":"models/blue.json","origin":"32 32 0","size":"64 64"},
        {"id":2,"image":"models/soft.json","origin":"32 32 0","size":"64 64",
            "colorBlendMode":31,"alpha":0.5,"color":"0.5 1 1"},
    ]);
    let mut model = scene(&objects, SOLID);
    assert!(model.layers[1].additive());
    let mut group = built(&mut model);
    let pixel = row(&mut group)[32];
    group.destroy();
    let want = [32, 0, 255, 255];
    assert!(pixel.iter().zip(want).all(|(pixel, want)| pixel.abs_diff(want) <= 1), "{pixel:?}");
}

#[test]
#[ignore = "requires Vulkan"]
fn material_blend_once() {
    const OVER: [u8; 4] = [128, 0, 128, 255];
    const ADDED: [u8; 4] = [128, 0, 255, 255];
    for (image, mode, effects, want) in [
        ("models/custom.json", 0, true, OVER),
        ("models/added.json", 0, false, ADDED),
        ("models/added.json", 0, true, ADDED),
        ("models/custom.json", 31, false, ADDED),
        ("models/custom.json", 31, true, ADDED),
        ("models/added.json", 31, true, ADDED),
    ] {
        let mut layer = serde_json::json!({"id":2,"image":image,"origin":"32 32 0","size":"64 64",
            "colorBlendMode":mode,"alpha":0.5});
        if effects {
            layer["effects"] = serde_json::json!([{"file":"effects/test.json"}]);
        }
        let objects = serde_json::json!([
            {"id":1,"image":"models/blue.json","origin":"32 32 0","size":"64 64"},
            layer,
        ]);
        let mut model = with_materials(&objects, COPY);
        model.layers[0].material = None;
        let mut group = built(&mut model);
        let pixel = row(&mut group)[32];
        group.destroy();
        assert!(
            pixel.iter().zip(want).all(|(pixel, want)| pixel.abs_diff(want) <= 1),
            "{image} {mode} {effects} {pixel:?}"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn additive_puppet_adds() {
    for effects in [false, true] {
        let mut layer = serde_json::json!({"id":2,"image":"models/soft.json","origin":"32 32 0",
            "size":"32 32","colorBlendMode":31,"alpha":0.5,"color":"0.5 1 1"});
        if effects {
            layer["effects"] = serde_json::json!([{"file":"effects/test.json"}]);
        }
        let objects = serde_json::json!([
            {"id":1,"image":"models/blue.json","origin":"32 32 0","size":"64 64"},
            layer,
        ]);
        let mut model = scene(&objects, COPY);
        model.layers[1].puppet = Some(puppet(-16.0, 16.0, Vec::new()));
        let mut group = built(&mut model);
        let pixel = row(&mut group)[32];
        group.destroy();
        let want = [32, 0, 255, 255];
        assert!(
            pixel.iter().zip(want).all(|(pixel, want)| pixel.abs_diff(want) <= 1),
            "{effects} {pixel:?}"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn clip_planes_released() {
    use paper_scene::puppet::masks::{Mask, Masks};
    let before = live();
    for _ in 0..2 {
        let objects = serde_json::json!([{"id":1,"image":"models/flat.json","origin":"32 32 0",
            "size":"32 32"}]);
        let mut model = scene(&objects, SOLID);
        let mut masked = puppet(-16.0, 16.0, vec![clock_layer()]);
        let record = Mask { texture: String::new(), flags: 0, clipped: vec![1], base: vec![0] };
        masked.mesh.masks = Masks { records: vec![record], spans: vec![0..3, 3..6] };
        let white = paper_scene::tex::Pixels::rgba(1, 1, vec![u8::MAX; RGBA_CHANNELS]);
        masked.mask_textures = vec![std::sync::Arc::new(white)];
        model.layers[0].puppet = Some(masked);
        let mut group = built(&mut model);
        assert!(group.clip_planes.is_some());
        group.compose(1.0, 0.1).unwrap();
        let row = row(&mut group);
        assert_eq!(row[20], BLACK, "clipped outside base");
        assert_eq!(row[44], RED, "base");
        group.destroy();
        assert_eq!(live(), before);
    }
}
