use super::{Startup, TextBackdrop, effect_objects, script_update_due, update_backdrop};
use serde_json::json;

#[test]
fn effect_objects_preserve_first_match_and_refresh_after_scene_changes() {
    let mut scene = json!({"objects":[
        {"id":1,"alpha":0.25},
        {"id":1,"alpha":0.75},
        {"id":"quoted\"id","alpha":0.5}
    ]});
    let objects = effect_objects(&scene);
    assert_eq!(objects["1"]["alpha"], 0.25);
    assert_eq!(objects[r#"quoted\"id"#]["alpha"], 0.5);
    drop(objects);
    scene["objects"][0]["id"] = json!(2);
    scene["objects"][0]["alpha"] = json!(1);
    scene["objects"].as_array_mut().unwrap().swap(0, 1);
    let objects = effect_objects(&scene);
    assert_eq!(objects["1"]["alpha"], 0.75);
    assert_eq!(objects["2"]["alpha"], 1);
}

#[test]
fn startup_updates_once() {
    let mut startup = Startup::Pending;
    assert!(script_update_due(&mut startup, 0.0));
    assert!(!script_update_due(&mut startup, 0.0));
    assert!(script_update_due(&mut startup, 0.05));
    assert!(script_update_due(&mut startup, 0.0));
    let mut startup = Startup::Pending;
    assert!(script_update_due(&mut startup, 0.3));
    assert!(script_update_due(&mut startup, 0.1));
}

#[test]
#[ignore = "requires Vulkan"]
fn first_frame_scripted() {
    use super::super::{FillMode, build_group};
    let mut model = super::super::effect_lifetime_tests::model_files(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/flat.json","origin":"16 32 0","size":"16 16","alpha":{"value":0,"script":"let n=0;export function update(){n++;return n==1?1:0;}"}},{"id":2,"image":"models/flat.json","origin":"48 32 0","size":"16 16","alpha":{"value":0,"script":"export function init(){return 1;}"}}]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let lit = |group: &mut super::super::Group| {
        let pixels = group.read_canvas().unwrap().2;
        [16, 48].map(|x| pixels[(32 * 64 + x) * 4] > 0)
    };
    assert_eq!(lit(&mut group), [true, true]);
    group.compose(0.0, 0.0).unwrap();
    assert_eq!(lit(&mut group), [true, true]);
    group.compose(0.5, 0.5).unwrap();
    assert_eq!(lit(&mut group), [false, true]);
    group.destroy();
}

#[test]
fn backdrop_tracks_text_frame() {
    let mut quad = crate::vk::SceneQuad {
        uv_axes: None,
        uv_padding: false,
        projection: None,
        order_bias: -1,
        rect: [0.0; 4],
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: [0.0; 4],
        angle: 0.0,
        texture: 0,
        blend: crate::vk::SceneBlend::Alpha,
    };
    let mut frame = paper_scene::model::script::Frame {
        planar_owner: true,
        origin: [100.0, 200.0],
        geometry: Some(paper_scene::model::Geometry {
            spatial: None,
            depth_scale: 1.0,
            transform: paper_scene::affine::Affine::new([100.0, 200.0], [2.0, 0.5], 0.25),
            size: [60.0, 120.0],
            offset: [0.0; 2],
        }),
        visible: true,
        rect: [100.0, 200.0, 120.0, 60.0],
        angle: 0.25,
        tint: [1.0, 1.0, 1.0, 0.75],
        source_tint: [1.0; 4],
        scale: [2.0, 0.5],
        depth: 0.0,
        text: None,
    };
    let style = paper_scene::text::script::Backdrop {
        color: [0.0, 0.5, 0.0],
        padding: [10.0, 20.0],
        visible: true,
    };
    update_backdrop(
        &mut quad,
        &frame,
        &style,
        paper_scene::text::Bounds { size: [60.0, 120.0], offset: [0.0; 2] },
        (800.0, 600.0),
    );
    assert_eq!(quad.rect, [100.0, 200.0, 160.0, 80.0]);
    assert_eq!(quad.angle, 0.25);
    assert_eq!(quad.tint, [0.0, 0.5, 0.0, 0.75]);
    let parent = paper_scene::affine::Affine::new([300.0, 200.0], [-2.0, 0.5], 0.7)
        .then(paper_scene::affine::Affine::new([20.0, 30.0], [1.5, 0.75], -0.4));
    let center = parent.point([76.0, 38.0, 0.0]);
    frame.geometry = Some(paper_scene::model::Geometry {
        spatial: None,
        depth_scale: 1.0,
        transform: paper_scene::affine::Affine { x: center[0], y: center[1], ..parent },
        size: [152.0, 214.0],
        offset: [76.0, 38.0],
    });
    let style = paper_scene::text::script::Backdrop { padding: [12.0; 2], ..style };
    let bounds = paper_scene::text::Bounds { size: [150.0, 211.0], offset: [75.0, 36.5] };
    update_backdrop(&mut quad, &frame, &style, bounds, (800.0, 600.0));
    let projection = quad.projection.unwrap();
    for (u, v, x, y) in [
        (-0.5, -0.5, -12.0, -81.0),
        (0.5, -0.5, 162.0, -81.0),
        (-0.5, 0.5, -12.0, 154.0),
        (0.5, 0.5, 162.0, 154.0),
    ] {
        let expected = parent.point([x, y, 0.0]);
        let actual = [
            (projection[0][0] * u + projection[0][1] * v + projection[0][3] + 1.0) * 400.0,
            (projection[1][0] * u + projection[1][1] * v + projection[1][3] + 1.0) * 300.0,
        ];
        for axis in 0..2 {
            assert!((actual[axis] - expected[axis]).abs() < 0.0001);
        }
    }
}

#[test]
fn affine_pointer_hit() {
    let canvas = (800.0, 600.0);
    let geometry = paper_scene::model::Geometry {
        spatial: None,
        depth_scale: 1.0,
        transform: paper_scene::affine::Affine::new([300.0, 200.0], [-2.0, 1.0], 0.7)
            .then(paper_scene::affine::Affine::new([0.0; 2], [1.0; 2], 0.4)),
        size: [80.0, 40.0],
        offset: [0.0; 2],
    };
    let mut layer = super::super::tests::test_layer(true, 0);
    layer.geometry = Some(geometry);
    let mut quad = super::super::layer_quad(&layer, 0, canvas);
    for (point, hit) in [([30.0, 15.0], true), ([45.0, 15.0], false)] {
        let screen = geometry.transform.point([point[0], point[1], 0.0]);
        assert_eq!(super::quad_contains(&quad, [screen[0], screen[1]], canvas), hit);
    }
    quad.projection = Some([[0.0; 4], [0.0; 4], [0.0, 0.0, 0.0, 1.0]]);
    assert!(!super::quad_contains(&quad, [300.0, 200.0], canvas));
}

#[test]
fn hidden_alpha_geometry_hits() {
    let canvas = (800.0, 600.0);
    let mut layer = super::super::tests::test_layer(false, 0);
    layer.center = (400.0, 300.0);
    layer.size = (180.0, 100.0);
    let layout = paper_scene::model::script::Layout {
        id: "hidden".into(),
        size: [180.0, 100.0],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let hidden = super::super::layer_quad(&layer, 0, canvas);
    assert_eq!(hidden.tint[3], 0.0);
    assert_eq!(super::script_hits(&[layout], &[hidden], [400.0, 300.0], canvas), ["hidden"]);
    layer.visible = true;
    layer.alpha = 0.0;
    let alpha_zero = super::super::layer_quad(&layer, 0, canvas);
    assert_eq!(alpha_zero.tint[3], 0.0);
    let layout = paper_scene::model::script::Layout {
        id: "alpha-zero".into(),
        size: [180.0, 100.0],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    assert_eq!(
        super::script_hits(&[layout], std::slice::from_ref(&alpha_zero), [400.0, 300.0], canvas),
        ["alpha-zero"]
    );
    assert!(
        super::script_hits(&[], std::slice::from_ref(&alpha_zero), [400.0, 300.0], canvas)
            .is_empty()
    );
}

thread_local! {
    static FAIL_BACKDROP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAULT: std::cell::Cell<Option<Fault>> = const { std::cell::Cell::new(None) };
    static BACKDROP_UPLOAD_IMAGES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TEXT_RASTERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BACKDROP_INSTALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, PartialEq)]
pub(in crate::app::scene) enum Fault {
    Image,
    Model,
    Particle,
    Targets,
}

pub(in crate::app::scene) fn set_fault(kind: Fault) {
    FAULT.set(Some(kind));
}

pub(in crate::app::scene) fn fault(kind: Fault) -> anyhow::Result<()> {
    if FAULT.get() == Some(kind) {
        FAULT.set(None);
        anyhow::bail!("injected layer failure");
    }
    Ok(())
}

pub(super) fn backdrop_upload(images: usize) -> anyhow::Result<()> {
    BACKDROP_UPLOAD_IMAGES.set(images);
    if FAIL_BACKDROP.replace(false) {
        anyhow::bail!("injected backdrop upload failure");
    }
    Ok(())
}

pub(super) fn backdrop_installed() {
    BACKDROP_INSTALLS.set(BACKDROP_INSTALLS.get() + 1);
}

#[test]
#[ignore = "requires pinned dynamic text package and Vulkan"]
fn backdrop_failure_discards() {
    use super::super::{FillMode, build_group};
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"8 8 0","size":"4 4","alpha":{"value":1,
        "script":"let done=false;export function update(value){if(!done && engine.runtime>=1){done=true;thisScene.createLayer({text:'A',font:'fonts/Probe.ttf',pointsize:8,origin:'32 32 0'});}return value;}"}}]});
    let mut model = super::super::effect_lifetime_tests::dynamic_text_model(&scene);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let baseline = crate::vk::Renderer::scene_texture_live_allocations();
    let before = (group.quads.len(), group.textures.len());
    let pixels = group.read_canvas().unwrap().2;
    FAIL_BACKDROP.set(true);
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(BACKDROP_UPLOAD_IMAGES.get(), baseline + 2);
    assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), baseline);
    assert_eq!((group.quads.len(), group.textures.len()), before);
    assert_eq!(group.read_canvas().unwrap().2, pixels);
    let scripts = group.scripts.as_ref().unwrap();
    assert!(scripts.text.is_empty());
    assert!(scripts.backdrops.is_empty());
    assert!(scripts.images.is_empty());
    assert!(scripts.backdrop_texture.is_none());
    group.destroy();
}

#[test]
#[ignore = "requires pinned dynamic text package and Vulkan"]
fn image_churn_backdrops() {
    use super::super::{FillMode, build_group};
    let mut objects: Vec<_> = (0..32)
        .map(|index| {
            json!({"id":index+1,"text":"A","font":"fonts/Probe.ttf",
        "pointsize":3,"origin":format!("{} {} 0",4+index%8*8,4+index/8*8),"opaquebackground":true})
        })
        .collect();
    objects.push(json!({"id":33,"alpha":{"value":1,"script":
        "let layer;let phase=0;export function update(value){const next=Math.floor(engine.runtime);if(next===phase)return value;phase=next;if(phase%2)layer=thisScene.createLayer({image:'models/flat.json',origin:'48 48 0',size:'4 4'});else thisScene.destroyLayer(layer);return value;}"}}));
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":objects});
    let mut model = super::super::effect_lifetime_tests::dynamic_text_model(&scene);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let before = group.read_canvas().unwrap().2;
    let installs = BACKDROP_INSTALLS.get();
    for step in 1..=16 {
        group.compose(step as f32, 1.0).unwrap();
        assert_eq!(BACKDROP_INSTALLS.get(), installs);
        let scripts = group.scripts.as_ref().unwrap();
        assert_eq!(scripts.backdrops.len(), 32);
        for &TextBackdrop { layer, quad: backdrop, .. } in &scripts.backdrops {
            assert!(layer < scripts.layouts.len());
            assert!(backdrop >= scripts.layouts.len());
            assert_eq!(group.quad_scene_order[layer], group.quad_scene_order[backdrop]);
        }
        if step % 2 == 0 {
            assert_eq!(group.read_canvas().unwrap().2, before);
        }
    }
    group.destroy();
}

pub(super) fn text_raster_attempt() {
    TEXT_RASTERS.set(TEXT_RASTERS.get() + 1);
}

#[test]
#[ignore = "requires Vulkan"]
fn failed_raster_waits() {
    use super::super::{FillMode, build_group};
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"origin":"32 32 0","font":"fonts/Latin.ttf","effects":[{"file":"effects/text.json"}],
        "pointsize":8,
        "text":{"value":"A","script":"export function update(){return engine.runtime<1?'A':engine.runtime<3?'W'.repeat(400):'BB';}"}},
        {"id":2,"alpha":{"value":1,"script":"export function update(){return engine.runtime;}"}}]});
    let mut model = super::super::effect_lifetime_tests::text_effect_model(&scene.to_string());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let pixels = group.read_canvas().unwrap().2;
    let layout = group.quads[0].rect;
    let attempts = TEXT_RASTERS.get();
    let bounds = group.scripts.as_ref().unwrap().text[0].2.bounds;
    let backdrop = group.scripts.as_ref().unwrap().backdrops[0].bounds;
    for time in [1.0, 1.25, 1.5, 2.0, 2.5] {
        group.compose(time, 0.25).unwrap();
        assert_eq!(TEXT_RASTERS.get(), attempts + 1);
        assert_eq!(group.read_canvas().unwrap().2, pixels);
        assert_eq!(group.quads[0].rect, layout);
        assert_eq!(group.scripts.as_ref().unwrap().text[0].2.bounds, bounds);
        assert_eq!(group.scripts.as_ref().unwrap().backdrops[0].bounds, backdrop);
    }
    group.compose(3.0, 0.5).unwrap();
    assert_eq!(TEXT_RASTERS.get(), attempts + 2);
    let scripts = group.scripts.as_ref().unwrap();
    assert_ne!(scripts.text[0].2.bounds, bounds);
    assert_eq!(scripts.text[0].2.bounds, scripts.backdrops[0].bounds);
    assert_ne!(group.read_canvas().unwrap().2, pixels);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

#[test]
#[ignore = "requires pinned dynamic text package and Vulkan"]
fn spare_backdrop_order() {
    use super::super::{FillMode, build_group};
    let script = r"
let layers=[];
let extra;
let phase=0;
function text(){return thisScene.createLayer({text:'A',font:'fonts/Probe.ttf',pointsize:8,origin:'32 32 0',opaquebackground:true,backgroundcolor:'0 0 1'});}
export function init(){for(let index=0;index<16;index++)layers.push(text());}
export function update(value){
    const next=Math.floor(engine.runtime);
    if(next===phase)return value;
    phase=next;
    if(phase===1){for(const layer of layers)thisScene.destroyLayer(layer);layers=[];}
    if(phase===2){for(let index=0;index<16;index++)layers.push(thisScene.createLayer({image:'models/flat.json',origin:'32 32 0',size:'16 16',color:'1 0 0'}));}
    if(phase===3){for(const layer of layers)thisScene.sortLayer(layer,0);}
    if(phase===4)extra=thisScene.createLayer({image:'models/flat.json',origin:'48 48 0',size:'4 4',color:'0 1 0'});
    if(phase===5){for(const layer of layers)thisScene.destroyLayer(layer);thisScene.destroyLayer(extra);layers=[];for(let index=0;index<16;index++)layers.push(text());}
    return value;
}";
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","visible":false,"alpha":{"value":1,"script":script}}]});
    let mut model = super::super::effect_lifetime_tests::dynamic_text_model(&scene);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let initial = group.read_canvas().unwrap().2;
    let slots = (group.quads.len(), group.textures.len());
    for phase in 1..=5 {
        group.compose(phase as f32, 1.0).unwrap();
        let scripts = group.scripts.as_ref().unwrap();
        assert!(scripts.host.as_ref().unwrap().diagnostics.is_empty());
        for (&owner, &backdrop) in &scripts.spare_backdrops {
            assert_eq!(group.quad_scene_order[owner], group.quad_scene_order[backdrop]);
            assert_eq!(group.quads[backdrop].tint, [0.0; 4]);
        }
        for &TextBackdrop { layer: owner, quad: backdrop, .. } in &scripts.backdrops {
            assert_eq!(group.quad_scene_order[owner], group.quad_scene_order[backdrop]);
        }
        let ranks: std::collections::HashSet<_> =
            scripts.order.iter().map(|(_, rank)| *rank).collect();
        assert!(group.quad_scene_order.iter().all(|rank| ranks.contains(rank)));
        assert_eq!(scripts.spare_backdrops.len(), if phase < 5 { 16 } else { 0 });
        if (2..=4).contains(&phase) {
            let pixels = group.read_canvas().unwrap().2;
            let center = (32 * 64 + 32) * 4;
            assert_eq!(&pixels[center..center + 4], &[255, 0, 0, 255]);
        }
    }
    assert_eq!(group.read_canvas().unwrap().2, initial);
    assert_eq!((group.quads.len(), group.textures.len()), (slots.0 + 1, slots.1 + 1));
    group.destroy();
}

#[test]
#[ignore = "requires pinned dynamic image package and Vulkan"]
fn dynamic_sprite_reuse() {
    use super::super::{FillMode, build_group};
    let script = r"
let layer;
let old;
let phase=0;
function create(){return thisScene.createLayer({image:'models/ordinary.json',origin:'32 32 0',size:'32 16'});}
export function init(){layer=create();old=layer.getTextureAnimation();}
export function update(value){
    if(engine.runtime>=1 && phase===0){phase=1;old.pause();old.setFrame(0);}
    if(engine.runtime>=2 && phase===1){phase=2;thisScene.destroyLayer(layer);layer=thisScene.createLayer({image:'models/white.json',origin:'32 32 0',size:'32 16',color:'1 0 0'});}
    if(engine.runtime>=3 && phase===2){phase=3;thisScene.destroyLayer(layer);layer=create();const sprite=layer.getTextureAnimation();sprite.pause();sprite.setFrame(1);}
    if(engine.runtime>=4 && phase===3){phase=4;old.play();old.setFrame(0);old.pause();}
    if(engine.runtime>=5 && phase===4){phase=5;thisScene.destroyLayer(layer);}
    return value;
}";
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"alpha":{"value":1,"script":script}}]});
    let mut model = super::super::effect_lifetime_tests::dynamic_sprite_model(&scene, true);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let first = group.read_canvas().unwrap().2;
    let slots = (group.quads.len(), group.textures.len());
    let live = crate::vk::Renderer::scene_texture_live_allocations();
    assert_eq!(group.animations.len(), 1);
    let owner = group.animations[0].object.unwrap();
    let identity =
        group.scripts.as_ref().unwrap().host.as_ref().unwrap().scene["objects"][owner]["id"]
            .clone();
    group.compose(0.3, 0.3).unwrap();
    let second = group.read_canvas().unwrap().2;
    assert_ne!(first, second);
    group.compose(1.0, 0.7).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, first);
    group.compose(2.0, 1.0).unwrap();
    assert!(group.animations.is_empty());
    group.compose(3.0, 1.0).unwrap();
    assert_eq!(group.animations.len(), 1);
    assert_eq!(group.animations[0].object, Some(owner));
    assert_ne!(
        group.scripts.as_ref().unwrap().host.as_ref().unwrap().scene["objects"][owner]["id"],
        identity
    );
    assert_eq!(group.read_canvas().unwrap().2, second);
    group.compose(4.0, 1.0).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, second);
    assert_eq!((group.quads.len(), group.textures.len()), slots);
    assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), live);
    group.compose(5.0, 1.0).unwrap();
    assert!(group.animations.is_empty());
    assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), live - 1);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

#[test]
#[ignore = "requires pinned dynamic image package and Vulkan"]
fn sprite_stage_keeps_image() {
    use super::super::{FillMode, build_group};
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/white.json","visible":false,"alpha":{"value":1,"script":
        "let done=false;export function update(value){if(!done && engine.runtime>=1){done=true;thisScene.createLayer({image:'models/ordinary.json'});thisScene.createLayer({text:'A',font:'fonts/Probe.ttf',pointsize:8});}return value;}"}}]});
    let mut model = super::super::effect_lifetime_tests::dynamic_sprite_model(&scene, false);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let before = (group.quads.len(), group.textures.len());
    let baseline = crate::vk::Renderer::scene_texture_live_allocations();
    FAIL_BACKDROP.set(true);
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(BACKDROP_UPLOAD_IMAGES.get(), baseline + 3);
    assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), baseline + 1);
    assert_eq!((group.quads.len(), group.textures.len()), (before.0 + 1, before.1 + 1));
    let scripts = group.scripts.as_ref().unwrap();
    assert_eq!(scripts.images.len(), 1);
    assert!(scripts.backdrops.is_empty());
    group.destroy();
}

#[test]
#[ignore = "requires C11 scene package and Vulkan"]
fn orthographic_camera_keeps_images() {
    const FIXTURE: &str = "SKWD_C11_SCENE_PACKAGE";
    let path = std::env::var(FIXTURE).expect("C11 scene package");
    let package = paper_scene::pkg::Package::open(std::path::Path::new(&path)).unwrap();
    let mut model =
        paper_scene::model::load_with(&package, &paper_scene::effects::Assets::discover(None))
            .unwrap();
    assert_eq!(model.projection, paper_scene::model::ProjectionMode::Orthographic);
    assert!(model.scripts.is_some());
    assert!(model.layers.iter().all(|layer| layer.material.is_none()));
    let shared = crate::shared::test_device();
    let mut group = super::super::build_group(
        &shared,
        &mut model,
        true,
        &[(1280, 720)],
        super::super::FillMode::Fit,
    )
    .unwrap();
    group.compose(0.1, 1.0 / 30.0).unwrap();
    let (width, height, rgba) = group.read_canvas().unwrap();
    assert_eq!((width, height), (1280, 720));
    let pixel = |x, y| {
        let start = ((y * width + x) * 4) as usize;
        &rgba[start..start + 3]
    };
    assert!(pixel(587, 87).iter().all(|channel| *channel >= 150));
    assert!(pixel(693, 87)[1..].iter().all(|channel| *channel >= 10));
    assert_eq!(pixel(52, 447), &[0, 0, 0]);
    group.destroy();
}

#[test]
#[ignore = "requires pinned dynamic text package and Vulkan"]
fn failed_upload_keeps_bounds() {
    use super::super::{FillMode, build_group};
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"origin":"32 32 0","font":"fonts/Probe.ttf","text":"A",
        "pointsize":{"value":8,"script":"export function update(){return engine.runtime>=1?12:8;}"},
        "opaquebackground":true,"padding":"2 2"}]});
    let mut model = super::super::effect_lifetime_tests::dynamic_text_model(&scene);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let scripts = group.scripts.as_ref().unwrap();
    let bounds = scripts.text[0].2.bounds;
    let backdrop = scripts.backdrops[0];
    let rect = group.quads[0].rect;
    let texture = group.textures[scripts.text[0].1].view;
    let pixels = group.read_canvas().unwrap().2;
    group.scripts.as_mut().unwrap().text_upload_error =
        Some(anyhow::anyhow!("injected text upload failure"));
    assert!(
        group.compose(1.0, 0.1).unwrap_err().to_string().contains("injected text upload failure")
    );
    let scripts = group.scripts.as_ref().unwrap();
    assert_eq!(scripts.text[0].2.bounds, bounds);
    assert_eq!(scripts.backdrops[0], backdrop);
    assert_eq!(group.textures[scripts.text[0].1].view, texture);
    assert_eq!(group.quads[0].rect, rect);
    assert_eq!(group.read_canvas().unwrap().2, pixels);
    group.compose(2.0, 0.1).unwrap();
    let scripts = group.scripts.as_ref().unwrap();
    assert_ne!(scripts.text[0].2.bounds, bounds);
    assert_ne!(group.textures[scripts.text[0].1].view, texture);
    assert_ne!(group.quads[0].rect, rect);
    assert_eq!(scripts.text[0].2.bounds, scripts.backdrops[0].bounds);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn failed_resize_rejects_text() {
    use super::super::{FillMode, build_group};
    let mut model = super::super::effect_lifetime_tests::text_effect_model(
        super::super::effect_lifetime_tests::TEXT_EFFECT_SCENE,
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let scripts = group.scripts.as_ref().unwrap();
    let (bounds, slot) = (scripts.text[0].2.bounds, scripts.text[0].1);
    let texture = group.textures[slot].view;
    let ping = group.fx[0].ping.get(&group.fx_pool).view;
    let bytes = group.effect_target_allocation_bytes();
    let targets = crate::vk::Renderer::scene_target_live_allocations();
    crate::vk::Renderer::fail_scene_target_after(Some(1));
    group.compose(1.0, 0.1).unwrap();
    crate::vk::Renderer::fail_scene_target_after(None);
    let scripts = group.scripts.as_ref().unwrap();
    assert_eq!(scripts.text[0].2.bounds, bounds);
    assert_eq!(group.textures[slot].view, texture);
    assert_eq!(group.fx[0].ping.get(&group.fx_pool).view, ping);
    assert_eq!(group.effect_target_allocation_bytes(), bytes);
    assert_eq!(crate::vk::Renderer::scene_target_live_allocations(), targets);
    group.compose(2.0, 0.1).unwrap();
    assert_eq!(group.textures[slot].view, texture);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_LIFECYCLE_ROOT"]
fn effect_padding_transaction() {
    use super::super::{FillMode, build_group};
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_TEXT_LIFECYCLE_ROOT").unwrap());
    let dir = root.join("length-cycle");
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&package, &dir).unwrap();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let layer = group.fx[0].quad;
    let scripts = group.scripts.as_ref().unwrap();
    let (_, slot, text) = scripts.text.iter().find(|(index, _, _)| *index == layer).unwrap();
    let slot = *slot;
    let bounds = text.bounds;
    let backdrop = *scripts.backdrops.iter().find(|backdrop| backdrop.layer == layer).unwrap();
    let rect = group.quads[layer].rect;
    let texture = group.textures[slot].view;
    let target = group.fx[0].ping.get(&group.fx_pool);
    let view = target.view;
    let extent = (target.extent.width, target.extent.height);
    let pixels = group.read_canvas().unwrap().2;
    let scripts = group.scripts.as_mut().unwrap();
    let host = scripts.host.as_mut().unwrap();
    let object = paper_scene::model::object_index(&host.scene, &scripts.layouts[layer].id).unwrap();
    host.scene["objects"][object]["padding"] = json!(12);
    scripts.pending = true;
    scripts.text_upload_error = Some(anyhow::anyhow!("injected text upload failure"));
    assert!(
        group.compose(0.1, 0.1).unwrap_err().to_string().contains("injected text upload failure")
    );
    let scripts = group.scripts.as_ref().unwrap();
    assert_eq!(*scripts.backdrops.iter().find(|entry| entry.layer == layer).unwrap(), backdrop);
    assert_eq!(group.textures[slot].view, texture);
    assert_eq!(group.fx[0].ping.get(&group.fx_pool).view, view);
    assert_eq!(group.quads[layer].rect, rect);
    assert_eq!(group.read_canvas().unwrap().2, pixels);
    group.compose(0.3, 0.1).unwrap();
    let target = group.fx[0].ping.get(&group.fx_pool);
    assert_eq!((target.extent.width, target.extent.height), (extent.0 + 24, extent.1 + 24));
    assert_ne!(target.view, view);
    assert_ne!(group.textures[slot].view, texture);
    let scripts = group.scripts.as_ref().unwrap();
    let accepted = scripts.backdrops.iter().find(|entry| entry.layer == layer).unwrap();
    assert_eq!(accepted.bounds, bounds);
    assert_eq!(accepted.padding, Some([12.0; 2]));
    let texture = group.textures[slot].view;
    group.compose(0.4, 0.1).unwrap();
    assert_eq!(group.textures[slot].view, texture);
    group.destroy();
}

#[test]
fn passthrough_ignores_tint() {
    let frame = paper_scene::model::script::Frame {
        planar_owner: true,
        origin: [0.0; 2],
        geometry: None,
        visible: true,
        rect: [400.0, 300.0, 800.0, 600.0],
        angle: 0.0,
        tint: [1.0, 0.0, 0.0, 0.0],
        source_tint: [1.0, 0.0, 0.0, 0.0],
        scale: [1.0; 2],
        depth: 0.0,
        text: None,
    };
    for (copy_background, base) in [(true, [1.0; 4]), (false, [0.0; 4])] {
        let tints = super::chain_tints(&frame, false, false, Some(copy_background));
        assert_eq!(tints.base, base);
        assert_eq!(tints.fallback, [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(tints.alpha, 1.0);
    }
    let plain = super::chain_tints(&frame, false, false, None);
    assert_eq!((plain.base, plain.alpha), (frame.source_tint, 0.0));
}
