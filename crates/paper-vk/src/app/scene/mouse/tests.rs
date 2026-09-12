use super::*;

fn mouse() -> SceneMouse {
    SceneMouse {
        enabled: true,
        config: Parallax::default(),
        layers: Vec::new(),
        position: [0.5; 2],
        previous: [0.5; 2],
        buttons: [false; 3],
        displacement: [0.0; 2],
        parallax_position: None,
        revision: 0,
        dirty: false,
        settling: false,
        canvas: [1920.0, 1080.0],
        fov: 50.0,
        shadows: Vec::new(),
    }
}

#[test]
fn pointer_mapping_accounts_for_crop_bars_and_buttons() {
    let mut mouse = mouse();
    mouse.update(1, [0.0, 0.5], [true, false, false], (1080, 1080), paper_geom::FillMode::Fill);
    assert_eq!(mouse.position, [0.21875, 0.5]);
    assert!(mouse.pending());
    let mut uniforms = std::collections::BTreeMap::new();
    write_uniforms(&mut uniforms, &mouse);
    assert_eq!(uniforms["g_PointerState"], [1.0, 0.0, 0.0, 0.0]);
    mouse.update(2, [0.5, 0.1], [false; 3], (1080, 1080), paper_geom::FillMode::Fit);
    assert_eq!(mouse.position, [0.5, 0.0]);
    mouse.update(3, [0.5; 2], [false; 3], (2160, 2160), paper_geom::FillMode::Fit);
    assert_eq!(mouse.position, [0.5; 2]);
}

#[test]
fn neutral_projection_preserves_rect_and_tilt_has_perspective() {
    let clock = Clock3d { origin: [500.0, 400.0] };
    let rect = [500.0, 400.0, 200.0, 100.0];
    let base = projection(clock, rect, [1000.0, 800.0], [0.5; 2], false, 50.0);
    assert_eq!(base, [[0.4, 0.0, 0.0, 0.0], [0.0, 0.25, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    let tilted = projection(clock, rect, [1000.0, 800.0], [0.1, 0.2], false, 50.0);
    assert_ne!(tilted[2][0], 0.0);
    assert_ne!(tilted[2][1], 0.0);
    assert!(tilted.iter().flatten().all(|value| value.is_finite()));
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn rendered_clock_and_shadow_follow_pointer_and_stop_when_it_stops() {
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_CLOCK_LIBRARY").unwrap())
        .join("2138975215");
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    model.layers.retain(|layer| layer.id == "67");
    model.particles.clear();
    model.clear = [0.5; 3];
    assert!(model.layers[0].mouse.clock.is_some());
    let sd = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = super::super::build_group(
        &sd,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fit,
    )
    .unwrap();
    group.live_text_due = f32::INFINITY;
    group
        .refresh_live_text(paper_scene::dynamic_text::LocalTime {
            hour: 20,
            minute: 45,
            second: 0,
            day: 11,
            month: 9,
            year: 2026,
            weekday: 5,
        })
        .unwrap();
    assert!(!group.frozen);
    assert!(!group.animated());
    assert_eq!(group.quads.len(), 2);
    assert_eq!(group.quads[0].texture, group.quads[1].texture);
    let ordered =
        super::super::ordered_scene_quads(&group.quads, &group.quad_scene_order, &[], None);
    assert_eq!(ordered[0].tint[..3], [0.0; 3]);
    assert_eq!(ordered[1].tint[..3], [1.0; 3]);
    if let Ok(evidence) = std::env::var("SKWD_WE_CLOCK_EVIDENCE") {
        let clock = group.mouse.layers[0].0.clock.unwrap();
        group.mouse.position = [
            clock.origin[0] / group.mouse.canvas[0],
            1.0 - clock.origin[1] / group.mouse.canvas[1],
        ];
        group.compose(0.0, 1.0 / 30.0).unwrap();
        let (width, height, rgba) = group.read_canvas().unwrap();
        image::save_buffer(
            std::path::Path::new(&evidence).join("mouse-clock-neutral.png"),
            &rgba,
            width,
            height,
            image::ColorType::Rgba8,
        )
        .unwrap();
        std::fs::write(std::path::Path::new(&evidence).join("mouse-clock-geometry.json"), serde_json::json!({"rect":group.mouse.layers[0].1,"origin":clock.origin,"canvas":group.mouse.canvas,"fov":group.mouse.fov}).to_string()).unwrap();
    }
    let mut frames = Vec::new();
    for (index, pointer) in [[0.1, 0.2], [0.9, 0.8], [0.1, 0.2]].into_iter().enumerate() {
        group.mouse.update(
            index as u64 + 1,
            pointer,
            [false; 3],
            (1920, 1080),
            paper_geom::FillMode::Fit,
        );
        assert!(group.mouse.pending());
        group.compose(index as f32, 1.0 / 30.0).unwrap();
        let (width, height, rgba) = group.read_canvas().unwrap();
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[0] > 230));
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[0] < 30));
        if let Ok(evidence) = std::env::var("SKWD_WE_CLOCK_EVIDENCE") {
            image::save_buffer(
                std::path::Path::new(&evidence).join(format!("mouse-clock-{index}.png")),
                &rgba,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        frames.push(rgba);
        group.compose(index as f32, 1.0 / 30.0).unwrap();
        assert!(!group.mouse.pending());
    }
    assert_ne!(frames[0], frames[1]);
    assert_eq!(frames[0], frames[2]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn pointer_shader_survives_baking_and_receives_position_and_button() {
    use paper_scene::effects::{Effect, EffectPass};
    use paper_scene::shader::{Stage, translate};
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_CLOCK_LIBRARY").unwrap())
        .join("2138975215");
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    model.layers.retain(|layer| layer.id == "67");
    model.canvas = (64.0, 64.0);
    model.particles.clear();
    let layer = &mut model.layers[0];
    layer.mouse = LayerMouse::default();
    layer.live_text = None;
    layer.is_text = false;
    layer.center = (32.0, 32.0);
    layer.size = (64.0, 64.0);
    layer.texture = paper_scene::model::solid_texture();
    layer.effects = vec![Effect {
        name: "Pointer probe".into(),
        fbos: Vec::new(),
        swaps: Vec::new(),
        passes: vec![EffectPass {
            name: "Pointer probe".into(),
            vertex: translate(
                "attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }",
                Stage::Vertex,
                &Default::default(),
            ),
            fragment: translate(
                "uniform vec2 g_PointerPosition;\nuniform vec4 g_PointerState;\nvoid main() { gl_FragColor = vec4(g_PointerPosition, g_PointerState.x, 1.0); }",
                Stage::Fragment,
                &Default::default(),
            ),
            hlsl: None,
            textures: Vec::new(),
            values: Default::default(),
            target: None,
            binds: Vec::new(),
        }],
    }];
    let sd = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group =
        super::super::build_group(&sd, &mut model, true, &[(64, 64)], paper_geom::FillMode::Fit)
            .unwrap();
    assert!(!group.frozen);
    assert!(!group.animated());
    assert_eq!(group.fx.len(), 1);
    group.mouse.update(1, [0.1, 0.8], [true, false, false], (64, 64), paper_geom::FillMode::Fit);
    group.compose(0.0, 1.0 / 30.0).unwrap();
    let (width, height, rgba) = group.read_canvas().unwrap();
    let center = ((height / 2 * width + width / 2) * 4) as usize;
    assert!((24..=27).contains(&rgba[center]), "{:?}", &rgba[center..center + 4]);
    assert!((202..=206).contains(&rgba[center + 1]));
    assert_eq!(rgba[center + 2], 255);
    group.mouse.update(2, [0.1, 0.8], [false; 3], (64, 64), paper_geom::FillMode::Fit);
    group.compose(0.0, 1.0 / 30.0).unwrap();
    let (_, _, released) = group.read_canvas().unwrap();
    assert_eq!(released[center + 2], 0);
    assert!(!group.mouse.pending());
    group.destroy();
}

#[test]
fn positive_engine_roll_is_clockwise_in_world_coordinates() {
    let point = rotate([1.0, 0.0, 0.0], [0.0, 0.0, std::f32::consts::FRAC_PI_2]);
    assert!(point[0].abs() < 0.00001);
    assert_eq!(point[1], -1.0);
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn cherry_blossom_particles_follow_pointer_in_a_particle_only_scene() {
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_CLOCK_LIBRARY").unwrap())
        .join("3735385298");
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    model.layers.clear();
    model.particles.retain(|layer| layer.system.follows_mouse());
    assert_eq!(model.particles.len(), 1);
    model.clear = [0.0; 3];
    let sd = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group =
        super::super::build_group(&sd, &mut model, true, &[(320, 180)], paper_geom::FillMode::Fit)
            .unwrap();
    assert!(group.mouse.enabled);
    let mut centers = Vec::new();
    for (index, pointer) in [[0.25, 0.5], [0.75, 0.5]].into_iter().enumerate() {
        group.mouse.update(
            index as u64 + 1,
            pointer,
            [false; 3],
            (320, 180),
            paper_geom::FillMode::Fit,
        );
        for step in 0..30 {
            group.compose((index * 30 + step) as f32 * 0.1, 0.1).unwrap();
        }
        let (width, height, rgba) = group.read_canvas().unwrap();
        let lit: Vec<usize> = rgba
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, p)| (p[0] > 15 || p[1] > 15 || p[2] > 15).then_some(i))
            .collect();
        assert!(lit.len() > 10, "particle frame should have visible pixels");
        centers
            .push(lit.iter().map(|i| (i % width as usize) as f32).sum::<f32>() / lit.len() as f32);
        if let Ok(evidence) = std::env::var("SKWD_WE_CLOCK_EVIDENCE") {
            image::save_buffer(
                std::path::Path::new(&evidence).join(format!("particle-mouse-{index}.png")),
                &rgba,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
    assert!(centers[0] < 120.0 && centers[1] > 200.0, "particle centers: {centers:?}");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn depth_parallax_moves_with_zero_camera_amount_and_settles() {
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_CLOCK_LIBRARY").unwrap())
        .join("3016047975");
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
    model.layers.retain(|layer| layer.id == "13");
    model.scripts = None;
    model.particles.clear();
    assert_eq!(model.mouse.amount, 0.0);
    assert_eq!(model.mouse.influence, -0.2);
    model.layers[0]
        .effects
        .retain(|effect| effect.passes.iter().any(|pass| pass.name.contains("depthparallax")));
    assert_eq!(model.layers[0].effects.len(), 1);
    let sd = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group =
        super::super::build_group(&sd, &mut model, true, &[(640, 360)], paper_geom::FillMode::Fit)
            .unwrap();
    assert!(group.mouse.enabled);
    assert_eq!(group.mouse.parallax_position, Some([0.5; 2]));
    assert!(!group.frozen);
    assert!(!group.animated());
    assert_eq!(group.fx.len(), 1);
    let mut frames = Vec::new();
    for (index, x) in [0.25, 0.75, 0.25].into_iter().enumerate() {
        group.mouse.update(
            index as u64 + 1,
            [x, 0.5],
            [false; 3],
            (640, 360),
            paper_geom::FillMode::Fit,
        );
        for _ in 0..100 {
            group.compose(0.0, 0.1).unwrap();
            if !group.mouse.pending() {
                break;
            }
        }
        assert!(!group.mouse.pending());
        assert_eq!(group.mouse.displacement, [0.0; 2]);
        let expected = if x < 0.5 { 0.55 } else { 0.45 };
        assert_eq!(group.scene_uniforms["g_ParallaxPosition"], [expected, 0.5]);
        let (width, height, rgba) = group.read_canvas().unwrap();
        if let Ok(evidence) = std::env::var("SKWD_WE_CLOCK_EVIDENCE") {
            image::save_buffer(
                std::path::Path::new(&evidence).join(format!("depth-gpu-{index}.png")),
                &rgba,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        frames.push(rgba);
    }
    let changed = frames[0]
        .chunks_exact(4)
        .zip(frames[1].chunks_exact(4))
        .filter(|(a, b)| a[..3].iter().zip(&b[..3]).any(|(x, y)| x.abs_diff(*y) > 10))
        .count();
    assert!(changed > 1000, "depth effect changed only {changed} pixels");
    assert_eq!(frames[0], frames[2]);
    group.destroy();
}

#[test]
fn parallax_uniform_clamps_after_camera_smoothing() {
    let mut mouse = mouse();
    mouse.parallax_position = Some([-0.1, 1.2]);
    let mut uniforms = std::collections::BTreeMap::new();
    write_uniforms(&mut uniforms, &mouse);
    assert_eq!(uniforms["g_ParallaxPosition"], [0.0, 1.0]);
    assert_eq!(mouse.parallax_position, Some([-0.1, 1.2]));
}

#[test]
#[ignore = "requires Vulkan, Wallpaper Engine assets, and SKWD_WE_CLOCK_LIBRARY"]
fn parallax_layer_translation_matches_proton_and_zero_depth_pixels_stay_fixed() {
    let dir = std::path::PathBuf::from(std::env::var("SKWD_WE_CLOCK_LIBRARY").unwrap())
        .join("3016047975");
    let pkg = paper_scene::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let sd = crate::shared::create(std::ptr::null_mut()).unwrap();
    for (width, height) in [(1366, 768), (1920, 1080), (2560, 1440)] {
        for depth in [0.0, 1.0] {
            let mut model = paper_scene::model::load_from_dir(&pkg, &dir).unwrap();
            model.layers.retain(|layer| layer.id == "13");
            model.scripts = None;
            model.particles.clear();
            model.canvas = (width as f32, height as f32);
            model.clear = [0.7; 3];
            model.mouse =
                Parallax { amount: 0.5, influence: 0.5, delay: 0.0, ..Default::default() };
            let layer = &mut model.layers[0];
            layer.effects.clear();
            layer.texture = paper_scene::model::solid_texture();
            layer.center = (width as f32 * 0.5, height as f32 * 0.5);
            layer.size = (width as f32, height as f32);
            layer.mouse = LayerMouse { parallax: [depth; 2], clock: None };
            let mut group = super::super::build_group(
                &sd,
                &mut model,
                true,
                &[(width, height)],
                paper_geom::FillMode::Fit,
            )
            .unwrap();
            group.compose(0.0, 1.0 / 30.0).unwrap();
            let (_, _, before) = group.read_canvas().unwrap();
            let base = [width as f32 * 0.5, height as f32 * 0.5];
            group.mouse.update(
                1,
                [1900.0 / 1920.0, 20.0 / 1080.0],
                [false; 3],
                (width, height),
                paper_geom::FillMode::Fit,
            );
            group.compose(0.0, 1.0 / 30.0).unwrap();
            let (_, _, after) = group.read_canvas().unwrap();
            if depth == 0.0 {
                assert_eq!(before, after);
            } else {
                let dx = group.quads[0].rect[0] - base[0];
                let dy = group.quads[0].rect[1] - base[1];
                assert!((dx + 235.0 * width as f32 / 1920.0).abs() < 0.001);
                assert!((dy - 130.0 * height as f32 / 1080.0).abs() < 0.001);
                assert_ne!(before, after);
            }
            group.compose(0.0, 1.0 / 30.0).unwrap();
            assert!(!group.mouse.pending());
            if let Ok(evidence) = std::env::var("SKWD_WE_CLOCK_EVIDENCE") {
                image::save_buffer(
                    std::path::Path::new(&evidence)
                        .join(format!("parallax-{width}x{height}-depth{depth}.png")),
                    &after,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            group.destroy();
        }
    }
}
