use super::*;
use crate::vk;

const PROJECT_ENV: &str = "SKWD_WE_FROZEN_PROJECT";
const NATIVE_FULL_ENV: &str = "SKWD_WE_FROZEN_NATIVE_FULL";
const NATIVE_HALF_ENV: &str = "SKWD_WE_FROZEN_NATIVE_HALF";

const ORTHO_PROJECT_ENV: &str = "SKWD_WE_FROZEN_ORTHO_PROJECT";
const ORTHO_FULL_ENV: &str = "SKWD_WE_FROZEN_ORTHO_FULL";
const ORTHO_HALF_ENV: &str = "SKWD_WE_FROZEN_ORTHO_HALF";
const BUDGET_PROJECT_ENV: &str = "SKWD_WE_FROZEN_BUDGET_PROJECT";
const BUDGET_HALF_ENV: &str = "SKWD_WE_FROZEN_BUDGET_HALF";
const EFFECT_PROJECT_ENV: &str = "SKWD_WE_FROZEN_EFFECT_PROJECT";

fn pixels(group: &mut Group) -> Result<image::RgbImage> {
    let (width, height, rgba) = group.renderer.read_scene_target(&group.target)?;
    let image = image::RgbaImage::from_raw(width, height, rgba).expect("scene pixel extent");
    Ok(image::DynamicImage::ImageRgba8(image).to_rgb8())
}

#[test]
#[ignore = "requires Vulkan and pinned resize project and native captures"]
fn frozen_resize_matches_native() -> Result<()> {
    native_resize(false)
}

#[test]
#[ignore = "requires Vulkan and orthographic mesh project with pinned native captures"]
fn frozen_orthographic_resize() -> Result<()> {
    native_resize(true)
}

fn native_resize(orthographic: bool) -> Result<()> {
    let shared = crate::shared::test_device();
    let project = if orthographic { ORTHO_PROJECT_ENV } else { PROJECT_ENV };
    let full = if orthographic { ORTHO_FULL_ENV } else { NATIVE_FULL_ENV };
    let half = if orthographic { ORTHO_HALF_ENV } else { NATIVE_HALF_ENV };
    let dir = std::env::var(project)?;
    let properties = paper_scene::model::Properties::new();
    let full = image::open(std::env::var(full)?)?.to_rgb8();
    let half = image::open(std::env::var(half)?)?.to_rgb8();
    let identity = identity(&dir, &properties)?;
    let mut source = SceneSource {
        shared: &shared,
        dir: &dir,
        properties: &properties,
        identity: &identity,
        outputs: &[(1280, 720)],
        mode: FillMode::Fill,
        strict: true,
        particles_disabled: false,
    };
    let mut active = source.build()?;
    assert_eq!(pixels(&mut active)?, full);
    assert!(active.frozen);
    assert_eq!(active.target.viewport.is_some(), orthographic);
    assert!(active.meshes.is_empty());
    for (outputs, native) in [(&[(640, 360)][..], &half), (&[(1280, 720)][..], &full)] {
        source.outputs = outputs;
        let mut candidate = source.build()?;
        assert_eq!(pixels(&mut candidate)?, *native);
        assert!(candidate.frozen);
        assert_eq!(candidate.target.viewport.is_some(), orthographic);
        let old = std::mem::replace(&mut active, candidate);
        old.destroy();
    }
    let live = vk::Renderer::scene_target_live_allocations();
    let resources = vk::Renderer::effect_live_allocations();
    for fault in [Fault::Target, Fault::Compose] {
        for _ in 0..3 {
            let allocations = active.renderer.scene_image_allocations();
            FAULT.set(fault);
            assert!(source.build().is_err());
            if fault == Fault::Compose {
                assert!(OBSERVED.get()[2] > resources[2]);
            }
            assert_eq!(vk::Renderer::effect_live_allocations(), resources);
            assert!(active.renderer.scene_image_allocations() > allocations);
            assert_eq!(vk::Renderer::scene_target_live_allocations(), live);
            assert_eq!(pixels(&mut active)?, full);
        }
        let recovered = source.build()?;
        recovered.destroy();
        assert_eq!(vk::Renderer::effect_live_allocations(), resources);
        assert_eq!(vk::Renderer::scene_target_live_allocations(), live);
    }
    source.dir = "missing-resize-project";
    assert!(source.build().is_err());
    assert_eq!(pixels(&mut active)?, full);
    active.destroy();
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Target,
    Effect,
    Compose,
}

thread_local! {
    static OBSERVED: std::cell::Cell<[usize; 3]> = const { std::cell::Cell::new([0; 3]) };
    static OBSERVED_HISTORY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FAULT: std::cell::Cell<Fault> = const { std::cell::Cell::new(Fault::None) };
}

pub(crate) fn fail_next_effect() {
    FAULT.set(Fault::Effect);
}

pub(crate) fn after_target() -> Result<()> {
    fail_at(Fault::Target)
}

pub(crate) fn after_effect() -> Result<()> {
    if FAULT.get() == Fault::Effect {
        OBSERVED.set(vk::Renderer::effect_live_allocations());
    }
    fail_at(Fault::Effect)
}

pub(crate) fn after_compose() -> Result<()> {
    if FAULT.get() == Fault::Compose {
        OBSERVED.set(vk::Renderer::effect_live_allocations());
        OBSERVED_HISTORY.set(vk::Renderer::scene_history_live_allocations());
    }
    fail_at(Fault::Compose)
}

fn fail_at(point: Fault) -> Result<()> {
    if FAULT.get() == point {
        FAULT.set(Fault::None);
        anyhow::bail!("injected scene build failure");
    }
    Ok(())
}

#[test]
fn span_tracks_arrangement() {
    let outputs = [(640, 720), (640, 720)];
    let horizontal = composition_outputs(&outputs, FillMode::Span, Some((1280, 720)));
    let vertical = composition_outputs(&outputs, FillMode::Span, Some((640, 1440)));
    assert_ne!(horizontal, vertical);
    assert_eq!(horizontal, [(1280, 720)]);
    assert_eq!(vertical, [(640, 1440)]);
    assert_eq!(composition_outputs(&outputs, FillMode::Fill, Some((640, 1440))), outputs);
}

#[test]
fn span_freeze_scope() {
    assert!(freezes_composition(FillMode::Span, true, None));
    assert!(!freezes_composition(FillMode::Span, true, Some("DP-1")));
    assert!(!freezes_composition(FillMode::Span, false, None));
    assert!(!freezes_composition(FillMode::Fill, true, None));
}

#[test]
#[ignore = "requires Vulkan and pinned resize project and native captures"]
fn changed_project_preserves_frame() -> Result<()> {
    let original = std::env::var(PROJECT_ENV)?;
    let directory = tempfile::tempdir()?;
    for name in ["scene.pkg", "project.json"] {
        std::fs::copy(std::path::Path::new(&original).join(name), directory.path().join(name))?;
    }
    let dir = directory.path().to_str().expect("fixture path");
    let properties = paper_scene::model::Properties::new();
    let identity = identity(dir, &properties)?;
    let shared = crate::shared::test_device();
    let source = SceneSource {
        shared: &shared,
        dir,
        identity: &identity,
        properties: &properties,
        outputs: &[(1280, 720)],
        mode: FillMode::Fill,
        strict: true,
        particles_disabled: false,
    };
    let mut active = source.build()?;
    let original = pixels(&mut active)?;
    let path = directory.path().join("project.json");
    let mut project: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    project["description"] = serde_json::Value::String("changed during resize".into());
    std::fs::write(&path, serde_json::to_vec(&project)?)?;
    let allocations = active.renderer.scene_image_allocations();
    assert!(source.build().is_err());
    assert_eq!(active.renderer.scene_image_allocations(), allocations);
    assert_eq!(pixels(&mut active)?, original);
    active.destroy();
    Ok(())
}

fn identity(
    dir: &str,
    properties: &paper_scene::model::Properties,
) -> Result<paper_scene::source::Identity> {
    let capture = paper_scene::source::Capture::start()?;
    let package = paper_scene::pkg::Package::open(&locate_pkg(dir)?)?;
    let _ =
        paper_scene::model::load_from_dir_with(&package, std::path::Path::new(dir), properties)?;
    capture.finish()
}

#[test]
#[ignore = "requires Vulkan, pinned resize inputs, and SKWD_VK_SCENE_FX_BYTES=4"]
fn budget_failure_keeps_frame() -> Result<()> {
    let shared = crate::shared::test_device();
    let dir = std::env::var(BUDGET_PROJECT_ENV)?;
    let properties = paper_scene::model::Properties::new();
    let identity = identity(&dir, &properties)?;
    let native = image::open(std::env::var(BUDGET_HALF_ENV)?)?.to_rgb8();
    let mut source = SceneSource {
        shared: &shared,
        dir: &dir,
        identity: &identity,
        properties: &properties,
        outputs: &[(640, 360)],
        mode: FillMode::Fill,
        strict: true,
        particles_disabled: false,
    };
    let mut active = source.build()?;
    assert_eq!(pixels(&mut active)?, native);
    let live = vk::Renderer::scene_target_live_allocations();
    source.outputs = &[(1280, 720)];
    for _ in 0..3 {
        let allocations = active.renderer.scene_image_allocations();
        let error = source.build().err().expect("budget rejection");
        assert!(error.to_string().contains("scene input targets exceed effect budget"));
        assert!(active.renderer.scene_image_allocations() > allocations);
        assert_eq!(vk::Renderer::scene_target_live_allocations(), live);
        assert_eq!(pixels(&mut active)?, native);
    }
    source.outputs = &[(640, 360)];
    let mut recovered = source.build()?;
    assert_eq!(pixels(&mut recovered)?, native);
    recovered.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live);
    active.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan, resize project, full native and SKWD_WE_FROZEN_NATIVE_STACKED"]
fn span_resize_matches_native() -> Result<()> {
    const STACKED_ENV: &str = "SKWD_WE_FROZEN_NATIVE_STACKED";
    let shared = crate::shared::test_device();
    let dir = std::env::var(PROJECT_ENV)?;
    let properties = paper_scene::model::Properties::new();
    let identity = identity(&dir, &properties)?;
    let horizontal = image::open(std::env::var(NATIVE_FULL_ENV)?)?.to_rgb8();
    let vertical = image::open(std::env::var(STACKED_ENV)?)?.to_rgb8();
    let outputs = [(640, 720), (640, 720)];
    for (span, native) in
        [((1280, 720), &horizontal), ((640, 1440), &vertical), ((1280, 720), &horizontal)]
    {
        let composition = composition_outputs(&outputs, FillMode::Span, Some(span));
        let source = SceneSource {
            shared: &shared,
            dir: &dir,
            identity: &identity,
            properties: &properties,
            outputs: &composition,
            mode: FillMode::Span,
            strict: true,
            particles_disabled: false,
        };
        let mut candidate = source.build()?;
        assert_eq!(pixels(&mut candidate)?, *native);
        candidate.destroy();
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_FROZEN_EFFECT_PROJECT containing a static effect chain"]
fn effect_failure_keeps_frame() -> Result<()> {
    let shared = crate::shared::test_device();
    let dir = std::env::var(EFFECT_PROJECT_ENV)?;
    let properties = paper_scene::model::Properties::new();
    let identity = identity(&dir, &properties)?;
    let source = SceneSource {
        shared: &shared,
        dir: &dir,
        identity: &identity,
        properties: &properties,
        outputs: &[(1280, 720)],
        mode: FillMode::Fill,
        strict: true,
        particles_disabled: false,
    };
    let mut active = source.build()?;
    assert!(active.frozen);
    let original = pixels(&mut active)?;
    let targets = vk::Renderer::scene_target_live_allocations();
    let resources = vk::Renderer::effect_live_allocations();
    for fault in [Fault::Effect, Fault::Compose] {
        for _ in 0..3 {
            FAULT.set(fault);
            let error = source.build().err().expect("injected failure");
            assert!(error.to_string().contains("injected scene build failure"));
            if fault == Fault::Effect {
                let observed = OBSERVED.get();
                assert!(observed[0] > resources[0]);
                assert!(observed[1] > resources[1]);
            }
            assert_eq!(vk::Renderer::effect_live_allocations(), resources);
            assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
            assert_eq!(pixels(&mut active)?, original);
        }
        let mut recovered = source.build()?;
        assert_eq!(pixels(&mut recovered)?, original);
        recovered.destroy();
        assert_eq!(vk::Renderer::effect_live_allocations(), resources);
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
    }
    active.destroy();
    Ok(())
}

const HISTORY_PROJECT_ENV: &str = "SKWD_WE_HISTORY_RESIZE_PROJECT";
const HISTORY_FULL_ENV: &str = "SKWD_WE_HISTORY_RESIZE_FULL";
const HISTORY_HALF_ENV: &str = "SKWD_WE_HISTORY_RESIZE_HALF";

fn history_model() -> SceneModel {
    super::super::tests::mesh_sampler_model(
        br#"{"general":{"orthogonalprojection":{"width":16,"height":16}},"objects":[{"id":1,"image":"models/sample.json","origin":"8 8 0","size":"16 16"}]}"#,
        br#"{"passes":[{"shader":"sample","textures":["_rt_MipMappedFrameBuffer"],"blending":"normal"}]}"#,
        b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=vec4(texSample2D(g_Texture0,vec2(0.5)).rgb,1.0);}",
    )
}

#[test]
fn history_rejects_clocks() {
    for vertex in [false, true] {
        for name in [
            "g_Time",
            "g_Frametime",
            "g_Daytime",
            "g_AudioSpectrum16Left",
            "g_PointerPosition",
            "g_ParallaxPosition",
        ] {
            let mut model = history_model();
            assert!(static_inputs(&model));
            let material = model.layers[0].material.as_mut().unwrap();
            let pass = &mut material.pass;
            let shader = if vertex { &mut pass.vertex } else { &mut pass.fragment };
            shader.uniforms.push(paper_scene::shader::Uniform {
                name: name.into(),
                kind: paper_scene::shader::UniformKind::Float,
                default: None,
                material: None,
                offset: 0,
                count: 1,
            });
            assert!(!static_inputs(&model));
        }
    }
}
#[test]
fn history_rejects_state() {
    let cases: [fn(&mut SceneModel); 7] = [
        |model| model.hdr = true,
        |model| model.shake = Some(paper_scene::mouse::Shake::default()),
        |model| model.mouse.amount = 1.0,
        |model| model.mouse.influence = 1.0,
        |model| model.layers[0].spritesheet = true,
        |model| {
            model.layers[0].texture.system_texture =
                Some(paper_scene::model::MediaTexture::Current);
        },
        |model| {
            model.animated_scene =
                Some((serde_json::Value::Null, std::collections::BTreeMap::default()));
        },
    ];
    for change in cases {
        let mut model = history_model();
        assert!(static_inputs(&model));
        change(&mut model);
        assert!(!static_inputs(&model));
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn history_rebuild_clears_rgb() -> Result<()> {
    let shared = crate::shared::test_device();
    let histories = vk::Renderer::scene_history_live_allocations();
    let mut active = build_group(&shared, &mut history_model(), true, &[(16, 16)], FillMode::Fill)?;
    assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 1);
    assert!(active.reconstructable() && active.animated() && !active.frozen);
    active.renderer.render_scene(&active.target, [1.0, 0.0, 0.0, 1.0], &[], &[])?;
    active.update_scene_history()?;
    active.compose(1.0, 0.0)?;
    assert!(pixels(&mut active)?.pixels().all(|pixel| pixel.0 == [255, 0, 0]));
    let mut candidate =
        build_group(&shared, &mut history_model(), true, &[(8, 8)], FillMode::Fill)?;
    assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 2);
    assert!(pixels(&mut candidate)?.pixels().all(|pixel| pixel.0 == [0, 0, 0]));
    active.compose(2.0, 0.0)?;
    assert!(pixels(&mut active)?.pixels().all(|pixel| pixel.0 == [255, 0, 0]));
    candidate.destroy();
    assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 1);
    active.destroy();
    assert_eq!(vk::Renderer::scene_history_live_allocations(), histories);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original history resize project and native captures"]
fn history_resize_matches_native() -> Result<()> {
    history_resize(HISTORY_PROJECT_ENV, HISTORY_FULL_ENV, HISTORY_HALF_ENV)
}

const LATCH_PROJECT_ENV: &str = "SKWD_WE_HISTORY_LATCH_PROJECT";
const LATCH_FULL_ENV: &str = "SKWD_WE_HISTORY_LATCH_FULL";
const LATCH_HALF_ENV: &str = "SKWD_WE_HISTORY_LATCH_HALF";

#[test]
#[ignore = "requires Vulkan and static history latch project and native captures"]
fn history_latch_matches_native() -> Result<()> {
    history_resize(LATCH_PROJECT_ENV, LATCH_FULL_ENV, LATCH_HALF_ENV)
}

fn history_resize(project: &str, full: &str, half: &str) -> Result<()> {
    let shared = crate::shared::test_device();
    let histories = vk::Renderer::scene_history_live_allocations();
    let dir = std::env::var(project)?;
    let properties = paper_scene::model::Properties::new();
    let identity = identity(&dir, &properties)?;
    let full = image::open(std::env::var(full)?)?.to_rgb8();
    let half = image::open(std::env::var(half)?)?.to_rgb8();
    let mut source = SceneSource {
        shared: &shared,
        dir: &dir,
        identity: &identity,
        properties: &properties,
        outputs: &[(1280, 720)],
        mode: FillMode::Fill,
        strict: true,
        particles_disabled: false,
    };
    let mut active = source.build()?;
    assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 1);
    assert!(active.reconstructable() && active.animated() && !active.frozen);
    assert_eq!(pixels(&mut active)?, full);
    for (outputs, native) in [(&[(640, 360)][..], &half), (&[(1280, 720)][..], &full)] {
        source.outputs = outputs;
        let original = pixels(&mut active)?;
        let targets = vk::Renderer::scene_target_live_allocations();
        let resources = vk::Renderer::effect_live_allocations();
        for _ in 0..3 {
            OBSERVED_HISTORY.set(histories + 1);
            FAULT.set(Fault::Compose);
            let error = source.build().err().expect("injected failure");
            assert!(error.to_string().contains("injected scene build failure"));
            assert_eq!(OBSERVED_HISTORY.get(), histories + 2);
            assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 1);
            assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
            assert_eq!(vk::Renderer::effect_live_allocations(), resources);
            active.compose(1.0, 0.0)?;
            assert_eq!(pixels(&mut active)?, original);
        }
        let mut candidate = source.build()?;
        assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 2);
        assert!(candidate.scene_history.is_some() && candidate.animated() && !candidate.frozen);
        assert!(candidate.scene_history.as_ref().unwrap().extent == candidate.target.extent);
        assert_eq!(pixels(&mut candidate)?, *native);
        let old = std::mem::replace(&mut active, candidate);
        old.destroy();
        assert_eq!(vk::Renderer::scene_history_live_allocations(), histories + 1);
    }
    active.destroy();
    assert_eq!(vk::Renderer::scene_history_live_allocations(), histories);
    Ok(())
}

#[test]
fn history_rejects_feedback() {
    use paper_scene::effects::{Effect, EffectBind, EffectState, Fbo, FboFormat, FboSize};
    let mut model = history_model();
    let mut read = model.layers[0].material.take().unwrap().pass;
    read.binds = vec![(0, EffectBind::Named("retained".into()))];
    let mut write = history_model().layers[0].material.take().unwrap().pass;
    write.binds.clear();
    write.target = Some("retained".into());
    model.layers[0].effects.push(Effect {
        name: "feedback".into(),
        state: EffectState::default(),
        swaps: Vec::new(),
        clear_functions: std::collections::BTreeMap::new(),
        passes: vec![read, write],
        fbos: vec![Fbo {
            name: "retained".into(),
            size: FboSize::Scale(1.0),
            format: FboFormat::Rgba8,
            repeat: None,
            clear: [0.0; 4],
            clear_on_setup: false,
            unique: false,
        }],
    });
    assert!(!static_inputs(&model));
    model.layers[0].effects[0].passes.reverse();
    assert!(static_inputs(&model));
}

#[test]
fn history_rejects_scripts() {
    let mut model = history_model();
    let mut scene = serde_json::json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function update(){return 1;}"}}]});
    model.scripts = paper_scene::script::SceneScripts::load(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &serde_json::json!({}),
    )
    .unwrap();
    assert!(model.scripts.is_some());
    assert!(!static_inputs(&model));
    model.scripts = None;
    let mut scene = serde_json::json!({"objects":[{"id":1,"origin":{"value":"8 8 0","animation":{"options":{"fps":10,"length":20,"mode":"single","name":"move"},"c0":[{"frame":0,"value":8},{"frame":20,"value":12}]}}}]});
    model.keyframes = paper_scene::keyframe::Bindings::parse(&mut scene).unwrap();
    assert!(!model.keyframes.is_empty());
    assert!(!static_inputs(&model));
}

#[test]
#[ignore = "requires exact native C05 package, image, isolated Vulkan and DXC"]
fn weighted_matches_native() -> Result<()> {
    use sha2::{Digest, Sha256};
    const PROJECT: &str = "SKWD_WEIGHTED_PUPPET_PROJECT";
    const NATIVE: &str = "SKWD_WEIGHTED_PUPPET_NATIVE";
    const PACKAGE_SHA: &str = "f12706b717a6fc801408592d3b41afa0208c3b12dedf1e0e1adc2bc5ac646c3d";
    const FRAME_SHA: &str = "77cc860f67004e9bbbe711c1d9d945c8099e3a1699687b9d7275520b4fdc1364";
    let dir = std::env::var(PROJECT)?;
    let package = std::fs::read(std::path::Path::new(&dir).join("scene.pkg"))?;
    assert_eq!(format!("{:x}", Sha256::digest(package)), PACKAGE_SHA);
    let image = std::fs::read(std::env::var(NATIVE)?)?;
    assert_eq!(format!("{:x}", Sha256::digest(&image)), FRAME_SHA);
    let native = image::load_from_memory(&image)?.to_rgba8();
    let shared = crate::shared::test_device();
    let package = paper_scene::pkg::Package::open(&locate_pkg(&dir)?)?;
    super::super::validate_native_compatibility(&package, true)?;
    let mut model = paper_scene::model::load_from_dir_with_storage(
        &package,
        std::path::Path::new(&dir),
        &paper_scene::model::Properties::new(),
        None,
        Some([1280, 720]),
    )?;
    assert!(model.scripts.is_some());
    validate_scene_content(&model)?;
    validate_scene_skips(true, &model.skipped)?;
    let mut active = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fill)?;
    let captured = active.renderer.read_scene_target(&active.target);
    active.destroy();
    let (width, height, rgba) = captured?;
    assert_eq!((width, height), native.dimensions());
    assert_eq!(rgba, native.into_raw());
    Ok(())
}
