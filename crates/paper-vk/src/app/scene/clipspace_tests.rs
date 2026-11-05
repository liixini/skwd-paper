use super::*;

#[test]
#[ignore = "requires packed clipspace probes and isolated Vulkan"]
fn clipspace_native_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_CLIPSPACE_PROBES").unwrap());
    let shared = crate::shared::test_device();
    let effects = vk::Renderer::effect_live_allocations();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for variant in ["bare", "mvp", "normalized"] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{variant}: {:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
        group.compose(1.0, 1.0 / 60.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        let rendered = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        rendered.save(root.join(format!("{variant}-paper.png")))?;
        let native = image::open(root.join(format!("{variant}-native/at12s.png")))?.into_rgb8();
        assert_eq!(rendered.dimensions(), native.dimensions());
        let error = rendered
            .pixels()
            .zip(native.pixels())
            .flat_map(|(actual, expected)| {
                actual.0[..3]
                    .iter()
                    .zip(expected.0)
                    .map(|(&actual, expected)| actual.abs_diff(expected))
            })
            .max()
            .unwrap();
        records.push(serde_json::json!({
            "variant": variant,
            "max_channel_error": error,
            "flattened": group.quads.is_empty() && group.textures.is_empty(),
            "baked_draws": group.baked_fx_draws.len(),
            "source_dynamic": group.fx.iter().map(|fx| fx.source_dynamic).collect::<Vec<_>>(),
            "frame_dependent": group.fx.iter().map(LayerFx::frame_state_dependent).collect::<Vec<_>>(),
            "scene_bytes": group.target.allocation_bytes,
            "retained_effect_bytes": group.baked_fx_outputs.iter().map(|target| target.allocation_bytes).sum::<u64>(),
            "effect_allocations": vk::Renderer::effect_live_allocations(),
        }));
        group.destroy();
        assert_eq!(vk::Renderer::effect_live_allocations(), effects);
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
        std::fs::write(root.join("paper-measurements.json"), serde_json::to_vec_pretty(&records)?)?;
        errors.push((variant, error));
    }
    assert!(errors.iter().all(|(_, error)| *error == 0), "{errors:?}");
    Ok(())
}

#[test]
#[ignore = "requires Vulkan"]
fn clipspace_visibility() {
    let shared = crate::shared::test_device();
    let effects = vk::Renderer::effect_live_allocations();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut model = super::effect_lifetime_tests::model_files(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"name":"probe","image":"models/flat.json","origin":"20 32 0","size":"8 8","effects":[{"file":"effects/test.json"}]},{"id":2,"alpha":{"value":1,"script":"export function update(value){thisScene.getLayer('probe').getEffect(0).visible=engine.runtime<1||engine.runtime>=2;return value;}"}}]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
        ("materials/test.json", br#"{"passes":[{"shader":"test"}]}"#),
        ("shaders/test.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/test.frag", b"uniform float g_Time;\nvoid main(){gl_FragColor=vec4(step(2.0,g_Time),1.0-step(2.0,g_Time),0.0,1.0);}"),
    ]);
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    for (time, expected) in [(0.0, [0, 255, 0]), (1.0, [0, 0, 0]), (2.0, [255, 0, 0])] {
        group.compose(time, 0.1).unwrap();
        let (_, _, pixels) = group.read_canvas().unwrap();
        assert_eq!(pixels[..3], expected, "{time}");
        assert_eq!(group.textures[group.fx[0].slot].effect.is_some(), time != 1.0);
    }
    group.destroy();
    assert_eq!(vk::Renderer::effect_live_allocations(), effects);
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}
