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
