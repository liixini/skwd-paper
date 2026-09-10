use super::*;

fn texture_key(rgba: &[u8], clamp: bool, nearest: bool) -> TextureKey {
    TextureKey { pixels: paper_scene::tex::Pixels::rgba(2, 1, rgba.to_vec()), clamp, nearest }
}

#[test]
fn texture_interning_uses_exact_payload_and_sampler_identity() {
    let base = texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false);
    let mut slots = HashMap::new();
    slots.insert(base, 7);

    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false)), Some(&7));
    let mut wrong_dimensions = texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, false);
    wrong_dimensions.pixels = paper_scene::tex::Pixels::rgba(1, 2, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(slots.get(&wrong_dimensions), None);
    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 9], true, false)), None);
    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], false, false)), None);
    assert_eq!(slots.get(&texture_key(&[1, 2, 3, 4, 5, 6, 7, 8], true, true)), None);
}

#[test]
fn taking_a_texture_key_releases_the_model_payload() {
    let mut texture = paper_scene::model::solid_texture();
    let expected = texture.pixels.clone();
    let key = TextureKey::take(&mut texture);

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
fn scene_swap_transition_never_inherits_a_previous_duration() {
    assert_eq!(scene_swap_duration(0), None);
    assert_eq!(scene_swap_duration(40), Some(80));
    assert_eq!(scene_swap_duration(600), Some(600));
    assert_eq!(scene_swap_duration(8000), Some(4000));
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
    let available = |free: &[Vec<usize>]| shared_scene_slot(free.iter().map(Vec::as_slice), 2);
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
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 2), None);
    for output in &mut free {
        output.push(2);
    }
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 3), Some(2));
    free[0].retain(|&bi| bi != 2);
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 3), None);
    free[0].push(2);
    assert_eq!(shared_scene_slot(free.iter().map(Vec::as_slice), 3), Some(2));
}

#[test]
fn shared_scene_pool_keeps_fast_outputs_independent_of_retained_frames() {
    let count = 6;
    let mut free = [vec![1, 2, 3, 4, 5], vec![0, 3, 4, 5], vec![0, 1, 2, 5]];
    let mut current = 0;
    for _ in 0..100 {
        let next = shared_scene_slot(free.iter().map(Vec::as_slice), count).unwrap();
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
    assert_eq!(shared_scene_slot([live.as_slice(), closed.as_slice()].into_iter(), 3), Some(2));
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
fn scene_sampled_ping_pong_target_never_enters_scratch_alias_pool() {
    let plan = lifetime::plan_targets(&[], &[None, None], &[vec![], vec![]], &[0, 0]).unwrap();
    assert!(plan.lifetimes[FxTargetId::Ping.index()].scratch_eligible());

    assert!(scratch_candidate(&plan, &BTreeSet::new(), FxTargetId::Ping));
    assert!(!scratch_candidate(&plan, &BTreeSet::from([CompositeBuffer::A]), FxTargetId::Ping));
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
        live_text: None,
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
        passthrough: false,
        solid: false,
        effects: Vec::new(),
    };

    let quad = passive_target_quad(&layer, 7, ash::vk::Extent2D { width: 300, height: 500 });
    assert_eq!(quad.rect, [150.0, 250.0, 300.0, 500.0]);
    assert_eq!(quad.uv, [0.0, 0.0, 0.75, 0.625]);
    assert_eq!(quad.texture, 7);
    assert!(quad.blend == vk::SceneBlend::Copy);
}

#[test]
fn active_and_passive_duplicate_ids_remain_ambiguous() {
    let layer = |name: &str| paper_scene::model::Layer {
        live_text: None,
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
        passthrough: false,
        solid: false,
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
        rect: [0.0; 4],
        uv: [0.0; 4],
        tint: [1.0; 4],
        angle: 0.0,
        texture,
        blend: vk::SceneBlend::Alpha,
    };
    let images = [image(10), image(30)];
    let particles = [OrderedParticleQuad { scene_order: 1, quad: image(20) }];

    let prefix = ordered_scene_quads(&images, &[0, 2], &particles, Some(2));
    assert_eq!(prefix.iter().map(|quad| quad.texture).collect::<Vec<_>>(), [10, 20]);
    let full = ordered_scene_quads(&images, &[0, 2], &particles, None);
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

fn sound_package(files: &[(&str, &[u8])]) -> paper_scene::pkg::Package {
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
        SpriteFrame { uv: [0.0, 0.0, 0.5, 1.0], rotated: false, time: 0.1, image: 0 },
        SpriteFrame { uv: [0.5, 0.0, 0.5, 1.0], rotated: false, time: 0.3, image: 0 },
    ];
    assert_eq!(frame_uv(&frames, 0.4, 0.0), frames[0].uv);
    assert_eq!(frame_uv(&frames, 0.4, 0.05), frames[0].uv);
    assert_eq!(frame_uv(&frames, 0.4, 0.2), frames[1].uv);
    assert_eq!(frame_uv(&frames, 0.4, 0.45), frames[0].uv);
    assert_eq!(frame_uv(&frames, 0.0, 9.0), frames[0].uv);
}

fn transform(m: &Mat4, p: [f32; 4]) -> [f32; 4] {
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
    let m = scene_perspective((1920.0, 1080.0), false);
    let d3d = scene_perspective((1920.0, 1080.0), true);
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
fn scene_perspective_matches_the_engine_eye_distance() {
    for (w, h) in [(1920.0f32, 1080.0f32), (2560.0, 1440.0), (1920.0, 1920.0)] {
        let m = scene_perspective((w, h), false);
        let d = super::particle_eye_distance(h);
        assert!((d - 0.45 * h).abs() < 1e-3);
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
fn sprite_grid_reports_engine_render_var() {
    use paper_scene::model::{SpriteFrame, Texture};
    let mut texture = paper_scene::model::solid_texture();
    texture.width = 400;
    texture.height = 200;
    assert_eq!(sprite_grid(&texture), [0.0, 0.0, 0.0, 0.5]);
    let frame =
        |x: f32, y: f32| SpriteFrame { uv: [x, y, 0.25, 0.5], rotated: false, time: 0.1, image: 0 };
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
        live_text: None,
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
        passthrough: false,
        solid: false,
        effects: Vec::new(),
    };
    let (m, inv) = layer_model_matrix(&layer, (1920.0, 1080.0));
    let x_axis = transform(&m, [1.0, 0.0, 0.0, 0.0]);
    assert!((x_axis[0]).abs() < 1e-5 && (x_axis[1] - 7.375).abs() < 1e-5, "{x_axis:?}");
    assert_eq!(transform(&m, [0.0, 0.0, 0.0, 1.0]), [300.0, 880.0, 5.0, 1.0]);
    let back = transform(&inv, [300.0, 880.0, 5.0, 1.0]);
    assert!(back.iter().zip([0.0, 0.0, 0.0, 1.0]).all(|(a, b)| (a - b).abs() < 1e-4), "{back:?}");
    layer.angle = 0.0;
    let (m, _) = layer_model_matrix(&layer, (1920.0, 1080.0));
    assert!((m[0] - 7.375).abs() < 1e-6 && (m[5] - 1.15).abs() < 1e-6);
    assert_ne!(m[0], 944.0);
}
