use super::*;

pub(crate) fn particle_clock(dt: f32) -> paper_scene::effects::FrameClock {
    paper_scene::effects::FrameClock { dt, ..Default::default() }
}

#[test]
#[ignore = "requires a Vulkan device"]
fn flat_scene_rebuild() -> Result<()> {
    const FIRST: u32 = 32;
    const SECOND: u32 = 64;
    let shared = crate::shared::test_device();
    let baseline = vk::Renderer::scene_target_live_allocations();
    for side in [FIRST, SECOND] {
        let canvas = side as f32;
        let mut layer = test_layer(true, 0);
        layer.center = (canvas * 0.5, canvas * 0.5);
        layer.size = (canvas, canvas);
        let mut model = SceneModel {
            reflection: paper_scene::model::ReflectionUse::default(),
            clear_enabled: true,
            unreflected: BTreeSet::new(),
            custom_shaders: false,
            cameras_move: false,
            camera_source: paper_scene::model::CameraSource::default(),
            skin_viewport: true,
            lights: Vec::new(),
            sun_view: paper_scene::model::lighting::SunView::default(),
            fog: paper_scene::model::fog::Volume::default(),
            cookies: None,
            image_sources: BTreeSet::new(),
            dependencies: BTreeSet::new(),
            scripts: None,
            keyframes: paper_scene::keyframe::Bindings::default(),
            animated_scene: None,
            canvas: (canvas, canvas),
            mouse: paper_scene::mouse::Parallax::default(),
            shake: None,
            camera_fov: 50.0,
            perspective_fov: 95.0,
            camera_view: paper_scene::model::CameraView::UNIT,
            camera: None,
            projection: paper_scene::model::ProjectionMode::Orthographic,
            clear: [0.0; 3],
            ambient: [1.0; 3],
            skylight: [1.0; 3],
            layers: vec![layer],
            meshes: Vec::new(),
            particles: Vec::new(),
            skipped: Vec::new(),
            background_only: false,
            hdr: false,
        };
        let mut group = build_group(&shared, &mut model, true, &[(side, side)], FillMode::Fill)?;
        anyhow::ensure!(
            group.target.samples == ash::vk::SampleCountFlags::TYPE_1,
            "flat scene multisampled"
        );
        group.compose(0.0, 0.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        anyhow::ensure!((width, height) == (side, side), "scene rebuild extent changed");
        anyhow::ensure!(pixels.iter().any(|value| *value != 0), "scene rebuild rendered blank");
        group.destroy();
        anyhow::ensure!(
            vk::Renderer::scene_target_live_allocations() == baseline,
            "scene rebuild retained targets"
        );
    }
    Ok(())
}

thread_local! {
    pub(super) static REPLAY_SNAPSHOTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn texture_key(rgba: &[u8], clamp: bool, nearest: bool) -> TextureKey {
    TextureKey {
        pixels: std::sync::Arc::new(paper_scene::tex::Pixels::rgba(2, 1, rgba.to_vec())),
        clamp,
        nearest,
        sampling: vk::MipSampling::Anisotropic,
        text: false,
        text_batch: None,
    }
}

#[test]
fn text_uses_base_sampling() {
    assert_eq!(layer_sampling(true), vk::MipSampling::Disabled);
    assert!(!layer_sampling(true).has_mips());
    assert_eq!(layer_sampling(false), vk::MipSampling::Anisotropic);
}

#[test]
fn texture_interning_uses_exact_payload_and_sampler_identity() {
    let base = texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false);
    let mut slots = HashMap::new();
    slots.insert(base, 7);

    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false)), Some(&7));
    let mut wrong_dimensions = texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false);
    wrong_dimensions.pixels =
        std::sync::Arc::new(paper_scene::tex::Pixels::rgba(1, 2, vec![1, 2, 3, 4, 5, 6, 7, 8]));
    assert_eq!(slots.get(&wrong_dimensions), None);
    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 9], true, false)), None);
    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], false, false)), None);
    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, true)), None);
    let mut linear = texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false);
    linear.sampling = vk::MipSampling::Linear;
    assert_eq!(slots.get(&linear), None);
}

#[test]
fn taking_a_texture_key_releases_the_model_payload() {
    let mut texture = paper_scene::model::solid_texture();
    let expected = texture.pixels.clone();
    let key = TextureKey::take(&mut texture, vk::MipSampling::Anisotropic);

    assert!(texture.pixels.levels.is_empty());
    assert_eq!(texture.pixels.bytes(), 0);
    assert_eq!(key.pixels, expected);
    assert_eq!((key.pixels.width(), key.pixels.height()), (1, 1));
    assert_eq!(key.pixels.bytes(), 4);
}

#[test]
fn scene_transitions_accept_standard_effects() {
    let effect = paper_shaders::effect_index("inkwell-drop").unwrap();
    assert_eq!(transition_style(Some("inkwell-drop")), TransitionStyle::Effect(effect));
}

#[test]
fn scene_transitions_keep_sand_and_fade_fallbacks() {
    let sand = paper_shaders::sand_style_index("sand-donut").unwrap();
    assert_eq!(transition_style(Some("sand-donut")), TransitionStyle::Sand(sand));
    assert_eq!(transition_style(Some("fade")), TransitionStyle::Fade);
    assert_eq!(transition_style(Some("not-a-transition")), TransitionStyle::Fade);
    assert_eq!(transition_style(None), TransitionStyle::Fade);
}

#[test]
fn clamped_scene_raster_keeps_logical_layer_coordinates() {
    let dimensions = scene_dimensions_for((3840.0, 2160.0), 2048);
    assert_eq!(dimensions.logical, [3840.0, 2160.0]);
    assert_eq!(dimensions.raster, (2048, 1152));

    // Mirror layer.vert for an axis-aligned full-canvas quad. Its corners must
    // still cover clip space even though the backing image is rasterized smaller.
    let rect = [1920.0_f32, 1080.0_f32, 3840.0_f32, 2160.0_f32];
    let project = |corner: [f32; 2]| {
        let point = [rect[0] + (corner[0] - 0.5) * rect[2], rect[1] + (corner[1] - 0.5) * rect[3]];
        [point[0] / dimensions.logical[0] * 2.0 - 1.0, point[1] / dimensions.logical[1] * 2.0 - 1.0]
    };

    assert_eq!(project([0.0, 0.0]), [-1.0, -1.0]);
    assert_eq!(project([0.5, 0.5]), [0.0, 0.0]);
    assert_eq!(project([1.0, 1.0]), [1.0, 1.0]);

    // Passing the raster extent as the shader canvas recreates the old zoom.
    let old_right = rect[2] / dimensions.raster.0 as f32 * 2.0 - 1.0;
    assert!(old_right > 1.0);
}

#[test]
fn fit_scene_raster_tracks_the_largest_output() {
    let dimensions = scene_dimensions_for_outputs(
        (3840.0, 2160.0),
        4096,
        &[(1920, 1080), (2560, 1440)],
        FillMode::Fit,
    );

    assert_eq!(dimensions.logical, [3840.0, 2160.0]);
    assert_eq!(dimensions.raster, (2560, 1440));
}

#[test]
fn small_scene_tracks_output() {
    for (mode, raster) in [
        (FillMode::Fit, (1280, 720)),
        (FillMode::Fill, (1422, 800)),
        (FillMode::Span, (1422, 800)),
        (FillMode::Center, (640, 360)),
        (FillMode::Tile, (640, 360)),
        (FillMode::Stretch, (640, 360)),
    ] {
        let dimensions = scene_dimensions_for_outputs((640.0, 360.0), 2048, &[(1280, 800)], mode);
        assert_eq!(dimensions.logical, [640.0, 360.0]);
        assert_eq!(dimensions.raster, raster);
    }
}

#[test]
fn upscaled_scene_respects_cap() {
    for (limit, output, raster) in [
        (2048, (3840, 2160), (2048, 1152)),
        (SCENE_DEFAULT_LIMIT, (3840, 2160), (3840, 2160)),
        (SCENE_DEFAULT_LIMIT, (7680, 4320), (4096, 2304)),
    ] {
        let dimensions =
            scene_dimensions_for_outputs((640.0, 360.0), limit, &[output], FillMode::Fill);
        assert_eq!(dimensions.logical, [640.0, 360.0]);
        assert_eq!(dimensions.raster, raster);
    }
}

#[test]
fn fill_scene_raster_covers_portrait_outputs() {
    let dimensions =
        scene_dimensions_for_outputs((3840.0, 2160.0), 4096, &[(1080, 1920)], FillMode::Fill);

    assert_eq!(dimensions.raster, (3413, 1920));
}

#[test]
fn stretch_scene_raster_tracks_portrait_output_axes() {
    let dimensions =
        scene_dimensions_for_outputs((3840.0, 2160.0), 4096, &[(1080, 1920)], FillMode::Stretch);

    assert_eq!(dimensions.logical, [3840.0, 2160.0]);
    assert_eq!(dimensions.raster, (1080, 1920));
}

#[test]
fn stretch_scene_raster_covers_each_multi_output_axis() {
    let dimensions = scene_dimensions_for_outputs(
        (3840.0, 2160.0),
        4096,
        &[(2560, 1080), (1080, 1920)],
        FillMode::Stretch,
    );

    assert_eq!(dimensions.raster, (2560, 1920));
}

#[test]
fn configured_scene_limit_still_bounds_output_raster() {
    let dimensions =
        scene_dimensions_for_outputs((3840.0, 2160.0), 2048, &[(3840, 2160)], FillMode::Fill);

    assert_eq!(dimensions.raster, (2048, 1152));
}

#[test]
fn center_and_tile_keep_authored_raster_dimensions() {
    for mode in [FillMode::Center, FillMode::Tile] {
        let dimensions =
            scene_dimensions_for_outputs((3840.0, 2160.0), 4096, &[(1920, 1080)], mode);
        assert_eq!(dimensions.raster, (3840, 2160));
    }
}

#[test]
fn effect_targets_follow_scene_raster_scale() {
    let dimensions =
        scene_dimensions_for_outputs((3840.0, 2160.0), 4096, &[(1920, 1080)], FillMode::Fit);

    assert_eq!(effect_dimensions_for((3840.0, 2160.0), dimensions), (1920, 1080));
    assert_eq!(effect_dimensions_for((1920.0, 1080.0), dimensions), (960, 540));
}

#[test]
fn effect_target_cap_preserves_layer_aspect() {
    let dimensions = scene_dimensions_for((3840.0, 2160.0), 4096);

    assert_eq!(effect_dimensions_for((3840.0, 2160.0), dimensions), (2048, 1152));
}

#[test]
fn scene_fade_commits_exact_endpoints_before_leaving_the_transition_path() {
    assert_eq!(
        scene_fade_step(None, false),
        SceneFadeStep { mix: 1.0, render_transition: false, finish_after_commit: false }
    );
    assert_eq!(
        scene_fade_step(Some(0.18), true),
        SceneFadeStep { mix: 0.0, render_transition: true, finish_after_commit: false }
    );
    assert_eq!(
        scene_fade_step(Some(0.18), false),
        SceneFadeStep { mix: 0.18, render_transition: true, finish_after_commit: false }
    );
    assert_eq!(
        scene_fade_step(Some(1.04), false),
        SceneFadeStep { mix: 1.0, render_transition: true, finish_after_commit: true }
    );
}

#[test]
fn scene_duration_bounds() {
    assert_eq!(scene_swap_duration(0), None);
    assert_eq!(scene_swap_duration(40), Some(80));
    assert_eq!(scene_swap_duration(600), Some(600));
    assert_eq!(scene_swap_duration(8200), Some(8200));
    assert_eq!(scene_swap_duration(12_000), Some(10_000));
}

#[test]
fn every_output_owns_an_independent_scene_presenter() {
    let outputs = [(2560, 1440), (1920, 1080), (3840, 2160)];
    let (shared, shared_indices) = scene_presenter_layout(&outputs, (3440, 1440), true);
    assert_eq!(shared, [(3440, 1440), (3440, 1440), (3440, 1440)]);
    assert_eq!(shared_indices, [0, 1, 2]);

    let (native, native_indices) = scene_presenter_layout(&outputs, (3440, 1440), false);
    assert_eq!(native, outputs);
    assert_eq!(native_indices, [0, 1, 2]);
}

#[test]
fn shared_scene_slot_waits_for_every_borrower() {
    let mut free = [vec![], vec![], vec![]];
    let available = |free: &[Vec<usize>]| shared_scene_slot(free.iter().map(Vec::as_slice), 2, &[]);
    free[0].push(0);
    free[1].push(0);
    assert_eq!(available(&free), None);
    free[2].push(1);
    assert_eq!(available(&free), None);
    free[2].push(0);
    assert_eq!(available(&free), Some(0));
}

#[test]
fn shared_scene_pool_growth_unblocks_fragmented_outputs() {
    let mut free = [vec![0], vec![1], vec![0]];
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 2, &[]), None);
    for output in &mut free {
        output.push(2);
    }
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 3, &[]), Some(2));
    free[0].retain(|&bi| bi != 2);
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 3, &[]), None);
    free[0].push(2);
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 3, &[]), Some(2));
}

#[test]
fn shared_scene_pool_keeps_fast_outputs_independent_of_retained_frames() {
    let count = 6;
    let mut free = [vec![1, 2, 3, 4, 5], vec![0, 3, 4, 5], vec![0, 1, 2, 5]];
    let mut current = 0;
    for _ in 0..100 {
        let next = shared_scene_slot(free.iter().map(Vec::as_slice), count, &[]).unwrap();
        assert!(next == 0 || next == 5);
        free[0].retain(|&bi| bi != next);
        assert_eq!(count - free[0].len(), 2);
        free[0].push(current);
        current = next;
    }
    assert_eq!(free[1], [0, 3, 4, 5]);
    assert_eq!(free[2], [0, 1, 2, 5]);
}

#[test]
fn shared_scene_pool_does_not_reuse_closed_outputs_unreleased_storage() {
    let live = [1, 2];
    let closed = [0, 2];
    assert_eq!(
        shared_scene_slot([live.as_slice(), closed.as_slice()].into_iter(), 3, &[]),
        Some(2)
    );
}

#[test]
fn effect_target_accounting_retains_only_the_selected_output() {
    let ping_output = classify_effect_target_bytes(&[64, 64, 16, 8], FxTargetId::Ping).unwrap();
    assert_eq!(ping_output, EffectTargetBytes { retained: 64, transient: 88 });

    let named_output = classify_effect_target_bytes(&[64, 64, 16, 8], FxTargetId::Fbo(1)).unwrap();
    assert_eq!(named_output, EffectTargetBytes { retained: 8, transient: 144 });
}

#[test]
fn effect_target_accounting_rejects_an_unknown_named_output() {
    assert_eq!(classify_effect_target_bytes(&[64, 64, 16], FxTargetId::Fbo(1)), None);
}

#[test]
fn scratch_layout_reuses_only_compatible_cross_layer_targets() {
    let full = FxTargetClass { width: 1920, height: 1080, repeat: false, format: 37 };
    let half = FxTargetClass { width: 960, height: 540, repeat: false, format: 37 };
    let repeating = FxTargetClass { repeat: true, ..full };

    let layout = plan_scratch_layout(&[vec![full, full, half], vec![full, half], vec![repeating]]);

    // Targets in one layer never alias even when their image class matches.
    assert_ne!(layout.assignments[0][0], layout.assignments[0][1]);
    // Sequential layers reuse matching scratch slots.
    assert_eq!(layout.assignments[1][0], layout.assignments[0][0]);
    assert_eq!(layout.assignments[1][1], layout.assignments[0][2]);
    // Sampler addressing is part of compatibility.
    assert_ne!(layout.assignments[2][0], layout.assignments[0][0]);
    assert_eq!(layout.physical_classes.len(), 4);
}

#[test]
fn sampled_target_aliases() {
    let plan = lifetime::plan_targets(&[], &[None, None], &[vec![], vec![]], &[0, 0]).unwrap();
    assert_eq!(plan.output, FxTargetId::Ping);
    assert!(!scratch_candidate(&plan, &BTreeSet::new(), FxTargetId::Ping));

    let plan = lifetime::plan_targets(&[], &[None, None], &[vec![], vec![]], &[0, 1]).unwrap();
    assert_eq!(plan.output, FxTargetId::Pong);
    assert!(plan.lifetimes[FxTargetId::Ping.index()].scratch_eligible());

    assert!(scratch_candidate(&plan, &BTreeSet::new(), FxTargetId::Ping));
    assert!(scratch_candidate(&plan, &BTreeSet::from([CompositeBuffer::A]), FxTargetId::Ping));
    assert!(!scratch_candidate(&plan, &BTreeSet::from([CompositeBuffer::A]), plan.output));
    assert!(!scratch_candidate(&plan, &BTreeSet::new(), plan.output));
}

#[test]
fn passive_target_quad_crops_padded_texture_at_layer_extent() {
    let mut texture = paper_scene::model::solid_texture();
    texture.width = 4;
    texture.height = 8;
    texture.img_width = 3;
    texture.img_height = 5;
    let layer = paper_scene::model::Layer {
        texture_name: None,
        material: None,
        live_text: None,
        script_text: None,
        kind: paper_scene::model::LayerKind::Image,
        mouse: paper_scene::mouse::LayerMouse::default(),
        id: "producer".into(),
        name: "producer".into(),
        visible: true,
        texture,
        puppet: None,
        center: (0.0, 0.0),
        size: (300.0, 500.0),
        scale: (1.0, 1.0),
        depth: 0.0,
        scene_order: 0,
        alpha: 1.0,
        angle: 0.0,
        color: [1.0; 3],
        color_blend: 0,
        pad_uv: false,
        spritesheet: false,
        passthrough: false,
        fullscreen: false,
        geometry: None,
        composition_size: None,
        copy_background: false,
        compose_depth: false,
        solid: false,
        base_scene_input: false,
        effects: Vec::new(),
    };

    let quad = passive_target_quad(&layer, 7, ash::vk::Extent2D { width: 300, height: 500 });
    assert_eq!(quad.rect, [150.0, 250.0, 300.0, 500.0]);
    assert_eq!(quad.uv, [0.0, 0.0, 0.75, 0.625]);
    assert_eq!(quad.texture, 7);
    assert!(quad.blend == vk::SceneBlend::Copy);
}

#[test]
fn ordinary_layer_uv_padding() {
    let mut layer = test_layer(true, 0);
    layer.texture.width = 4;
    layer.texture.height = 8;
    layer.texture.img_width = 3;
    layer.texture.img_height = 5;
    assert_eq!(layer_quad(&layer, 0, (800.0, 600.0)).uv, [0.0, 0.0, 0.75, 0.625]);
    layer.pad_uv = true;
    let uv = layer_quad(&layer, 0, (800.0, 600.0)).uv;
    assert!(
        uv.into_iter()
            .zip([0.0375, 0.01875, 0.675, 0.5875])
            .all(|(actual, expected)| { (actual - expected).abs() < 1e-6 })
    );
    layer.pad_uv = false;
    assert_eq!(layer_quad(&layer, 0, (800.0, 600.0)).uv, [0.0, 0.0, 0.75, 0.625]);
}

#[test]
fn active_and_passive_duplicate_ids_remain_ambiguous() {
    let layer = |name: &str| paper_scene::model::Layer {
        texture_name: None,
        material: None,
        live_text: None,
        script_text: None,
        kind: paper_scene::model::LayerKind::Image,
        mouse: paper_scene::mouse::LayerMouse::default(),
        id: "duplicate".into(),
        name: name.into(),
        visible: true,
        texture: paper_scene::model::solid_texture(),
        puppet: None,
        center: (0.0, 0.0),
        size: (1.0, 1.0),
        scale: (1.0, 1.0),
        depth: 0.0,
        scene_order: 0,
        alpha: 1.0,
        angle: 0.0,
        color: [1.0; 3],
        color_blend: 0,
        pad_uv: false,
        spritesheet: false,
        passthrough: false,
        fullscreen: false,
        geometry: None,
        composition_size: None,
        copy_background: false,
        compose_depth: false,
        solid: false,
        base_scene_input: false,
        effects: Vec::new(),
    };
    let layers = [layer("active"), layer("passive")];
    let active_layers = BTreeSet::from([0]);
    let referenced = BTreeSet::from(["duplicate".to_string()]);
    let passive_indices = passive_provider_indices(&layers, &active_layers, &referenced);
    assert_eq!(passive_indices, [1]);

    let consumer_binds = vec![vec![(
        0,
        EffectBind::LayerComposite { layer: "duplicate".into(), buffer: CompositeBuffer::A },
    )]];
    let nodes = [
        LayerTargetNode {
            id: &layers[0].id,
            scene_order: 0,
            local_targets: &[],
            binds: &[],
            dynamic: false,
            prefix_dynamic: false,
            passthrough: false,
        },
        LayerTargetNode {
            id: "consumer",
            scene_order: 1,
            local_targets: &[],
            binds: &consumer_binds,
            dynamic: false,
            prefix_dynamic: false,
            passthrough: false,
        },
    ];
    let passive = [PassiveLayerTarget { id: &layers[passive_indices[0]].id, dynamic: false }];

    assert_eq!(
        plan_scene_targets(&nodes, &passive),
        Err(paper_scene::scene_targets::SceneTargetPlanError::AmbiguousLayer {
            consumer: 1,
            layer: "duplicate".into(),
        })
    );
}

#[test]
fn full_frame_prefix_merges_lower_particles_in_unified_order() {
    let image = |texture| vk::SceneQuad {
        uv_axes: None,
        uv_padding: false,
        projection: None,
        order_bias: 0,
        rect: [0.0; 4],
        uv: [0.0; 4],
        tint: [1.0; 4],
        angle: 0.0,
        texture,
        blend: vk::SceneBlend::Alpha,
    };
    let images = [image(10), image(30)];
    let particles = [OrderedParticleQuad { scene_order: 1, renderer: (0, 0), quad: image(20) }];

    let prefix = ordered_scene_quads(&images, &[0, 2], &particles, 0, Some(2));
    assert_eq!(prefix.iter().map(|quad| quad.texture).collect::<Vec<_>>(), [10, 20]);
    let full = ordered_scene_quads(&images, &[0, 2], &particles, 0, None);
    assert_eq!(full.iter().map(|quad| quad.texture).collect::<Vec<_>>(), [10, 20, 30]);
}

#[test]
fn strict_startup_rejects_any_scene_loader_skip() {
    let skipped = vec![
        "layer: unsupported object".to_string(),
        "layer: effect material unavailable".to_string(),
    ];

    assert!(validate_scene_skips(false, &skipped).is_ok());
    let error = validate_scene_skips(true, &skipped).unwrap_err().to_string();
    assert!(error.contains("2 skipped element(s)"));
    assert!(error.contains("unsupported object"));
    assert!(error.contains("effect material unavailable"));
    assert!(validate_scene_skips(true, &[]).is_ok());
}

#[test]
fn strict_startup_rejects_pipeline_fallback_but_best_effort_accepts_it() {
    assert!(validate_pipeline_skip(false, "clock", "shader compile failed").is_ok());
    let error =
        validate_pipeline_skip(true, "clock", "shader compile failed").unwrap_err().to_string();
    assert!(error.contains("effect pipeline on clock"));
    assert!(error.contains("shader compile failed"));
}

pub(super) fn sound_package(files: &[(&str, &[u8])]) -> paper_scene::pkg::Package {
    let mut bytes = Vec::new();
    let push = |bytes: &mut Vec<u8>, text: &str| {
        bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    };
    push(&mut bytes, "PKGV0007");
    bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    for (path, data) in files {
        push(&mut bytes, path);
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in files {
        bytes.extend_from_slice(data);
    }
    paper_scene::pkg::Package::parse(bytes).unwrap()
}

#[test]
fn preset_sound_visibility_and_user_override_share_scene_defaults() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("1");
    let preset = root.path().join("2");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&preset).unwrap();
    std::fs::write(
        parent.join("project.json"),
        br#"{"general":{"properties":{"music":{"type":"bool","value":true}}}}"#,
    )
    .unwrap();
    std::fs::write(preset.join("project.json"), br#"{"dependency":"1","preset":{"music":false}}"#)
        .unwrap();
    let package = sound_package(&[
        ("scene.json", br#"{"objects":[{"id":1,"sound":["clip.ogg"],"visible":{"user":"music","value":true}}]}"#),
        ("clip.ogg", b"fixture"),
    ]);
    let defaults = paper_scene::model::Properties::new();
    let properties = super::properties::resolved(preset.to_str().unwrap(), &defaults);
    assert!(paper_scene::sound::scene_sounds(&package, &properties).unwrap().is_empty());
    let overrides = paper_scene::model::Properties::from([("music".into(), vec![1.0])]);
    let properties = super::properties::resolved(preset.to_str().unwrap(), &overrides);
    assert_eq!(paper_scene::sound::scene_sounds(&package, &properties).unwrap().len(), 1);
    let properties = super::properties::resolved(parent.to_str().unwrap(), &defaults);
    assert_eq!(paper_scene::sound::scene_sounds(&package, &properties).unwrap().len(), 1);
}

#[test]
fn every_scene_sound_is_staged_as_its_own_voice_with_the_authored_playback() {
    let scene = br#"{"objects":[
        {"name":"Ambience","sound":["s/wind.ogg"],"volume":0.4,"playbackmode":"loop"},
        {"name":"Birds","sound":["s/a.ogg","s/b.ogg"],"volume":1.3,"playbackmode":"random",
         "mintime":10.0,"maxtime":30.0},
        {"name":"Sting","sound":"s/hit.ogg","playbackmode":"single"},
        {"name":"Quiet","sound":"s/wind.ogg","startsilent":true}
    ]}"#;
    let pkg = sound_package(&[
        ("scene.json", scene),
        ("s/wind.ogg", b"wind"),
        ("s/a.ogg", b"a"),
        ("s/b.ogg", b"b"),
        ("s/hit.ogg", b"hit"),
    ]);
    let audio = SceneAudio::extract(&pkg, &paper_scene::model::Properties::new())
        .unwrap()
        .expect("scene declares sounds");
    let voices = audio.voices();
    assert_eq!(
        voices.iter().map(|voice| voice.name.as_str()).collect::<Vec<_>>(),
        ["Ambience", "Birds", "Sting"],
        "a start-silent object must not become a voice"
    );
    assert_eq!(voices[0].mode, paper_audio::VoiceMode::Loop);
    assert_eq!(voices[1].mode, paper_audio::VoiceMode::Random);
    assert_eq!(voices[2].mode, paper_audio::VoiceMode::Once);
    assert_eq!(voices[1].clips.len(), 2, "a random pool keeps every clip");
    assert!((voices[1].gain - 1.3).abs() < f32::EPSILON);
    assert!((voices[1].min_gap - 10.0).abs() < f32::EPSILON);
    assert!((voices[1].max_gap - 30.0).abs() < f32::EPSILON);

    for voice in &voices {
        for clip in &voice.clips {
            assert!(std::path::Path::new(clip).is_file(), "each clip is staged on disk: {clip}");
        }
    }
    assert_eq!(audio.summary().split(", ").count(), 4, "every staged clip is named in the log");
}

#[test]
fn a_scene_without_playable_sounds_produces_no_audio() {
    let pkg = sound_package(&[(
        "scene.json",
        br#"{"objects":[{"sound":"s/missing.ogg"},{"image":"models/bg.json"}]}"#,
    )]);
    assert!(SceneAudio::extract(&pkg, &paper_scene::model::Properties::new()).unwrap().is_none());
}

#[test]
fn a_user_bound_sound_volume_follows_the_scene_property() {
    let scene = br#"{"objects":[{"name":"Music","sound":"s/theme.ogg",
        "volume":{"user":"music_volume","value":0.9}}]}"#;
    let pkg = sound_package(&[("scene.json", scene), ("s/theme.ogg", b"theme")]);
    let baked =
        SceneAudio::extract(&pkg, &paper_scene::model::Properties::new()).unwrap().expect("sound");
    assert!((baked.voices()[0].gain - 0.9).abs() < f32::EPSILON);

    let mut properties = paper_scene::model::Properties::new();
    properties.insert("music_volume".into(), vec![0.2]);
    let tuned = SceneAudio::extract(&pkg, &properties).unwrap().expect("sound");
    assert!((tuned.voices()[0].gain - 0.2).abs() < f32::EPSILON);
}

#[test]
fn atlas_frames_advance_by_cumulative_frame_time_and_wrap() {
    use paper_scene::model::SpriteFrame;
    let frames = vec![
        SpriteFrame {
            axes: [0.5, 0.0, 0.0, 1.0],
            uv: [0.0, 0.0, 0.5, 1.0],
            rotated: false,
            time: 0.1,
            image: 0,
        },
        SpriteFrame {
            axes: [0.5, 0.0, 0.0, 1.0],
            uv: [0.5, 0.0, 0.5, 1.0],
            rotated: false,
            time: 0.3,
            image: 0,
        },
    ];
    assert_eq!(animation_frame(&frames, 0.4, 0.0).uv, frames[0].uv);
    assert_eq!(animation_frame(&frames, 0.4, 0.05).uv, frames[0].uv);
    assert_eq!(animation_frame(&frames, 0.4, 0.2).uv, frames[1].uv);
    assert_eq!(animation_frame(&frames, 0.4, 0.45).uv, frames[0].uv);
    assert_eq!(animation_frame(&frames, 0.0, 9.0).uv, frames[0].uv);
}

pub(super) fn lit_pixels(pixels: &[u8]) -> usize {
    pixels.chunks_exact(4).filter(|pixel| pixel[..3] != [0, 0, 0]).count()
}

pub(super) fn transform(m: &Mat4, p: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (row, slot) in out.iter_mut().enumerate() {
        *slot = (0..4).map(|k| m[k * 4 + row] * p[k]).sum();
    }
    out
}

#[test]
fn scene_ortho_maps_engine_space_to_vulkan_clip() {
    let m = scene_ortho((1920.0, 1080.0), false);
    assert_eq!(transform(&m, [0.0, 0.0, 0.0, 1.0]), [-1.0, 1.0, 0.5, 1.0]);
    assert_eq!(transform(&m, [1920.0, 1080.0, 0.0, 1.0]), [1.0, -1.0, 0.5, 1.0]);
    let d3d = scene_ortho((1920.0, 1080.0), true);
    assert_eq!(transform(&d3d, [0.0, 0.0, 0.0, 1.0]), [-1.0, -1.0, 0.5, 1.0]);
    assert_eq!(transform(&d3d, [1920.0, 1080.0, 0.0, 1.0]), [1.0, 1.0, 0.5, 1.0]);
    let model = model_matrix((100.0, 50.0, 0.0), [2.0, 3.0, 1.0], 0.0);
    let mvp = mat4_mul(&m, &model);
    let p = transform(&mvp, [10.0, 10.0, 0.0, 1.0]);
    assert!((p[0] - (2.0 * 120.0 / 1920.0 - 1.0)).abs() < 1e-5);
    assert!((p[1] - (1.0 - 2.0 * 80.0 / 1080.0)).abs() < 1e-5);
    let inverse = model_inverse((100.0, 50.0, 0.0), [2.0, 3.0, 1.0], 0.0);
    let back = transform(&inverse, [120.0, 80.0, 0.0, 1.0]);
    assert!((back[0] - 10.0).abs() < 1e-5 && (back[1] - 10.0).abs() < 1e-5);

    let turned = model_matrix((100.0, 50.0, 0.0), [2.0, 3.0, 1.0], std::f32::consts::FRAC_PI_2);
    let up = transform(&turned, [10.0, 0.0, 0.0, 1.0]);
    assert!(
        (up[0] - 100.0).abs() < 1e-4 && (up[1] - 70.0).abs() < 1e-4,
        "a quarter turn sends local +x to world +y: {up:?}"
    );
    let undo = model_inverse((100.0, 50.0, 0.0), [2.0, 3.0, 1.0], std::f32::consts::FRAC_PI_2);
    let back = transform(&undo, [up[0], up[1], 0.0, 1.0]);
    assert!(
        (back[0] - 10.0).abs() < 1e-4 && back[1].abs() < 1e-4,
        "the inverse undoes rotation and scale: {back:?}"
    );
}

#[test]
fn scene_perspective_keeps_the_centre_and_depth_in_range() {
    let m = scene_perspective([0.0, 0.0, 1920.0, 1080.0], 95.0, false);
    let d3d = scene_perspective([0.0, 0.0, 1920.0, 1080.0], 95.0, true);
    let up = transform(&m, [960.0, 1080.0, 0.0, 1.0]);
    let up_d3d = transform(&d3d, [960.0, 1080.0, 0.0, 1.0]);
    assert!((up[1] / up[3] + up_d3d[1] / up_d3d[3]).abs() < 1e-5);
    assert!(up_d3d[1] / up_d3d[3] > 0.0);
    let centre = transform(&m, [960.0, 540.0, 0.0, 1.0]);
    assert!(centre[0].abs() < 1e-5 && centre[1].abs() < 1e-5);
    let ndc_z = centre[2] / centre[3];
    assert!(ndc_z > 0.0 && ndc_z < 1.0, "{ndc_z}");
    let near = transform(&m, [960.0, 540.0, 500.0, 1.0]);
    let far = transform(&m, [960.0, 540.0, -500.0, 1.0]);
    assert!(near[3] < far[3]);
    let right = transform(&m, [1500.0, 540.0, 0.0, 1.0]);
    assert!(right[0] / right[3] > 0.0);
    let up = transform(&m, [960.0, 1000.0, 0.0, 1.0]);
    assert!(up[1] / up[3] < 0.0);
}

#[test]
fn particle_eyes_follow_projection() {
    use paper_scene::model::ProjectionMode;

    let canvas = (256.0, 256.0);
    let ortho = particle_eye(ProjectionMode::Orthographic, None, canvas, 95.0);
    assert_eq!(ortho.map(f32::to_bits), [0x4300_0000, 0x4300_0000, 0x44fa_0000]);
    for projection in [ProjectionMode::Automatic, ProjectionMode::Other] {
        let eye = particle_eye(projection, None, canvas, 95.0);
        assert_eq!(eye[..2], [128.0, 128.0]);
        assert!((eye[2] - 117.29).abs() < 0.01, "{eye:?}");
    }
    let camera = Camera {
        eye: [12.0, 34.0, 56.0],
        center: [0.0; 3],
        up: [0.0, 1.0, 0.0],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    for projection in
        [ProjectionMode::Orthographic, ProjectionMode::Automatic, ProjectionMode::Other]
    {
        assert_eq!(particle_eye(projection, Some(camera), canvas, 95.0), camera.eye);
    }
}

#[test]
fn override_fov_eye() {
    for (height, fov, eye) in [
        (1080.0, 95.0, 494.8),
        (1440.0, 95.0, 659.7),
        (1080.0, 30.0, 2015.4),
        (4320.0, 21.0, 11654.3),
    ] {
        let distance = super::perspective_eye_distance(height, fov);
        assert!((distance - eye).abs() < 0.1, "{fov} {distance}");
    }
}

#[test]
fn scene_perspective_matches_the_engine_eye_distance() {
    for (w, h) in [(1920.0f32, 1080.0f32), (2560.0, 1440.0), (1920.0, 1920.0)] {
        let m = scene_perspective([0.0, 0.0, w, h], 95.0, false);
        let d = super::perspective_eye_distance(h, 95.0);
        let flat = transform(&m, [w * 0.5, h * 0.5 + 100.0, 0.0, 1.0]);
        assert!((flat[1] / flat[3] - (-100.0 / (h * 0.5))).abs() < 1e-5, "{:?}", flat);
        for z in [200.0f32, 100.0, -100.0, -200.0] {
            let p = transform(&m, [w * 0.5 + 100.0, h * 0.5 + 100.0, z, 1.0]);
            let s = d / (d - z);
            assert!((p[0] / p[3] - 100.0 * s / (w * 0.5)).abs() < 1e-4, "z={z} {:?}", p);
            assert!((p[1] / p[3] + 100.0 * s / (h * 0.5)).abs() < 1e-4, "z={z} {:?}", p);
        }
    }
}

#[test]
fn authored_camera_projects_depth() {
    let camera = Camera {
        eye: [0.0; 3],
        center: [0.0, 0.0, -1.0],
        up: [0.0, 1.0, 0.0],
        fov: 50.0,
        near: 0.01,
        far: 10000.0,
    };
    let (matrix, right, up) = authored_perspective(camera, (1280.0, 720.0), false);
    assert_eq!((right, up), ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    let center = transform(&matrix, [0.0, 0.0, -5.0, 1.0]);
    assert!(center[0].abs() < 1e-6 && center[1].abs() < 1e-6);
    let east = transform(&matrix, [3.0, 0.0, -5.0, 1.0]);
    let pixels = east[0] / east[3] * 640.0;
    assert!((pixels - 463.0).abs() < 1.0);
    let near = transform(&matrix, [3.0, 0.0, -2.5, 1.0]);
    assert!(near[0] / near[3] > east[0] / east[3]);
}

#[test]
fn native_camera_bits() {
    for (fov, horizontal, vertical) in [
        (50.0, 0x3f9a_678c, 0x4009_3f99),
        (60.0, 0x3f79_6a52, 0x3fdd_b3d7),
        (90.0, 0x3f0f_ffff, 0x3f7f_ffff),
        (120.0, 0x3ea6_46e0, 0x3f13_cd39),
    ] {
        let camera = Camera {
            eye: [0.0, 0.0, 5.0],
            center: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            fov,
            near: 0.1,
            far: 1000.0,
        };
        let (matrix, ..) = authored_perspective(camera, (1280.0, 720.0), true);
        let bits = [0, 5, 10, 14].map(|index| matrix[index].to_bits());
        assert_eq!(bits, [horizontal, vertical, 0x38d1_bc76, 0x3dcb_cbdf]);
        assert_eq!((matrix[11], matrix[15]), (-1.0, 5.0));
    }
}

#[test]
fn camera_depth_reverses() {
    let camera = Camera {
        eye: [0.0; 3],
        center: [0.0, 0.0, -1.0],
        up: [0.0, 1.0, 0.0],
        fov: 50.0,
        near: 0.1,
        far: 1000.0,
    };
    let (matrix, ..) = authored_perspective(camera, (1280.0, 720.0), true);
    let near = transform(&matrix, [0.0, 0.0, -camera.near, 1.0]);
    let far = transform(&matrix, [0.0, 0.0, -camera.far, 1.0]);
    assert!((near[2] / near[3] - 1.0).abs() < 0.000_001);
    assert!(far[2].abs() < 0.000_001);
    let above = transform(&matrix, [0.0, 1.0, -5.0, 1.0]);
    assert!(above[1] > 0.0);
}

#[test]
fn authored_camera_uses_basis() {
    let camera = Camera {
        eye: [2.0, 1.0, 0.0],
        center: [2.0, 1.0, -1.0],
        up: [1.0, 1.0, 0.0],
        fov: 50.0,
        near: 0.01,
        far: 10000.0,
    };
    let (matrix, right, up) = authored_perspective(camera, (1280.0, 720.0), false);
    let center = transform(&matrix, [2.0, 1.0, -5.0, 1.0]);
    assert!(center[0].abs() < 1e-6 && center[1].abs() < 1e-6);
    let diagonal = 1.0 / 2.0_f32.sqrt();
    assert_eq!((right, up), ([diagonal, -diagonal, 0.0], [diagonal, diagonal, 0.0]));
    let mut narrow = camera;
    narrow.fov = 25.0;
    let (narrow, ..) = authored_perspective(narrow, (1280.0, 720.0), false);
    let east = transform(&matrix, [3.0, 1.0, -5.0, 1.0]);
    let magnified = transform(&narrow, [3.0, 1.0, -5.0, 1.0]);
    assert!(magnified[0] / magnified[3] > east[0] / east[3]);
    let (portrait, ..) = authored_perspective(camera, (720.0, 1280.0), false);
    let portrait_east = transform(&portrait, [3.0, 1.0, -5.0, 1.0]);
    assert!(portrait_east[0] / portrait_east[3] > east[0] / east[3]);
}

#[test]
fn screen_basis_matches_native() {
    let mut camera = Camera {
        eye: [3.0, 2.0, 0.0],
        center: [1.0, 0.0, -5.0],
        up: [0.0, 1.0, 0.0],
        fov: 50.0,
        near: 0.01,
        far: 10000.0,
    };
    let model = model_matrix((0.0, 0.0, -5.0), [1.0, 1.5, 1.0], 0.0);
    let first = screen_basis(camera, &model).unwrap();
    let expected = [0.928_476_7, 0.0, -0.371_390_67];
    for (actual, expected) in first.right.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 0.0001);
    }

    camera.center = [0.0, 0.0, -5.0];
    let rotated = model_matrix((0.0, 0.0, -5.0), [1.0, 1.5, 1.0], 35.0);
    let second = screen_basis(camera, &rotated).unwrap();
    let expected = [-0.576_131, -0.126_654_73, 0.807_484_75];
    for (actual, expected) in second.forward.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 0.0001);
    }

    camera.up = [1.0, 1.0, 0.0];
    let third = screen_basis(camera, &model).unwrap();
    let expected = [-0.114_686_91, 0.974_838_73, -0.191_144_85];
    for (actual, expected) in third.up.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 0.0001);
    }
}

#[test]
fn screen_basis_rejects_singular() {
    let camera = Camera {
        eye: [0.0; 3],
        center: [0.0, 1.0, 0.0],
        up: [1.0, 0.0, 0.0],
        fov: 50.0,
        near: 0.01,
        far: 10000.0,
    };
    let model = model_matrix((0.0, 0.0, 0.0), [1.0; 3], 0.0);
    assert!(screen_basis(camera, &model).is_none());

    let mut camera = camera;
    camera.center = [0.0, 0.0, -1.0];
    let singular = model_matrix((0.0, 0.0, 0.0), [1.0, 0.0, 1.0], 0.0);
    assert!(screen_basis(camera, &singular).is_none());
}

#[test]
fn authored_camera_raster_size() {
    let dimensions = camera_dimensions(&[(1280, 720)], 2048).unwrap();
    assert_eq!(dimensions.logical, [1280.0, 720.0]);
    assert_eq!(dimensions.raster, (1280, 720));
    let bounded = camera_dimensions(&[(3840, 2160)], 2048).unwrap();
    assert_eq!(bounded.raster, (2048, 1152));
    let ordinary =
        scene_dimensions_for_outputs((1920.0, 1080.0), 2048, &[(1280, 720)], FillMode::Fill);
    assert_eq!(ordinary.logical, [1920.0, 1080.0]);
    assert_eq!(ordinary.raster, (1280, 720));
}

#[test]
fn sprite_grid_reports_engine_render_var() {
    use paper_scene::model::{SpriteFrame, Texture};
    let mut texture = paper_scene::model::solid_texture();
    texture.width = 400;
    texture.height = 200;
    assert_eq!(sprite_grid(&texture), [0.0, 0.0, 0.0, 0.5]);
    let frame = |x: f32, y: f32| SpriteFrame {
        axes: [0.25, 0.0, 0.0, 0.5],
        uv: [x, y, 0.25, 0.5],
        rotated: false,
        time: 0.1,
        image: 0,
    };
    texture.frames = vec![frame(0.0, 0.0), frame(0.25, 0.0), frame(0.5, 0.0)];
    let grid = sprite_grid(&texture);
    assert_eq!(grid[0], 0.25);
    assert_eq!(grid[1], 0.5);
    assert_eq!(grid[2], 3.0);
    assert!((grid[3] - 1.0).abs() < 1e-6);
    let _: &Texture = &texture;
}

#[test]
fn layer_model_matrix_composes_origin_rotation_and_scale_without_size() {
    let mut layer = paper_scene::model::Layer {
        texture_name: None,
        material: None,
        live_text: None,
        script_text: None,
        kind: paper_scene::model::LayerKind::Image,
        mouse: paper_scene::mouse::LayerMouse::default(),
        id: "1".into(),
        name: "L".into(),
        visible: true,
        texture: paper_scene::model::solid_texture(),
        puppet: None,
        center: (300.0, 200.0),
        size: (944.0, 147.0),
        scale: (7.375, 1.15),
        depth: 5.0,
        scene_order: 0,
        alpha: 1.0,
        angle: -std::f32::consts::FRAC_PI_2,
        color: [1.0, 1.0, 1.0],
        color_blend: 0,
        pad_uv: false,
        spritesheet: false,
        passthrough: false,
        fullscreen: false,
        geometry: None,
        composition_size: None,
        copy_background: false,
        compose_depth: false,
        solid: false,
        base_scene_input: false,
        effects: Vec::new(),
    };
    let (model, inv) = layer_model_matrix(LayerPose::from(&layer), (1920.0, 1080.0));
    let x_axis = transform(&model, [1.0, 0.0, 0.0, 0.0]);
    assert!((x_axis[0]).abs() < 1e-5 && (x_axis[1] - 7.375).abs() < 1e-5, "{x_axis:?}");
    assert_eq!(transform(&model, [0.0, 0.0, 0.0, 1.0]), [300.0, 880.0, 5.0, 1.0]);
    let back = transform(&inv, [300.0, 880.0, 5.0, 1.0]);
    assert!(back.iter().zip([0.0, 0.0, 0.0, 1.0]).all(|(a, b)| (a - b).abs() < 1e-4), "{back:?}");
    layer.angle = 0.0;
    let (model, _) = layer_model_matrix(LayerPose::from(&layer), (1920.0, 1080.0));
    assert!((model[0] - 7.375).abs() < 1e-6 && (model[5] - 1.15).abs() < 1e-6);
    assert_ne!(model[0], 944.0);
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn clock_effects_refresh_pixels_across_a_minute_without_continuous_animation() {
    let root = std::env::var("SKWD_WE_CLOCK_LIBRARY").expect("Workshop library path");
    let sd = crate::shared::test_device();
    for (item, layer_id, has_effect) in
        [("3735385298", "66", true), ("2138975215", "67", false), ("3735385298", "58", true)]
    {
        let dir = std::path::Path::new(&root).join(item);
        let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
        let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
        assert!(model.scripts.is_none(), "known native clocks must not allocate a JS runtime");
        model.layers.retain(|layer| layer.id == layer_id);
        model.particles.clear();
        model.skipped.clear();
        assert_eq!(model.layers.len(), 1);
        model.canvas = (1024.0, 512.0);
        model.clear = [0.5; 3];
        model.layers[0].geometry = None;
        model.layers[0].center = (512.0, 256.0);
        model.layers[0].visible = true;
        model.layers[0].mouse = paper_scene::mouse::LayerMouse::default();
        assert_eq!(!model.layers[0].effects.is_empty(), has_effect);
        assert!(model.layers[0].live_text.is_some());
        let raster =
            layer_effect_dimensions(&model.layers[0], scene_dimensions_for(model.canvas, 4096));
        assert_eq!(raster, (model.layers[0].texture.width, model.layers[0].texture.height));
        let mut group = build_group(&sd, &mut model, true, &[(1024, 512)], FillMode::Fit).unwrap();
        assert!(!group.animated(), "clock effects must sleep between clock ticks");
        assert_eq!(!group.fx.is_empty(), has_effect, "live effects must survive static baking");
        group.live_text_due = f32::INFINITY;
        let now = paper_scene::dynamic_text::LocalTime {
            hour: 20,
            minute: 44,
            second: 59,
            day: 11,
            month: 9,
            year: 2026,
            weekday: 5,
            millisecond: 0,
        };
        let mut frames = Vec::new();
        for (index, instant) in [
            now,
            paper_scene::dynamic_text::LocalTime {
                minute: 45,
                second: 0,
                day: 12,
                weekday: 6,
                ..now
            },
            now,
        ]
        .into_iter()
        .enumerate()
        {
            group.refresh_live_text(instant).unwrap();
            group.compose(index as f32, 1.0 / 60.0).unwrap();
            let expected = if layer_id == "58" {
                if index == 1 { "12.septembre.2026" } else { "11.septembre.2026" }
            } else if index == 1 {
                "20:45:00"
            } else {
                "20:44:59"
            };
            assert_eq!(group.live_text[0].shown, expected);
            let (w, h, pixels) = group.read_canvas().unwrap();
            if let Ok(evidence) = std::env::var("SKWD_WE_CLOCK_EVIDENCE") {
                image::save_buffer(
                    std::path::Path::new(&evidence).join(format!("{item}-{layer_id}-{index}.png")),
                    &pixels,
                    w,
                    h,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            frames.push(pixels);
        }
        assert_ne!(frames[0], frames[1], "composed clock pixels must change");
        assert_eq!(frames[0], frames[2], "only the requested clock value should change the frame");
        group.destroy();
    }
}

#[test]
fn pointer_unprojection_uses_centered_layer_coordinates() {
    let canvas = (1920.0, 1080.0);
    let centered =
        layer_projection_inverse(&model_inverse((960.0, 540.0, 0.0), [1.0; 3], 0.0), canvas);
    assert_eq!([centered[12], centered[13]], [0.0, 0.0]);
    assert_eq!([centered[0], centered[5]], [960.0, 540.0]);
    let shifted =
        layer_projection_inverse(&model_inverse((480.0, 270.0, 0.0), [2.0, 3.0, 1.0], 0.0), canvas);
    assert_eq!([shifted[12], shifted[13]], [240.0, 90.0]);
    assert_eq!([shifted[0], shifted[5]], [480.0, 180.0]);
    let rotated = layer_projection_inverse(
        &model_inverse((960.0, 540.0, 0.0), [1.0; 3], std::f32::consts::FRAC_PI_2),
        canvas,
    );
    assert!((rotated[1] + 960.0).abs() < 0.001);
    assert!((rotated[4] - 540.0).abs() < 0.001);
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_XRAY_PROJECT"]
fn hidden_animated_layer_remains_available_to_xray_on_hover() {
    let dir = std::path::PathBuf::from(
        std::env::var("SKWD_WE_XRAY_PROJECT").expect("ROG Workshop project path"),
    );
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    let sd = crate::shared::test_device();
    let mut group = build_group(&sd, &mut model, false, &[(1280, 720)], FillMode::Fit).unwrap();
    assert!(group.scripts.is_some());
    let source = group.fx.iter().find(|fx| fx.layer_id == "13").unwrap();
    assert!(!source.sampled_targets.is_empty());
    let mut counts = Vec::new();
    for (index, position) in [[0.0, 0.0], [0.5, 0.5], [0.0, 0.0]].into_iter().enumerate() {
        group.mouse.update(index as u64 + 1, position, [false; 3], (1280, 720), FillMode::Fit);
        group.compose(index as f32 / 15.0, 1.0 / 15.0).unwrap();
        let (w, h, pixels) = group.read_canvas().unwrap();
        let count = pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(index, rgba)| {
                let (x, y) = (*index as u32 % w, *index as u32 / w);
                x > w * 35 / 100
                    && x < w * 65 / 100
                    && y > h * 40 / 100
                    && y < h * 62 / 100
                    && rgba[0] > 120
                    && u16::from(rgba[0]) > u16::from(rgba[1]) * 2
                    && f32::from(rgba[0]) > f32::from(rgba[2]) * 1.25
            })
            .count();
        counts.push(count);
    }
    group.destroy();
    assert!(counts[1] > counts[0] + 250, "reveal on hover: {counts:?}");
    assert!(counts[1] > counts[2] + 250, "clear on departure: {counts:?}");
}

pub(super) fn test_layer(visible: bool, effects: usize) -> paper_scene::model::Layer {
    let mut layer = paper_scene::model::Layer {
        texture_name: None,
        material: None,
        id: String::new(),
        name: String::new(),
        visible,
        texture: paper_scene::model::solid_texture(),
        puppet: None,
        center: (0.0, 0.0),
        size: (1.0, 1.0),
        scale: (1.0, 1.0),
        depth: 0.0,
        scene_order: 0,
        alpha: 1.0,
        angle: 0.0,
        color: [1.0; 3],
        color_blend: 0,
        pad_uv: false,
        spritesheet: false,
        passthrough: false,
        fullscreen: false,
        geometry: None,
        composition_size: None,
        copy_background: false,
        compose_depth: false,
        solid: false,
        base_scene_input: false,
        live_text: None,
        script_text: None,
        kind: paper_scene::model::LayerKind::Image,
        mouse: paper_scene::mouse::LayerMouse::default(),
        effects: Vec::new(),
    };
    layer.effects = (0..effects)
        .map(|_| paper_scene::effects::Effect {
            state: paper_scene::effects::EffectState::default(),
            name: String::new(),
            passes: Vec::new(),
            fbos: Vec::new(),
            swaps: Vec::new(),
            clear_functions: std::collections::BTreeMap::new(),
        })
        .collect();
    layer
}

pub(super) fn particle_package() -> paper_scene::pkg::Package {
    let tex = effect_lifetime_tests::texture_levels(&[paper_scene::tex::MipLevel {
        width: 1,
        height: 1,
        data: vec![255, 80, 40, 255],
    }]);
    sound_package(&[
        ("scene.json", br#"{"objects":[{"id":1,"particle":"particles/p.json","origin":"128 128 0"}]}"#),
        ("particles/p.json", br#"{"renderer":[{"name":"sprite"}],"material":"materials/p.json","emitter":[{"name":"boxrandom","rate":20}],"initializer":[{"name":"lifetimerandom","min":10,"max":10},{"name":"sizerandom","min":64,"max":64}]}"#),
        ("materials/p.json", br#"{"passes":[{"shader":"particle","textures":["mask"],"blending":"translucent"}]}"#),
        ("materials/mask.tex", &tex),
        ("shaders/particle.vert", b"attribute vec3 a_Position;\nattribute vec4 a_TexCoordVec4;\nattribute vec4 a_TexCoordVec4C1;\nattribute vec4 a_Color;\nattribute vec2 a_TexCoordC2;\nuniform mat4 g_ModelViewProjectionMatrix;\nvarying vec2 v_TexCoord;\nvarying vec4 v_Color;\nvoid main(){vec3 position=a_Position+vec3((a_TexCoordVec4.xy*2.0-1.0)*a_TexCoordVec4.w,0.0);gl_Position=mul(vec4(position,1.0),g_ModelViewProjectionMatrix);v_TexCoord=a_TexCoordVec4.xy;v_Color=a_Color;}\n"),
        ("shaders/particle.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvarying vec4 v_Color;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}\n"),
    ])
}

pub(super) struct DynamicParticleFixture {
    pub(super) group: Group,
    package: paper_scene::pkg::Package,
    shared: std::sync::MutexGuard<'static, crate::shared::SharedDevice>,
}

impl DynamicParticleFixture {
    pub(super) fn new() -> Result<Self> {
        let package = particle_package();
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some("/nonexistent")),
        )?;
        anyhow::ensure!(model.skipped.is_empty(), "particle fixture skipped: {:?}", model.skipped);
        model.particles.clear();
        let mut backdrop = test_layer(false, 0);
        backdrop.id = "2".into();
        backdrop.texture.pixels =
            std::sync::Arc::new(paper_scene::tex::Pixels::rgba(1, 1, vec![0, 0, 0, 255]));
        backdrop.center = (128.0, 128.0);
        backdrop.size = (256.0, 256.0);
        model.layers.push(backdrop);
        model.canvas = (256.0, 256.0);
        let mut scene = serde_json::json!({"objects":[{
            "id":2,
            "alpha":{"value":1,"script":"export function update(){return 1;}"}
        }]});
        model.scripts = paper_scene::script::SceneScripts::load(
            &mut scene,
            &paper_scene::model::Properties::new(),
            &serde_json::json!({}),
        )?;
        let shared = crate::shared::test_device();
        let group = build_group(&shared, &mut model, true, &[(256, 256)], FillMode::Fill)?;
        anyhow::ensure!(!group.frozen, "dynamic scene froze before particle creation");
        Ok(Self { group, package, shared })
    }

    pub(super) fn layer(&self, id: &str) -> Result<paper_scene::model::ParticleLayer> {
        let mut scene = paper_scene::model::load_with(
            &self.package,
            &paper_scene::effects::Assets::discover(Some("/nonexistent")),
        )?;
        let mut layer = scene.particles.pop().context("particle fixture missing")?;
        anyhow::ensure!(layer.system.pass.is_some(), "particle fixture shader pass unavailable");
        anyhow::ensure!(
            layer.system.texture.as_ref().is_some_and(|texture| texture
                .pixels
                .levels
                .first()
                .is_some_and(|level| level.data.starts_with(&[255, 80, 40]))),
            "particle fixture texture unavailable"
        );
        anyhow::ensure!(
            layer.system.pass.as_ref().is_some_and(|pass| pass
                .fragment
                .samplers
                .iter()
                .any(|sampler| sampler.index == 0)),
            "particle fixture lost its base sampler"
        );
        layer.id = id.into();
        layer.scene_order = 1;
        Ok(layer)
    }

    fn media_layer(&self, id: &str) -> Result<paper_scene::model::ParticleLayer> {
        let mut layer = self.layer(id)?;
        layer.system.texture.as_mut().context("particle texture missing")?.system_texture =
            Some(paper_scene::model::MediaTexture::Current);
        Ok(layer)
    }
}

fn scripted_particle_model(script: &str) -> Result<SceneModel> {
    let package = particle_package();
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":256,"height":256}},
        "objects":[{"id":1,"particle":"particles/p.json","origin":"128 128 0",
            "alpha":{"value":1,"script":script}}]
    });
    let bytes = serde_json::to_vec(&scene)?;
    let files: Vec<_> = package
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.path.as_str(),
                if entry.path == "scene.json" { &bytes } else { package.read(entry) },
            )
        })
        .collect();
    let package = sound_package(&files);
    let model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    Ok(model)
}

#[test]
#[ignore = "requires a Vulkan device"]
fn particle_fault_discards() -> Result<()> {
    let mut model = scripted_particle_model(
        r"
const asset = engine.registerAsset('particles/p.json');
let effect;
export function update(value) {
    if (!effect && engine.runtime >= 1) effect = thisScene.createLayer(asset);
    return value;
}
",
    )?;
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(256, 256)], FillMode::Fit)?;
    group.compose(0.0, 0.1)?;
    let particles = group.particles.len();
    let textures = vk::Renderer::scene_texture_live_allocations();
    script::tests::set_fault(script::tests::Fault::Particle);
    group.compose(1.0, 0.1)?;
    group.compose(2.0, 0.1)?;
    group.compose(3.0, 0.1)?;
    assert_eq!(group.particles.len(), particles);
    assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
    let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
    assert!(host.created_layer_slots().is_empty());
    assert!(host.animated());
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn initialized_particle_lifecycle() -> Result<()> {
    let script = r"
const asset = engine.registerAsset('particles/p.json');
let effect, count = 0;
function create() {
    effect = thisScene.createLayer(asset);
    effect.origin = new Vec3(128, 128, 0);
}
export function init(value) { create(); return value; }
export function update(value) {
    if (++count % 2) thisScene.destroyLayer(effect);
    else create();
    return value;
}
";
    let mut model = scripted_particle_model(script)?;
    assert_eq!(model.particles.len(), 1);
    let shared = crate::shared::test_device();
    let textures = vk::Renderer::scene_texture_live_allocations();
    let effects = vk::Renderer::effect_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(256, 256)], FillMode::Fit)?;
    assert_eq!(group.particles.len(), 2);
    let slots = group.textures.len();
    let resources =
        group.scripts.as_ref().unwrap().host.as_ref().unwrap().image_resources().unwrap();
    let payload = resources.payload_bytes();
    for frame in 1..=20 {
        group.compose(frame as f32 * 0.1, 0.1)?;
        assert_eq!(group.particles.len(), if frame % 2 == 0 { 2 } else { 1 });
        if frame % 2 == 0 {
            assert_eq!(group.textures.len(), slots);
            assert_eq!(resources.payload_bytes(), payload);
        } else {
            assert!(resources.payload_bytes() < payload);
        }
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert_eq!(host.scene["objects"].as_array().unwrap().len(), 2);
        assert_eq!(host.created_layer_slots().len(), usize::from(frame % 2 == 0));
        assert!(host.diagnostics.is_empty());
    }
    let (_, _, pixels) = group.read_canvas()?;
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > 128));
    group.destroy();
    assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
    assert_eq!(vk::Renderer::effect_live_allocations(), effects);
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn dynamic_particle_renders() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let textures = vk::Renderer::scene_texture_live_allocations();
    let effects = vk::Renderer::effect_live_allocations();
    let layer = fixture.layer("@dynamic-1")?;
    let group = &mut fixture.group;
    group.add_dynamic_particle(layer)?;
    anyhow::ensure!(group.particles.len() == 1, "dynamic particle was not installed");
    anyhow::ensure!(!group.particles[0].engines.is_empty(), "particle pipeline was not built");
    anyhow::ensure!(
        vk::Renderer::scene_texture_live_allocations() > textures,
        "particle texture was not uploaded"
    );
    anyhow::ensure!(
        vk::Renderer::effect_live_allocations()[0] > effects[0],
        "particle pipeline was not allocated"
    );
    group.compose(0.5, 0.5)?;
    anyhow::ensure!(!group.particles[0].sim.particles.is_empty(), "dynamic particle did not emit");
    let mut packed = Vec::new();
    let count = paper_scene::particles::pack_sprites(
        &group.particles[0].system,
        &group.particles[0].sim,
        0,
        &mut packed,
    );
    anyhow::ensure!(count > 0, "dynamic particles were not drawable");
    let (_, _, pixels) = group.read_canvas()?;
    anyhow::ensure!(
        pixels.chunks_exact(4).any(|pixel| pixel[0] > pixel[2]),
        "dynamic particle did not render"
    );
    group.renderer.wait_render()?;
    group.retire_dynamic_particle("@dynamic-1", false);
    anyhow::ensure!(group.particles.is_empty(), "dynamic particle remained active");
    anyhow::ensure!(
        vk::Renderer::scene_texture_live_allocations() == textures,
        "dynamic texture was retained"
    );
    anyhow::ensure!(
        vk::Renderer::effect_live_allocations() == effects,
        "dynamic pipeline or buffers were retained"
    );
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn particle_buffer_demand() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = vk::Renderer::effect_live_allocations();
    let mut layer = fixture.layer("@buffer-growth")?;
    layer.system.max_count = 19;
    let group = &mut fixture.group;
    group.add_dynamic_particle(layer)?;
    let engine = group.particles[0].engines.first().context("missing particle engine")?;
    assert_eq!((engine.capacity, engine.limit), (0, 19));
    assert!(engine.buffers.is_none());
    let allocations = vk::Renderer::effect_live_allocations();
    assert_eq!(allocations[2], baseline[2]);
    for (time, capacity) in [(0.25, 8), (0.5, 16), (0.75, 16), (1.0, 19), (1.25, 19)] {
        group.compose(time, 0.25)?;
        let engine = group.particles[0].engines.first().unwrap();
        assert_eq!(engine.capacity, capacity);
        let (vertices, indices) = engine.buffers.as_ref().context("missing particle buffers")?;
        assert_eq!(
            vertices.size,
            capacity * particle_stride(engine.mesh) * std::mem::size_of::<f32>()
        );
        assert_eq!(indices.size, capacity * 6 * std::mem::size_of::<u32>());
        assert_eq!(vk::Renderer::effect_live_allocations()[2], allocations[2] + 2);
        let (_, _, pixels) = group.read_canvas()?;
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > pixel[2]));
    }
    group.retire_dynamic_particle("@buffer-growth", false);
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn particle_first_fallback() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = vk::Renderer::effect_live_allocations();
    let layer = fixture.layer("@first-failure")?;
    let group = &mut fixture.group;
    group.add_dynamic_particle(layer)?;
    PARTICLE_INDEX_FAILURE.set(true);
    group.compose(0.5, 0.5)?;
    let engine = group.particles[0].engines.first().context("missing particle engine")?;
    assert_eq!(engine.capacity, 0);
    assert!(engine.buffers.is_none());
    assert_eq!(vk::Renderer::effect_live_allocations()[2], baseline[2]);
    let (_, _, pixels) = group.read_canvas()?;
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > pixel[2]));
    PARTICLE_INDEX_FAILURE.set(true);
    group.compose(1.0, 0.5)?;
    assert!(group.particles[0].engines.first().unwrap().buffers.is_none());
    assert!(group.particles[0].engines.first().unwrap().failed_growth);
    let (_, _, pixels) = group.read_canvas()?;
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > pixel[2]));
    assert_eq!(vk::Renderer::effect_live_allocations()[2], baseline[2]);
    group.compose(1.5, 0.5)?;
    assert!(group.particles[0].engines.first().unwrap().buffers.is_some());
    assert!(!group.particles[0].engines.first().unwrap().failed_growth);
    group.retire_dynamic_particle("@first-failure", false);
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn particle_growth_fallback() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = vk::Renderer::effect_live_allocations();
    let mut layer = fixture.layer("@growth-failure")?;
    layer.system.max_count = 19;
    let group = &mut fixture.group;
    group.add_dynamic_particle(layer)?;
    group.compose(0.25, 0.25)?;
    let allocations = vk::Renderer::effect_live_allocations();
    assert_eq!(group.particles[0].engines.first().unwrap().capacity, 8);
    PARTICLE_INDEX_FAILURE.set(true);
    group.compose(0.5, 0.25)?;
    assert_eq!(group.particles[0].engines.first().unwrap().capacity, 8);
    assert_eq!(vk::Renderer::effect_live_allocations(), allocations);
    let (_, _, pixels) = group.read_canvas()?;
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > pixel[2]));
    PARTICLE_INDEX_FAILURE.set(true);
    group.compose(0.75, 0.25)?;
    assert_eq!(group.particles[0].engines.first().unwrap().capacity, 8);
    assert!(group.particles[0].engines.first().unwrap().failed_growth);
    assert_eq!(vk::Renderer::effect_live_allocations(), allocations);
    let (_, _, pixels) = group.read_canvas()?;
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > pixel[2]));
    group.compose(1.0, 0.25)?;
    assert_eq!(group.particles[0].engines.first().unwrap().capacity, 19);
    assert!(!group.particles[0].engines.first().unwrap().failed_growth);
    group.retire_dynamic_particle("@growth-failure", false);
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn particle_extras_without_buffers() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let layer = fixture.layer("@empty-buffer")?;
    let group = &mut fixture.group;
    group.add_dynamic_particle(layer)?;
    let particle = &mut group.particles[0];
    particle.system.extra_renderers.push(particle.system.primary_render());
    particle.engines.first_mut().unwrap().limit = 0;
    group.step_particles(particle_clock(0.5))?;
    let (both, draws) = group.particle_quads(0.5, 0.5)?;
    assert!(draws.is_empty());
    group.particles[0].system.extra_renderers.clear();
    let (primary, _) = group.particle_quads(0.5, 0.0)?;
    assert!(!primary.is_empty());
    assert_eq!(both.len(), primary.len() * 2);
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn nan_particles_draw_nothing() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let layer = fixture.layer("@nan")?;
    let group = &mut fixture.group;
    group.add_dynamic_particle(layer)?;
    group.particles[0].engines.first_mut().unwrap().limit = 0;
    group.step_particles(particle_clock(0.5))?;
    let (quads, _) = group.particle_quads(0.5, 0.0)?;
    assert!(!quads.is_empty());
    for (index, particle) in group.particles[0].sim.particles.iter_mut().enumerate() {
        match index % 3 {
            0 => particle.pos[0] = f32::NAN,
            1 => particle.alpha = f32::NAN,
            _ => particle.size = f32::NAN,
        }
    }
    let (quads, draws) = group.particle_quads(0.5, 0.0)?;
    assert!(quads.is_empty());
    assert!(draws.is_empty());
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn mixed_particle_lifecycle() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = vk::Renderer::effect_live_allocations();
    for reverse in [false, true] {
        let mut layer = fixture.layer("@mixed")?;
        let pass = fixture.layer("@extra")?.system.pass;
        let sprite = layer.system.primary_render();
        let trail = paper_scene::particles::ParticleRender {
            renderer: paper_scene::particles::Renderer::Trail,
            ..sprite
        };
        if reverse {
            layer.system.renderer = trail.renderer;
        }
        layer.system.extra_renderers = vec![if reverse { sprite } else { trail }, sprite];
        layer.system.extra_passes = vec![pass, None];
        let group = &mut fixture.group;
        group.add_dynamic_particle(layer)?;
        assert_eq!(group.particles[0].engines.len(), 2);
        assert!(group.particles[0].engines.iter().all(|engine| engine.buffers.is_none()));
        group.step_particles(particle_clock(0.5))?;
        PARTICLE_INDEX_FAILURE.set(true);
        let (quads, draws) = group.particle_quads(0.5, 0.5)?;
        assert!(!quads.is_empty());
        assert!(
            quads.iter().all(|quad| quad.renderer.1 == 0 || (!reverse && quad.renderer.1 == 2))
        );
        assert!(draws.iter().all(|draw| draw.engine == 1));
        assert_eq!(draws.len(), if reverse { 2 } else { 1 });
        let span = ParticleBatches { from: 0, before: None, mirrored: false };
        let batches = particle_batches(&group.particles, &[], &quads, &draws, span, None);
        let head = quads.iter().filter(|quad| quad.renderer.1 == 0).count();
        assert_eq!(batches[0].after, head);
        assert!(group.particles[0].engines[0].buffers.is_none());
        let (quads, draws) = group.particle_quads(0.5, 0.0)?;
        assert!(quads.is_empty());
        assert_eq!(draws.iter().map(|draw| draw.renderer).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(group.particles[0].engines.iter().all(|engine| engine.buffers.is_some()));
        group.retire_dynamic_particle("@mixed", false);
        assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    }
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn mixed_camera_rollback() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    fixture.group.camera = Some(Camera {
        eye: [0.0, 0.0, 10.0],
        center: [0.0; 3],
        up: [0.0, 1.0, 0.0],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    });
    let effects = vk::Renderer::effect_live_allocations();
    let textures = vk::Renderer::scene_texture_live_allocations();
    for primary in [false, true] {
        let mut layer = fixture.layer("@mixed-rejected")?;
        let mut render = layer.system.primary_render();
        render.renderer = paper_scene::particles::Renderer::Trail;
        layer.system.extra_renderers.push(render);
        if primary {
            layer.system.extra_passes.push(layer.system.pass.take());
        } else {
            layer.system.extra_passes.push(None);
        }
        let error = fixture.group.add_dynamic_particle(layer).unwrap_err();
        assert_eq!(error.to_string(), DYNAMIC_CAMERA_ENGINE_UNAVAILABLE);
        assert!(fixture.group.particles.is_empty());
        assert_eq!(vk::Renderer::effect_live_allocations(), effects);
        assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
    }
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native trail limit fixtures"]
fn sprite_trail_pixels() -> Result<()> {
    let path = std::env::var("SKWD_TRAIL_PROBE")?;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let output = std::env::var_os("SKWD_TRAIL_OUTPUT").map(std::path::PathBuf::from);
    if let Some(output) = &output {
        std::fs::create_dir_all(output)?;
    }
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.particles.len(), 6);
    assert!(
        model.particles.iter().all(|layer| layer
            .system
            .pass
            .as_ref()
            .is_some_and(|pass| pass.geometry.is_some()))
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    assert!(group.particles.iter().all(|particle| !particle.engines.is_empty()));
    for frame in 0..=90 {
        group.compose(frame as f32 / 30.0, 1.0 / 30.0)?;
    }
    for gpu in [true, false] {
        if !gpu {
            for particle in &mut group.particles {
                for engine in particle.engines.drain(..) {
                    engine.destroy(&group.renderer);
                }
            }
            group.compose(3.0, 0.0)?;
        }
        let (width, height, pixels) = group.read_canvas()?;
        if let Some(output) = &output {
            image::save_buffer(
                output.join(if gpu { "gpu.png" } else { "cpu.png" }),
                &pixels,
                width,
                height,
                image::ColorType::Rgba8,
            )?;
        }
        for (index, expected) in [150usize, 1500, 30, 180, 30, 180].into_iter().enumerate() {
            let y = height as usize - (90 + index * 180);
            let row = &pixels[y * width as usize * 4..(y + 1) * width as usize * 4];
            let lit: Vec<_> = row
                .chunks_exact(4)
                .enumerate()
                .filter_map(|(x, pixel)| (pixel[0] > 128).then_some(x))
                .collect();
            assert!(lit.len().abs_diff(expected) <= 1, "trail {index}, gpu {gpu}: {}", lit.len());
            assert_eq!(lit.last().unwrap() - lit[0] + 1, lit.len());
        }
    }
    group.destroy();
    Ok(())
}

fn clock_pixels(prewarm: bool) -> Result<(image::RgbaImage, image::RgbaImage)> {
    let path = std::env::var("SKWD_PARTICLE_CLOCK_PROBE")?;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let clock = std::fs::read(std::env::var("SKWD_PARTICLE_CLOCK_TIMESTEPS")?)?;
    let clock: Vec<f32> = serde_json::from_slice(&clock)?;
    let reference = image::open(std::env::var("SKWD_PARTICLE_CLOCK_REFERENCE")?)?.to_rgba8();
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.particles.len(), 6);
    assert_eq!(clock.len(), 65);
    let rates =
        model.particles.iter().map(|particle| particle.system.rate_scale).collect::<Vec<_>>();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    let scripted_rates =
        group.particles.iter().map(|particle| particle.system.rate_scale).collect::<Vec<_>>();
    if !prewarm {
        for (particle, rate) in group.particles.iter_mut().zip(rates) {
            particle.system.rate_scale = rate;
        }
    }
    for (index, particle) in group.particles.iter_mut().enumerate() {
        particle.sim = paper_scene::particles::Sim::new(PARTICLE_SEED ^ (index as u32 + 1));
        particle.followers = paper_scene::particles::Followers::default();
    }
    if prewarm {
        prewarm_particles(&mut group.particles, 0, particle_clock(0.0));
    }
    group.step_particles(particle_clock(clock[0]))?;
    for (particle, rate) in group.particles.iter_mut().zip(scripted_rates) {
        particle.system.rate_scale = rate;
    }
    let mut time = clock[0];
    for &dt in clock.iter().skip(1).take(60) {
        time += dt;
        group.compose(time, dt)?;
    }
    let (quads, draws) = group.particle_quads(time, 0.0)?;
    let engines = group.particles.iter().map(|particle| particle.engines.len()).sum::<usize>();
    eprintln!(
        "particle paths: {engines} pipelines, {} draws, {} fallback quads",
        draws.len(),
        quads.len()
    );
    assert!(quads.is_empty(), "particle fallback active");
    for particle in &group.particles {
        for render in std::iter::once(particle.system.primary_render())
            .chain(particle.system.extra_renderers.iter().copied())
        {
            assert!(
                particle.engines.iter().any(|engine| engine.render == render),
                "particle engine missing"
            );
        }
    }
    let (width, height, pixels) = group.read_canvas()?;
    if let Some(output) = std::env::var_os("SKWD_PARTICLE_CLOCK_OUTPUT") {
        image::save_buffer(output, &pixels, width, height, image::ColorType::Rgba8)?;
    }
    assert_eq!(reference.dimensions(), (width, height));
    group.destroy();
    Ok((image::RgbaImage::from_raw(width, height, pixels).unwrap(), reference))
}

#[test]
#[ignore = "requires Vulkan and native particle clock fixtures"]
fn particle_clock_pixels() -> Result<()> {
    let (actual, reference) = clock_pixels(false)?;
    let (width, height) = actual.dimensions();
    let bounds = |row: &[u8]| {
        let mut lit =
            row.chunks_exact(4).enumerate().filter_map(|(x, pixel)| (pixel[0] > 128).then_some(x));
        let first = lit.next()?;
        Some((first, lit.next_back().unwrap_or(first)))
    };
    for index in 0..6 {
        for offset in [0, 40] {
            let y = height as usize - (90 + index * 180 + offset);
            let range = y * width as usize * 4..(y + 1) * width as usize * 4;
            let Some(expected) = bounds(&reference.as_raw()[range.clone()]) else { continue };
            let actual = bounds(&actual.as_raw()[range]).unwrap();
            eprintln!("particle {index}, row {offset}: {actual:?}, native {expected:?}");
            assert!(actual.0.abs_diff(expected.0) <= 1 && actual.1.abs_diff(expected.1) <= 1);
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native rope attribute fixtures"]
fn rope_attribute_pixels() -> Result<()> {
    compare_clock_pixels(clock_pixels(false)?);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native particle prewarm fixtures"]
fn particle_prewarm_pixels() -> Result<()> {
    compare_clock_pixels(clock_pixels(true)?);
    Ok(())
}

fn compare_clock_pixels((actual, reference): (image::RgbaImage, image::RgbaImage)) {
    let different = actual
        .pixels()
        .zip(reference.pixels())
        .filter(|(actual, expected)| {
            actual.0[..3]
                .iter()
                .zip(&expected.0[..3])
                .any(|(actual, expected)| actual.abs_diff(*expected) > 1)
        })
        .count();
    assert_eq!(different, 0);
}

#[test]
#[ignore = "requires Vulkan and native rope history fixtures"]
fn rope_history_pixels() -> Result<()> {
    let path = std::env::var("SKWD_ROPE_HISTORY_PROBE")?;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.particles.len(), 6);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    assert!(group.particles.iter().all(|particle| !particle.engines.is_empty()));
    for frame in 0..=90 {
        group.compose(frame as f32 / 30.0, 1.0 / 30.0)?;
    }
    let (width, height, pixels) = group.read_canvas()?;
    if let Some(output) = std::env::var_os("SKWD_ROPE_HISTORY_OUTPUT") {
        image::save_buffer(output, &pixels, width, height, image::ColorType::Rgba8)?;
    }
    for (index, expected) in [124usize, 150, 124, 124, 124, 203].into_iter().enumerate() {
        let y = height as usize - (90 + index * 180);
        let row = &pixels[y * width as usize * 4..(y + 1) * width as usize * 4];
        let lit = row.chunks_exact(4).filter(|pixel| pixel[0] > 128).count();
        assert!(lit.abs_diff(expected) <= 1, "rope {index}: {lit}");
    }
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and a RopeTrail fixture"]
fn rope_buffer_demand() -> Result<()> {
    let path = std::env::var("SKWD_ROPE_PROBE")?;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    anyhow::ensure!(model.skipped.is_empty(), "{:?}", model.skipped);
    model
        .particles
        .retain(|layer| layer.system.renderer == paper_scene::particles::Renderer::Ribbon);
    anyhow::ensure!(model.particles.len() == 1, "missing RopeTrail fixture");
    anyhow::ensure!(
        model.particles[0].system.pass.as_ref().is_some_and(|pass| pass.geometry.is_some()),
        "missing rope shader"
    );
    model.particles[0].system.max_count = 32_768;
    model.particles[0].system.trail.segments = paper_scene::particles::MAX_TRAIL_SEGMENTS;
    let shared = crate::shared::test_device();
    let baseline = vk::Renderer::effect_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    let engine = group.particles[0].engines.first().context("missing rope pipeline")?;
    assert!(engine.mesh == vk::ParticleMesh::RopePoints);
    assert_eq!(engine.limit, 32_768 * 32);
    assert!(engine.capacity <= 1);
    assert_eq!(engine.buffers.is_some(), engine.capacity > 0);
    for frame in 1..=120 {
        group.compose(frame as f32 / 60.0, 1.0 / 60.0)?;
    }
    let engine = group.particles[0].engines.first().unwrap();
    assert!(engine.capacity > 0 && engine.capacity <= 64);
    let (vertices, indices) = engine.buffers.as_ref().context("missing rope buffers")?;
    assert!(vertices.size + indices.size < 16 * 1024);
    eprintln!(
        "rope buffers: capacity={} limit={} vertices={} indices={}",
        engine.capacity, engine.limit, vertices.size, indices.size
    );
    let (_, _, pixels) = group.read_canvas()?;
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > 128));
    group.destroy();
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn dynamic_particle_slots_reuse() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = fixture.group.textures.len();
    let effects = vk::Renderer::effect_live_allocations();
    for id in ["@dynamic-2", "@dynamic-3"] {
        let layer = fixture.layer(id)?;
        fixture.group.add_dynamic_particle(layer)?;
    }
    let two_slots = fixture.group.textures.len();
    fixture.group.retire_dynamic_particle("@dynamic-2", false);
    let layer = fixture.layer("@dynamic-4")?;
    fixture.group.add_dynamic_particle(layer)?;
    anyhow::ensure!(
        fixture.group.textures.len() == two_slots,
        "dynamic texture slot was not reused"
    );
    for id in ["@dynamic-3", "@dynamic-4"] {
        fixture.group.retire_dynamic_particle(id, false);
    }
    anyhow::ensure!(fixture.group.textures.len() == baseline, "dynamic texture slots grew");
    anyhow::ensure!(
        vk::Renderer::effect_live_allocations() == effects,
        "dynamic pipelines retained"
    );
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn dynamic_media_retires() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = fixture.group.textures.len();
    let textures = vk::Renderer::scene_texture_live_allocations();
    let kind = paper_scene::model::MediaTexture::Current;
    for id in ["@dynamic-media-1", "@dynamic-media-2"] {
        let layer = fixture.media_layer(id)?;
        fixture.group.add_dynamic_particle(layer)?;
    }
    let slot = fixture.group.media_slots[&kind];
    anyhow::ensure!(fixture.group.media.is_some(), "media watcher missing");
    fixture.group.retire_dynamic_particle("@dynamic-media-1", false);
    anyhow::ensure!(fixture.group.media_slots[&kind] == slot, "shared media slot lost");
    anyhow::ensure!(fixture.group.media.is_some(), "shared media watcher stopped");
    fixture.group.media_art = Some(paper_scene::tex::Pixels::rgba(1, 1, vec![0; 4]));
    fixture.group.retire_dynamic_particle("@dynamic-media-2", false);
    anyhow::ensure!(fixture.group.media_slots.is_empty(), "retired media slot remained");
    anyhow::ensure!(fixture.group.media.is_none(), "retired media watcher remained");
    anyhow::ensure!(fixture.group.media_art.is_none(), "retired media artwork remained");
    anyhow::ensure!(
        fixture.group.textures.len() == baseline,
        "retired media slot remained allocated"
    );
    anyhow::ensure!(
        vk::Renderer::scene_texture_live_allocations() == textures,
        "retired media texture remained allocated"
    );
    let layer = fixture.media_layer("@dynamic-media-script")?;
    fixture.group.add_dynamic_particle(layer)?;
    fixture.group.retire_dynamic_particle("@dynamic-media-script", true);
    anyhow::ensure!(fixture.group.media_slots.is_empty(), "script retained unused media slot");
    anyhow::ensure!(fixture.group.media.is_some(), "script media watcher stopped");
    let layer = fixture.media_layer("@dynamic-media-last")?;
    fixture.group.add_dynamic_particle(layer)?;
    fixture.group.retire_dynamic_particle("@dynamic-media-last", false);
    anyhow::ensure!(fixture.group.media.is_none(), "unused script media watcher remained");
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn dynamic_video_retires() -> Result<()> {
    let mut fixture = DynamicParticleFixture::new()?;
    let baseline = fixture.group.textures.len();
    let video_dir = tempfile::tempdir()?;
    let video_path = video_dir.path().join("particle.mp4");
    let encoded = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=16x16:r=1",
            "-frames:v",
            "1",
            "-c:v",
            "mpeg4",
            "-y",
        ])
        .arg(&video_path)
        .status()?;
    anyhow::ensure!(encoded.success(), "video fixture encoding failed");
    let mut layer = fixture.layer("@dynamic-video")?;
    layer.system.texture.as_mut().context("particle texture missing")?.video =
        Some(paper_scene::model::Video::Bytes(std::fs::read(&video_path)?.into()));
    let targets = vk::Renderer::scene_target_live_allocations();
    fixture.group.add_dynamic_particle(layer)?;
    anyhow::ensure!(fixture.group.particles[0].dynamic_videos.len() == 1, "video decoder missing");
    anyhow::ensure!(
        vk::Renderer::scene_target_live_allocations() > targets,
        "video texture target missing"
    );
    fixture.group.compose(1.0, 0.5)?;
    let (_, _, pixels) = fixture.group.read_canvas()?;
    anyhow::ensure!(
        pixels.chunks_exact(4).any(|pixel| pixel[2] > pixel[0]),
        "decoded video particle not visible"
    );
    fixture.group.renderer.wait_render()?;
    fixture.group.retire_dynamic_particle("@dynamic-video", false);
    anyhow::ensure!(
        vk::Renderer::scene_target_live_allocations() == targets,
        "video texture target remained"
    );
    anyhow::ensure!(fixture.group.textures.len() == baseline, "video texture slot remained");
    fixture.group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn static_media_survives() -> Result<()> {
    let fixture = DynamicParticleFixture::new()?;
    let kind = paper_scene::model::MediaTexture::Current;
    let mut model = paper_scene::model::load_with(
        &fixture.package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )?;
    let mut static_layer = test_layer(true, 0);
    static_layer.id = "static-media".into();
    static_layer.texture = kind.texture();
    model.layers.push(static_layer);
    model.particles.clear();
    model.canvas = (256.0, 256.0);
    let mut group = build_group(&fixture.shared, &mut model, true, &[(256, 256)], FillMode::Fill)?;
    let slot = group.media_slots[&kind];
    let textures = vk::Renderer::scene_texture_live_allocations();
    let layer = fixture.media_layer("@dynamic-media-3")?;
    group.add_dynamic_particle(layer)?;
    anyhow::ensure!(group.media_slots[&kind] == slot, "static media slot changed");
    group.retire_dynamic_particle("@dynamic-media-3", false);
    anyhow::ensure!(group.media_slots[&kind] == slot, "static media slot lost");
    anyhow::ensure!(group.media.is_some(), "static media watcher stopped");
    anyhow::ensure!(
        vk::Renderer::scene_texture_live_allocations() == textures,
        "static media texture count changed"
    );
    group.destroy();
    fixture.group.destroy();
    Ok(())
}

#[test]
fn chain_admission_prefers_visible_layers_in_scene_order() {
    let layers = vec![
        test_layer(false, 1),
        test_layer(true, 1),
        test_layer(true, 0),
        test_layer(false, 1),
        test_layer(true, 1),
    ];
    let none = BTreeSet::new();
    assert_eq!(chain_admission_order(&layers, true, &none), vec![1, 4, 0, 3]);
    assert_eq!(chain_admission_order(&layers, false, &none), vec![1, 4]);
}

#[test]
fn dependency_chains_admitted() {
    let mut layers = vec![test_layer(false, 1), test_layer(false, 1), test_layer(true, 1)];
    layers[1].id = "source".into();
    let dependencies = BTreeSet::from([String::from("source")]);
    assert_eq!(chain_admission_order(&layers, false, &dependencies), vec![2, 1]);
}

#[test]
fn sprite_controls_hold_play_and_rejoin_the_shared_clock() {
    use paper_scene::script::SpriteOp;
    let frame = |time: f32| paper_scene::model::SpriteFrame {
        image: 0,
        time,
        uv: [0.0; 4],
        axes: [0.0; 4],
        rotated: false,
    };
    let mut animation = LayerAnimation {
        mesh_slot: None,
        quad: 0,
        object: Some(0),
        frames: vec![frame(0.5), frame(0.5), frame(0.5)],
        total: 1.5,
        source: FrameSource::Pages(vec![0]),
        slot: 0,
        image: -1,
        control: SpriteControl::Follow,
        rate: 1.0,
        refresh: None,
    };
    assert_eq!(animation.current_frame(0.6), 1);
    animation.apply(SpriteOp::Frame(2), 0.6);
    assert_eq!(animation.current_frame(0.6), 2);
    assert_eq!(animation.current_frame(0.7), 2);
    animation.apply(SpriteOp::Pause, 0.7);
    assert_eq!(animation.current_frame(5.0), 2);
    animation.apply(SpriteOp::Play, 5.0);
    assert_eq!(animation.current_frame(5.0), 2);
    assert_eq!(animation.current_frame(5.4), 0);
    animation.apply(SpriteOp::Stop, 6.0);
    assert_eq!(animation.current_frame(9.0), 0);
    animation.apply(SpriteOp::Join, 9.0);
    assert_eq!(animation.current_frame(0.6), 1);
}

#[test]
fn background_projection_inverts_translated_rotated_and_mirrored_layer_corners() {
    let canvas = (3840.0, 2160.0);
    for rect in [[1920.0, 440.0, -600.0, 240.0], [3460.0, 220.0, -380.0, 280.0]] {
        for angle in [0.0, std::f32::consts::FRAC_PI_2, std::f32::consts::PI, 0.3] {
            let mut quad = layer_quad(&test_layer(true, 0), 0, canvas);
            quad.rect = rect;
            quad.angle = angle;
            quad.projection = None;
            let matrix = background_projection(&quad, canvas);
            let (sin, cos) = angle.sin_cos();
            for corner in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
                let x = corner[0] * rect[2];
                let y = corner[1] * rect[3];
                let world = [rect[0] + x * cos - y * sin, rect[1] + x * sin + y * cos];
                let source = [world[0] / canvas.0 - 0.5, world[1] / canvas.1 - 0.5, 0.0, 1.0];
                let projected =
                    matrix.map(|row| row.iter().zip(source).map(|(a, b)| a * b).sum::<f32>());
                assert!((projected[0] - corner[0] * 2.0).abs() < 1e-5);
                assert!((projected[1] - corner[1] * 2.0).abs() < 1e-5);
                assert!((projected[2] - 1.0).abs() < 1e-5);
            }
        }
    }
}

#[test]
fn composition_effects_keep_authored_resolution_when_geometry_or_output_shrinks() {
    let mut layer = test_layer(true, 1);
    layer.passthrough = true;
    layer.copy_background = true;
    layer.size = (-1920.0 * 0.30868, 1080.0 * 0.21645);
    layer.scale = (-0.30868, 0.21645);
    layer.composition_size = Some((1920.0, 1080.0));
    let mut low_resolution = scene_dimensions_for((3840.0, 2160.0), 4096);
    low_resolution.raster = (1920, 1080);
    assert_eq!(layer_effect_dimensions(&layer, low_resolution), (1920, 1080));
    low_resolution.raster = (960, 540);
    assert_eq!(layer_effect_dimensions(&layer, low_resolution), (1920, 1080));
    layer.scale = (-0.61736, 0.43290);
    layer.composition_size = Some((960.0, 540.0));
    assert_eq!(layer_effect_dimensions(&layer, low_resolution), (960, 540));
    layer.copy_background = false;
    assert_eq!(layer_effect_dimensions(&layer, low_resolution), (960, 540));
    layer.composition_size = Some((3840.0, 2160.0));
    assert_eq!(layer_effect_dimensions(&layer, low_resolution), (2048, 1152));
    layer.composition_size = None;
    low_resolution.raster = (1920, 1080);
    assert_eq!(layer_effect_dimensions(&layer, low_resolution), (296, 117));
}

#[test]
fn hidden_compositions_defer_authored_targets_until_a_visibility_reload() {
    let mut layer = test_layer(false, 1);
    layer.id = "composition".into();
    layer.size = (-592.6656, 233.766);
    layer.composition_size = Some((1920.0, 1080.0));
    let dimensions = SceneDimensions { logical: [3840.0, 2160.0], raster: (1920, 1080) };
    assert_eq!(layer_effect_dimensions(&layer, dimensions), (296, 117));
    assert_eq!(
        deferred_composition_layers(std::slice::from_ref(&layer), dimensions),
        ["composition"]
    );
    layer.visible = true;
    assert_eq!(layer_effect_dimensions(&layer, dimensions), (1920, 1080));
    assert!(deferred_composition_layers(std::slice::from_ref(&layer), dimensions).is_empty());
    layer.visible = false;
    layer.effects.clear();
    assert!(deferred_composition_layers(std::slice::from_ref(&layer), dimensions).is_empty());
}

#[test]
fn untimed_sprites_advance_once_per_update_and_obey_controls() {
    use paper_scene::script::SpriteOp;
    let mut animation = LayerAnimation {
        mesh_slot: None,
        quad: 0,
        object: Some(0),
        frames: (0..7)
            .map(|image| paper_scene::model::SpriteFrame {
                image,
                time: 1.0,
                uv: [0.0; 4],
                axes: [0.0; 4],
                rotated: false,
            })
            .collect(),
        total: 7.0,
        source: FrameSource::Pages(vec![]),
        slot: 0,
        image: -1,
        control: SpriteControl::Follow,
        rate: 1.0,
        refresh: Some(SpriteRefresh::default()),
    };
    assert_eq!(animation.advance(0.0).image, 0);
    assert_eq!(animation.advance(0.01).image, 1);
    assert_eq!(animation.advance(0.01).image, 1);
    assert_eq!(animation.advance(10.0).image, 2);
    for (tick, image) in (11..18).zip([3, 4, 5, 6, 0, 1, 2]) {
        assert_eq!(animation.advance(tick as f32).image, image);
    }
    animation.apply(SpriteOp::Pause, 17.0);
    assert_eq!(animation.advance(18.0).image, 2);
    animation.apply(SpriteOp::Play, 18.0);
    assert_eq!(animation.advance(18.0).image, 2);
    assert_eq!(animation.advance(19.0).image, 3);
    animation.apply(SpriteOp::Frame(6), 19.0);
    assert_eq!(animation.advance(19.0).image, 6);
    assert_eq!(animation.advance(20.0).image, 0);
    animation.apply(SpriteOp::Stop, 20.0);
    assert_eq!(animation.advance(21.0).image, 0);
    animation.apply(SpriteOp::Rate(0.5), 21.0);
    animation.apply(SpriteOp::Play, 21.0);
    assert_eq!(animation.advance(21.0).image, 0);
    assert_eq!(animation.advance(22.0).image, 1);
    assert_eq!(animation.advance(23.0).image, 1);
    animation.apply(SpriteOp::Rate(1.0), 24.0);
    animation.apply(SpriteOp::Frame(4), 25.0);
    assert_eq!(animation.advance(25.0).image, 4);
    assert_eq!(animation.advance(26.0).image, 5);
}

pub(super) fn mesh_model(scene: &[u8]) -> SceneModel {
    mesh_sampler_model(scene, br#"{"passes":[{"shader":"sample","textures":["_rt_FullFrameBuffer"],"blending":"normal"}]}"#, b"uniform sampler2D g_Texture0;\nuniform float g_Time;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.43359375,0.6484375)+vec2(g_Time*0.001));}")
}

pub(super) fn triangle_mdl() -> Vec<u8> {
    let mut mesh = b"MDLV0004\0".to_vec();
    for value in [9_u32, 1, 1] {
        mesh.extend(value.to_le_bytes());
    }
    mesh.extend(b"materials/mesh.json\0");
    mesh.extend(0_u32.to_le_bytes());
    mesh.extend(60_u32.to_le_bytes());
    for vertex in
        [[-1.0_f32, -1.0, 0.0, 0.0, 1.0], [1.0, -1.0, 0.0, 1.0, 1.0], [-1.0, 1.0, 0.0, 0.0, 0.0]]
    {
        for value in vertex {
            mesh.extend(value.to_le_bytes());
        }
    }
    mesh.extend(6_u32.to_le_bytes());
    for index in [0_u16, 1, 2] {
        mesh.extend(index.to_le_bytes());
    }
    mesh
}

pub(super) const MESH_MATERIAL: &[u8] = b"materials/mesh.json";
const READER_MATERIAL: &[u8] = b"materials/look.json";

pub(super) fn mesh_sampler_model(scene: &[u8], material: &[u8], fragment: &[u8]) -> SceneModel {
    let mesh = triangle_mdl();
    let mut reader = mesh.clone();
    let at =
        reader.windows(MESH_MATERIAL.len()).position(|window| window == MESH_MATERIAL).unwrap();
    reader[at..at + MESH_MATERIAL.len()].copy_from_slice(READER_MATERIAL);
    let white = effect_lifetime_tests::texture_levels(&[paper_scene::tex::MipLevel {
        width: 1,
        height: 1,
        data: vec![255; 4],
    }]);
    let package = sound_package(&[
        ("scene.json", scene),
        ("materials/textures/white.tex", &white),
        ("models/mesh.mdl", &mesh),
        ("models/reader.mdl", &reader),
        ("materials/look.json", br#"{"passes":[{"shader":"look","textures":["_rt_Reflection"],"blending":"normal"}]}"#),
        ("shaders/look.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/look.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
        ("models/sample.json", br#"{"material":"materials/sample.json","instanced":true}"#),
        ("materials/sample.json", material),
        ("shaders/sample.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/sample.frag", fragment),

        ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","depthtest":"enabled","depthwrite":"enabled","cullmode":"nocull","blending":"normal"}]}"#),
        ("shaders/mesh.vert", b"#include \"common.h\"\nattribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/mesh.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
    ]);
    let model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )
    .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    model
}

#[test]
fn mesh_only_scene_admitted() {
    let model = mesh_model(br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl"}]}"#);
    assert!(model.layers.is_empty() && model.particles.is_empty());
    assert_eq!(model.meshes.len(), 1);
    assert!(validate_scene_content(&model).is_ok());
}

#[test]
fn background_only_scenes_distinguish_hidden_content_from_failed_content() {
    use serde_json::json;
    let hidden = json!({"id":1,"image":"broken.json","visible":false});
    let cases = [
        (json!([]), true),
        (json!([hidden.clone()]), true),
        (json!([{"id":2}, {"id":1,"parent":2,"image":"broken.json","visible":false}]), true),
        (json!([hidden.clone(), {"id":2,"sound":["sound.ogg"]}]), true),
        (json!([{"id":1,"image":"broken.json","visible":true}]), false),
        (json!([hidden.clone(), {"id":2,"image":"broken.json"}]), false),
        (json!([hidden.clone(), {"id":2,"particle":"missing.json"}]), false),
        (json!([hidden.clone(), {"id":2,"light":"missing.json"}]), false),
        (json!([hidden, {"id":2,"unknown":"unsupported"}]), false),
        (json!([42]), false),
    ];
    for (objects, expected) in cases {
        let scene = json!({"general":{"clearcolor":"0 1 0"},"objects":objects}).to_string();
        let package = sound_package(&[("scene.json", scene.as_bytes()), ("broken.json", b"{}")]);
        let model =
            paper_scene::model::load_with(&package, &paper_scene::effects::Assets::discover(None))
                .unwrap();
        assert!(model.layers.is_empty() && model.particles.is_empty());
        assert_eq!(validate_scene_content(&model).is_ok(), expected, "{scene}");
        assert_eq!(model.clear, [0.0, 1.0, 0.0]);
    }
    let package = sound_package(&[("scene.json", b"{}")]);
    let model =
        paper_scene::model::load_with(&package, &paper_scene::effects::Assets::discover(None))
            .unwrap();
    assert!(validate_scene_content(&model).is_err());
}

#[test]
fn scene_prefix_segments_preserve_interleaved_order_without_replaying_particles() {
    let quad = |texture, order_bias| vk::SceneQuad {
        uv_axes: None,
        uv_padding: false,
        projection: None,
        order_bias,
        rect: [0.0; 4],
        uv: [0.0; 4],
        tint: [1.0; 4],
        angle: 0.0,
        texture,
        blend: vk::SceneBlend::Alpha,
    };
    let images = [quad(1, 0), quad(3, 0), quad(5, 0), quad(7, 0)];
    let orders = [0, 2, 4, 6];
    let particles = [
        OrderedParticleQuad { scene_order: 1, renderer: (0, 0), quad: quad(2, 0) },
        OrderedParticleQuad { scene_order: 3, renderer: (0, 0), quad: quad(4, 0) },
        OrderedParticleQuad { scene_order: 4, renderer: (0, 0), quad: quad(6, 1) },
    ];
    let mut accumulated = Vec::new();
    let mut from = 0;
    for before in [2, 4, 6, 7] {
        accumulated.extend(
            ordered_scene_quads(&images, &orders, &particles, from, Some(before))
                .iter()
                .map(|quad| quad.texture),
        );
        let replay = ordered_scene_quads(&images, &orders, &particles, 0, Some(before));
        assert_eq!(accumulated, replay.iter().map(|quad| quad.texture).collect::<Vec<_>>());
        from = before;
    }
    assert_eq!(accumulated, [1, 2, 3, 4, 5, 6, 7]);
    assert!(ordered_scene_quads(&images, &orders, &particles, 4, Some(4)).is_empty());
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_PREFIX_PROJECT"]
fn particle_prefix_matches_replay() {
    let dir = std::path::PathBuf::from(
        std::env::var("SKWD_WE_PREFIX_PROJECT").expect("Rooftop Garden Store Workshop path"),
    );
    let project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap()).unwrap();
    let sd = crate::shared::test_device();
    let mut groups = [false, true].map(|replay| {
        let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
        let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
        assert!(model.skipped.is_empty());
        let host = model.scripts.as_ref().expect("scene clock scripts");
        let properties = host.properties.clone();
        let mut scene = pkg.scene_json().unwrap();
        let mut clocks = 0;
        for object in scene["objects"].as_array_mut().unwrap() {
            if let Some(script) = object["text"]["script"].as_str()
                && script.contains("new Date()")
            {
                object["text"]["script"] = serde_json::Value::String(script.replace(
                    "new Date()",
                    "new Date(engine.runtime < 100 ? 1790769600000 : 1790773200000)",
                ));
                clocks += 1;
            }
        }
        assert_eq!(clocks, 3);
        model.scripts =
            paper_scene::script::SceneScripts::load(&mut scene, &properties, &project).unwrap();
        let previous = REPLAY_SNAPSHOTS.replace(replay);
        let built = build_group(&sd, &mut model, false, &[(2560, 1440)], FillMode::Fit);
        REPLAY_SNAPSHOTS.set(previous);
        let mut group = built.unwrap();
        group.fixed_daytime = Some(0.5);
        assert!(group.scene_grab.is_none());
        assert!(group.particles.iter().all(|particle| particle.system.grab_slot.is_none()));
        assert_eq!(group.fx.iter().filter(|fx| fx.text_backdrop.is_some()).count(), clocks);
        assert!(group.fx.iter().filter(|fx| fx.text_backdrop.is_some()).all(|fx| fx.snapshot));
        assert_eq!(
            group.fx.iter().filter(|fx| fx.snapshot && fx.text_backdrop.is_none()).count(),
            8
        );
        group
    });
    matching_scene_pixels(&mut groups, "initial composition");
    let mut first = None;
    let mut changed = false;
    for frame in 0..3000 {
        let time = frame as f32 / 30.0;
        for group in &mut groups {
            group.compose(time, 1.0 / 30.0).unwrap();
        }
        if frame % 100 == 99 {
            let actual = matching_scene_pixels(&mut groups, &format!("frame {frame}"));
            if let Some(first) = &first {
                changed |= first != &actual;
            } else {
                first = Some(actual);
            }
            let draws = |group: &Group| {
                group
                    .snapshot_draws
                    .iter()
                    .fold((0, 0), |(quads, particles), (q, p)| (quads + q, particles + p))
            };
            let incremental = draws(&groups[0]);
            let replay = draws(&groups[1]);
            assert!(incremental.0 < replay.0, "{incremental:?} versus {replay:?}");
            assert!(incremental.1 < replay.1, "{incremental:?} versus {replay:?}");
            eprintln!(
                "frame {frame}: snapshot quads/particle batches {incremental:?} versus {replay:?}"
            );
        }
    }
    assert!(changed, "the scene must animate during the comparison");
    eprintln!("prefix phase: clock change begin");
    let before_clock = matching_scene_pixels(&mut groups, "before clock change");
    for group in &mut groups {
        let before = group.scripts.as_ref().unwrap().host.as_ref().unwrap().scene.clone();
        group.compose(100.0, 0.0).unwrap();
        let after = &group.scripts.as_ref().unwrap().host.as_ref().unwrap().scene;
        assert!(
            before["objects"]
                .as_array()
                .unwrap()
                .iter()
                .zip(after["objects"].as_array().unwrap())
                .any(|(before, after)| before["text"] != after["text"])
        );
    }
    assert!(
        before_clock != matching_scene_pixels(&mut groups, "after clock change"),
        "clock update did not change rendered pixels"
    );
    eprintln!("prefix phase: clock change end, reorder begin");
    for group in &mut groups {
        group.fx.rotate_left(1);
        assert!(group.fx.windows(2).any(|pair| pair[0].scene_order > pair[1].scene_order));
    }
    for frame in 3000..3006 {
        eprintln!("prefix phase: reordered frame {frame}");
        for group in &mut groups {
            group.compose(frame as f32 / 30.0, 1.0 / 30.0).unwrap();
        }
        assert_eq!(groups[0].snapshot_draws, groups[1].snapshot_draws);
        matching_scene_pixels(&mut groups, &format!("reordered frame {frame}"));
    }
    for group in groups {
        group.destroy();
    }
}

fn matching_scene_pixels(groups: &mut [Group; 2], phase: &str) -> Vec<u8> {
    let (width, height, actual) = groups[0].read_canvas().unwrap();
    let (expected_width, expected_height, expected) = groups[1].read_canvas().unwrap();
    assert_eq!((width, height, actual.len()), (expected_width, expected_height, expected.len()));
    let different = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
    assert_eq!(different, 0, "{phase}: incremental prefix changed pixels");
    actual
}

#[test]
#[ignore = "requires Vulkan"]
fn sorting_changes_native_overlap() {
    let mut scene = serde_json::json!({"general":{"orthogonalprojection":{"width":16,"height":16}},"objects":[
        {"id":10,"name":"red","origin":"8 8 0","size":"16 16","color":"1 0 0","alpha":{"value":1,"script":"export function init(){thisScene.sortLayer(thisLayer,2);} export function update(){if(engine.runtime>0)thisScene.sortLayer(thisLayer,engine.runtime<2?0:2);return 1;}"}},
        {"id":15,"name":"unused"},
        {"id":20,"name":"green","origin":"8 8 0","size":"16 16","color":"0 1 0"}
    ]});
    let scripts = paper_scene::script::SceneScripts::load(
        &mut scene,
        &paper_scene::model::Properties::new(),
        &serde_json::json!({}),
    )
    .unwrap();
    let layers = [("10", [1.0, 0.0, 0.0]), ("20", [0.0, 1.0, 0.0]), ("@bloom", [0.0; 3])]
        .into_iter()
        .enumerate()
        .map(|(rank, (id, color))| {
            let mut layer = test_layer(true, 0);
            layer.id = id.into();
            layer.center = (8.0, 8.0);
            layer.size = (16.0, 16.0);
            layer.color = color;
            layer.scene_order = rank;
            if id == "@bloom" {
                layer.alpha = 0.0;
            }
            layer
        })
        .collect();
    let mut model = SceneModel {
        reflection: paper_scene::model::ReflectionUse::default(),
        clear_enabled: true,
        unreflected: BTreeSet::new(),
        custom_shaders: false,
        cameras_move: false,
        camera_source: paper_scene::model::CameraSource::default(),
        skin_viewport: true,
        lights: Vec::new(),
        sun_view: paper_scene::model::lighting::SunView::default(),
        fog: paper_scene::model::fog::Volume::default(),
        cookies: None,
        image_sources: BTreeSet::new(),
        dependencies: BTreeSet::new(),
        scripts,
        keyframes: paper_scene::keyframe::Bindings::default(),
        animated_scene: None,
        canvas: (16.0, 16.0),
        mouse: paper_scene::mouse::Parallax::default(),
        shake: None,
        camera_fov: 50.0,
        perspective_fov: 95.0,
        camera_view: paper_scene::model::CameraView::UNIT,
        camera: None,
        projection: paper_scene::model::ProjectionMode::Orthographic,
        clear: [0.0; 3],
        ambient: [1.0; 3],
        skylight: [1.0; 3],
        layers,
        meshes: Vec::new(),
        particles: Vec::new(),
        skipped: Vec::new(),
        background_only: false,
        hdr: false,
    };
    let sd = crate::shared::test_device();
    let mut group = build_group(&sd, &mut model, true, &[(16, 16)], FillMode::Fit).unwrap();
    group.quads.push(group.quads[0].clone());
    group.quad_scene_order.push(group.quad_scene_order[0]);
    for (time, color, ranks) in [
        (0.0, [255, 0, 0], [2, 1, 5, 2]),
        (1.0, [0, 255, 0], [0, 2, 5, 0]),
        (2.0, [255, 0, 0], [2, 1, 5, 2]),
    ] {
        group.compose(time, 0.016).unwrap();
        let (_, _, pixels) = group.read_canvas().unwrap();
        let center = (8 * 16 + 8) * 4;
        assert_eq!(&pixels[center..center + 3], &color);
        assert_eq!(group.quad_scene_order, ranks);
        assert_eq!(
            group.scripts.as_ref().unwrap().host.as_ref().unwrap().scene["objects"][0]["id"],
            10
        );
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_SORT_CYCLE_PROJECT"]
fn sorting_rejects_graph_cycles() {
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_SORT_CYCLE_PROJECT").unwrap());
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    assert!(model.skipped.is_empty());
    for (id, bind) in [
        ("20", EffectBind::SceneSoFar),
        ("30", EffectBind::LayerComposite { layer: "20".into(), buffer: CompositeBuffer::A }),
    ] {
        let layer = model.layers.iter_mut().find(|layer| layer.id == id).unwrap();
        layer.effects[0].passes[0].binds = vec![(0, bind)];
    }
    let mut scene = pkg.scene_json().unwrap();
    for object in scene["objects"].as_array_mut().unwrap() {
        object["visible"] = serde_json::json!(true);
    }
    scene["objects"][0]["alpha"] = serde_json::json!({"value":1,"script":"export function update(){if(engine.runtime>0&&engine.runtime<3)thisScene.sortLayer(thisScene.getLayer('prefix'),2);if(engine.runtime===3)thisScene.sortLayer(thisLayer,2);thisLayer.name=thisScene.enumerateLayers().map(layer=>thisScene.getInitialLayerConfig(layer).name).join(',');return 1;}"});
    model.scripts = paper_scene::script::SceneScripts::load(
        &mut scene,
        &paper_scene::model::Properties::new(),
        &serde_json::json!({}),
    )
    .unwrap();
    let sd = crate::shared::test_device();
    let mut group = build_group(&sd, &mut model, true, &[(320, 180)], FillMode::Fit).unwrap();
    assert_eq!(group.fx.len(), 2);
    let prefix = group.fx.iter().find(|fx| fx.layer_id == "20").unwrap();
    assert!(prefix.snapshot, "prefix snapshot missing");
    assert!(
        prefix.binds.iter().flatten().any(|(_, bind)| matches!(bind, EffectBind::SceneSoFar)),
        "prefix binding missing"
    );
    let consumer = group.fx.iter().find(|fx| fx.layer_id == "30").unwrap();
    assert!(consumer.binds.iter().flatten().any(|(_, bind)| matches!(bind, EffectBind::LayerComposite { layer, buffer: CompositeBuffer::A } if layer == "20")), "foreign binding missing");
    let ranks = group.quad_scene_order.clone();
    let effects: Vec<_> =
        group.fx.iter().map(|fx| (fx.layer_id.clone(), fx.scene_order, fx.snapshot)).collect();
    let (_, _, baseline) = group.read_canvas().unwrap();
    for time in [1.0, 2.0] {
        group.compose(time, 0.016).unwrap();
        assert_eq!(group.quad_scene_order, ranks);
        assert_eq!(
            group
                .fx
                .iter()
                .map(|fx| (fx.layer_id.clone(), fx.scene_order, fx.snapshot))
                .collect::<Vec<_>>(),
            effects
        );
        let (_, _, pixels) = group.read_canvas().unwrap();
        assert_eq!(pixels, baseline);
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert_eq!(
            host.diagnostics,
            ["layer sort rejected: unsupported scene target dependencies"]
        );
        group.compose(0.0, 0.016).unwrap();
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert_eq!(
            host.scene["objects"][0]["name"],
            "background,prefix,consumer,cell-0,cell-1,cell-2,cell-3"
        );
    }
    group.compose(3.0, 0.016).unwrap();
    group.compose(4.0, 0.016).unwrap();
    assert_eq!(&group.quad_scene_order[..3], &[2, 0, 1]);
    let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
    assert_eq!(
        host.scene["objects"][0]["name"],
        "prefix,consumer,background,cell-0,cell-1,cell-2,cell-3"
    );
    assert_eq!(host.diagnostics.len(), 1);
    group.destroy();
}

#[test]
fn shape_effect_uv() {
    let mut layer = test_layer(true, 1);
    layer.texture.img_width = 1;
    layer.texture.img_height = 1;
    assert_eq!(effect_uv_inset(&layer), [0.15; 2]);
    layer.kind = paper_scene::model::LayerKind::Quad;
    assert_eq!(effect_uv_inset(&layer), [0.0; 2]);
    layer.kind = paper_scene::model::LayerKind::Text;
    layer.texture.img_width = 52;
    layer.texture.img_height = 211;
    assert_eq!(effect_uv_inset(&layer), [0.15 / 52.0, 0.15 / 211.0]);
}

fn empty_effect() -> paper_scene::effects::Effect {
    paper_scene::effects::Effect {
        state: paper_scene::effects::EffectState::default(),
        name: String::new(),
        passes: Vec::new(),
        fbos: Vec::new(),
        swaps: Vec::new(),
        clear_functions: BTreeMap::new(),
    }
}

#[test]
fn compose_ignores_tint() {
    let mut layer = test_layer(true, 0);
    layer.passthrough = true;
    layer.color = [1.0, 0.0, 0.0];
    layer.alpha = 0.5;
    layer.effects.push(empty_effect());
    assert_eq!(layer_quad(&layer, 0, (800.0, 600.0)).tint, [1.0; 4]);
}

#[test]
fn bare_blend_follows_effects() {
    let shown = paper_scene::effects::EffectState::default();
    let hidden = paper_scene::effects::EffectState { visible: false, ..shown };
    assert!(bare_quad_blend(vk::SceneBlend::Add, &[0], &[shown]) == vk::SceneBlend::Alpha);
    assert!(bare_quad_blend(vk::SceneBlend::Add, &[0], &[hidden]) == vk::SceneBlend::Add);
    assert!(bare_quad_blend(vk::SceneBlend::Add, &[], &[shown]) == vk::SceneBlend::Add);
    assert!(chainless_blend(None, Some(vk::SceneBlend::Add)) == Some(vk::SceneBlend::Add));
    assert!(chainless_blend(Some(vk::SceneBlend::Copy), None) == Some(vk::SceneBlend::Alpha));
    assert!(chainless_blend(None, None).is_none());
}

#[test]
fn text_additive_quads() {
    let mut layer = test_layer(true, 0);
    layer.kind = paper_scene::model::LayerKind::Text;
    layer.color_blend = paper_scene::effects::ADDITIVE_COLOR_BLEND;
    assert!(layer_quad(&layer, 0, (800.0, 600.0)).blend == vk::SceneBlend::Add);
    layer.effects.push(empty_effect());
    assert!(layer_quad(&layer, 0, (800.0, 600.0)).blend == vk::SceneBlend::Add);
}

#[test]
#[ignore = "requires Vulkan validation on the renderer qualification host"]
fn material_failure_releases_resources() -> Result<()> {
    let shared = crate::shared::test_device();
    let mut renderer = shared_renderer(&shared, 32, 32)?;
    renderer.ensure_scene_pool(16)?;
    let vertex = b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";
    let package = sound_package(&[
        ("scene.json", br#"{"objects":[]}"#),
        ("shaders/good.vert", vertex),
        ("shaders/bad.vert", vertex),
        ("shaders/good.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
        ("shaders/bad.frag", b"uniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=texSample2D(g_Texture1,vec2(0.5,0.5));}"),
    ]);
    let assets = paper_scene::effects::Assets::discover(Some("/nonexistent"));
    for _ in 0..16 {
        let mut model = paper_scene::model::load_with(&package, &assets)?;
        for name in ["good", "bad"] {
            let mut layer = test_layer(true, 0);
            layer.name = name.into();
            let mut vertex = paper_scene::shader::translate(
                std::str::from_utf8(package.find(&format!("shaders/{name}.vert")).unwrap())
                    .unwrap(),
                paper_scene::shader::Stage::Vertex,
                &BTreeMap::new(),
            );
            let mut fragment = paper_scene::shader::translate(
                std::str::from_utf8(package.find(&format!("shaders/{name}.frag")).unwrap())
                    .unwrap(),
                paper_scene::shader::Stage::Fragment,
                &BTreeMap::new(),
            );
            paper_scene::shader::unify_uniforms(&mut vertex, &mut fragment);
            let pass = paper_scene::effects::EffectPass {
                blend: paper_scene::effects::PassBlend::Copy,
                property_source: None,
                user_uniforms: std::collections::BTreeSet::new(),
                name: name.into(),
                vertex,
                geometry: None,
                fragment,
                hlsl: None,
                textures: Vec::new(),
                values: BTreeMap::new(),
                target: None,
                binds: Vec::new(),
            };
            layer.material = Some(paper_scene::effects::material::Material {
                state: paper_scene::effects::material::ImageState::default(),
                skin: None,
                prelit: None,
                pass,
                blend: paper_scene::effects::PassBlend::Copy,
                texture_copy: false,
            });
            model.layers.push(layer);
        }
        let texture = renderer.create_scene_texture_pixels(
            &model.layers[0].texture.pixels,
            true,
            false,
            vk::MipSampling::Disabled,
        )?;
        let mut textures = vec![texture];
        let mut interner = TextureInterner {
            device: Some(&shared),
            media_slots: HashMap::new(),
            videos: Vec::new(),
            video_slots: HashMap::new(),
            slots: HashMap::new(),
            reused: 0,
            reused_bytes: 0,
        };
        let mut slots = vec![0; model.layers.len()];
        let mut quads = quads(&model, &slots, model.canvas);
        let loaded = material::Build {
            mirrored: false,
            unreflected: BTreeSet::new(),
            renderer: &mut renderer,
            interner: &mut interner,
            textures: &mut textures,
            slots: &mut slots,
            quads: &mut quads,
            screen: ash::vk::Extent2D { width: 32, height: 32 },
            world: false,
        }
        .load(&mut model);
        assert!(loaded.err().unwrap().to_string().contains("missing base sampler"));
    }
    Ok(())
}

#[test]
fn scripted_pose_matrices() {
    let mut layer = test_layer(true, 0);
    layer.center = (320.0, 180.0);
    layer.size = (120.0, 60.0);
    let canvas = (1280.0, 720.0);
    let first = pose_uniforms(LayerPose::from(&layer), canvas, false);
    let pose = LayerPose {
        geometry: None,
        rect: [640.0, 360.0, 180.0, 90.0],
        scale: [1.5, 0.75],
        angle: 0.7,
        depth: 5.0,
    };
    let next = pose_uniforms(pose, canvas, false);
    for name in [
        FX_MODEL_MATRIX,
        FX_LAYER_MODEL,
        FX_NORMAL_MATRIX,
        "g_ModelMatrixInverse",
        "g_EffectModelMatrix",
        "g_EffectTextureProjectionMatrix",
    ] {
        assert_ne!(first[name], next[name]);
    }
    let matrix: Mat4 = next[FX_MODEL_MATRIX].as_slice().try_into().unwrap();
    let inverse: Mat4 = next["g_ModelMatrixInverse"].as_slice().try_into().unwrap();
    let identity = mat4_mul(&matrix, &inverse);
    for (index, value) in identity.into_iter().enumerate() {
        assert!((value - f32::from(index % 5 == 0)).abs() < 0.0001);
    }
}

#[test]
fn viewport_effect_geometry() {
    use paper_scene::model::LayerKind;
    for output in [(1280, 720), (720, 1280), (640, 360)] {
        let dimensions = camera_dimensions(&[output], 4096).unwrap();
        let canvas = (output.0 as f32, output.1 as f32);
        for kind in [
            LayerKind::Image,
            LayerKind::Viewport { hdr: false },
            LayerKind::Viewport { hdr: true },
        ] {
            let mut layer = test_layer(true, 1);
            layer.kind = kind;
            layer.center = (960.0, 540.0);
            layer.size = (1920.0, 1080.0);
            layer.copy_background = true;
            layer.fit_viewport(canvas, dimensions.raster);
            if kind == LayerKind::Image {
                assert_eq!(layer.center, (960.0, 540.0));
                assert_eq!(layer.size, (1920.0, 1080.0));
            } else {
                assert_eq!(layer.center, (canvas.0 * 0.5, canvas.1 * 0.5));
                assert_eq!(layer.size, canvas);
                assert_eq!(layer_effect_dimensions(&layer, dimensions), output);
            }
        }
    }
}

#[test]
fn affine_projection_roundtrips() {
    let canvas = (1280.0, 720.0);
    let geometry = paper_scene::model::Geometry {
        spatial: None,
        depth_scale: 6.0,
        transform: paper_scene::affine::Affine::new([320.0, 180.0], [-2.0, 1.5], 0.7)
            .then(paper_scene::affine::Affine::new([10.0, -20.0], [1.0; 2], 0.4)),
        size: [80.0, 60.0],
        offset: [0.0; 2],
    };
    let mut layer = test_layer(true, 0);
    layer.geometry = Some(geometry);
    layer.depth = 35.0;
    let quad = layer_quad(&layer, 0, canvas);
    let background = background_projection(&quad, canvas);
    let pose = LayerPose::from(&layer);
    let (model, inverse) = layer_model_matrix(pose, canvas);
    let identity = mat4_mul(&model, &inverse);
    for (index, value) in identity.into_iter().enumerate() {
        assert!((value - f32::from(index % 5 == 0)).abs() < 0.0001);
    }
    for point in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
        let screen = geometry.transform.point([
            point[0] * geometry.size[0],
            point[1] * geometry.size[1],
            0.0,
        ]);
        let input = [screen[0] / canvas.0 - 0.5, screen[1] / canvas.1 - 0.5];
        let mapped = background.map(|row| row[0] * input[0] + row[1] * input[1] + row[3]);
        for axis in 0..2 {
            assert!((mapped[axis] / mapped[2] - point[axis] * 2.0).abs() < 0.0001);
        }
    }
    let (projection, inverse) = effect_texture_projection(pose, canvas);
    for point in [[-1.0, -1.0], [1.0, 1.0]] {
        let clip = [
            projection[0] * point[0] + projection[4] * point[1] + projection[12],
            projection[1] * point[0] + projection[5] * point[1] + projection[13],
        ];
        let local = [
            inverse[0] * clip[0] + inverse[4] * clip[1] + inverse[12],
            inverse[1] * clip[0] + inverse[5] * clip[1] + inverse[13],
        ];
        for axis in 0..2 {
            assert!((local[axis] - point[axis]).abs() < 0.0001);
        }
    }
}

#[test]
fn subpixel_projection_inverts() {
    let canvas = (8192.0, 8192.0);
    let mut layer = test_layer(true, 0);
    layer.geometry = Some(paper_scene::model::Geometry {
        spatial: None,
        depth_scale: 1.0,
        transform: paper_scene::affine::Affine::new([4096.0; 2], [1.0; 2], 0.0),
        size: [1.0; 2],
        offset: [0.0; 2],
    });
    let (projection, inverse) = effect_texture_projection(LayerPose::from(&layer), canvas);
    assert!(inverse[0] > 0.0);
    assert!(inverse[5] > 0.0);
    for point in [[-1.0, -1.0], [1.0, 1.0]] {
        let clip = [projection[0] * point[0], projection[5] * point[1]];
        let local = [inverse[0] * clip[0], inverse[5] * clip[1]];
        assert!((local[0] - point[0]).abs() < 0.0001);
        assert!((local[1] - point[1]).abs() < 0.0001);
    }

    let mut quad = layer_quad(&layer, 0, canvas);
    quad.rect = [4096.0, 4096.0, 0.5, 0.5];
    quad.projection = None;
    let background = background_projection(&quad, canvas);
    assert!(background[0][0] > 0.0);
    assert!(background[1][1] > 0.0);
    let clip = [0.25 / canvas.0, 0.25 / canvas.1];
    assert!((background[0][0] * clip[0] - 1.0).abs() < 0.0001);
    assert!((background[1][1] * clip[1] - 1.0).abs() < 0.0001);
}

#[test]
fn projection_rejects_singular() {
    let scale = 1e-5;
    let projection = [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, 0.0, scale]];
    let inverse = inverse_projection(projection).unwrap();
    assert!((inverse[0][0] * scale - 1.0).abs() < 0.0001);
    assert!((inverse[1][1] * scale - 1.0).abs() < 0.0001);
    assert!((inverse[2][3] * scale - 1.0).abs() < 0.0001);
    assert!(inverse_projection([[0.0; 4]; 3]).is_none());
    let mut nonfinite = projection;
    nonfinite[0][0] = f32::NAN;
    assert!(inverse_projection(nonfinite).is_none());
}

#[test]
fn scene_input_full_uv() {
    let mut layer = test_layer(true, 0);
    layer.base_scene_input = true;
    assert_eq!(effect_uv_inset(&layer), [0.0; 2]);
    layer.base_scene_input = false;
    assert_eq!(effect_uv_inset(&layer), [paper_scene::model::IMAGE_UV_INSET; 2]);
}

#[test]
fn placeholder_full_uv() {
    let mut solid = test_layer(true, 0);
    solid.solid = true;
    let mut media = test_layer(true, 0);
    media.texture = paper_scene::model::MediaTexture::Current.texture();
    for layer in [solid, media] {
        assert_eq!(effect_uv_inset(&layer), [0.0; 2]);
    }
}

#[test]
fn text_interning_is_distinct() {
    let mut ordinary = texture_key(&[255, 255, 255, 128, 0, 0, 0, 0], true, false);
    ordinary.sampling = vk::MipSampling::Linear;
    let mut text = texture_key(&[255, 255, 255, 128, 0, 0, 0, 0], true, false);
    text.sampling = vk::MipSampling::Linear;
    text.text = true;
    let mut slots = HashMap::new();
    slots.insert(ordinary, 3);
    assert_eq!(slots.get(&text), None);
    slots.insert(text, 4);
    let mut repeated = texture_key(&[255, 255, 255, 128, 0, 0, 0, 0], true, false);
    repeated.sampling = vk::MipSampling::Linear;
    repeated.text = true;
    assert_eq!(slots.get(&repeated), Some(&4));
    repeated.sampling = vk::MipSampling::Disabled;
    assert_eq!(slots.get(&repeated), None);
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn clock_slots_are_private() {
    let root = std::env::var("SKWD_WE_CLOCK_LIBRARY").expect("Workshop library path");
    let dir = std::path::Path::new(&root).join("2138975215");
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    assert!(model.scripts.is_none());
    model.layers.retain(|layer| layer.id == "67");
    assert_eq!(model.layers.len(), 1);
    let pixels = model.layers[0].texture.pixels.clone();
    let text_batch = model.layers[0].texture.text_batch.clone();
    let dimensions = (model.layers[0].texture.width, model.layers[0].texture.height);
    for _ in 0..2 {
        let mut copy = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
        let index = copy.layers.iter().position(|layer| layer.id == "67").unwrap();
        model.layers.push(copy.layers.remove(index));
    }
    for (index, layer) in model.layers.iter_mut().enumerate() {
        assert!(layer.live_text.is_some());
        assert!(layer.script_text.is_none());
        assert!(layer.effects.is_empty());
        layer.id = format!("clock-{index}");
        layer.texture.pixels = pixels.clone();
        layer.texture.text_batch = text_batch.clone();
        layer.texture.width = dimensions.0;
        layer.texture.height = dimensions.1;
        layer.texture.img_width = dimensions.0;
        layer.texture.img_height = dimensions.1;
        layer.geometry = None;
        layer.center = (512.0, 256.0);
        layer.visible = true;
        layer.alpha = if index == 0 { 1.0 } else { 0.0 };
        layer.mouse = paper_scene::mouse::LayerMouse::default();
    }
    model.layers[0].live_text = None;
    model.particles.clear();
    model.skipped.clear();
    model.canvas = (1024.0, 512.0);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1024, 512)], FillMode::Fit).unwrap();
    assert_eq!(group.live_text.len(), 2);
    let static_slot = group.quads[0].texture;
    let first = group.live_text[0].slot;
    let second = group.live_text[1].slot;
    assert_ne!(first, second);
    assert_ne!(first, static_slot);
    assert_ne!(second, static_slot);
    let static_view = group.textures[static_slot].view;
    group.live_text_due = f32::INFINITY;
    group.compose(0.0, 1.0 / 60.0).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    group
        .refresh_live_text(paper_scene::dynamic_text::LocalTime {
            hour: 20,
            minute: 44,
            second: 59,
            day: 11,
            month: 9,
            year: 2026,
            weekday: 5,
            millisecond: 0,
        })
        .unwrap();
    group.compose(1.0, 1.0 / 60.0).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(group.textures[static_slot].view, static_view);
    assert_eq!(before, after);
    assert_eq!(group.live_text[0].shown, "20:44:59");
    assert_eq!(group.live_text[1].shown, "20:44:59");
    group.destroy();
}

#[test]
fn camera_clips_image() {
    let camera = Camera {
        eye: [1.0, 2.0, -5.0],
        center: [0.0; 3],
        up: [0.3, 1.0, 0.2],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    let pose = LayerPose {
        geometry: Some(paper_scene::model::Geometry {
            spatial: None,
            depth_scale: 1.0,
            transform: paper_scene::affine::Affine::new([100.0, 980.0], [1.0; 2], 0.0),
            size: [1.0; 2],
            offset: [0.0; 2],
        }),
        rect: [100.0, 980.0, 1.0, 1.0],
        scale: [1.0; 2],
        angle: 0.0,
        depth: 0.0,
    };
    let (view, ..) = authored_perspective(camera, (1280.0, 720.0), false);
    let [x, y, w] = camera_quad(pose, (1920.0, 1080.0), &view);
    for position in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
        let clip_w = w[0] * position[0] + w[1] * position[1] + w[3];
        let clip_z = x[2] * position[0] + y[2] * position[1] + w[2];
        assert!(clip_w < 0.0);
        assert!(clip_z > 0.0);
    }
}

#[test]
fn camera_depth_planes() {
    let camera = Camera {
        eye: [0.0, 0.0, -5.0],
        center: [0.0; 3],
        up: [0.0, 1.0, 0.0],
        fov: 60.0,
        near: 1.0,
        far: 10.0,
    };
    let canvas = (1920.0, 1080.0);
    let (view, ..) = authored_perspective(camera, (1280.0, 720.0), false);
    for (depth, visible) in [(-4.5, false), (0.0, true), (6.0, false)] {
        let pose = LayerPose {
            geometry: Some(paper_scene::model::Geometry {
                spatial: None,
                depth_scale: 1.0,
                transform: paper_scene::affine::Affine::new([0.0, canvas.1], [1.0; 2], 0.0),
                size: [1.0; 2],
                offset: [0.0; 2],
            }),
            rect: [0.0, canvas.1, 1.0, 1.0],
            scale: [1.0; 2],
            angle: 0.0,
            depth,
        };
        let [_, _, w] = camera_quad(pose, canvas, &view);
        assert_eq!(w[2] >= 0.0 && w[2] <= w[3], visible);
    }
}

#[test]
fn camera_inverse_roundtrip() {
    let camera = Camera {
        eye: [1.0, 2.0, -5.0],
        center: [0.0; 3],
        up: [0.3, 1.0, 0.2],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    let model =
        paper_scene::matrix::model_matrix([1.0, 0.5, 2.0], [1.5, 0.5, 1.0], [0.0, 0.0, 0.2]);
    for d3d_clip in [false, true] {
        let (view, ..) = authored_perspective(camera, (1280.0, 720.0), d3d_clip);
        let projection = mat4_mul(&view, &model);
        let inverse = paper_scene::matrix::mat4_inverse(&projection).unwrap();
        for result in [mat4_mul(&projection, &inverse), mat4_mul(&inverse, &projection)] {
            for (index, value) in result.into_iter().enumerate() {
                assert!((value - f32::from(index % 5 == 0)).abs() < 0.00001);
            }
        }
    }
}

#[test]
fn separates_glyph_placements() {
    use paper_scene::text::batch::{Batch, Quad};
    let mut first = texture_key(&[255; 8], true, false);
    first.text = true;
    first.text_batch = Some(Batch {
        canvas: [2, 1],
        pixels: paper_scene::tex::Pixels::rgba(1, 1, vec![255; 4]),
        quads: vec![Quad { bounds: [0, 0, 1, 1], uv: [0, 0, 1, 1] }],
    });
    let mut second = texture_key(&[255; 8], true, false);
    second.text = true;
    second.text_batch = first.text_batch.clone();
    second.text_batch.as_mut().unwrap().quads[0].bounds[0] = 1;
    let slots = HashMap::from([(first, 0), (second, 1)]);
    assert_eq!(slots.len(), 2);
}

#[test]
#[ignore = "requires Vulkan and pinned automatic projection fixture"]
fn automatic_output_presentation() -> Result<()> {
    const SIZE: (u32, u32) = (1280, 720);
    let directory = std::env::var("SKWD_WE_IMAGE_AUTO_PROJECT")?;
    let native = image::open(std::env::var("SKWD_WE_IMAGE_AUTO_NATIVE")?)?.to_rgb8();
    assert_eq!(native.dimensions(), SIZE);
    let package = paper_scene::pkg::Package::open(&locate_pkg(&directory)?)?;
    let mut model = paper_scene::model::load_from_dir_with_storage(
        &package,
        std::path::Path::new(&directory),
        &paper_scene::model::Properties::new(),
        None,
        Some([SIZE.0, SIZE.1]),
    )?;
    assert!(model.skipped.is_empty());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[SIZE], FillMode::Fill)?;
    group.compose(0.0, 0.0)?;
    assert_eq!(group.target.extent.width, group.target.extent.height);
    let mut presenter = build_presenter(&shared, SIZE.0, SIZE.1, 1, None)?;
    let bytes = u64::from(SIZE.0) * u64::from(SIZE.1) * RGBA_CHANNELS as u64;
    let readback = presenter.renderer.create_readback_buf(bytes)?;
    presenter.present(&group.target, 0)?;
    presenter.wait_render()?;
    presenter.read_export_to(0, &readback)?;
    presenter.wait_render()?;
    let mut pixels = unsafe { std::slice::from_raw_parts(readback.ptr, bytes as usize) }.to_vec();
    for pixel in pixels.chunks_exact_mut(RGBA_CHANNELS) {
        pixel.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(SIZE.0, SIZE.1, pixels).unwrap();
    image.save(std::env::var("SKWD_WE_IMAGE_AUTO_EVIDENCE")?)?;
    let actual = image::DynamicImage::ImageRgba8(image).to_rgb8();
    let mismatched =
        actual.pixels().zip(native.pixels()).filter(|(first, second)| first != second).count();
    assert_eq!(mismatched, 0);
    drop(readback);
    drop(presenter);
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_BACKDROP_PROJECT"]
fn text_effect_follows_backdrop() -> Result<()> {
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_TEXT_BACKDROP_PROJECT")?);
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty());
    let mut scene = package.scene_json()?;
    let panel = scene["objects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|object| object["name"] == "backdrop-panel")
        .unwrap();
    panel["color"] = serde_json::json!({"value":"0.75 0.125 0.25","script":"export function update(){return engine.runtime<1?new Vec3(0.75,0.125,0.25):new Vec3(0.125,0.25,0.5);}"});
    model.scripts = paper_scene::script::SceneScripts::load(
        &mut scene,
        &paper_scene::model::Properties::new(),
        &serde_json::json!({}),
    )?;
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
    assert_eq!(group.fx.len(), 9);
    assert!(group.fx.iter().all(|fx| fx.text_backdrop.is_some() && fx.snapshot));
    for (time, expected) in [(0.0, [199, 59, 87]), (1.5, [59, 87, 143]), (0.0, [199, 59, 87])] {
        group.compose(time, 0.0)?;
        let (width, _, pixels) = group.renderer.read_scene_target(&group.target)?;
        let start = (348 * width as usize + 227) * RGBA_CHANNELS;
        for (actual, expected) in pixels[start..start + 3].iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 1, "backdrop pixel");
        }
    }
    group.destroy();
    Ok(())
}

fn check_text_phase(variable: &str, widths: [usize; 3]) -> Result<()> {
    let dir = std::path::PathBuf::from(std::env::var(variable)?);
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty());
    let extents: Vec<_> = model
        .layers
        .iter()
        .filter(|layer| layer.is_text())
        .map(|layer| (layer.texture.width, layer.texture.height))
        .collect();
    assert_eq!(extents, widths.map(|width| (width as u32, 211)).repeat(3));
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
    group.compose(0.0, 0.0)?;
    let (width, _, pixels) = group.renderer.read_scene_target(&group.target)?;
    for top in [51, 281, 511] {
        for (center, extent) in [210, 630, 1050].into_iter().zip(widths) {
            let left = center as f32 - extent as f32 * 0.5;
            for y in top..(top + 211).min(720) {
                for x in left.ceil() as usize..(left + extent as f32).floor() as usize {
                    let start = (y * width as usize + x) * RGBA_CHANNELS;
                    for (channel, (position, origin, size)) in
                        [(x, left, extent), (y, top as f32, 211)].into_iter().enumerate()
                    {
                        let inset = paper_scene::model::IMAGE_UV_INSET;
                        let q = inset
                            + (position as f32 + 0.5 - origin) * (size as f32 - 2.0 * inset)
                                / size as f32;
                        let expected = (q.fract() * 255.0).round() as u8;
                        assert!(pixels[start + channel].abs_diff(expected) <= 1);
                    }
                }
            }
        }
    }
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_PHASE_PROJECT"]
fn text_effect_input_phase() -> Result<()> {
    check_text_phase("SKWD_WE_TEXT_PHASE_PROJECT", [52, 100, 74])
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_LONG_PHASE_PROJECT"]
fn text_odd_width_phase() -> Result<()> {
    check_text_phase("SKWD_WE_TEXT_LONG_PHASE_PROJECT", [109, 211, 158])
}

fn check_text_lifecycle(case: &str, names: &[&str], phases: &[(f32, usize)]) -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_TEXT_LIFECYCLE_ROOT")?);
    let shared = crate::shared::test_device();
    let mut references = Vec::new();
    let mut extents = Vec::new();
    for name in names {
        let dir = root.join(name);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty());
        extents.push(
            model
                .layers
                .iter()
                .filter(|layer| layer.is_text())
                .map(|layer| (layer.texture.width, layer.texture.height))
                .collect::<Vec<_>>(),
        );
        let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
        group.compose(0.0, 0.0)?;
        references.push(group.renderer.read_scene_target(&group.target)?.2);
        group.destroy();
    }
    assert_ne!(references[0], references[1]);
    let dir = root.join(case);
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty());
    assert!(model.scripts.is_some());
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
    assert_eq!(group.fx.len(), 9);
    let mut previous_targets = Vec::new();
    for &(time, reference) in phases {
        if case.ends_with("length-cycle") && time == 2.0 {
            let before: Vec<_> =
                group.fx.iter().map(|fx| fx.ping.get(&group.fx_scratch).view).collect();
            let old_bytes = group.effect_target_allocation_bytes();
            let fx = &group.fx[0];
            let size = extents[reference][0];
            let required = fx.target_ids().try_fold(0_u64, |bytes, id| {
                let extent = match id {
                    FxTargetId::Fbo(index) => fx.fbo_specs[index].extent(size),
                    FxTargetId::Ping | FxTargetId::Pong => size,
                };
                Ok::<_, anyhow::Error>(
                    bytes
                        + group.renderer.scene_target_allocation_size(
                            extent.0,
                            extent.1,
                            fx.target(id, &group.fx_scratch).format,
                        )?,
                )
            })?;
            let delta = required.checked_sub(fx.allocation_bytes()).expect("first layer grows");
            assert!(delta > 0);
            let allowance = std::mem::replace(&mut group.history_budget, delta);
            group.compose(time, 0.0)?;
            let committed = group.effect_target_allocation_bytes() - old_bytes;
            assert_eq!(committed, delta);
            assert_eq!(group.history_budget, 0);
            group.history_budget = allowance - committed;
            let after: Vec<_> =
                group.fx.iter().map(|fx| fx.ping.get(&group.fx_scratch).view).collect();
            assert_ne!(before[0], after[0]);
            assert_eq!(before[1..], after[1..]);
        }
        group.compose(time, 0.0)?;
        let pixels = group.renderer.read_scene_target(&group.target)?.2;
        let changed = pixels
            .chunks_exact(RGBA_CHANNELS)
            .zip(references[reference].chunks_exact(RGBA_CHANNELS))
            .filter(|(actual, expected)| actual[..3] != expected[..3])
            .count();
        assert_eq!(changed, 0, "text lifecycle {case} at {time}");
        let targets: Vec<_> =
            group.fx.iter().map(|fx| fx.ping.get(&group.fx_scratch).view).collect();
        if case.ends_with("length-cycle") && time == 1.0 {
            assert_eq!(previous_targets, targets);
        }
        previous_targets.clone_from(&targets);
        for fx in &group.fx {
            let base = fx.ping.get(&group.fx_scratch).extent;
            for (spec, (_, storage)) in fx.fbo_specs.iter().zip(&fx.fbos) {
                let target = storage.get(&group.fx_scratch);
                assert_eq!(
                    (target.extent.width, target.extent.height),
                    spec.extent((base.width, base.height))
                );
            }
        }
        let bytes = group.effect_target_allocation_bytes();
        group.compose(time + 0.25, 0.0)?;
        let repeated: Vec<_> =
            group.fx.iter().map(|fx| fx.ping.get(&group.fx_scratch).view).collect();
        assert_eq!(targets, repeated);
        assert_eq!(bytes, group.effect_target_allocation_bytes());
    }
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_LIFECYCLE_ROOT"]
fn text_toggle_matches_static() -> Result<()> {
    check_text_lifecycle("toggle", &["direct", "effect"], &[(0.0, 0), (1.0, 1), (2.0, 0)])
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_LIFECYCLE_ROOT"]
fn text_resize_matches_static() -> Result<()> {
    check_text_lifecycle(
        "length-cycle",
        &["effect", "same", "long", "empty"],
        &[(0.0, 0), (1.0, 1), (2.0, 2), (3.0, 3), (4.0, 0)],
    )
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_LIFECYCLE_ROOT"]
fn text_named_targets_resize() -> Result<()> {
    check_text_lifecycle(
        "named-length-cycle",
        &["named-effect", "named-same", "named-long", "named-empty"],
        &[(0.0, 0), (1.0, 1), (2.0, 2), (3.0, 3), (4.0, 0)],
    )
}
#[test]
#[ignore = "requires Vulkan and SKWD_WE_TEXT_LIFECYCLE_ROOT"]
fn text_resize_checks_budget() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_TEXT_LIFECYCLE_ROOT")?);
    let dir = root.join("length-cycle");
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
    group.compose(0.0, 0.0)?;
    let layer = group.fx[0].quad;
    let old = group.fx[0].ping.get(&group.fx_scratch);
    let extent = (old.extent.width, old.extent.height);
    assert_eq!(
        group.renderer.scene_target_allocation_size(extent.0, extent.1, old.format)?,
        old.allocation_bytes
    );
    let history = group.scene_history.as_ref().map_or(0, vk::SceneHistory::allocation_bytes);
    let resident = group.effect_target_allocation_bytes()
        + group.scene_snapshot.as_ref().map_or(0, |target| target.allocation_bytes)
        + group.scene_grab.as_ref().map_or(0, |target| target.allocation_bytes)
        + history;
    group.effect_budget = resident + resident / 4;
    group.history_budget = resident / 4 + history;
    assert!(resident > group.effect_budget / 2);
    let allowance = group.history_budget;
    let before = group.effect_target_allocation_bytes();
    group.resize_text_effect(layer, (extent.0 / 2, extent.1))?;
    let after = group.effect_target_allocation_bytes();
    assert!(after < before);
    assert_eq!(group.history_budget, allowance + before - after);
    let view = group.fx[0].ping.get(&group.fx_scratch).view;
    let allocations = group.renderer.scene_image_allocations();
    assert!(group.resize_text_effect(layer, (8192, 8192)).is_err());
    assert_eq!(group.renderer.scene_image_allocations(), allocations);
    assert_eq!(group.fx[0].ping.get(&group.fx_scratch).view, view);
    group.resize_text_effect(layer, extent)?;
    assert_eq!(group.history_budget, allowance);
    let view = group.fx[0].ping.get(&group.fx_scratch).view;
    for effect in &mut group.fx[0].effects {
        effect.visible = false;
    }
    group.history_budget = 0;
    group.effect_budget = 0;
    let allocations = group.renderer.scene_image_allocations();
    group.resize_text_effect(layer, (8192, 8192))?;
    assert_eq!(group.renderer.scene_image_allocations(), allocations);
    assert_eq!(group.fx[0].ping.get(&group.fx_scratch).view, view);
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_HIDDEN_TEXT_ROOT"]
fn hidden_text_defers_resize() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_HIDDEN_TEXT_ROOT")?);
    let shared = crate::shared::test_device();
    let rgb = |bytes: Vec<u8>| {
        bytes
            .chunks_exact(RGBA_CHANNELS)
            .flat_map(|pixel| pixel[..3].iter().copied())
            .collect::<Vec<_>>()
    };
    let mut references = Vec::new();
    for name in ["direct", "direct-long", "active-long"] {
        let dir = root.join(name);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty());
        let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
        group.compose(0.0, 0.0)?;
        references.push(rgb(group.renderer.read_scene_target(&group.target)?.2));
        group.destroy();
    }
    assert_ne!(references[0], references[1]);
    assert_ne!(references[1], references[2]);
    let dir = root.join("script");
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty());
    assert!(model.scripts.is_some());
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit)?;
    assert_eq!(group.fx.len(), 9);
    group.compose(0.0, 0.0)?;
    assert_eq!(rgb(group.renderer.read_scene_target(&group.target)?.2), references[0]);
    let views = |group: &Group| {
        group
            .fx
            .iter()
            .flat_map(|fx| fx.target_ids().map(move |id| fx.target(id, &group.fx_scratch).view))
            .collect::<Vec<_>>()
    };
    let targets = views(&group);
    let bytes = group.effect_target_allocation_bytes();
    let allowance = std::mem::replace(&mut group.history_budget, 0);
    group.compose(1.0, 0.0)?;
    assert_eq!(rgb(group.renderer.read_scene_target(&group.target)?.2), references[1]);
    assert_eq!(views(&group), targets);
    assert_eq!(group.effect_target_allocation_bytes(), bytes);
    assert!(group.fx.iter().all(|fx| fx.effects.iter().all(|effect| !effect.visible)));
    let frame = group.renderer.read_scene_target(&group.target)?.2;
    assert!(group.compose(2.0, 0.0).is_err());
    assert_eq!(group.renderer.read_scene_target(&group.target)?.2, frame);
    assert_eq!(views(&group), targets);
    assert_eq!(group.effect_target_allocation_bytes(), bytes);
    group.history_budget = allowance;
    group.compose(2.0, 0.0)?;
    assert_eq!(rgb(group.renderer.read_scene_target(&group.target)?.2), references[2]);
    assert_ne!(views(&group), targets);
    assert!(group.effect_target_allocation_bytes() > bytes);
    group.destroy();
    Ok(())
}

#[test]
fn orthographic_camera_motion() {
    let canvas = (1280.0, 720.0);
    for (eye, center, up, expected) in [
        ([32.0, 0.0, 0.0], [32.0, 0.0, -1.0], [0.0, 1.0, 0.0], [608.0, 360.0]),
        ([0.0; 3], [0.0, 0.0, -1.0], [0.5, 0.866_025_4, 0.0], [374.256_26, 631.769_17]),
    ] {
        let basis = paper_scene::model::CameraBasis::of(eye, center, up).unwrap();
        let matrix = mat4_mul(&scene_ortho(canvas, false), &camera_view(eye, basis));
        let clip = transform(&matrix, [640.0, 360.0, 0.0, 1.0]);
        let world = [(clip[0] + 1.0) * canvas.0 * 0.5, (1.0 - clip[1]) * canvas.1 * 0.5];
        for (actual, expected) in world.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.001);
        }
    }
}

#[test]
fn clear_target_scope() {
    use paper_scene::effects::{Fbo, FboFormat, FboSize};
    let names = ["_rt_A", "_rt_B", "_rt_A", "_rt_B", "_rt_Unrelated"];
    let owners = [Some(0), Some(0), Some(1), Some(1), None];
    let remap = [
        std::cell::Cell::new(1),
        std::cell::Cell::new(0),
        std::cell::Cell::new(3),
        std::cell::Cell::new(2),
        std::cell::Cell::new(4),
    ];
    let specs: Vec<_> = (0..names.len())
        .map(|index| Fbo {
            name: names[index].into(),
            size: FboSize::Fit(64),
            format: FboFormat::Rgba8,
            repeat: None,
            clear: [index as f32; 4],
            clear_on_setup: true,
            unique: owners[index].is_some(),
        })
        .collect();
    let resolve = |owner, targets: &[&str]| {
        clear_function_targets(
            names.iter().copied(),
            &owners,
            &remap,
            &specs,
            owner,
            &targets.iter().map(|name| (*name).to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(resolve(0, &["_rt_A", "_rt_B"]), Some(vec![(1, [0.0; 4]), (0, [1.0; 4])]));
    assert_eq!(resolve(1, &["_rt_A"]), Some(vec![(3, [2.0; 4])]));
    assert_eq!(resolve(0, &["_rt_A", "missing"]), None);
    assert_eq!(resolve(0, &["_rt_Unrelated"]), Some(vec![(4, [4.0; 4])]));
}

#[test]
fn pending_clear_upsert() {
    let mut pending = Vec::new();
    merge_pending_clears(&mut pending, [(0, [1.0; 4]), (1, [2.0; 4])]);
    for color in 3..20 {
        merge_pending_clears(&mut pending, [(0, [color as f32; 4]), (1, [color as f32; 4])]);
        assert_eq!(pending.len(), 2);
    }
    assert_eq!(pending, [(0, [19.0; 4]), (1, [19.0; 4])]);
    let capacity = pending.capacity();
    pending.clear();
    assert_eq!(pending.capacity(), capacity);
}

#[test]
#[ignore = "requires Vulkan and native remap noise fixtures"]
fn remap_noise_pixels() -> Result<()> {
    let path = std::env::var("SKWD_REMAP_PROBE")?;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let reference = image::open(std::env::var("SKWD_REMAP_REFERENCE")?)?.to_rgba8();
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.particles.len(), 6);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    let encoded = |pixel: &image::Rgba<u8>| {
        u32::from(pixel[0]) * 65_536 + u32::from(pixel[1]) * 256 + u32::from(pixel[2])
    };
    for (index, particle) in group.particles.iter_mut().enumerate() {
        assert_eq!(particle.sim.particles.len(), 1);
        let seed =
            encoded(reference.get_pixel(477, 990 - index as u32 * 180)) as f32 / 16_777_216.0;
        particle.sim.particles[0].noise_seed = seed.to_bits();
    }
    group.step_particles(particle_clock(0.01))?;
    for particle in &mut group.particles {
        particle.system.rate_scale = 0.0;
    }
    group.compose(0.01, 0.0)?;
    let (width, height, pixels) = group.read_canvas()?;
    if let Some(output) = std::env::var_os("SKWD_REMAP_OUTPUT") {
        image::save_buffer(output, &pixels, width, height, image::ColorType::Rgba8)?;
    }
    let actual = image::RgbaImage::from_raw(width, height, pixels).unwrap();
    assert_eq!(reference.dimensions(), actual.dimensions());
    for (x, y, expected) in reference.enumerate_pixels() {
        let in_particle = (0..6).any(|index| (60 + index * 180..120 + index * 180).contains(&y));
        if in_particle && (470..485).contains(&x) {
            continue;
        }
        let pixel = actual.get_pixel(x, y);
        if in_particle && (485..530).contains(&x) {
            assert!(
                encoded(pixel).abs_diff(encoded(expected)) <= 1,
                "{x},{y}: {pixel:?} vs {expected:?}"
            );
        } else {
            assert_eq!(&pixel.0[..3], &expected.0[..3], "{x},{y}");
        }
    }
    group.destroy();
    Ok(())
}

#[test]
fn spatial_material_uniforms() {
    let matrix =
        [0.0, 0.0, -2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 10.0, 20.0, 30.0, 1.0];
    let pose = LayerPose {
        geometry: Some(paper_scene::model::Geometry {
            spatial: Some(matrix),
            depth_scale: 4.0,
            transform: paper_scene::affine::Affine::new([10.0, 20.0], [2.0, 3.0], 0.0),
            size: [100.0; 2],
            offset: [0.0; 2],
        }),
        rect: [10.0, 20.0, 100.0, 100.0],
        scale: [2.0, 3.0],
        angle: 0.0,
        depth: 30.0,
    };
    let values = spatial_uniforms(pose, (1920.0, 1080.0)).unwrap();
    assert_eq!(values[FX_MODEL_MATRIX], matrix);
    assert_eq!(
        values[FX_NORMAL_MATRIX],
        [0.0, 0.0, -0.5, 0.0, 0.0, 1.0 / 3.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0]
    );
    assert_ne!(pose_uniforms(pose, (1920.0, 1080.0), false)[FX_MODEL_MATRIX], matrix);
    assert_eq!(pose_uniforms(pose, (1920.0, 1080.0), true)[FX_MODEL_MATRIX], matrix);
    let view = mat4_mul(&scene_ortho((1920.0, 1080.0), false), &matrix);
    let mut sized = view;
    for (column, scale) in sized.chunks_exact_mut(MATRIX_COLUMN_COMPONENTS).zip([100.0, -100.0]) {
        for value in column {
            *value *= scale;
        }
    }
    assert_eq!(
        camera_quad(pose, (1920.0, 1080.0), &scene_ortho((1920.0, 1080.0), false)),
        vk::LayerProjection::from(sized).pack()
    );
    let mut planar = pose;
    planar.geometry.as_mut().unwrap().spatial = None;
    assert!(spatial_uniforms(planar, (1920.0, 1080.0)).is_none());
    let mut singular = pose;
    singular.geometry.as_mut().unwrap().spatial.as_mut().unwrap()[8] = 0.0;
    let values = spatial_uniforms(singular, (1920.0, 1080.0)).unwrap();
    assert_eq!(values[FX_MODEL_INVERSE], [0.0; 16]);
    assert_eq!(values[FX_MODEL_MATRIX][2], -2.0);
    let mut invalid = pose;
    invalid.geometry.as_mut().unwrap().spatial.as_mut().unwrap()[0] = f32::NAN;
    assert!(spatial_uniforms(invalid, (1920.0, 1080.0)).is_none());
}

#[test]
#[ignore = "requires authored puppet composition packages and Vulkan"]
fn scripted_spatial_scope() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_COMPOSITION").unwrap());
    let shared = crate::shared::test_device();
    for (variant, spatial) in [("tint", true), ("effect", false)] {
        let dir = root.join(variant);
        let original = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut scene = original.scene_json()?;
        scene["objects"][0]["angles"] = serde_json::json!({"value":"0 0 0","script":"export function update(value){return engine.runtime>=1?new Vec3(30,0,0):value;}"});
        let bytes = serde_json::to_vec(&scene)?;
        let files: Vec<_> = original
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.path.as_str(),
                    if entry.path == "scene.json" {
                        bytes.as_slice()
                    } else {
                        original.read(entry)
                    },
                )
            })
            .collect();
        let package = sound_package(&files);
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        assert_eq!(group.fx.is_empty(), spatial);
        group.compose(0.0, 0.0)?;
        let initial = group.read_canvas()?.2;
        assert_eq!(group.materials[0].uniforms[FX_MODEL_MATRIX][6], 0.0);
        group.compose(1.0, 0.1)?;
        assert_eq!(group.materials[0].uniforms[FX_MODEL_MATRIX][6] != 0.0, spatial);
        assert_eq!(group.read_canvas()?.2 != initial, spatial);
        assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
        group.destroy();
    }
    Ok(())
}

#[test]
fn release_counts_last_holder() {
    let mut first = paper_scene::model::solid_texture();
    let mut second = first.clone();
    assert_eq!(release_texture_payload(&mut first), 0);
    assert_eq!(release_texture_payload(&mut second), 4);
}

#[test]
fn uncopyable_provider_gate() {
    let mut model = mesh_sampler_model(
        br#"{"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":2,"image":"models/sample.json","origin":"32 32 0","size":"8 8"}]}"#,
        br#"{"passes":[{"shader":"sample","textures":["textures/white"]}]}"#,
        b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5));}",
    );
    let referenced = BTreeSet::from(["2".to_string()]);
    assert_eq!(
        uncopyable_provider(&model.layers, &referenced).map(|layer| layer.id.as_str()),
        Some("2")
    );
    assert!(uncopyable_provider(&model.layers, &BTreeSet::new()).is_none());
    model.layers[0].material.as_mut().unwrap().texture_copy = true;
    assert!(uncopyable_provider(&model.layers, &referenced).is_none());
}

#[test]
fn shared_snapshot_preserves_front() {
    let released = [0, 1, 2];
    let free = || [released.as_slice(), released.as_slice()].into_iter();
    let mut displayed = [Some(0), Some(1)];
    assert_eq!(shared_scene_slot(free(), 3, &displayed), Some(2));
    displayed[1] = Some(2);
    assert_eq!(shared_scene_slot(free(), 3, &displayed), Some(1));
    displayed[0] = None;
    assert_eq!(shared_scene_slot(free(), 3, &displayed), Some(0));
}

#[test]
#[ignore = "requires Vulkan"]
fn mesh_samples_dependency() {
    let green = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*vec4(0.0,1.0,0.0,1.0);}";
    let script = serde_json::json!({"script":"'use strict';\nexport function update(value) {\n\treturn value;\n}\n","value":1.0});
    for (color, effects, alpha) in [
        ("0 1 0", serde_json::json!([]), serde_json::json!(1.0)),
        ("1 1 1", serde_json::json!([{"file":"effects/green.json"}]), serde_json::json!(1.0)),
        ("1 1 1", serde_json::json!([{"file":"effects/green.json"}]), script),
    ] {
        let scene = serde_json::json!({
            "camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},
            "general":{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},
            "objects":[
                {"id":2,"image":"models/flat.json","size":"8 8","color":color,"alpha":alpha,"visible":false,"effects":effects},
                {"id":1,"model":"models/mesh.mdl","origin":"0.5 0.5 0","dependencies":[2]}
            ]
        })
        .to_string();
        let mesh = triangle_mdl();
        let package = sound_package(&[
            ("scene.json", scene.as_bytes()),
            ("models/mesh.mdl", &mesh),
            ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
            ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
            ("effects/green.json", br#"{"passes":[{"material":"materials/green.json"}]}"#),
            ("materials/green.json", br#"{"passes":[{"shader":"green"}]}"#),
            ("shaders/green.vert", super::effect_space_tests::PASS_VERTEX),
            ("shaders/green.frag", green),
            ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","textures":["_rt_imageLayerComposite_2_a"],"cullmode":"nocull","blending":"normal"}]}"#),
            ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/mesh.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
        ]);
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some("/nonexistent")),
        )
        .unwrap();
        assert!(model.skipped.is_empty(), "{effects}: {:?}", model.skipped);
        let shared = crate::shared::test_device();
        let mut group =
            build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
        for time in [0.0, 0.1] {
            group.compose(time, 0.1).unwrap();
        }
        group.bake_static_effects().unwrap();
        group.compose(0.2, 0.1).unwrap();
        let (width, height, rgba) = group.read_canvas().unwrap();
        let centre = ((height / 2 * width + width / 2) as usize) * RGBA_CHANNELS;
        assert_eq!(rgba[centre..centre + 3], [0, 255, 0], "{effects}");
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn wide_indices_render() {
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"0.5 0.5 0"}]}"#;
    let narrow = triangle_mdl();
    let flag = "MDLV0004\0".len() + 3 * size_of::<u32>() + "materials/mesh.json\0".len();
    let tail = narrow.len() - 3 * size_of::<u16>() - size_of::<u32>();
    let mut wide = narrow[..tail].to_vec();
    wide[flag..flag + size_of::<u32>()].copy_from_slice(&1_u32.to_le_bytes());
    wide.extend(12_u32.to_le_bytes());
    for index in [0_u32, 1, 2] {
        wide.extend(index.to_le_bytes());
    }
    let shared = crate::shared::test_device();
    let mut frames = Vec::new();
    for mesh in [narrow, wide] {
        let package = sound_package(&[
            ("scene.json", scene),
            ("models/mesh.mdl", &mesh),
            ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","cullmode":"nocull","blending":"normal"}]}"#),
            ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/mesh.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ]);
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some("/nonexistent")),
        )
        .unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group =
            build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
        group.compose(0.0, 0.1).unwrap();
        frames.push(group.read_canvas().unwrap());
        group.destroy();
    }
    let (width, height, rgba) = &frames[0];
    let centre = ((height / 2 * width + width / 2) as usize) * RGBA_CHANNELS;
    assert_eq!(rgba[centre..centre + 3], [255, 0, 0]);
    assert!(frames[0] == frames[1], "wide indices differ");
}

#[test]
#[ignore = "requires Vulkan"]
fn mounted_mesh_follows() {
    let shared = crate::shared::test_device();
    let triangle = triangle_mdl();
    let render = |hat: &str, time: f32| {
        let scene = format!(
            r#"{{"camera":{{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"}},"general":{{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"}},"objects":[{{"id":10,"model":"models/rig.mdl","origin":"-1 -1 0","animationlayers":[{{"id":5,"animation":1,"rate":1.0,"blend":1.0,"visible":true}}]}},{{"id":1,"model":"models/mesh.mdl",{hat}}}]}}"#
        );
        let package = sound_package(&[
            ("scene.json", scene.as_bytes()),
            ("models/rig.mdl", include_bytes!("../../../../paper-scene/src/fixtures/mounted.mdl")),
            ("models/mesh.mdl", &triangle),
            ("materials/a.json", br#"{"passes":[{"shader":"skin","cullmode":"nocull"}]}"#),
            ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","cullmode":"nocull","blending":"normal"}]}"#),
            ("shaders/skin.vert", b"uniform mat4x3 g_Bones[BONECOUNT];\nuniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nvoid main(){gl_Position=mul(vec4(mul(vec4(a_Position,1.0),g_Bones[a_BlendIndices.x])*a_BlendWeights.x,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/skin.frag", b"void main(){gl_FragColor=vec4(0.0,0.0,1.0,1.0);}"),
            ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/mesh.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ]);
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some("/nonexistent")),
        )
        .unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group =
            build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
        let animated = group.meshes.iter().filter(|actor| actor.animated()).count();
        group.compose(0.0, 0.0).unwrap();
        group.compose(time, time).unwrap();
        let (_, _, rgba) = group.read_canvas().unwrap();
        group.destroy();
        (rgba, animated)
    };
    let mounted = r#""parent":10,"attachment":"Grip""#;
    let rest = render(mounted, 0.0);
    let moved = render(mounted, 1.0 / 30.0);
    assert_eq!(rest.1, 2);
    assert_ne!(rest.0, moved.0);
    assert!(rest.0 == render(r#""origin":"-1 -1 0""#, 0.0).0, "rest mount");
    assert!(moved.0 == render(r#""origin":"0 -1 0""#, 1.0 / 30.0).0, "moved mount");
}

#[test]
#[ignore = "requires Vulkan"]
fn mesh_frame_buffer_empty() {
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"1 1 1"},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"0.5 0.5 0"}]}"#;
    let mesh = triangle_mdl();
    let package = sound_package(&[
        ("scene.json", scene),
        ("models/mesh.mdl", &mesh),
        ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","textures":[null,null,"_rt_FullFrameBuffer"],"cullmode":"nocull","blending":"normal"}]}"#),
        ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/mesh.frag", b"uniform sampler2D g_Texture2;\nvoid main(){gl_FragColor=vec4(texSample2D(g_Texture2,vec2(0.5,0.5)).rgb,1.0);}"),
    ]);
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )
    .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
    group.compose(0.0, 0.1).unwrap();
    let (width, height, rgba) = group.read_canvas().unwrap();
    let centre = ((height / 2 * width + width / 2) as usize) * RGBA_CHANNELS;
    assert_eq!(rgba[centre..centre + 3], [0, 0, 0]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn mesh_samples_history() {
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"0.5 0.5 0"}]}"#;
    let mesh = triangle_mdl();
    let white = effect_lifetime_tests::texture_levels(&[paper_scene::tex::MipLevel {
        width: 1,
        height: 1,
        data: vec![255; 4],
    }]);
    let package = sound_package(&[
        ("scene.json", scene),
        ("models/mesh.mdl", &mesh),
        ("materials/textures/white.tex", &white),
        ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","textures":["textures/white","_rt_MipMappedFrameBuffer"],"cullmode":"nocull","blending":"normal"}]}"#),
        ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/mesh.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=vec4(texSample2D(g_Texture0,vec2(0.5,0.5)).r,texSample2DLod(g_Texture1,vec2(0.5,0.5),0.0).r,0.0,1.0);}"),
    ]);
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )
    .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
    assert!(group.target.samples == group.renderer.scene_sample_count());
    assert!(group.target.samples != ash::vk::SampleCountFlags::TYPE_1, "mesh scene msaa");
    for time in [0.0, 0.1] {
        group.compose(time, 0.1).unwrap();
    }
    let (width, height, rgba) = group.read_canvas().unwrap();
    let centre = ((height / 2 * width + width / 2) as usize) * RGBA_CHANNELS;
    assert_eq!(rgba[centre..centre + 2], [255, 255]);
    group.destroy();
    assert_eq!(vk::Renderer::scene_history_live_allocations(), 0);
}

fn container_groups(mut root: serde_json::Value, child: &[u8]) -> Vec<ParticleGroup> {
    let scene = br#"{"objects":[{"id":1,"name":"Sparks","origin":"100 50 0","particle":"particles/root.json"}]}"#;
    root["children"] = serde_json::json!([{"name":"particles/child.json","origin":"10 0 0"}]);
    let root = root.to_string();
    let package = sound_package(&[
        ("scene.json", scene.as_slice()),
        ("particles/root.json", root.as_bytes()),
        ("particles/child.json", child),
    ]);
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some("/nonexistent")),
    )
    .unwrap();
    let layer = model.particles.remove(0);
    let mut parent = layer.system;
    let child = parent.children.remove(0);
    let binding =
        ParticleParent { index: 0, origin: child.origin, angle: child.angle, scale: child.scale3 };
    [(parent, None), (child, Some(binding))]
        .into_iter()
        .map(|(system, parent)| ParticleGroup {
            parallax: layer.parallax,
            camera_offset: [0.0; 2],
            id: layer.id.clone(),
            visible: true,
            system,
            sim: paper_scene::particles::Sim::new(1),
            texture: None,
            ratio: 1.0,
            frames: Vec::new(),
            scene_order: layer.scene_order,
            engines: Vec::new(),
            screen_basis: None,
            parent,
            followers: paper_scene::particles::Followers::default(),
            reset: false,
            prewarm_steps: 0,
            prewarm_dt: 0.0,
            dynamic_slots: None,
            media_kinds: Vec::new(),
            dynamic_videos: Vec::new(),
        })
        .collect()
}

const STREAM: &[u8] = br#"{"emitter":[{"name":"boxrandom","rate":100}]}"#;

fn burst(lifetime: f32) -> Vec<ParticleGroup> {
    let root = serde_json::json!({
        "emitter":[{"name":"boxrandom","rate":0,"instantaneous":5}],
        "initializer":[{"name":"lifetimerandom","min":lifetime,"max":lifetime}],
    });
    container_groups(root, STREAM)
}

fn oldest(group: &ParticleGroup) -> f32 {
    group.sim.particles.iter().map(|particle| particle.age).fold(0.0, f32::max)
}

fn youngest(group: &ParticleGroup) -> f32 {
    group.sim.particles.iter().map(|particle| particle.age).fold(f32::MAX, f32::min)
}

fn step_containers(groups: &mut [ParticleGroup], dt: f32) {
    advance_particles(
        groups,
        &ParticleStep {
            dt,
            frame: 0,
            clock: particle_clock(dt),
            mouse: None,
            audio: (&[], &[]),
            prewarm_step: None,
        },
        &mut |_| {},
    );
}

#[test]
fn container_moves_children() {
    let mut groups = container_groups(serde_json::json!({"emitter":[]}), STREAM);
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[1].system.origin, (110.0, 50.0, 0.0));
    assert!(!groups[1].sim.particles.is_empty());
    assert!(groups[0].sim.particles.is_empty());
    groups[0].system.origin.0 = 200.0;
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[1].system.origin.0, 210.0);
}

#[test]
fn hidden_container_keeps_children() {
    let mut groups = container_groups(serde_json::json!({"emitter":[]}), STREAM);
    step_containers(&mut groups, 0.1);
    let layer_time = groups[1].sim.layer_time();
    let before = oldest(&groups[1]);
    groups[0].visible = false;
    step_containers(&mut groups, 0.1);
    assert!(!groups[1].visible);
    assert_eq!(groups[1].sim.layer_time(), layer_time);
    assert!(oldest(&groups[1]) > before);
    groups[0].visible = true;
    step_containers(&mut groups, 0.1);
    assert!(groups[1].sim.layer_time() > layer_time);
}

const WORLD_STREAM: &[u8] = br#"{"flags":1,"emitter":[{"name":"boxrandom","rate":100}]}"#;

#[test]
fn world_child_detaches() {
    let mut groups = container_groups(serde_json::json!({"emitter":[]}), WORLD_STREAM);
    groups[0].system.world_matrix =
        Some(paper_scene::matrix::model_matrix([100.0, 50.0, 0.0], [1.0; 3], [0.5, 0.0, 0.0]));
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[1].system.origin, (10.0, 0.0, 0.0));
    assert!(groups[1].system.world_matrix.is_none());
}

#[test]
fn local_child_tilts() {
    let mut groups = container_groups(serde_json::json!({"flags":1,"emitter":[]}), STREAM);
    let parent = paper_scene::matrix::model_matrix([100.0, 50.0, 0.0], [1.0; 3], [0.5, 0.0, 0.0]);
    groups[0].system.world_matrix = Some(parent);
    step_containers(&mut groups, 0.1);
    let expected = mat4_mul(&parent, &model_matrix((10.0, 0.0, 0.0), [1.0; 3], 0.0));
    assert_eq!(groups[1].system.spatial, Some(expected));
}

#[test]
fn hidden_parent_stops_stream() {
    let mut groups = container_groups(serde_json::json!({"emitter":[]}), STREAM);
    step_containers(&mut groups, 0.1);
    groups[0].visible = false;
    step_containers(&mut groups, 0.1);
    assert!(youngest(&groups[1]) > 0.05);
    groups[0].visible = true;
    step_containers(&mut groups, 0.1);
    assert!(youngest(&groups[1]) < 0.05);
}

#[test]
fn hidden_burst_replays() {
    let mut groups = burst(10.0);
    for _ in 0..4 {
        step_containers(&mut groups, 0.1);
    }
    let youngest = groups[0].sim.particles.iter().map(|particle| particle.age).fold(1.0, f32::min);
    groups[0].visible = false;
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[0].sim.layer_time(), 0.0);
    groups[0].visible = true;
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[0].sim.particles.len(), 5);
    assert!(oldest(&groups[0]) < youngest);
    assert!(oldest(&groups[1]) < youngest);
}

#[test]
fn hidden_root_sleeps() {
    let mut groups = burst(10.0);
    step_containers(&mut groups, 0.1);
    groups[0].visible = false;
    for _ in 0..3 {
        step_containers(&mut groups, 0.1);
        assert!(groups[0].sim.particles.is_empty());
        assert_eq!(groups[0].sim.layer_time(), 0.0);
    }
}

#[test]
fn reshow_restarts_duration() {
    let root = serde_json::json!({
        "emitter":[{"name":"boxrandom","rate":100,"duration":0.25}],
        "initializer":[{"name":"lifetimerandom","min":10,"max":10}],
    });
    let mut groups = container_groups(root, STREAM);
    for _ in 0..5 {
        step_containers(&mut groups, 0.1);
    }
    let spent = groups[0].sim.particles.len();
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[0].sim.particles.len(), spent);
    groups[0].visible = false;
    for _ in 0..3 {
        step_containers(&mut groups, 0.1);
        assert!(groups[0].sim.particles.is_empty());
    }
    groups[0].visible = true;
    step_containers(&mut groups, 0.1);
    step_containers(&mut groups, 0.1);
    let fresh = groups[0].sim.particles.len();
    assert!(fresh > 0 && fresh < spent, "{fresh} of {spent}");
}

#[test]
fn reshow_skips_prewarm() {
    let root = serde_json::json!({
        "starttime":2,
        "emitter":[{"name":"boxrandom","rate":100}],
        "initializer":[{"name":"lifetimerandom","min":10,"max":10}],
    });
    let mut groups = container_groups(root, STREAM);
    groups[0].prewarm_dt = groups[0].system.prewarm_step();
    groups[0].prewarm_steps = groups[0].system.prewarm_steps(groups[0].prewarm_dt);
    prewarm_particles(&mut groups, 0, particle_clock(0.0));
    assert!(oldest(&groups[0]) > 1.0);
    groups[0].visible = false;
    step_containers(&mut groups, 0.1);
    groups[0].visible = true;
    step_containers(&mut groups, 0.1);
    assert!(!groups[0].sim.particles.is_empty());
    assert!(oldest(&groups[0]) <= 0.2);
}

#[test]
fn spent_burst_stays_spent() {
    let mut groups = burst(0.05);
    step_containers(&mut groups, 0.1);
    assert_eq!(groups[0].sim.particles.len(), 5);
    for _ in 0..3 {
        step_containers(&mut groups, 0.1);
    }
    assert!(groups[0].sim.particles.is_empty());
    let before = oldest(&groups[1]);
    groups[0].visible = false;
    step_containers(&mut groups, 0.1);
    groups[0].visible = true;
    step_containers(&mut groups, 0.1);
    assert!(groups[0].sim.particles.is_empty());
    assert!(oldest(&groups[1]) > before);
}
