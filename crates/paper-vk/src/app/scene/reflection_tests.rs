use super::*;
use crate::app::scene::effect_lifetime_tests::texture_levels;
use crate::app::scene::effect_space_tests::PASS_VERTEX;
use crate::app::scene::tests::{
    MESH_MATERIAL, particle_clock, particle_package, sound_package, triangle_mdl,
};
use crate::app::scene::{FillMode, build_group};
use serde_json::json;

const SIZE: u32 = 64;
const READER: &[u8] = br#"{"passes":[{"shader":"reader","cullmode":"nocull","textures":["_rt_Reflection"],"blending":"normal"}]}"#;
const SOURCE: &[u8] =
    br#"{"passes":[{"shader":"source","textures":["textures/white"],"blending":"normal"}]}"#;
const PLAIN: &[u8] =
    br#"{"passes":[{"shader":"genericimage4","textures":["textures/white"],"blending":"normal"}]}"#;
const VERTEX: &[u8] = b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";

const PARTICLE: &str = "particles/p.json";
const FLAT_MATERIAL: &[u8] = b"materials/flat.json";
const FLAT: &[u8] = br#"{"passes":[{"shader":"flat","cullmode":"nocull","blending":"normal"}]}"#;

fn scene(height: f32, objects: &serde_json::Value) -> paper_scene::model::SceneModel {
    let scene = json!({
        "camera":{"eye":format!("0 {height} -4"),"center":format!("0 {height} 0"),"up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"0 0 1"},
        "objects":objects
    });
    let scene = serde_json::to_vec(&scene).unwrap();
    let mesh = triangle_mdl();
    let mut flat = mesh.clone();
    let at = flat.windows(MESH_MATERIAL.len()).position(|window| window == MESH_MATERIAL).unwrap();
    flat[at..at + MESH_MATERIAL.len()].copy_from_slice(FLAT_MATERIAL);
    let white =
        texture_levels(&[paper_scene::tex::MipLevel { width: 1, height: 1, data: vec![255; 4] }]);
    let particles = particle_package();
    let particle = |path: &'static str| (path, particles.find(path).unwrap());
    let package = sound_package(&[
        particle(PARTICLE),
        ("models/util/fullscreenlayer.json", br#"{"material":"materials/util/fullscreenlayer.json","fullscreen":true,"passthrough":true}"#),
        ("materials/util/fullscreenlayer.json", br#"{"passes":[{"shader":"passthrough","depthtest":"disabled","depthwrite":"disabled","blending":"translucent","cullmode":"nocull","textures":["_rt_FullFrameBuffer"]}]}"#),
        ("effects/invert.json", br#"{"passes":[{"material":"materials/invert.json"}]}"#),
        ("materials/invert.json", br#"{"passes":[{"shader":"invert"}]}"#),
        ("shaders/invert.vert", PASS_VERTEX),
        ("shaders/invert.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=vec4(1.0-texSample2D(g_Texture0,v_TexCoord).rgb,1.0);}"),
        particle("materials/p.json"),
        particle("materials/mask.tex"),
        particle("shaders/particle.vert"),
        particle("shaders/particle.frag"),
        ("scene.json", &scene),
        ("materials/textures/white.tex", &white),
        ("models/reader.mdl", &mesh),
        ("models/flat.mdl", &flat),
        ("models/source.json", br#"{"material":"materials/source.json"}"#),
        ("materials/mesh.json", READER),
        ("materials/flat.json", FLAT),
        ("materials/source.json", SOURCE),
        ("models/plain.json", br#"{"material":"materials/plain.json"}"#),
        ("materials/plain.json", PLAIN),
        ("shaders/reader.vert", VERTEX),
        ("shaders/reader.frag", b"uniform float g_Time;\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=vec4(texSample2D(g_Texture0,vec2(0.5,0.5)).r,fract(g_Time),0.0,1.0);}"),
        ("shaders/flat.vert", VERTEX),
        ("shaders/flat.frag", b"void main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}"),
        ("shaders/source.vert", VERTEX),
        ("shaders/source.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
    ]);
    paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )
    .unwrap()
}

fn reflected(height: f32, objects: &serde_json::Value) -> Vec<u8> {
    let shared = crate::shared::test_device();
    let mut model = scene(height, objects);
    let mut group = build_group(&shared, &mut model, true, &[(SIZE, SIZE)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let reflection = group.reflection.as_ref().unwrap();
    let pixels = group.renderer.read_scene_target(reflection.target()).unwrap().2;
    group.destroy();
    pixels
}

fn covered(pixels: &[u8]) -> Vec<usize> {
    let rows = pixels.chunks_exact(4).enumerate();
    rows.filter(|(_, pixel)| pixel[0] > 200).map(|(index, _)| index).collect()
}

fn red_pixels(pixels: &[u8]) -> usize {
    pixels.chunks_exact(4).filter(|pixel| pixel[0] > 200 && pixel[2] < 50).count()
}

#[test]
fn orthographic_rows_leave_canvas() {
    let rows =
        orthographic_rows([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, -0.5], [0.0, 0.0, 0.0, 1.0]]);
    assert_eq!(rows[CLIP_Y], [0.0, -1.0, 0.0, -1.5]);
}

#[test]
#[ignore = "requires Vulkan"]
fn material_enters_reflection() {
    let reader = json!({"id":1,"model":"models/reader.mdl"});
    let source =
        json!({"id":2,"image":"models/source.json","origin":"0 0 0","size":"1 1","scale":"1 1 1"});
    let mut hidden = source.clone();
    hidden["reflected"] = json!(false);
    assert!(red_pixels(&reflected(0.0, &json!([reader, source]))) > 0);
    let unreflected = reflected(0.0, &json!([reader, hidden]));
    assert_eq!(red_pixels(&unreflected), 0);
    assert!(unreflected.chunks_exact(4).all(|pixel| pixel[2] > 200));
}

#[test]
#[ignore = "requires Vulkan"]
fn plain_matches_material() {
    let reader = json!({"id":1,"model":"models/reader.mdl"});
    let place = |image: &str| json!([reader, {"id":2,"image":image,"origin":"0 0.5 0","size":"1 1","scale":"1 1 1"}]);
    let material = covered(&reflected(0.25, &place("models/source.json")));
    let plain = covered(&reflected(0.25, &place("models/plain.json")));
    assert!(!material.is_empty());
    assert_eq!(material.first(), plain.first());
    assert_eq!(material.last(), plain.last());
}

#[test]
#[ignore = "requires a Vulkan device"]
fn particle_mirror_lifetime() -> Result<()> {
    let mut fixture = crate::app::scene::tests::DynamicParticleFixture::new()?;
    let layer = fixture.layer("@mirror")?;
    let group = &mut fixture.group;
    let reflection = super::super::mesh::Reflection::new(&mut group.renderer, &group.target, true)?;
    group.reflection = Some(reflection);
    let baseline = vk::Renderer::effect_live_allocations();
    group.add_dynamic_particle(layer)?;
    assert!(group.particles[0].engines[0].mirror.is_some());
    group.step_particles(particle_clock(0.5))?;
    let (_, draws) = group.particle_quads(0.5, 0.5)?;
    let span = ParticleBatches { from: 0, before: None, mirrored: true };
    let batches =
        crate::app::scene::particle_batches(&group.particles, &[], &[], &draws, span, None);
    assert_eq!(batches.len(), draws.len());
    assert!(!batches.is_empty());
    assert!(std::ptr::eq(
        batches[0].pipeline,
        group.particles[0].engines[0].mirror.as_ref().unwrap()
    ));
    group.retire_dynamic_particle("@mirror", false);
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan"]
fn runtime_model_samples() {
    let shared = crate::shared::test_device();
    let script = "let layer;export function update(value){if(!layer)layer=thisScene.createLayer('models/reader.mdl');return value;}";
    let source = json!({"id":2,"image":"models/source.json","size":"1 1","origin":{"script":script,"value":"0 0 0"}});
    let mut model = scene(0.0, &json!([source]));
    let mut group = build_group(&shared, &mut model, true, &[(SIZE, SIZE)], FillMode::Fit).unwrap();
    assert!(group.reflection.is_none());
    assert_eq!(group.unmirrored_layers(), 1);
    let green = |group: &mut Group, time: f32| {
        group.compose(time, 0.25).unwrap();
        let pixels = group.renderer.read_scene_target(&group.target).unwrap().2;
        pixels.chunks_exact(4).map(|pixel| u32::from(pixel[1])).sum::<u32>()
    };
    green(&mut group, 0.0);
    let early = green(&mut group, 0.25);
    let late = green(&mut group, 0.75);
    assert!(group.reflection.as_ref().is_some_and(|reflection| reflection.sampled));
    group.destroy();
    assert!(late > early, "{early} {late}");
}

#[test]
#[ignore = "requires Vulkan"]
fn runtime_twin_lifetime() {
    let shared = crate::shared::test_device();
    let script = "let layer;let phase=0;export function update(value){phase++;if(phase%2==1){layer=thisScene.createLayer('models/flat.mdl');}else if(layer){thisScene.destroyLayer(layer);layer=null;}return value;}";
    let reader = json!({"id":1,"model":"models/reader.mdl"});
    let host = json!({"id":2,"image":"models/source.json","size":"1 1","origin":{"script":script,"value":"0 0 0"}});
    let mut model = scene(0.0, &json!([reader, host]));
    let mut group = build_group(&shared, &mut model, true, &[(SIZE, SIZE)], FillMode::Fit).unwrap();
    let mut baseline = None;
    let mut mirrored = 0;
    for frame in 0..6_u8 {
        group.compose(f32::from(frame) * 0.25, 0.25).unwrap();
        let reflection = group.reflection.as_ref().unwrap();
        if reflection.twins.is_empty() {
            let live = vk::Renderer::effect_live_allocations();
            assert_eq!(*baseline.get_or_insert(live), live);
            continue;
        }
        let [twin] = reflection.twins.values().collect::<Vec<_>>()[..] else { panic!("twins") };
        assert!(group.textures[twin.quad.texture].draws_mesh());
        assert!(group.quad_scene_order.contains(&twin.order));
        mirrored += 1;
    }
    group.destroy();
    assert!(baseline.is_some() && mirrored > 0);
}

#[test]
#[ignore = "requires Vulkan"]
fn init_reader_mirrors_layers() {
    let script =
        "export function init(value){thisScene.createLayer('models/reader.mdl');return value;}";
    let source =
        json!({"id":1,"image":"models/source.json","origin":"0 0 0","size":"1 1","scale":"1 1 1"});
    let hidden = json!({"id":2,"image":"models/plain.json","origin":"0 0 0","size":"2 2","scale":"1 1 1","reflected":false});
    let host = json!({"id":3,"image":"models/plain.json","size":"0 0","origin":{"script":script,"value":"9 9 9"}});
    let pixels = reflected(0.0, &json!([hidden, source, host]));
    assert!(red_pixels(&pixels) > 0);
    assert!(!pixels.chunks_exact(4).any(|pixel| pixel[0] > 200 && pixel[1] > 200));
}

#[test]
fn staging_sorts_twins() {
    let quad = |texture| vk::SceneQuad {
        rect: [0.0; 4],
        uv: [0.0; 4],
        uv_axes: None,
        uv_padding: false,
        tint: [1.0; 4],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture,
        blend: vk::SceneBlend::Alpha,
    };
    let mut staging = Staging::default();
    staging.sources.extend([quad(0), quad(1), quad(2)]);
    staging.orders.extend([5, 1, 3]);
    staging.sort();
    assert_eq!(staging.orders, [1, 3, 5]);
    assert_eq!(staging.ordered.iter().map(|quad| quad.texture).collect::<Vec<_>>(), [1, 2, 0]);
}

#[test]
#[ignore = "requires Vulkan"]
fn particles_leave_once() {
    let shared = crate::shared::test_device();
    let reader = json!({"id":1,"model":"models/reader.mdl"});
    let particle = json!({"id":2,"particle":PARTICLE,"origin":"0 0 0"});
    let mut model = scene(0.0, &json!([reader, particle]));
    let mut group = build_group(&shared, &mut model, true, &[(SIZE, SIZE)], FillMode::Fit).unwrap();
    let mask = |group: &mut Group, time: f32| {
        group.compose(time, 0.5).unwrap();
        let target = group.reflection.as_ref().unwrap().target();
        let pixels = group.renderer.read_scene_target(target).unwrap().2;
        pixels.chunks_exact(4).filter(|pixel| pixel[0] > 200 && pixel[2] < 100).count()
    };
    mask(&mut group, 0.5);
    assert!(mask(&mut group, 1.0) > 0);
    group.particles[0].visible = false;
    let left = mask(&mut group, 1.5);
    group.destroy();
    assert_eq!(left, 0);
}

#[test]
#[ignore = "requires Vulkan"]
fn fullscreen_effects_rerun() {
    let reader = json!({"id":1,"model":"models/reader.mdl"});
    let source =
        json!({"id":2,"image":"models/source.json","origin":"0 0 0","size":"1 1","scale":"1 1 1"});
    let post = json!({"id":3,"image":"models/util/fullscreenlayer.json","effects":[{"file":"effects/invert.json"}]});
    let pixels = reflected(0.0, &json!([reader, source, post]));
    let cyan =
        pixels.chunks_exact(4).filter(|pixel| pixel[0] < 50 && pixel[1] > 200 && pixel[2] > 200);
    assert!(cyan.count() > 0);
    assert!(pixels.chunks_exact(4).all(|pixel| pixel[1] > 200));
}

#[test]
#[ignore = "requires Vulkan"]
fn disabled_clear_keeps() {
    let shared = crate::shared::test_device();
    for enabled in [true, false] {
        let reader = json!({"id":1,"model":"models/reader.mdl"});
        let source = json!({"id":2,"image":"models/source.json","origin":"0 0 0","size":"1 1","scale":"1 1 1"});
        let mut model = scene(0.0, &json!([source, reader]));
        model.clear_enabled = enabled;
        let mut group =
            build_group(&shared, &mut model, true, &[(SIZE, SIZE)], FillMode::Fit).unwrap();
        group.compose(0.0, 0.0).unwrap();
        group.quads[0].tint[super::super::ALPHA_CHANNEL] = 0.0;
        group.compose(1.0, 1.0).unwrap();
        let main = group.read_canvas().unwrap().2;
        let target = group.reflection.as_ref().unwrap().target();
        let mirrored = group.renderer.read_scene_target(target).unwrap().2;
        group.destroy();
        assert_eq!(red_pixels(&main) > 0, !enabled);
        assert_eq!(red_pixels(&mirrored) > 0, !enabled);
    }
}
