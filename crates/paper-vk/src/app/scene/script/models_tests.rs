use super::super::tests::{Fault, set_fault};
use super::*;
use serde_json::json;
use std::rc::Rc;

fn fixture(late: bool) -> paper_scene::model::SceneModel {
    let script = format!(
        r"
let handle;
let layer;
function create() {{
    handle = thisScene.createModelData({{shapes:[{{vertexBuffer:new Float32Array([0,20,0,0,0,1,0.5,1,-20,-20,0,0,0,1,0,0,20,-20,0,0,0,1,1,0]),vertexFormat:[IModelData.POSITION,IModelData.NORMAL,IModelData.UV],material:'materials/mesh.json'}}]}});
    layer = thisScene.createLayer({{name:'triangle',model:handle,origin:new Vec3(64,64,0)}});
}}
export function init(value) {{ if (!{late}) create(); return value; }}
export function update(value) {{
    if ({late} && engine.runtime >= 1 && !layer) create();
    if (!{late} && engine.runtime >= 1) thisScene.destroyModelData(handle);
    if (!{late} && engine.runtime >= 2) thisScene.destroyLayer(layer);
    return value;
}}
"
    );
    model_scene(&script)
}

fn model_scene(script: &str) -> paper_scene::model::SceneModel {
    let scene = json!({"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[{"id":1,"image":"models/sample.json","origin":{"value":"64 64 0","script":script},"size":"128 128","color":"0 0 0"}]});
    super::super::super::tests::mesh_sampler_model(
        &serde_json::to_vec(&scene).unwrap(),
        br#"{"passes":[{"shader":"genericimage2","textures":["textures/white"],"blending":"translucent","depthtest":"disabled","depthwrite":"disabled","cullmode":"nocull"}]}"#,
        b"void main(){gl_FragColor=vec4(0.0,0.0,0.0,1.0);}",
    )
}

#[test]
#[ignore = "requires Vulkan"]
fn model_lifetime_pixels() {
    let device = crate::shared::test_device();
    let before = vk::Renderer::effect_live_allocations();
    let mut control = fixture(true);
    let control = super::super::super::build_group(
        &device,
        &mut control,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    let idle = vk::Renderer::effect_live_allocations();
    let idle_bytes = control.target.allocation_bytes;
    assert!(!control.dynamic_depth);
    assert!(control.scripts.as_ref().unwrap().models.is_empty());
    control.destroy();
    assert_eq!(vk::Renderer::effect_live_allocations(), before);
    let mut model = fixture(false);
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    let allocated = vk::Renderer::effect_live_allocations();
    let [pipelines, _, buffers] = allocated;
    let [idle_pipelines, _, idle_buffers] = idle;
    assert!(pipelines > idle_pipelines);
    assert!(buffers > idle_buffers);
    assert!(group.dynamic_depth);
    let live_bytes = group.target.allocation_bytes;
    let live = group.read_canvas().unwrap().2;
    assert!(group.target.allocation_bytes > idle_bytes);
    assert_eq!(&live[(64 * 128 + 64) * 4..][..3], &[255, 0, 0]);
    let owner = Rc::downgrade(
        group.scripts.as_ref().unwrap().models.values().next().unwrap().owner.as_ref().unwrap(),
    );
    group.compose(1.0, 0.016).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, live);
    assert!(owner.upgrade().is_some());
    assert_eq!(vk::Renderer::effect_live_allocations(), allocated);
    assert_eq!(group.target.allocation_bytes, live_bytes);
    assert!(group.dynamic_depth);
    group.compose(2.0, 0.016).unwrap();
    let retired = group.read_canvas().unwrap().2;
    assert_eq!(&retired[(64 * 128 + 64) * 4..][..3], &[0, 0, 0]);
    assert!(owner.upgrade().is_none());
    assert!(group.scripts.as_ref().unwrap().models.is_empty());
    assert!(!group.dynamic_depth);
    assert_eq!(vk::Renderer::effect_live_allocations(), idle);
    assert_eq!(group.target.allocation_bytes, idle_bytes);
    group.destroy();
    assert_eq!(vk::Renderer::effect_live_allocations(), before);
}

#[test]
#[ignore = "requires Vulkan"]
fn late_depth_creation() {
    let device = crate::shared::test_device();
    let mut model = fixture(true);
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    assert_eq!(group.renderer.scene_pixel_bytes(), 4);
    group.compose(0.0, 0.016).unwrap();
    group.compose(1.0, 0.016).unwrap();
    let pixels = group.read_canvas().unwrap().2;
    assert_eq!(&pixels[(64 * 128 + 64) * 4..][..3], &[255, 0, 0]);
    assert_eq!(group.renderer.scene_pixel_bytes(), 4);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn camera_updates_mesh() {
    let device = crate::shared::test_device();
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl","origin":{"value":"0 0 0","script":r"
let changed = false;
export function update(value) {
    if (!changed && engine.runtime >= 1) {
        thisScene.setCameraTransforms({eye:new Vec3(0.8,0.3,-4),center:new Vec3(0,0,0),up:new Vec3(0,1,0)});
        changed = true;
    }
    return value;
}"}}]});
    let mut model = super::super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    group.compose(0.0, 0.016).unwrap();
    let initial = group.read_canvas().unwrap().2;
    group.compose(1.0, 0.016).unwrap();
    let changed = group.read_canvas().unwrap().2;
    assert_ne!(initial, changed);
    group.compose(2.0, 0.016).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, changed);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn zero_scale_hides_mesh() {
    let device = crate::shared::test_device();
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl","scale":{"value":"1 1 1","script":r"
export function update(value) {
    return engine.runtime >= 1 ? new Vec3(0) : value;
}"}}]});
    let mut model = super::super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    group.compose(0.0, 0.016).unwrap();
    let shown = group.read_canvas().unwrap().2;
    group.compose(1.0, 0.016).unwrap();
    let hidden = group.read_canvas().unwrap().2;
    assert_ne!(shown, hidden);
    assert!(group.meshes.iter().all(|mesh| !mesh.visible));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn zero_scale_starts_hidden() {
    let device = crate::shared::test_device();
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl","scale":{"value":"0 0 0","script":r"
export function update(value) {
    return engine.runtime >= 1 ? new Vec3(1) : value;
}"}}]});
    let mut model = super::super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    group.compose(0.0, 0.016).unwrap();
    let hidden = group.read_canvas().unwrap().2;
    assert!(group.meshes.iter().all(|mesh| !mesh.visible));
    group.compose(1.0, 0.016).unwrap();
    assert_ne!(group.read_canvas().unwrap().2, hidden);
    assert!(group.meshes.iter().all(|mesh| mesh.visible));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn mixed_model_reuse() {
    let device = crate::shared::test_device();
    let mut model = model_scene(
        r"
const imageAsset = engine.registerAsset('models/sample.json');
let model;
let layer;
let image;
let phase = 0;
function create() {
    const shapes = [-24,24].map(x => ({vertexBuffer:new Float32Array([x,16,0,0,0,1,0.5,1,x-16,-16,0,0,0,1,0,0,x+16,-16,0,0,0,1,1,0]),vertexFormat:[IModelData.POSITION,IModelData.NORMAL,IModelData.UV],material:'materials/mesh.json'}));
    model = thisScene.createModelData({shapes});
    layer = thisScene.createLayer({name:'paired',model,origin:new Vec3(64,64,0)});
    thisScene.destroyModelData(model);
}
export function init(value) { create(); return value; }
export function update(value) {
    if (phase === 0 && engine.runtime >= 1) {
        image = thisScene.createLayer({name:'cover',image:imageAsset,origin:new Vec3(40,64,0),size:new Vec2(32,32),color:new Vec3(0,1,0)});
        phase++;
    } else if (phase === 1 && engine.runtime >= 2) {
        thisScene.sortLayer(image, 1);
        phase++;
    } else if (phase === 2 && engine.runtime >= 3) {
        thisScene.destroyLayer(layer);
        phase++;
    } else if (phase === 3 && engine.runtime >= 4) {
        create();
        phase++;
    } else if (phase === 4 && engine.runtime >= 5) {
        thisScene.sortLayer(layer, 1);
        phase++;
    } else if (phase === 5 && engine.runtime >= 6) {
        thisScene.destroyLayer(layer);
        thisScene.destroyLayer(image);
        phase++;
    }
    return value;
}
",
    );
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    let mut slots = 0;
    for (time, expected) in [
        (0.0, [[255, 0, 0], [255, 0, 0]]),
        (1.0, [[0, 255, 0], [255, 0, 0]]),
        (2.0, [[255, 0, 0], [255, 0, 0]]),
        (3.0, [[0, 255, 0], [0, 0, 0]]),
        (4.0, [[255, 0, 0], [255, 0, 0]]),
        (5.0, [[0, 255, 0], [255, 0, 0]]),
        (6.0, [[0, 0, 0], [0, 0, 0]]),
    ] {
        group.compose(time, 0.016).unwrap();
        let pixels = group.read_canvas().unwrap().2;
        for (x, color) in [40, 88].into_iter().zip(expected) {
            assert_eq!(&pixels[(64 * 128 + x) * 4..][..3], &color, "time {time}");
        }
        if time == 1.0 {
            slots = group.quads.len();
        }
        if time >= 2.0 {
            assert_eq!(group.quads.len(), slots);
        }
    }
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

fn toggled_frames(objects: serde_json::Value) -> Vec<(Vec<u8>, bool)> {
    let device = crate::shared::test_device();
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":objects});
    let mut model = super::super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    let mut frames = Vec::new();
    for time in [0.0, 1.0, 2.0] {
        group.compose(time, 0.016).unwrap();
        frames.push((group.read_canvas().unwrap().2, group.meshes[0].visible));
    }
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
    frames
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_mesh_toggles() {
    let hide = json!({"value":true,"script":"export function update(value) { return engine.runtime < 1; }"});
    let show = json!({"value":false,"script":"export function update(value) { return engine.runtime >= 1; }"});
    let hidden = toggled_frames(json!([{"id":1,"model":"models/mesh.mdl","visible":hide}]));
    let shown = toggled_frames(json!([{"id":1,"model":"models/mesh.mdl","visible":show}]));
    assert_eq!(hidden.iter().map(|frame| frame.1).collect::<Vec<_>>(), [true, false, false]);
    assert_eq!(shown.iter().map(|frame| frame.1).collect::<Vec<_>>(), [false, true, true]);
    assert_ne!(hidden[0].0, hidden[1].0);
    assert_eq!(shown[0].0, hidden[1].0);
    assert_eq!(shown[1].0, hidden[0].0);
    let parent = |visible| json!([{"id":10,"visible":visible},{"id":1,"parent":10,"model":"models/mesh.mdl"}]);
    assert_eq!(toggled_frames(parent(hide)), hidden);
    assert_eq!(toggled_frames(parent(show)), shown);
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_perspective_pixels() {
    let device = crate::shared::test_device();
    let scene = json!({"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"64 64 20","scale":"20 20 20","perspective":{"value":false,"script":r"
export function update(value) {
    return engine.runtime >= 1;
}"}}]});
    let mut model = super::super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    group.compose(0.0, 0.016).unwrap();
    let flat = super::super::super::tests::lit_pixels(&group.read_canvas().unwrap().2);
    group.compose(1.0, 0.016).unwrap();
    let deep = super::super::super::tests::lit_pixels(&group.read_canvas().unwrap().2);
    assert!(deep > flat, "{flat} {deep}");
    assert!(group.meshes.iter().all(|mesh| mesh.perspective));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn retired_model_rerenders() {
    let shared = crate::shared::test_device();
    let mut model = crate::app::scene::lighting_tests::sun_shadow_model(
        true,
        crate::app::scene::lighting_tests::scripted_intensity(),
    );
    let mut group = crate::app::scene::lighting_tests::sun_group(&shared, &mut model);
    group.compose(0.0, 1.0 / 60.0).unwrap();
    let mut scripts = group.scripts.take().unwrap();
    let empty = Model {
        quads: Vec::new(),
        textures: Vec::new(),
        depth: false,
        actors: Vec::new(),
        owner: None,
    };
    scripts.models.insert(0, empty);
    group.retire_models(&mut scripts, &[0]);
    group.scripts = Some(scripts);
    group.compose(1.0 / 60.0, 1.0 / 60.0).unwrap();
    assert_eq!(group.shadows.as_ref().unwrap().renders, 2);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn sort_after_destroy() {
    let device = crate::shared::test_device();
    let mut model = model_scene(
        r"
const imageAsset = engine.registerAsset('models/sample.json');
let first;
let second;
let third;
let phase = 0;
export function init(value) {
    first = thisScene.createLayer({name:'first',image:imageAsset,origin:new Vec3(64,64,0),size:new Vec2(16,16),color:'1 0 0'});
    second = thisScene.createLayer({name:'second',image:imageAsset,origin:new Vec3(64,64,0),size:new Vec2(48,48)});
    third = thisScene.createLayer({name:'third',image:imageAsset,origin:new Vec3(16,16,0),size:new Vec2(8,8)});
    return value;
}
export function update(value) {
    if (phase === 0 && engine.runtime >= 1) {
        thisScene.destroyLayer(third);
        phase++;
    } else if (phase === 1 && engine.runtime >= 2) {
        const shapes = [{vertexBuffer:new Float32Array([0,16,0,0,0,1,0.5,1,-16,-16,0,0,0,1,0,0,16,-16,0,0,0,1,1,0]),vertexFormat:[IModelData.POSITION,IModelData.NORMAL,IModelData.UV],material:'materials/mesh.json'}];
        const data = thisScene.createModelData({shapes});
        thisScene.createLayer({name:'mesh',model:data,origin:new Vec3(64,112,0)});
        thisScene.destroyModelData(data);
        phase++;
    } else if (phase === 2 && engine.runtime >= 3) {
        thisScene.sortLayer(second, thisScene.getLayerIndex(first));
        phase++;
    } else if (phase === 3 && engine.runtime >= 4) {
        thisScene.destroyLayer(first);
        phase++;
    }
    return value;
}
",
    );
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    const WHITE: [u8; 3] = [255, 255, 255];
    const RED: [u8; 3] = [255, 0, 0];
    for (time, center) in [(0.0, WHITE), (1.0, WHITE), (2.0, WHITE), (3.0, RED), (4.0, WHITE)] {
        group.compose(time, 0.016).unwrap();
        let pixels = group.read_canvas().unwrap().2;
        assert_eq!(&pixels[(64 * 128 + 64) * 4..][..3], &center, "time {time}");
        assert_eq!(&pixels[(64 * 128 + 80) * 4..][..3], &WHITE, "time {time}");
        let order = &group.scripts.as_ref().unwrap().order;
        let slots: std::collections::HashSet<_> =
            order.iter().filter_map(|(slot, _)| *slot).collect();
        assert_eq!(slots.len(), order.iter().filter(|(slot, _)| slot.is_some()).count());
        let ranks: std::collections::HashSet<_> = order.iter().map(|(_, rank)| *rank).collect();
        assert_eq!(ranks.len(), order.len(), "time {time}");
    }
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

fn late_image() -> paper_scene::model::SceneModel {
    model_scene(
        r"
const imageAsset = engine.registerAsset('models/sample.json');
let layer;
export function update(value) {
    if (!layer && engine.runtime >= 1) layer = thisScene.createLayer({name:'late',image:imageAsset,origin:new Vec3(64,64,0),size:new Vec2(16,16)});
    return value;
}
",
    )
}

fn discards_on_fault(mut model: paper_scene::model::SceneModel, kind: Fault) {
    let device = crate::shared::test_device();
    let mut group = super::super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 128)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    group.compose(0.0, 0.016).unwrap();
    let textures = vk::Renderer::scene_texture_live_allocations();
    let effects = vk::Renderer::effect_live_allocations();
    let drawn = |group: &super::super::super::Group| {
        group.quads.iter().filter(|quad| quad.tint[3] > 0.0).count()
    };
    let shown = drawn(&group);
    set_fault(kind);
    group.compose(1.0, 0.016).unwrap();
    if kind == Fault::Targets {
        assert_ne!(vk::Renderer::effect_live_allocations(), effects);
    }
    group.compose(2.0, 0.016).unwrap();
    group.compose(3.0, 0.016).unwrap();
    let scripts = group.scripts.as_ref().unwrap();
    assert!(scripts.models.is_empty());
    assert!(scripts.images.is_empty());
    let host = scripts.host.as_ref().unwrap();
    assert!(host.created_layer_slots().is_empty());
    assert!(host.animated());
    assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
    assert_eq!(vk::Renderer::effect_live_allocations(), effects);
    assert_eq!(drawn(&group), shown);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn model_fault_discards() {
    discards_on_fault(fixture(true), Fault::Model);
}

#[test]
#[ignore = "requires Vulkan"]
fn image_fault_discards() {
    discards_on_fault(late_image(), Fault::Image);
}

#[test]
#[ignore = "requires Vulkan"]
fn target_fault_discards() {
    discards_on_fault(fixture(true), Fault::Targets);
}
