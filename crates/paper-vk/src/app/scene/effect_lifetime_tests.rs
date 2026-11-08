use super::*;

fn model(visible: serde_json::Value, fragment: &[u8]) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0",
            "size":"64 64","visible":visible,"effects":[{"file":"effects/test.json"}]}]
    });
    model_scene(scene, fragment)
}

fn model_scene(scene: serde_json::Value, fragment: &[u8]) -> SceneModel {
    let scene = scene.to_string();
    let files: &[(&str, &[u8])] = &[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
        ("materials/test.json", br#"{"passes":[{"shader":"test"}]}"#),
        (
            "shaders/test.vert",
            b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){\ngl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);\n}",
        ),
        ("shaders/test.frag", fragment),
    ];
    model_files(files)
}

pub(super) fn model_files(files: &[(&str, &[u8])]) -> SceneModel {
    paper_scene::model::load_with(
        &package_files(files),
        &paper_scene::effects::Assets::discover(None),
    )
    .unwrap()
}

pub(super) fn package_files(files: &[(&str, &[u8])]) -> paper_scene::pkg::Package {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(b"PKGV0007");
    bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    for (name, data) in files {
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
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
#[ignore = "requires Vulkan"]
fn user_shader_pixels() {
    let texture =
        texture_levels(&[paper_scene::tex::MipLevel { width: 1, height: 1, data: vec![255; 4] }]);
    let package = package_files(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/custom.json","origin":"32 32 0","size":"64 64"}]}"#),
        ("models/custom.json", br#"{"material":"materials/custom.json"}"#),
        ("materials/probe.tex", &texture),
        ("materials/custom.json", br#"{"passes":[{"shader":"custom","textures":["probe"],"constantshadervalues":{"tint":"1 1 1"},"usershadervalues":{"tint":"tint"}}]}"#),
        ("shaders/custom.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/custom.frag", b"uniform vec3 g_Tint; // {\"material\":\"tint\"}\nvoid main(){gl_FragColor=vec4(g_Tint,1.0);}"),
    ]);
    let shared = crate::shared::test_device();
    let allocations = vk::Renderer::scene_target_live_allocations();
    for (tint, expected) in [
        (None, [255, 255, 255, 255]),
        (Some(vec![1.0, 0.0, 0.0]), [255, 0, 0, 255]),
        (Some(vec![0.0, 1.0, 0.0]), [0, 255, 0, 255]),
        (None, [255, 255, 255, 255]),
    ] {
        let properties = tint.map(|tint| ("tint".into(), tint)).into_iter().collect();
        let assets = paper_scene::effects::Assets::discover(Some("")).with_properties(properties);
        let mut model = paper_scene::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        assert_eq!(center(&mut group), expected);
        group.destroy();
        assert_eq!(vk::Renderer::scene_target_live_allocations(), allocations);
    }
}

pub(super) fn center(group: &mut Group) -> [u8; 4] {
    let (width, _, rgba) = group.read_canvas().unwrap();
    let offset = ((32 * width + 32) * 4) as usize;
    rgba[offset..offset + 4].try_into().unwrap()
}

#[test]
#[ignore = "requires Vulkan"]
fn fbo_sampler_defaults() {
    let files: &[(&str, &[u8])] = &[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64","effects":[{"file":"effects/sample.json"}]}]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/sample.json", br#"{"fbos":[{"name":"cache","width":4,"height":2,"format":"rgba8888"}],"passes":[{"material":"materials/write.json","target":"cache"},{"material":"materials/read.json","bind":[{"name":"cache","index":0}]}]}"#),
        ("materials/write.json", br#"{"passes":[{"shader":"write","blending":"normal"}]}"#),
        ("materials/read.json", br#"{"passes":[{"shader":"read","blending":"normal"}]}"#),
        ("shaders/write.vert", b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}"),
        ("shaders/write.frag", b"varying vec2 v_TexCoord;\nvoid main(){gl_FragColor=v_TexCoord.x<0.5?vec4(1.0,0.0,0.0,1.0):vec4(0.0,1.0,0.0,1.0);}"),
        ("shaders/read.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/read.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(1.25,0.5));}"),
    ];
    let shared = crate::shared::test_device();
    let allocations = vk::Renderer::scene_target_live_allocations();
    for format in [
        paper_scene::effects::FboFormat::Rgba8,
        paper_scene::effects::FboFormat::Rg16f,
        paper_scene::effects::FboFormat::Rgba16,
    ] {
        for source_clamp in [false, true] {
            for repeat in [None, Some(false), Some(true)] {
                let mut model = model_files(files);
                model.layers[0].texture.clamp = source_clamp;
                let fbo = &mut model.layers[0].effects[0].fbos[0];
                fbo.format = format;
                fbo.repeat = repeat;
                let mut group =
                    build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
                let pixel = center(&mut group);
                group.destroy();
                let expected =
                    if repeat == Some(true) { [255, 0, 0, 255] } else { [0, 255, 0, 255] };
                assert_eq!(pixel, expected, "{format:?} {source_clamp} {repeat:?}");
            }
        }
    }
    assert_eq!(vk::Renderer::scene_target_live_allocations(), allocations);
}

fn source_model(fragment: &[u8], intermediate: bool) -> SceneModel {
    let pixels =
        (0_u8..32).flat_map(|y| (0_u8..32).flat_map(move |x| [x * 8, y * 8, 0, 255])).collect();
    let texture =
        texture_levels(&[paper_scene::tex::MipLevel { width: 32, height: 32, data: pixels }]);
    let mut effect = serde_json::json!({
        "fbos":[{"name":"cache","width":16,"height":16,"format":"rgba8888"}],
        "passes":[{"material":"materials/capture.json","target":"cache"},
            {"material":"materials/show.json","bind":[{"name":"cache","index":0}]}]
    });
    if intermediate {
        effect["passes"]
            .as_array_mut()
            .unwrap()
            .insert(0, serde_json::json!({"material":"materials/copy.json"}));
    }
    let effect = effect.to_string();
    model_files(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":1,"image":"models/source.json","origin":"32 32 0","size":"64 64","effects":[{"file":"effects/source.json","visible":{"value":true,"script":"export function update(value){return value;}"}}]}]}"#),
        ("models/source.json", br#"{"material":"materials/source.json"}"#),
        ("materials/probe.tex", &texture),
        ("materials/source.json", br#"{"passes":[{"shader":"source","textures":["probe"]}]}"#),
        ("shaders/source.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);v_TexCoord=a_TexCoord;}"),
        ("shaders/source.frag", b"uniform sampler2D g_Texture0;\nuniform vec4 g_Color4;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*g_Color4;}"),
        ("effects/source.json", effect.as_bytes()),
        ("materials/capture.json", br#"{"passes":[{"shader":"capture","blending":"normal"}]}"#),
        ("shaders/capture.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/capture.frag", fragment),
        ("materials/copy.json", br#"{"passes":[{"shader":"source","blending":"normal"}]}"#),
        ("materials/show.json", br#"{"passes":[{"shader":"show","blending":"normal"}]}"#),
        ("shaders/show.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/show.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
    ])
}

#[test]
fn dynamic_source_extent() {
    let mut model = source_model(b"void main(){gl_FragColor=vec4(1.0);}", false);
    let layer = &mut model.layers[0];
    let dimensions = scene_dimensions_for(model.canvas, 4096);
    assert_eq!(layer_effect_dimensions(layer, dimensions), (32, 32));
    for kind in
        [paper_scene::model::MediaTexture::Current, paper_scene::model::MediaTexture::Previous]
    {
        layer.texture.system_texture = Some(kind);
        assert_eq!(layer_effect_dimensions(layer, dimensions), (64, 64));
    }
    layer.texture.system_texture = None;
    layer.texture.video = Some(paper_scene::model::Video::Bytes(Vec::new().into()));
    assert_eq!(layer_effect_dimensions(layer, dimensions), (64, 64));
    layer.texture.video = None;
    layer.texture.frames.push(paper_scene::model::SpriteFrame {
        uv: [0.0, 0.0, 1.0, 1.0],
        axes: [1.0, 0.0, 0.0, 1.0],
        rotated: false,
        time: 0.1,
        image: 0,
    });
    assert!(!layer.spritesheet);
    assert_eq!(layer_effect_dimensions(layer, dimensions), (64, 64));
}

#[test]
#[ignore = "requires Vulkan"]
fn padded_source_pixels() {
    let shared = crate::shared::test_device();
    let targets = vk::Renderer::scene_target_live_allocations();
    let allocations = vk::Renderer::effect_live_allocations();
    for (custom, size) in [(false, 24.0), (true, 24.0), (false, 64.0), (true, 64.0)] {
        let mut model = source_model(b"uniform sampler2D g_Texture0;\nuniform vec4 g_Texture0Resolution;\nvoid main(){vec4 pixel=texLoad2D(g_Texture0,vec2(0.546875,0.546875),vec2(32.0,32.0));gl_FragColor=vec4(g_Texture0Resolution.x/255.0,pixel.x,pixel.y,1.0);}", false);
        let layer = &mut model.layers[0];
        if !custom {
            layer.material = None;
        }
        layer.texture.img_width = 24;
        layer.texture.img_height = 24;
        layer.size = (size, size);
        layer.geometry.as_mut().unwrap().size = [size, size];
        assert!(layer.material.is_some() || !custom);
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        assert_eq!(group.materials.len(), usize::from(custom));
        assert_eq!(group.fx.len(), 1);
        let pixel = center(&mut group);
        group.destroy();
        assert_eq!(pixel, [24, 136, 136, 255], "custom {custom} size {size}");
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
        assert_eq!(vk::Renderer::effect_live_allocations(), allocations);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_source_pixels() {
    let shared = crate::shared::test_device();
    let allocations = vk::Renderer::scene_target_live_allocations();
    let samples: &[(&[u8], [u8; 4])] = &[
        (b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texLoad2D(g_Texture0,vec2(0.546875,0.546875),vec2(32.0,32.0));}", [136, 136, 0, 255]),
        (b"uniform vec4 g_Texture0Resolution;\nvoid main(){gl_FragColor=vec4(g_Texture0Resolution.xy/255.0,0.0,1.0);}", [32, 32, 0, 255]),
    ];
    for (fragment, expected) in samples {
        for intermediate in [false, true] {
            let mut model = source_model(fragment, intermediate);
            let mut group =
                build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
            let pixel = center(&mut group);
            group.destroy();
            assert_eq!(pixel, *expected, "intermediate {intermediate}");
        }
    }
    assert_eq!(vk::Renderer::scene_target_live_allocations(), allocations);
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine fluid assets"]
fn fluid_preview_renders() -> anyhow::Result<()> {
    const DEFAULT_FPS: u32 = 60;
    const MAX_FPS: u32 = 240;
    let fps = std::env::var("SKWD_WE_FLUID_FPS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=MAX_FPS).contains(value))
        .unwrap_or(DEFAULT_FPS);
    let capture_frame = 4 * fps;
    let capture = std::env::var("SKWD_WE_FLUID_CAPTURE").ok();
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let scene = std::env::var("SKWD_WE_FLUID_SCENE")?;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(scene)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    let effect = model.layers.iter().flat_map(|layer| &layer.effects).next().expect("fluid effect");
    assert_eq!(effect.passes.len(), 17);
    let shared = crate::shared::test_device();
    let screen = if capture.is_some() { (1920, 1080) } else { (256, 256) };
    let mut group = build_group(&shared, &mut model, true, &[screen], FillMode::Fill)?;
    assert_eq!(group.fx.len(), 1);
    if let Ok(path) = std::env::var("SKWD_WE_FLUID_TIMESTEPS") {
        let steps: Vec<f32> = serde_json::from_slice(&std::fs::read(path)?)?;
        assert!(steps.len() > 1);
        let times: Option<Vec<f32>> = std::env::var("SKWD_WE_FLUID_TIMES")
            .ok()
            .map(|path| -> anyhow::Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
            .transpose()?;
        if let Some(times) = &times {
            assert_eq!(times.len(), steps.len());
            assert!(times.iter().all(|time| time.is_finite() && *time >= 0.0));
            assert!(times.windows(2).all(|pair| pair[1] >= pair[0]));
        }
        let mut time = 0.0;
        for (index, step) in steps.into_iter().enumerate() {
            assert!(step.is_finite() && step >= 0.0);
            time = times.as_ref().map_or(time + step, |times| times[index]);
            group.compose(time, step)?;
        }
        if let Some(path) = capture {
            let (width, height, rgba) = group.read_canvas()?;
            let canvas = image::RgbaImage::from_raw(width, height, rgba).expect("fluid canvas");
            let (x, y, width, height) =
                paper_geom::fill_crop_rect(width, height, screen.0, screen.1);
            let output = image::imageops::crop_imm(&canvas, x, y, width, height).to_image();
            output.save(path)?;
            if let Ok(reference) = std::env::var("SKWD_WE_FLUID_REFERENCE") {
                let expected = image::open(reference)?.into_rgba8();
                assert_eq!(output.dimensions(), expected.dimensions());
                assert!(output.as_raw() == expected.as_raw(), "fluid pixels");
            }
        }
        group.destroy();
        assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
        return Ok(());
    }
    let step = 1.0 / fps as f32;
    group.compose(0.0, step)?;
    let before = group.read_canvas()?.2;
    for frame in 1..=fps {
        group.compose(frame as f32 * step, step)?;
    }
    let after = group.read_canvas()?.2;
    assert_ne!(before, after);
    if let Some(path) = capture {
        for frame in fps + 1..=capture_frame {
            group.compose(frame as f32 * step, step)?;
            if frame == 2 * fps {
                let (width, height, rgba) = group.read_canvas()?;
                image::save_buffer(
                    format!("{path}.two.png"),
                    &rgba,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )?;
            }
        }
        let (width, height, rgba) = group.read_canvas()?;
        image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8)?;
    }
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine HDR assets"]
fn bloom_postprocessing_paths() -> anyhow::Result<()> {
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let shared = crate::shared::test_device();
    let mut lit = Vec::new();
    let states = if std::env::var_os("SKWD_WE_BLOOM_ULTRA").is_some() {
        &["disabled", "enabled", "ultra"][..]
    } else {
        &["disabled", "enabled"][..]
    };
    for &state in states {
        let path = std::env::var(format!("SKWD_WE_BLOOM_{}", state.to_ascii_uppercase()))?;
        let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some(&assets)),
        )?;
        assert_eq!(model.hdr, state == "ultra");
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
        group.compose(0.0, 0.0)?;
        let (width, height, rgba) = group.read_canvas()?;
        lit.push(rgba.chunks_exact(4).filter(|pixel| pixel[0] > 8).count());
        if let Ok(folder) = std::env::var("SKWD_WE_BLOOM_CAPTURE") {
            image::save_buffer(
                std::path::Path::new(&folder).join(format!("{state}.png")),
                &rgba,
                width,
                height,
                image::ColorType::Rgba8,
            )?;
        }
        group.destroy();
    }
    assert!(lit[1] > lit[0], "bloom halo");
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine bloom assets"]
fn bloom_runtime_toggle() -> anyhow::Result<()> {
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let path = std::env::var("SKWD_WE_BLOOM_RUNTIME")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    assert!(model.layers.iter().all(|layer| layer.id != "@bloom"));
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    let idle_targets = vk::Renderer::scene_target_live_allocations();
    let idle_bytes = group.effect_target_allocation_bytes();
    let mut lit = Vec::new();
    let mut active_targets = Vec::new();
    let mut allocated_bytes = Vec::new();
    let mut texture_slots = Vec::new();
    for time in [0.5, 1.5, 2.5, 3.5, 4.5] {
        group.compose(time, 0.5)?;
        let (_, _, rgba) = group.read_canvas()?;
        lit.push(rgba.chunks_exact(4).filter(|pixel| pixel[0] > 8).count());
        active_targets.push(vk::Renderer::scene_target_live_allocations());
        allocated_bytes.push(group.effect_target_allocation_bytes());
        texture_slots.push(group.textures.len());
    }
    assert!(lit[1] > lit[0] && lit[3] > lit[0], "bloom enable readback");
    assert_eq!(lit[2], lit[0]);
    assert_eq!(lit[4], lit[0]);
    assert!(active_targets[1] > idle_targets);
    assert!(active_targets[3] > idle_targets);
    assert_eq!(active_targets[2], idle_targets);
    assert_eq!(active_targets[4], idle_targets);
    assert!(allocated_bytes[1] > idle_bytes);
    assert!(allocated_bytes[3] > idle_bytes);
    assert_eq!(allocated_bytes[2], idle_bytes);
    assert_eq!(allocated_bytes[4], idle_bytes);
    assert_eq!(texture_slots[3], texture_slots[1]);
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine bloom assets"]
fn bloom_activation_retry() -> anyhow::Result<()> {
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let path = std::env::var("SKWD_WE_BLOOM_RUNTIME")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    let idle_targets = vk::Renderer::scene_target_live_allocations();
    let idle_effects = vk::Renderer::effect_live_allocations();
    super::resize::tests::fail_next_effect();
    assert!(group.compose(1.5, 0.5).is_err());
    assert!(group.fx.iter().all(|fx| fx.layer_id != "@bloom"));
    assert_eq!(vk::Renderer::scene_target_live_allocations(), idle_targets);
    assert_eq!(vk::Renderer::effect_live_allocations(), idle_effects);
    group.compose(1.5, 0.5)?;
    assert!(group.fx.iter().any(|fx| fx.layer_id == "@bloom"));
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine bloom assets"]
fn bloom_initially_on_toggle() -> anyhow::Result<()> {
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let path = std::env::var("SKWD_WE_BLOOM_RUNTIME_ON")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    let mut lit = Vec::new();
    let mut targets = Vec::new();
    for time in [0.5, 1.5, 2.5, 3.5, 4.5] {
        group.compose(time, 0.5)?;
        let (_, _, rgba) = group.read_canvas()?;
        lit.push(rgba.chunks_exact(4).filter(|pixel| pixel[0] > 8).count());
        targets.push(vk::Renderer::scene_target_live_allocations());
    }
    assert_eq!(lit[1], lit[3]);
    assert!(lit[0] > lit[1] && lit[2] > lit[1] && lit[4] > lit[1]);
    assert_eq!(targets[1], targets[3]);
    assert!(targets[0] > targets[1] && targets[2] > targets[1]);
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine bloom assets"]
fn bloom_staged_values() -> anyhow::Result<()> {
    const FULL: (u32, u32) = (1920, 1080);
    const HALF: (u32, u32) = (960, 540);
    const GREEN_MARGIN: u8 = 8;
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let path = std::env::var("SKWD_WE_BLOOM_VALUES")?;
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let shared = crate::shared::test_device();
    for size in [FULL, HALF] {
        let package = paper_scene::pkg::Package::parse(std::fs::read(&path)?)?;
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some(&assets)),
        )?;
        let mut group = build_group(&shared, &mut model, true, &[size], FillMode::Fit)?;
        group.compose(0.5, 0.5)?;
        let idle_bytes = group.effect_target_allocation_bytes();
        group.compose(1.5, 0.5)?;
        let (_, _, rgba) = group.read_canvas()?;
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[1] > pixel[0].saturating_add(GREEN_MARGIN)));
        assert!(group.effect_target_allocation_bytes() > idle_bytes);
        group.compose(2.5, 0.5)?;
        assert_eq!(group.effect_target_allocation_bytes(), idle_bytes);
        group.destroy();
        assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_material_value_uploads() -> anyhow::Result<()> {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0",
            "size":"64 64","visible":{"value":true,"script":
                "let done=false; export function update(){if(engine.runtime>1&&!done){thisLayer.getEffect(0).getMaterial().red=0;done=true;}return true;}"},
            "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{"red":1}}]}]}]
    });
    let mut model = model_scene(
        scene,
        b"uniform float u_Red; // {\"material\":\"red\"}\nvoid main(){gl_FragColor=vec4(u_Red,0.0,0.0,1.0);}",
    );
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let live_effects = vk::Renderer::effect_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit)?;
    group.compose(0.5, 0.5)?;
    let before = center(&mut group);
    group.compose(1.5, 0.5)?;
    let after = center(&mut group);
    assert!(before[0] > after[0]);
    group.compose(2.5, 0.5)?;
    assert_eq!(center(&mut group), after);
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    assert_eq!(vk::Renderer::effect_live_allocations(), live_effects);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan"]
fn indexed_material_pixels() -> anyhow::Result<()> {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0",
            "size":"64 64","visible":{"value":true,"script":
                "let stage=0; export function update(){const effect=thisLayer.getEffect(0); if(engine.runtime>1&&stage===0){effect.getMaterial(1).green=0;stage=1;} if(engine.runtime>2&&stage===1){effect.getMaterial(0).red=0;stage=2;} return true;}"},
            "effects":[{"file":"effects/test.json","passes":[
                {"constantshadervalues":{"red":1}},
                {"constantshadervalues":{"green":1}}
            ]}]}]
    });
    let document = scene.to_string();
    let vertex = b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}";
    let files: &[(&str, &[u8])] = &[
        ("scene.json", document.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/test.json", br#"{"fbos":[{"name":"_rt_first"}],"passes":[{"material":"materials/first.json","target":"_rt_first"},{"material":"materials/second.json","bind":[{"index":0,"name":"_rt_first"}]}]}"#),
        ("materials/first.json", br#"{"passes":[{"shader":"first"}]}"#),
        ("materials/second.json", br#"{"passes":[{"shader":"second"}]}"#),
        ("shaders/first.vert", vertex),
        ("shaders/second.vert", vertex),
        ("shaders/first.frag", b"uniform float u_Red; // {\"material\":\"red\"}\nvoid main(){gl_FragColor=vec4(u_Red,0.0,0.0,1.0);}"),
        ("shaders/second.frag", b"varying vec2 v_TexCoord;\nuniform sampler2D g_Texture0;\nuniform float u_Green; // {\"material\":\"green\"}\nvoid main(){vec4 base=texSample2D(g_Texture0,v_TexCoord);gl_FragColor=vec4(base.r,u_Green,0.0,1.0);}"),
    ];
    let mut model = model_files(files);
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let live_effects = vk::Renderer::effect_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit)?;
    group.compose(0.5, 0.5)?;
    let first = center(&mut group);
    group.compose(1.5, 0.5)?;
    let second = center(&mut group);
    group.compose(2.5, 0.5)?;
    let third = center(&mut group);
    assert_eq!(first[0], second[0]);
    assert!(first[1] > second[1]);
    assert_eq!(second[1], third[1]);
    assert!(second[0] > third[0]);
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    assert_eq!(vk::Renderer::effect_live_allocations(), live_effects);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and original Wallpaper Engine image assets"]
fn material_alias_pixels() -> anyhow::Result<()> {
    const SAMPLE_X: [u32; 3] = [380, 960, 1540];
    const MIDPOINT: u32 = 2;
    const CAPTURE: &str = "SKWD_WE_MATERIAL_CAPTURE";
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let path = std::env::var("SKWD_WE_MATERIAL_ALIAS")?;
    let package = paper_scene::pkg::Package::parse(std::fs::read(path)?)?;
    let mut model = paper_scene::model::load_with(
        &package,
        &paper_scene::effects::Assets::discover(Some(&assets)),
    )?;
    assert!(model.layers.iter().all(|layer| layer.material.is_none()));
    let live_targets = vk::Renderer::scene_target_live_allocations();
    let live_effects = vk::Renderer::effect_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
    group.compose(2.0, 0.5)?;
    let (width, height, rgba) = group.read_canvas()?;
    let reds = SAMPLE_X.map(|x| {
        let offset = ((height / MIDPOINT * width + x) * RGBA_CHANNELS as u32) as usize;
        rgba[offset]
    });
    assert_eq!(reds, [reds[0]; SAMPLE_X.len()]);
    assert!(group.materials.is_empty());
    if let Ok(path) = std::env::var(CAPTURE) {
        image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8)?;
    }
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live_targets);
    assert_eq!(vk::Renderer::effect_live_allocations(), live_effects);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan"]
fn audio_only_effect_updates_without_scripts_or_animation() {
    let mut model = model(
        serde_json::json!(true),
        b"uniform float g_AudioSpectrum16Left[16];\nvoid main(){\nfloat a=g_AudioSpectrum16Left[0];\ngl_FragColor=vec4(a,1.0-a,0.0,1.0);\n}",
    );
    assert!(model.scripts.is_none());
    assert!(model.layers[0].texture.atlas_frames().is_none());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.audio = None;
    group.configure_scene_targets(true).unwrap();
    assert!(!group.frozen);
    assert_eq!(group.fx.len(), 1);
    assert!(!group.animated());
    for (time, level, expected) in
        [(1.0, 0.0, [0, 255, 0, 255]), (2.0, 1.0, [255, 0, 0, 255]), (3.0, 0.0, [0, 255, 0, 255])]
    {
        group.fx[0].uniforms.insert("g_AudioSpectrum16Left".into(), vec![level; 16]);
        group.compose(time, 1.0 / 60.0).unwrap();
        assert_eq!(center(&mut group), expected);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn packed_audio_spectrum_reads_the_requested_band_and_updates_pixels() {
    let mut model = model(
        serde_json::json!(true),
        b"uniform float g_AudioSpectrum16Left[16];\nvoid main(){\nfloat i=5.0;\nfloat a=g_AudioSpectrum16Left[i/4][i%4];\ngl_FragColor=vec4(a,1.0-a,0.0,1.0);\n}",
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.audio = None;
    group.configure_scene_targets(true).unwrap();
    for (time, band, expected) in
        [(1.0, 1, [0, 255, 0, 255]), (2.0, 5, [255, 0, 0, 255]), (3.0, 6, [0, 255, 0, 255])]
    {
        let mut values = vec![0.0; 16];
        values[band] = 1.0;
        group.fx[0].uniforms.insert("g_AudioSpectrum16Left".into(), values);
        group.compose(time, 1.0 / 60.0).unwrap();
        assert_eq!(center(&mut group), expected);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn hidden_chain_leases_late() {
    let mut model = model(
        serde_json::json!({"value":false,"script":"export function update(v){return engine.runtime>=1.0;}"}),
        b"void main(){\ngl_FragColor=vec4(1.0);\n}",
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert_eq!(group.fx.len(), 1);
    assert!(group.fx[0].output.is_none());
    assert_eq!(group.fx_pool.len(), 0);
    group.compose(1.5, 1.0 / 60.0).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(group.fx[0].output.is_some());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_effect_opacity_renders_without_a_script_runtime_and_settles() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{"alpha":{
                "value":0,"animation":{"options":{"fps":30,"length":150,"mode":"single"},
                "c0":[{"frame":0,"value":0},{"frame":63,"value":0},{"frame":150,"value":1}]}
            }}}]}]}]
    });
    let mut model = model_scene(scene, b"uniform float g_UserAlpha; // {\"material\":\"alpha\",\"default\":1.0}\nvoid main(){gl_FragColor=vec4(g_UserAlpha,g_UserAlpha,g_UserAlpha,1.0);}");
    assert!(model.scripts.is_none());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert!(group.animated());
    group.compose(3.55, 0.1).unwrap();
    let mid = center(&mut group);
    assert!((127..=128).contains(&mid[0]), "{mid:?}");
    group.compose(5.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(!group.animated());
    group.compose(8.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_parent_origin_moves_the_rendered_child() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":2,"origin":{"value":"0 0 0","animation":{"relative":true,
            "options":{"fps":30,"length":30,"mode":"single"},
            "c0":[{"frame":0,"value":0},{"frame":30,"value":32}],"c1":[],"c2":[]}}},
            {"id":1,"parent":2,"image":"models/flat.json","origin":"16 32 0","size":"16 16",
            "effects":[{"file":"effects/test.json"}]}]
    });
    let mut model = model_scene(scene, b"void main(){\ngl_FragColor=vec4(1.0);\n}");
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    assert_eq!(before[(32 * 64 + 16) * 4], 255);
    assert_eq!(before[(32 * 64 + 48) * 4], 0);
    group.compose(1.0, 0.1).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(after[(32 * 64 + 16) * 4], 0);
    assert_eq!(after[(32 * 64 + 48) * 4], 255);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_visibility_retains_an_initially_hidden_layer() {
    let visible = serde_json::json!({"value":false,"animation":{
        "options":{"fps":30,"length":30,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":30,"value":1,"step":true}]
    }});
    let mut model = model(visible, b"void main(){\ngl_FragColor=vec4(1.0);\n}");
    assert!(model.scripts.is_none());
    assert_eq!(model.layers.len(), 1);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(!group.animated());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_parent_vector_moves_the_rendered_child() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":2,"origin":{"value":"0 0 0",
            "script":"export function update(){return new Vec3(engine.runtime*32,0,0);}"}},
            {"id":1,"parent":2,"image":"models/flat.json","origin":"16 32 0","size":"16 16",
            "effects":[{"file":"effects/test.json"}]}]
    });
    let mut model = model_scene(scene, b"void main(){\ngl_FragColor=vec4(1.0);\n}");
    assert!(model.scripts.is_some());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    assert_eq!(before[(32 * 64 + 16) * 4], 255);
    assert_eq!(before[(32 * 64 + 48) * 4], 0);
    group.compose(1.0, 0.1).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(after[(32 * 64 + 16) * 4], 0);
    assert_eq!(after[(32 * 64 + 48) * 4], 255);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan and Wallpaper Engine font assets"]
fn scripted_text_relayouts_after_replacement_and_keeps_unchanged_geometry() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"origin":"60 32 0","pointsize":6,
            "font":"fonts/NotoSans-Regular.ttf","horizontalalign":"right","verticalalign":"center",
            "text":{"value":"A","script":"export function update(){return engine.runtime<1?'A':'AAAA';}"}},
            {"id":2,"alpha":{"value":0,"script":"export function update(){return engine.runtime;}"}}]
    });
    let mut model = model_scene(scene, b"void main(){gl_FragColor=vec4(1.0);}");
    assert!(model.layers[0].script_text.is_some());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let before = group.quads[0].rect;
    let (_, _, before_pixels) = group.read_canvas().unwrap();
    group.compose(1.0, 0.1).unwrap();
    let after = group.quads[0].rect;
    let (_, _, after_pixels) = group.read_canvas().unwrap();
    assert!(after[2] > before[2]);
    assert!(after[0] < before[0]);
    assert_ne!(before_pixels, after_pixels);
    group.compose(2.0, 0.1).unwrap();
    assert_eq!(group.quads[0].rect, after);
    assert_eq!(group.read_canvas().unwrap().2, after_pixels);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn text_additive_toggle() {
    let font = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/fonts/latin.ttf"
    ));
    let shared = crate::shared::test_device();
    for (show, blends) in [
        ("engine.runtime<1.5", [vk::SceneBlend::Alpha, vk::SceneBlend::Add]),
        ("engine.runtime>1.5", [vk::SceneBlend::Add, vk::SceneBlend::Alpha]),
    ] {
        let script = format!(
            "export function update(value){{thisLayer.getEffect('fx').visible={show};return value;}}"
        );
        let scene = serde_json::json!({
            "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
            "objects":[{"id":1,"origin":"32 32 0","pointsize":24,"font":"fonts/latin.ttf",
                "text":"H","colorBlendMode":31,"effects":[{"file":"effects/test.json","name":"fx"}],
                "alpha":{"value":1,"script":script}}]
        })
        .to_string();
        let mut model = model_files(&[
            ("scene.json", scene.as_bytes()),
            ("fonts/latin.ttf", font),
            ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
            ("materials/test.json", br#"{"passes":[{"shader":"test"}]}"#),
            ("shaders/test.vert", super::effect_space_tests::PASS_VERTEX),
            (
                "shaders/test.frag",
                b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}",
            ),
        ]);
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        let quad = group.fx[0].quad;
        for (time, blend) in [1.0, 2.0].into_iter().zip(blends) {
            group.compose(time, 0.1).unwrap();
            assert!(group.quads[quad].blend == blend, "{show} {time}");
        }
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_effect_values_reach_the_matching_layer() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64",
                "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{
                    "alpha":{"value":0,"script":"export function update(){return engine.runtime<1?0:1;}"}
                }}]}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64",
                "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{"alpha":0.25}}]}]}
        ]
    });
    let mut model = model_scene(scene, b"uniform float g_UserAlpha; // {\"material\":\"alpha\"}\nvoid main(){gl_FragColor=vec4(g_UserAlpha,g_UserAlpha,g_UserAlpha,1.0);}");
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    assert_eq!(before[(32 * 64 + 16) * 4], 0);
    assert!((63..=64).contains(&before[(32 * 64 + 48) * 4]));
    group.compose(1.0, 0.1).unwrap();
    let (_, _, after) = group.read_canvas().unwrap();
    assert_eq!(after[(32 * 64 + 16) * 4], 255);
    assert!((63..=64).contains(&after[(32 * 64 + 48) * 4]));
    group.destroy();
}

fn lifetime_model() -> SceneModel {
    let script = r"
let image;
let phase = 0;
export function update(value) {
    const next = Math.floor(engine.runtime);
    if (next === phase) return value;
    phase = next;
    if (phase === 1 || phase === 4) {
        image = thisScene.createLayer({name:'created',image:'models/flat.json',origin:'16 32 0',size:'16 16',color:'1 0 0'});
        const origin = image.origin;
        origin.x = 32;
        if (image.origin.x !== 16) throw Error('origin getter mutated layer');
        image.origin = origin;
        image.color = new Vec3(0,1,1);
    } else if (phase === 2) {
        const origin = image.origin;
        origin.x = 48;
        if (image.origin.x !== 32) throw Error('origin getter mutated layer');
        image.origin = origin;
        image.color = new Vec3(1,0,0);
    } else if (phase === 3) {
        thisScene.destroyLayer(image);
    }
    return value;
}
";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","visible":false,"alpha":{"value":1,"script":script}}, {"id":2,"name":"clock","image":"models/flat.json","origin":"8 8 0","size":"8 8","color":"0 1 0"}]
    });
    model_scene(scene, b"void main(){gl_FragColor=vec4(1.0);}")
}

#[test]
#[ignore = "requires Vulkan"]
fn dynamic_image_lifetime() {
    let mut model = lifetime_model();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let original_bytes: u64 = group.textures.iter().map(|texture| texture.allocation_bytes).sum();
    group.compose(0.0, 0.0).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 255, 255, 255]);
    assert_eq!(group.quads.len(), 3);
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    let (width, _, pixels) = group.read_canvas().unwrap();
    let right = ((32 * width + 48) * 4) as usize;
    assert_eq!(&pixels[right..right + 4], &[255, 0, 0, 255]);
    group.compose(3.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert_eq!(
        group.textures.iter().map(|texture| texture.allocation_bytes).sum::<u64>(),
        original_bytes
    );
    group.compose(4.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 255, 255, 255]);
    assert_eq!(group.quads.len(), 3);
    assert_eq!(group.textures.len(), 2);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn dynamic_descriptor_growth() {
    let script = r"
let images = [];
export function update(value) {
    if (Math.floor(engine.runtime) % 2) {
        images.push(thisScene.createLayer({image:'models/flat.json',origin:'32 32 0',size:'16 16',color:'0 1 1'}));
    } else {
        for (const image of images) thisScene.destroyLayer(image);
        images = [];
    }
    return value;
}
";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","visible":false,"alpha":{"value":1,"script":script}}]
    });
    let mut model = model_scene(scene, b"void main(){gl_FragColor=vec4(1.0);}");
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let original_bytes: u64 = group.textures.iter().map(|texture| texture.allocation_bytes).sum();
    for cycle in 0..2 {
        for step in 0..80 {
            let time = (cycle * 2 + 1) as f32 + step as f32 * 0.01;
            group.compose(time, 0.01).unwrap();
            let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
            assert!(host.diagnostics.is_empty(), "time {time}: {:?}", host.diagnostics);
        }
        assert_eq!(center(&mut group), [0, 255, 255, 255]);
        assert_eq!(group.quads.len(), 81);
        assert_eq!(group.textures.len(), 81);
        group.compose((cycle * 2 + 2) as f32, 0.01).unwrap();
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(center(&mut group), [0, 0, 0, 255]);
        assert_eq!(
            group.textures.iter().map(|texture| texture.allocation_bytes).sum::<u64>(),
            original_bytes
        );
    }
    group.destroy();
}

pub(super) fn texture_levels(levels: &[paper_scene::tex::MipLevel]) -> Vec<u8> {
    let first = &levels[0];
    let mut texture = b"TEXV0005\0TEXI0001\0".to_vec();
    for field in [
        0,
        paper_scene::tex::FLAG_CLAMP_UVS,
        first.width,
        first.height,
        first.width,
        first.height,
        0,
    ] {
        texture.extend_from_slice(&field.to_le_bytes());
    }
    texture.extend_from_slice(b"TEXB0002\0");
    texture.extend_from_slice(&1_u32.to_le_bytes());
    texture.extend_from_slice(&u32::try_from(levels.len()).unwrap().to_le_bytes());
    for level in levels {
        let bytes = u32::try_from(level.data.len()).unwrap();
        for field in [level.width, level.height, 0, bytes, bytes] {
            texture.extend_from_slice(&field.to_le_bytes());
        }
        texture.extend_from_slice(&level.data);
    }
    let mut parsed = paper_scene::tex::parse(&texture, None).unwrap();
    let pixels = paper_scene::tex::take_pixels(&mut parsed).unwrap();
    assert_eq!(pixels.levels.as_slice(), levels);
    texture
}

#[test]
#[ignore = "requires Vulkan and SKWD_PAPER_TEX_MIPS=1"]
fn dynamic_minified_texture() {
    assert_eq!(std::env::var("SKWD_PAPER_TEX_MIPS").as_deref(), Ok("1"));
    let mut levels = Vec::new();
    for edge in [64_u32, 32, 16, 8, 4, 2, 1] {
        let mut data = Vec::new();
        for _ in 0..edge {
            for x in 0..edge {
                data.extend_from_slice(if edge == 64 {
                    if x % 3 == 0 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] }
                } else {
                    &[85, 0, 170, 255]
                });
            }
        }
        levels.push(paper_scene::tex::MipLevel { width: edge, height: edge, data });
    }
    let texture = texture_levels(&levels);
    let script = r"
let created = false;
export function update(value) {
    if (!created && engine.runtime >= 1) {
        const config = thisScene.getInitialLayerConfig('static');
        config.name = 'dynamic';
        config.origin = '48 32 0';
        thisScene.createLayer(config);
        created = true;
    }
    return value;
}
";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"name":"static","image":"models/stripes.json","origin":"16 32 0","size":"7 7"},
            {"id":2,"image":"models/stripes.json","visible":false,"alpha":{"value":1,"script":script}}
        ]
    }).to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/stripes.json", br#"{"material":"materials/stripes.json"}"#),
        ("materials/stripes.json", br#"{"passes":[{"textures":["stripes"]}]}"#),
        ("materials/stripes.tex", &texture),
    ]);
    assert_eq!(model.layers[0].texture.width, 64);
    assert_eq!(model.layers[0].texture.pixels.levels.len(), 7);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (_, _, before) = group.read_canvas().unwrap();
    group.compose(1.0, 1.0).unwrap();
    let (width, _, after) = group.read_canvas().unwrap();
    for y in 28..36 {
        for x in 12..20 {
            let original = ((y * width + x) * 4) as usize;
            let created = ((y * width + x + 32) * 4) as usize;
            assert_eq!(&before[created..created + 4], &[0, 0, 0, 255]);
            assert_eq!(&before[original..original + 4], &after[original..original + 4]);
            assert_eq!(&after[original..original + 4], &after[created..created + 4]);
        }
    }
    let middle = ((32 * width + 16) * 4) as usize;
    assert!((30..225).contains(&after[middle]));
    assert!((30..225).contains(&after[middle + 2]));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn dynamic_image_shadow() {
    let mut model = lifetime_model();
    model.layers[1].mouse.clock = Some(paper_scene::mouse::Clock3d { origin: [8.0, 8.0] });
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.quads.len(), 3);
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 255, 255, 255]);
    assert_eq!(group.quads.len(), 4);
    assert!(group.quads[3].projection.is_some());
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    let (width, _, pixels) = group.read_canvas().unwrap();
    let moved = ((32 * width + 48) * 4) as usize;
    assert_eq!(&pixels[moved..moved + 4], &[255, 0, 0, 255]);
    group.compose(3.0, 1.0).unwrap();
    let (_, _, retired) = group.read_canvas().unwrap();
    assert_eq!(&retired[moved..moved + 4], &[0, 0, 0, 255]);
    group.compose(4.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 255, 255, 255]);
    assert_eq!(group.quads.len(), 4);
    assert!(group.quads[3].projection.is_some());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_masks() {
    let script = r"
export function update(value) {
    const layer = thisScene.getLayer('target');
    const mask = Math.floor(engine.runtime);
    for (let index = 0; index < 3; index++) layer.getEffect(index).visible = (mask & (1 << index)) !== 0;
    return value;
}";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64",
                "effects":[
                    {"file":"effects/test.json","visible":false,"passes":[{"constantshadervalues":{"mask":"0 1 1"}}]},
                    {"file":"effects/test.json","passes":[{"constantshadervalues":{"mask":"1 0 1"}}]},
                    {"file":"effects/test.json","passes":[{"constantshadervalues":{"mask":"1 1 0"}}]}
                ]},
            {"id":2,"alpha":{"value":1,"script":script}}
        ]
    });
    let mut model = model_scene(scene, b"uniform sampler2D g_Texture0;\nuniform vec3 g_Mask; // {\"material\":\"mask\"}\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5))*vec4(g_Mask,1.0);\n}");
    assert_eq!(model.layers[0].effects.len(), 3);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    for mask in [0u8, 1, 2, 4, 3, 5, 6, 7, 0] {
        group.compose(f32::from(mask) + 0.0625, 0.0625).unwrap();
        let expected = [
            if mask & 1 == 0 { 255 } else { 0 },
            if mask & 2 == 0 { 255 } else { 0 },
            if mask & 4 == 0 { 255 } else { 0 },
            255,
        ];
        assert_eq!(center(&mut group), expected, "mask {mask}");
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_uniforms() {
    let script = r"
export function update(value) {
    thisScene.getLayer('target').getEffect(0).visible = engine.runtime >= 2;
    return value;
}";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":1,"name":"target","image":"models/flat.json","origin":"16 32 0","size":"32 64",
                "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{
                    "level":{"value":0,"script":"export function update(){return engine.runtime/4;}"}
                }}]}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64",
                "effects":[{"file":"effects/test.json","passes":[{"constantshadervalues":{"level":0}}]}]},
            {"id":3,"alpha":{"value":1,"script":script}}
        ]
    });
    let mut model = model_scene(
        scene,
        b"uniform float g_Level; // {\"material\":\"level\"}\nvoid main(){gl_FragColor=vec4(g_Level,1.0-g_Level,0.0,1.0);}",
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    for (time, expected) in [
        (1.0, [255, 255, 255, 255]),
        (4.0, [255, 0, 0, 255]),
        (1.0, [255, 255, 255, 255]),
        (4.0, [255, 0, 0, 255]),
    ] {
        group.compose(time, 0.1).unwrap();
        let (_, _, pixels) = group.read_canvas().unwrap();
        assert_eq!(&pixels[(32 * 64 + 16) * 4..(32 * 64 + 16) * 4 + 4], &expected);
        assert_eq!(&pixels[(32 * 64 + 48) * 4..(32 * 64 + 48) * 4 + 4], &[0, 255, 0, 255]);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_keyframes() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[{"file":"effects/test.json","visible":{"value":0,"animation":{
                "options":{"fps":30,"length":30,"mode":"single"},
                "c0":[{"frame":0,"value":0},{"frame":30,"value":1}]}
            }}]}]
    });
    let mut model = model_scene(scene, b"void main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}");
    assert!(model.scripts.is_none());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [0, 255, 0, 255]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_swaps() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64",
                "effects":[{"file":"effects/write.json"},{"file":"effects/read.json"}]},
            {"id":2,"alpha":{"value":1,"script":"export function update(value){const layer=thisScene.getLayer('target');const mask=Math.floor(engine.runtime);layer.getEffect(0).visible=(mask&1)!==0;layer.getEffect(1).visible=(mask&2)!==0;return value;}"}}
        ]
    }).to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/write.json", br#"{"fbos":[{"name":"first","format":"rgba8888","clear":"0 0 1 1"},{"name":"second","format":"rgba8888","clear":"0 1 0 1"}],"passes":[{"material":"materials/write.json","target":"first"},{"command":"swap","source":"first","target":"second"}]}"#),
        ("effects/read.json", br#"{"fbos":[{"name":"first"},{"name":"second"}],"passes":[{"command":"swap","source":"first","target":"second"},{"material":"materials/read.json","bind":[{"index":0,"name":"first"}]}]}"#),
        ("materials/write.json", br#"{"passes":[{"shader":"write"}]}"#),
        ("materials/read.json", br#"{"passes":[{"shader":"read"}]}"#),
        ("shaders/write.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/read.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/write.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ("shaders/read.frag", b"uniform sampler2D g_Texture0;\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5));\n}"),
    ]);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    for (mask, expected) in [
        (3u8, [255, 0, 0, 255]),
        (0, [255, 255, 255, 255]),
        (2, [0, 255, 0, 255]),
        (1, [255, 0, 0, 255]),
        (3, [255, 0, 0, 255]),
    ] {
        group.compose(f32::from(mask), 0.1).unwrap();
        assert_eq!(center(&mut group), expected, "mask {mask}");
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_budget() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[
                {"file":"effects/test.json","visible":false,"passes":[{"constantshadervalues":{"color":"1 0 0"}}]},
                {"file":"effects/test.json","passes":[{"constantshadervalues":{"color":"0 1 0"}}]}
            ]},
            {"id":2,"alpha":{"value":1,"script":"export function update(value){if(engine.runtime>=2)thisScene.getLayer('target').getEffect(0).visible=true;return value;}"}}]
    });
    let mut model = model_scene(
        scene,
        b"uniform vec3 g_Color; // {\"material\":\"color\"}\nvoid main(){\ngl_FragColor=vec4(g_Color,1.0);\n}",
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(center(&mut group), [0, 255, 0, 255]);
    assert_eq!(group.fx[0].owner.last(), Some(&1));
    if !group.unavailable_effects.is_empty() {
        assert!(group.compose(2.0, 0.1).unwrap_err().to_string().contains("unavailable"));
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn effect_visibility_invalid() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[{"file":"effects/invalid.json","visible":false},{"file":"effects/valid.json"}]},
            {"id":2,"alpha":{"value":1,"script":"export function update(value){if(engine.runtime>=1)thisScene.getLayer('target').getEffect(0).visible=true;return value;}"}}]
    }).to_string();
    let files: &[(&str, &[u8])] = &[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/invalid.json", br#"{"fbos":[{"name":"discard","format":"rgba8888"}],"passes":[{"material":"materials/valid.json","target":"discard"},{"material":"materials/invalid.json"}]}"#),
        ("effects/valid.json", br#"{"passes":[{"material":"materials/valid.json"}]}"#),
        ("materials/valid.json", br#"{"passes":[{"shader":"valid"}]}"#),
        ("materials/invalid.json", br#"{"passes":[{"shader":"invalid"}]}"#),
        ("shaders/valid.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/invalid.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/valid.frag", b"void main(){\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}"),
        ("shaders/invalid.frag", b"void main(){\ngl_FragColor=undeclaredInput;\n}"),
    ];
    let mut model = model_files(files);
    assert_eq!(model.layers[0].effects.len(), 2);
    let directory = tempfile::tempdir().unwrap();
    let video_path = directory.path().join("probe.mp4");
    let encoded = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=16x16:r=1",
            "-frames:v",
            "1",
            "-c:v",
            "mpeg4",
            "-y",
        ])
        .arg(&video_path)
        .status()
        .unwrap();
    assert!(encoded.success());
    let payload: std::sync::Arc<[u8]> = std::fs::read(video_path).unwrap().into();
    let shared = crate::shared::test_device();
    let mut strict_model = model_files(files);
    let error = build_group(&shared, &mut strict_model, true, &[(64, 64)], FillMode::Fit)
        .err()
        .expect("invalid shader must fail");
    assert!(error.to_string().contains("strict native scene startup rejected effect pipeline"));
    drop(
        video::VideoTexture::open(
            &shared,
            &paper_scene::model::Video::Bytes(payload.clone()),
            true,
            false,
        )
        .unwrap(),
    );
    let mut video_texture = paper_scene::model::solid_texture();
    video_texture.video = Some(paper_scene::model::Video::Bytes(payload));
    let mut still_texture = paper_scene::model::solid_texture();
    still_texture.pixels =
        std::sync::Arc::new(paper_scene::tex::Pixels::rgba(1, 1, vec![255, 0, 0, 255]));
    model.layers[0].effects[0].passes[0].textures =
        vec![None, Some(video_texture), Some(still_texture)];
    let mut group = build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 255, 0, 255]);
    assert_eq!(group.fx[0].owner, [1]);
    assert!(group.fx[0].fbos.is_empty());
    assert!(group.videos.is_empty());
    assert_eq!(group.textures.len(), 2);
    assert!(group.compose(1.0, 0.1).unwrap_err().to_string().contains("unavailable"));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_feedback() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64",
                "effects":[{"file":"effects/cache.json","visible":false},{"file":"effects/read.json"}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64",
                "effects":[{"file":"effects/blue.json"}]},
            {"id":3,"origin":{"value":"0 0 0","animation":{"options":{"fps":30,"length":30,"mode":"single"},"c0":[{"frame":0,"value":0},{"frame":30,"value":1}]}}}
        ]
    }).to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/cache.json", br#"{"fbos":[{"name":"cache","format":"rgba8888","clear":"0 1 0 1"}],"passes":[{"material":"materials/blue.json","target":"cache"}]}"#),
        ("effects/read.json", br#"{"fbos":[{"name":"cache","format":"rgba8888","clear":"0 1 0 1"}],"passes":[{"material":"materials/read.json","bind":[{"index":0,"name":"cache"}]}]}"#),
        ("effects/blue.json", br#"{"passes":[{"material":"materials/blue.json"}]}"#),
        ("materials/blue.json", br#"{"passes":[{"shader":"blue"}]}"#),
        ("materials/read.json", br#"{"passes":[{"shader":"read"}]}"#),
        ("shaders/blue.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/read.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/blue.frag", b"uniform float g_Time;\nvoid main(){\ngl_FragColor=vec4(0.0,0.0,1.0,1.0+g_Time*0.0);\n}"),
        ("shaders/read.frag", b"uniform sampler2D g_Texture0;\nuniform float g_Time;\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5))+vec4(g_Time*0.0);\n}"),
    ]);
    assert!(model.scripts.is_none());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.fx[0].fbos[0].1.owned().is_some());
    for time in [1.0, 2.0, 3.0] {
        group.compose(time, 0.1).unwrap();
        let (_, _, pixels) = group.read_canvas().unwrap();
        assert_eq!(&pixels[(32 * 64 + 16) * 4..(32 * 64 + 16) * 4 + 4], &[0, 255, 0, 255]);
        assert_eq!(&pixels[(32 * 64 + 48) * 4..(32 * 64 + 48) * 4 + 4], &[0, 0, 255, 255]);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_visibility_missing() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "effects":[{"file":"effects/missing.json","visible":false}]},
            {"id":2,"alpha":{"value":1,"script":"export function update(value){if(engine.runtime>=1)thisScene.getLayer('target').getEffect(0).visible=true;return value;}"}}]
    });
    let mut model = model_scene(scene, b"void main(){\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}");
    assert!(model.skipped.is_empty());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [255, 255, 255, 255]);
    assert!(group.compose(1.0, 0.1).unwrap_err().to_string().contains("unavailable"));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_owner_input() {
    let shared = crate::shared::test_device();
    for override_middle in [false, true] {
        let mut scene = model(serde_json::json!(true), b"uniform sampler2D g_Texture0;\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5));\n}");
        let mut green =
            model(serde_json::json!(true), b"void main(){\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}");
        let effect = &mut scene.layers[0].effects[0];

        effect.passes.insert(0, green.layers[0].effects[0].passes.remove(0));
        if override_middle {
            effect.fbos.push(paper_scene::effects::Fbo {
                name: "local".into(),
                size: paper_scene::effects::FboSize::Fit(64),
                format: paper_scene::effects::FboFormat::Rgba8,
                repeat: None,
                clear: [1.0, 0.0, 0.0, 1.0],
                clear_on_setup: true,
                unique: true,
            });
            effect.passes[1].binds.push((0, EffectBind::Named("local".into())));
            let mut copy = model(serde_json::json!(true), b"uniform sampler2D g_Texture0;\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5));\n}");
            effect.passes.push(copy.layers[0].effects[0].passes.remove(0));
        }
        let mut group = build_group(&shared, &mut scene, true, &[(64, 64)], FillMode::Fit).unwrap();
        assert_eq!(center(&mut group), [255, 255, 255, 255]);
        group.compose(1.0, 0.1).unwrap();
        assert_eq!(center(&mut group), [255, 255, 255, 255]);
        group.destroy();
    }
}

#[test]
fn last_visible_effect() {
    let mut effects = vec![paper_scene::effects::EffectState::default(); 3];
    let owners = [0, 0, 1, 2, 2];
    for (mask, expected, counts) in [
        (7, Some(4), (5, 7)),
        (3, Some(2), (3, 4)),
        (1, Some(1), (2, 3)),
        (0, None, (0, 0)),
        (2, Some(2), (1, 1)),
    ] {
        for (index, effect) in effects.iter_mut().enumerate() {
            effect.visible = mask & (1 << index) != 0;
        }
        assert_eq!(last_effect_pass(&owners, &effects), expected);
        assert_eq!(active_effect_counts(&owners, &[0, 2], &effects), counts);
    }
}

fn projection_model(scene: serde_json::Value, fragment: &[u8]) -> SceneModel {
    let scene = scene.to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/producer.json", br#"{"fbos":[{"name":"cache","fit":128,"format":"rgba8888","clear":"0 0 1 1"}],"passes":[{"material":"materials/green.json","target":"cache"},{"material":"materials/copy.json","bind":[{"index":0,"name":"cache"}]}]}"#),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/green.json", br#"{"passes":[{"shader":"green"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/green.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){\ngl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);\n}"),
        ("shaders/copy.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){\ngl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);\nv_TexCoord=a_TexCoord;\n}"),
        ("shaders/green.frag", fragment),
        ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,v_TexCoord);\n}"),
    ]);
    let texture = &mut model.layers[0].texture;
    texture.width = 32;
    texture.height = 32;
    texture.img_width = 32;
    texture.img_height = 32;
    texture.pixels =
        std::sync::Arc::new(paper_scene::tex::Pixels::rgba(32, 32, vec![255; 32 * 32 * 4]));
    model
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_final_projection() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"name":"target","image":"models/flat.json","origin":"33 33 0","size":"32 32",
            "effects":[{"file":"effects/producer.json"},{"file":"effects/copy.json","visible":false}]},
            {"id":2,"alpha":{"value":1,"script":"let reset=false;export function update(value){const effect=thisScene.getLayer('target').getEffect(1);effect.visible=engine.runtime>=1&&engine.runtime<2;if(engine.runtime>=4&&!reset){effect.visible=true;effect.visible=false;reset=true;}return value;}"}}]
    });
    let mut model = projection_model(scene, b"uniform float g_Time;\nvoid main(){\nif(g_Time>=3.0) discard;\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}");
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let target = group.fx[0].fbos[0].1.get(&group.fx_pool);
    assert_eq!((target.extent.width, target.extent.height), (32, 32));
    for (time, dense) in [(0.0, false), (1.0, true), (2.0, false), (3.0, false), (4.0, false)] {
        group.compose(time, 0.1).unwrap();
        let (_, _, pixels) = group.read_canvas().unwrap();
        let green = pixels.chunks_exact(4).filter(|pixel| pixel[1] > 0).count();
        let blue = pixels.chunks_exact(4).filter(|pixel| pixel[2] > 0).count();
        if dense {
            assert!(green >= 1024);
        } else if time >= 4.0 {
            assert_eq!(green, 0);
            assert!(blue >= 1024);
        } else {
            assert!(green > 0 && green < 16);
            assert!(blue > 1000);
        }
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_direct_geometry() {
    let baseline = vk::Renderer::effect_live_allocations();
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":1920,"height":1080},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"1230 826.875 0","size":"240 150","effects":[{"file":"effects/producer.json"}]},
            {"id":2,"image":"models/flat.json","origin":"-100 -100 0","size":"3 3","color":"0 0 1"}
        ]
    });
    let mut model =
        projection_model(scene, b"void main(){\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}");
    model.layers[0].effects[0].fbos[0].clear = [0.0; 4];
    model.layers[0].mouse.parallax = [1.0, 0.0];
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit).unwrap();
    assert!(group.fx.is_empty());
    assert_eq!(group.baked_fx_draws.len(), 1);
    let (_, _, pixels) = group.read_canvas().unwrap();
    let scan: Vec<_> = (838..847).map(|x| pixels[(141 * 1280 + x) * 4 + 1]).collect();
    assert_eq!(scan, [6, 25, 56, 99, 155, 99, 56, 25, 6]);
    group.mouse.enabled = false;
    group.quads[0].rect[0] += 3.0;
    group.compose(1.0, 0.1).unwrap();
    let (_, _, pixels) = group.read_canvas().unwrap();
    assert_eq!(pixels[(141 * 1280 + 844) * 4 + 1], 155);
    group.quads[1].rect = [1266.0, 213.0, 3.0, 3.0];
    group.quads[1].projection = None;
    group.compose(2.0, 0.1).unwrap();
    let (_, _, pixels) = group.read_canvas().unwrap();
    assert_eq!(&pixels[(141 * 1280 + 844) * 4..][..3], &[0, 0, 255]);
    let prefix = ordered_scene_quads(
        &group.quads,
        &group.quad_scene_order,
        &[],
        0,
        Some(group.quad_scene_order[1]),
    );
    let snapshot = group.renderer.create_scene_target(1280, 720).unwrap();
    group
        .renderer
        .render_scene_with_canvas(
            &snapshot,
            [1920.0, 1080.0],
            [0.0; 4],
            &prefix,
            &group.textures,
            &[],
        )
        .unwrap();
    let (_, _, pixels) = group.renderer.read_scene_target(&snapshot).unwrap();
    assert_eq!(pixels[(141 * 1280 + 844) * 4 + 1], 155);
    group.renderer.destroy_scene_target(snapshot);
    assert_eq!(group.baked_fx_outputs.len(), 2);
    let output = group.baked_fx_outputs.last().unwrap();
    assert_eq!((output.extent.width, output.extent.height), (160, 100));
    let (_, _, pixels) = group.renderer.read_scene_target(output).unwrap();
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[1] > 0));
    group.destroy();
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
}

#[test]
#[ignore = "requires Vulkan"]
fn effect_direct_scale() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":1920,"height":1080},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"960 540 0","size":"240 150","scale":"2 1 1","effects":[{"file":"effects/producer.json"}]}]
    });
    let mut model =
        projection_model(scene, b"void main(){\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}");
    let effect = &mut model.layers[0].effects[0];
    let mut pass = effect.passes.remove(0);
    pass.target = None;
    effect.passes = vec![pass];
    effect.fbos.clear();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fit).unwrap();
    let (_, _, pixels) = group.read_canvas().unwrap();
    assert_eq!(pixels.chunks_exact(4).filter(|pixel| pixel[1] > 0).count(), 32000);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan and Wallpaper Engine assets"]
fn effect_global_bloom() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0","bloom":true,"bloomstrength":0,"bloomthreshold":1},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64","color":"1 0 0"}]
    }).to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]);
    assert_eq!(model.layers.last().unwrap().id, "@bloom");
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn cropped_effect_accounting() {
    let shared = crate::shared::test_device();
    for visible in [true, false] {
        let mut model = model(
            serde_json::json!({"value":visible,"script":"export function update(value){return value;}"}),
            b"uniform sampler2D g_Texture0;\nuniform float g_Time;\nvoid main(){\nvec4 screen=texSample2D(g_Texture0,vec2(0.5));\ngl_FragColor=vec4(fract(g_Time),screen.g,0.0,1.0);\n}",
        );
        model.clear = [0.0, 0.25, 0.0];
        model.layers[0].effects[0].passes[0].binds = vec![(0, EffectBind::SceneUnderLayer)];
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        let fx = &group.fx[0];
        let (Some(FxTargetStorage::Pooled(crop)), FxTargetStorage::Pooled(ping)) =
            (&fx.crop, &fx.ping)
        else {
            panic!("pooled crop");
        };
        assert_eq!(crop.class, ping.class);
        assert_eq!(
            group.effect_target_allocation_bytes(),
            fx.allocation_bytes() + group.fx_pool.allocation_bytes()
        );
        group.compose(1.5, 0.1).unwrap();
        let pixel = center(&mut group);
        assert_eq!(pixel[1], 64);
        if visible {
            assert!((127..=128).contains(&pixel[0]));
        } else {
            assert_eq!(pixel[0], 0);
        }
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn device_fixture_recovers() {
    assert!(
        std::thread::spawn(|| {
            let _shared = crate::shared::test_device();
            panic!("failed GPU fixture");
        })
        .join()
        .is_err()
    );
    let shared = crate::shared::test_device();
    let mut model =
        model(serde_json::json!(true), b"void main(){\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n}");
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(center(&mut group), [0, 255, 0, 255]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn reordered_effect_inputs() {
    let baseline = vk::Renderer::effect_live_allocations();
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
                "effects":[{"file":"effects/color.json"},{"file":"effects/color.json"}]},
            {"id":2,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
                "effects":[{"file":"effects/copy.json"}]}
        ]
    })
    .to_string();
    let vertex = b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);v_TexCoord=a_TexCoord;}";
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/color.json", br#"{"passes":[{"material":"materials/color.json"}]}"#),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/color.json", br#"{"passes":[{"shader":"color"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/color.vert", vertex),
        ("shaders/copy.vert", vertex),
        ("shaders/color.frag", b"uniform float g_Time;\nvoid main(){gl_FragColor=g_Time<0.5?vec4(1.0,0.0,0.0,1.0):(g_Time<1.5?vec4(0.0,1.0,0.0,1.0):vec4(0.0,0.0,1.0,1.0));}"),
        ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
    ]);
    model.layers[1].effects[0].passes[0].binds = vec![(0, EffectBind::SceneSoFar)];
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.fx.len(), 2);
    assert!(!group.fx[0].snapshot);
    assert!(group.fx[1].snapshot);
    assert_eq!(group.fx[0].presentation, Some(1));
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    let output = group.fx[0].output.unwrap();
    group.fx[0].effects[0].visible = false;
    group.fx.rotate_left(1);
    eprintln!("prefix phase: changed owner inputs");
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(group.fx[1].presentation, Some(1));
    assert!(!group.fx[1].snapshot);
    assert_ne!(group.fx[1].output.unwrap(), output);
    let snapshot = group.scene_snapshot.as_ref().unwrap();
    let (_, _, pixels) = group.renderer.read_scene_target(snapshot).unwrap();
    assert_eq!(&pixels[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4], &[255, 0, 0, 255]);
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    let owner = &group.fx[1];
    assert!(group.textures[owner.slot].effect.is_some());
    let target = group.renderer.create_scene_target(64, 64).unwrap();
    group
        .renderer
        .render_scene_with_canvas(
            &target,
            [64.0, 64.0],
            [0.0; 4],
            std::slice::from_ref(&group.quads[owner.quad]),
            &group.textures,
            &[],
        )
        .unwrap();
    let (_, _, pixels) = group.renderer.read_scene_target(&target).unwrap();
    assert_eq!(&pixels[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4], &[0, 255, 0, 255]);
    group.renderer.destroy_scene_target(target);
    group.fx.rotate_left(1);
    eprintln!("prefix phase: restored ordering");
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 0, 255, 255]);
    group.destroy();
    assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
}

fn scene_input_model(visible: serde_json::Value, consumer: bool, blend: &str) -> SceneModel {
    let mut data = Vec::new();
    for _ in 0..64 {
        for x in 0..64_u8 {
            let color = if (30..=33).contains(&x) {
                [148, 136, 85, 255]
            } else {
                [x * 4, 128, 255 - x * 4, 255]
            };
            data.extend_from_slice(&color);
        }
    }
    let texture = texture_levels(&[paper_scene::tex::MipLevel { width: 64, height: 64, data }]);
    let mut scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/seed.json","origin":"32 32 0","size":"64 64"},
            {"id":2,"image":"models/sample.json","origin":"32 32 0","size":"32 64",
                "color":"0.25 0.5 0.4","alpha":0.55,"visible":visible}
        ]
    });
    if consumer {
        scene["objects"].as_array_mut().unwrap().push(serde_json::json!({
            "id":3,"image":"models/consumer.json","origin":"32 32 0","size":"8 64"
        }));
    }
    let scene = scene.to_string();
    let material = serde_json::json!({"passes":[{"shader":"sample",
        "textures":["_rt_FullFrameBuffer"],"blending":blend,"depthtest":"disabled",
        "depthwrite":"disabled","cullmode":"nocull"}]})
    .to_string();
    model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/consumer.json", br#"{"material":"materials/consumer.json","autosize":false,"fullscreen":false,"passthrough":false}"#),
        ("materials/consumer.json", br#"{"passes":[{"shader":"consumer","textures":["_rt_FullFrameBuffer"],"blending":"normal"}]}"#),
        ("shaders/consumer.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/consumer.frag", b"uniform sampler2D g_Texture0;\nvoid main(){float a=texSample2D(g_Texture0,vec2(0.5,0.5)).a;gl_FragColor=vec4(a,a,a,1.0);}"),
        ("models/seed.json", br#"{"material":"materials/seed.json"}"#),
        ("materials/seed.json", br#"{"passes":[{"shader":"genericimage","textures":["gradient"]}]}"#),
        ("materials/gradient.tex", &texture),
        ("models/sample.json", br#"{"material":"materials/sample.json","autosize":false,"fullscreen":false,"passthrough":false}"#),
        ("materials/sample.json", material.as_bytes()),
        ("shaders/sample.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/sample.frag", b"uniform sampler2D g_Texture0;\nuniform vec4 g_Color4;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5))*g_Color4;}"),
    ])
}

#[test]
#[ignore = "requires Vulkan"]
fn scene_input_preserves_alpha() {
    let mut model = scene_input_model(serde_json::json!(true), false, "normal");
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.layers[1].base_scene_input);
    assert_eq!(model.layers[1].effects.len(), 1);
    assert!(layer_quad(&model.layers[0], 0, model.canvas).blend == vk::SceneBlend::Alpha);
    assert!(layer_quad(&model.layers[1], 1, model.canvas).blend == vk::SceneBlend::CopyRgb);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
    assert!(group.frozen);
    let (width, _, pixels) = group.read_canvas().unwrap();
    let rgb = |x: u32| {
        let offset = ((32 * width + x) * 4) as usize;
        &pixels[offset..offset + 3]
    };
    assert_eq!(pixels[((32 * width + 32) * 4 + 3) as usize], 255);
    assert_eq!(rgb(20), &[37, 68, 34]);
    assert_eq!(rgb(32), &[37, 68, 34]);
    assert_ne!(rgb(8), rgb(56));
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn copy_visibility_cycles() {
    let visible = serde_json::json!({"value":false,
        "script":"export function update(value){return engine.runtime>=1&&engine.runtime<2;}"});
    let mut model = scene_input_model(visible, false, "normal");
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
    assert!(!group.frozen);
    assert!(group.quads[1].blend == vk::SceneBlend::Alpha);
    let hidden = center(&mut group);
    group.compose(1.5, 1.0 / 60.0).unwrap();
    assert!(group.quads[1].blend == vk::SceneBlend::CopyRgb);
    assert_eq!(center(&mut group), [37, 68, 34, 255]);
    let fx = group.fx.iter().find(|fx| fx.quad == 1).unwrap();
    let output = fx.target(fx.output.unwrap(), &group.fx_pool);
    let (width, height, pixels) = group.renderer.read_scene_target(output).unwrap();
    let offset = ((height / 2 * width + width / 2) * 4) as usize;
    assert_eq!(&pixels[offset..offset + 4], &[37, 68, 34, 140]);
    group.compose(2.5, 1.0 / 60.0).unwrap();
    assert!(group.quads[1].blend == vk::SceneBlend::Alpha);
    assert_eq!(center(&mut group), hidden);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn scene_capture_stays_opaque() {
    let shared = crate::shared::test_device();
    for replay in [false, true] {
        let mut model = scene_input_model(serde_json::json!(true), true, "normal");
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let previous = tests::REPLAY_SNAPSHOTS.replace(replay);
        let result = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill);
        tests::REPLAY_SNAPSHOTS.set(previous);
        let mut group = result.unwrap();
        assert_eq!(center(&mut group), [255; 4]);
        let (width, _, pixels) = group.read_canvas().unwrap();
        let offset = ((32 * width + 20) * 4) as usize;
        assert_eq!(&pixels[offset..offset + 4], &[37, 68, 34, 255]);
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn scene_material_blends() {
    let shared = crate::shared::test_device();
    for (blend, expected) in
        [("translucent", [87, 99, 57, 255]), ("additive", [168, 173, 104, 255])]
    {
        let mut model = scene_input_model(serde_json::json!(true), false, blend);
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group =
            build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
        assert!(group.frozen);
        let actual = center(&mut group);
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 1, "{blend}: {actual} != {expected}");
        }
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn material_blend_visibility() {
    let shared = crate::shared::test_device();
    for (blend, expected) in
        [("translucent", [87, 99, 57, 255]), ("additive", [168, 173, 104, 255])]
    {
        let visible = serde_json::json!({"value":false,
            "script":"export function update(value){return engine.runtime>=1&&engine.runtime<2;}"});
        let mut model = scene_input_model(visible, false, blend);
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group =
            build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill).unwrap();
        assert!(!group.frozen);
        let hidden = center(&mut group);
        assert_eq!(hidden, [148, 136, 85, 255]);
        group.compose(1.5, 1.0 / 60.0).unwrap();
        for (actual, expected) in center(&mut group).into_iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 1, "{blend}: {actual} != {expected}");
        }
        let fx = group.fx.iter().find(|fx| fx.quad == 1).unwrap();
        assert!(fx.presentation.is_none());
        assert!(group.textures[fx.slot].effect.is_none());
        let output = fx.target(fx.output.unwrap(), &group.fx_pool);
        let (width, height, pixels) = group.renderer.read_scene_target(output).unwrap();
        let offset = ((height / 2 * width + width / 2) * 4) as usize;
        assert_eq!(&pixels[offset..offset + 4], &[37, 68, 34, 140]);
        group.compose(2.5, 1.0 / 60.0).unwrap();
        assert!(group.quads[1].blend == vk::SceneBlend::Alpha);
        assert_eq!(center(&mut group), hidden);
        group.destroy();
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn material_blend_snapshot() {
    let shared = crate::shared::test_device();
    for (blend, expected) in
        [("translucent", [56, 95, 97, 255]), ("additive", [100, 165, 194, 255])]
    {
        for replay in [false, true] {
            let mut model = scene_input_model(serde_json::json!(true), true, blend);
            assert!(model.skipped.is_empty(), "{:?}", model.skipped);
            let previous = tests::REPLAY_SNAPSHOTS.replace(replay);
            let result = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fill);
            tests::REPLAY_SNAPSHOTS.set(previous);
            let mut group = result.unwrap();
            assert_eq!(center(&mut group), [255; 4]);
            let (width, _, pixels) = group.read_canvas().unwrap();
            let offset = ((32 * width + 20) * 4) as usize;
            for (actual, expected) in pixels[offset..offset + 4].iter().zip(expected) {
                assert!(actual.abs_diff(expected) <= 1, "{blend}: {actual} != {expected}");
            }
            group.destroy();
        }
    }
}

#[test]
fn material_consumer_gate() {
    for blend in ["translucent", "additive"] {
        let mut model = scene_input_model(serde_json::json!(true), true, blend);
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(mesh::viewport(&model, &[(64, 64)], FillMode::Fill).is_ok());
        model.layers[2].effects[0].passes[0].binds[0].1 =
            EffectBind::LayerComposite { layer: "2".into(), buffer: CompositeBuffer::A };
        let error = mesh::viewport(&model, &[(64, 64)], FillMode::Fill).unwrap_err();
        assert_eq!(error.to_string(), "framebuffer input mapping unsupported");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn reflection_follows_keyframes() {
    let shared = crate::shared::test_device();
    let origin = serde_json::json!({"value":"0 0 0","animation":{
        "options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":0.0,"step":true},{"frame":10,"value":1.0,"step":true}],
        "c1":[{"frame":0,"value":0.0,"step":true}],
        "c2":[{"frame":0,"value":0.0,"step":true}]}});
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":100},
        "objects":[
            {"id":1,"model":"models/mesh.mdl","origin":origin},
            {"id":2,"image":"models/sample.json","origin":"0 0 0","size":"1 1","reflected":false},
            {"id":3,"model":"models/reader.mdl","origin":"0 0 -50"}
        ]
    });
    let mut model = super::tests::mesh_sampler_model(
        &serde_json::to_vec(&scene).unwrap(),
        br#"{"passes":[{"shader":"sample","textures":["_rt_Reflection"],"blending":"normal"}]}"#,
        b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.4,0.6));}",
    );
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let reflection = group.reflection.as_ref().unwrap();
    let first = group.renderer.read_scene_target(reflection.target()).unwrap().2;
    group.compose(2.0, 2.0).unwrap();
    let reflection = group.reflection.as_ref().unwrap();
    let second = group.renderer.read_scene_target(reflection.target()).unwrap().2;
    group.destroy();
    assert!(first.chunks_exact(4).any(|pixel| pixel[0] > 0));
    assert_ne!(first, second);
}

#[test]
#[ignore = "requires Vulkan"]
fn keeps_reflection_target() {
    let shared = crate::shared::test_device();
    for reflected in [true, false] {
        let mut model = super::tests::mesh_sampler_model(
            br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl"},{"id":3,"model":"models/reader.mdl","origin":"0 0 -50"},{"id":2,"image":"models/sample.json","origin":"0 0 0","size":"1 1","reflected":false}]}"#,
            br#"{"passes":[{"shader":"sample","textures":["_rt_Reflection","_rt_MipMappedFrameBuffer"],"blending":"normal"}]}"#,
            b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=mix(texSample2D(g_Texture0,vec2(0.4,0.6)),texSample2D(g_Texture1,vec2(0.4,0.6)),0.25);}",
        );
        model.meshes[0].reflected = reflected;
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        assert!(!group.frozen);
        assert!(group.scene_history.is_some());
        let reflection = group.reflection.as_ref().unwrap();
        assert!(reflection.ready);
        let view = reflection.target().view;
        let bytes = reflection.target().allocation_bytes;
        assert!(bytes > 0);
        let (_, _, pixels) = group.renderer.read_scene_target(reflection.target()).unwrap();
        assert_eq!(pixels.chunks_exact(4).any(|pixel| pixel[0] > 0), reflected);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[1] == 0 && pixel[2] == 0));
        let before = group.effect_target_allocation_bytes();
        let report = group.bake_static_effects().unwrap();
        assert!(report.retained_bytes >= bytes);
        assert_eq!(
            before,
            report.retained_bytes + report.transient_bytes + report.released_transient_bytes
        );
        assert_eq!(
            before,
            group.effect_target_allocation_bytes() + report.released_transient_bytes
        );
        for time in [1.0, 2.0] {
            group.compose(time, 1.0 / 60.0).unwrap();
            let reflection = group.reflection.as_ref().unwrap();
            assert_eq!(reflection.target().view, view);
            assert_eq!(group.renderer.read_scene_target(reflection.target()).unwrap().2, pixels);
        }
        group.release_composition_inputs();
        assert!(group.reflection.is_none());
        assert_eq!(group.effect_target_allocation_bytes(), 0);
        group.destroy();
    }
}

#[test]
#[ignore = "requires authored frozen fixture, Vulkan and DXC"]
fn authored_puppet_lifetime() {
    let path = std::env::var_os("SKWD_PUPPET_SCENE_FIXTURE").expect("authored fixture path");
    let assets = std::env::var("SKWD_PUPPET_ASSET_ROOT").expect("authored assets path");
    let package = paper_scene::pkg::Package::parse(std::fs::read(path).unwrap()).unwrap();
    let assets = paper_scene::effects::Assets::discover(Some(&assets));
    let shared = crate::shared::test_device();
    let baseline = vk::Renderer::effect_live_allocations();
    for invalid in [false, true] {
        let mut model = paper_scene::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert_eq!(model.layers.len(), 1);
        assert!(model.layers[0].material.as_ref().unwrap().skin.is_some());
        if invalid {
            let pass = &mut model.layers[0].material.as_mut().unwrap().pass;
            pass.vertex.source = "invalid GLSL".into();
            pass.hlsl = None;
        }
        let built = build_group(&shared, &mut model, true, &[(1280, 720)], FillMode::Fill);
        if invalid {
            assert!(built.is_err());
        } else {
            let mut group = built.unwrap();
            assert!(group.puppets.is_empty());
            assert!(group.frozen);
            assert!(group.materials.is_empty());
            assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
            assert!(model.layers[0].puppet.is_none());
            assert!(model.layers[0].material.as_ref().unwrap().skin.is_none());
            group.compose(0.0, 0.0).unwrap();
            let first = group.read_canvas().unwrap().2;
            assert!(
                first
                    .chunks_exact(RGBA_CHANNELS)
                    .any(|pixel| pixel[..RGB_CHANNELS].iter().any(|channel| *channel != 0))
            );
            assert!(
                first
                    .chunks_exact(RGBA_CHANNELS)
                    .any(|pixel| pixel[..RGB_CHANNELS].iter().all(|channel| *channel == 0))
            );
            group.compose(10.0, 0.0).unwrap();
            assert_eq!(group.read_canvas().unwrap().2, first);
            group.destroy();
        }
        assert_eq!(vk::Renderer::effect_live_allocations(), baseline);
    }
}

#[test]
#[ignore = "requires pinned dynamic text package and Vulkan"]
fn dynamic_text_slots() {
    let script = r"
let layer;
let phase = 0;
function text() { return thisScene.createLayer({text:'A',font:'fonts/Probe.ttf',pointsize:8,origin:'32 32 0',horizontalalign:'center',verticalalign:'center',opaquebackground:true,backgroundcolor:'0 0 1'}); }
export function init() { layer=text(); }
export function update(value) {
    const next=Math.floor(engine.runtime);
    if(next===phase) return value;
    phase=next;
    if(phase===1 || phase===6) thisScene.destroyLayer(layer);
    if(phase===2 || phase===7) layer=thisScene.createLayer({image:'models/flat.json',origin:'32 32 0',size:'16 16',color:'1 0 0'});
    if(phase===3) {thisScene.destroyLayer(layer);layer=text();}
    if(phase===4) {layer.text='AAAA';layer.pointsize=12;}
    if(phase===5) layer.visible=false;
    return value;
}";
    let scene = serde_json::json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"text":"A","font":"fonts/Probe.ttf","pointsize":6,"origin":"8 8 0","opaquebackground":true,"backgroundcolor":"0 1 0"},
        {"id":2,"image":"models/flat.json","visible":false,"alpha":{"value":1,"script":script}}]});
    let mut model = dynamic_text_model(&scene);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let created = group.read_canvas().unwrap().2;
    group.compose(1.0, 1.0).unwrap();
    let retired = group.read_canvas().unwrap().2;
    assert_ne!(created, retired);
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    group.compose(3.0, 1.0).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, created);
    let slots = (group.quads.len(), group.textures.len());
    group.compose(4.0, 1.0).unwrap();
    assert_ne!(group.read_canvas().unwrap().2, created);
    group.compose(5.0, 1.0).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, retired);
    group.compose(6.0, 1.0).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, retired);
    group.compose(7.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    assert_eq!((group.quads.len(), group.textures.len()), slots);
    let scripts = group.scripts.as_ref().unwrap();
    assert!(scripts.host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

pub(super) fn dynamic_text_model(scene: &serde_json::Value) -> SceneModel {
    let path = std::env::var_os("SKWD_DYNAMIC_TEXT_PACKAGE").expect("pinned text package");
    let package = paper_scene::pkg::Package::open(std::path::Path::new(&path)).unwrap();
    let font = package.find("fonts/Probe.ttf").expect("packaged probe font");
    let source = scene.to_string();
    model_files(&[
        ("scene.json", source.as_bytes()),
        ("fonts/Probe.ttf", font),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ])
}

#[test]
#[ignore = "requires pinned dynamic text package and Vulkan"]
fn dynamic_text_insertion() {
    let scene = serde_json::json!({"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"text":"A","font":"fonts/Probe.ttf","pointsize":6,"origin":"8 8 0","opaquebackground":true,"backgroundcolor":"0 1 0"},
        {"id":2,"alpha":{"value":1,"script":"let done=false; export function update(value){if(!done && engine.runtime>=1){done=true;thisScene.createLayer({text:'B',font:'fonts/Probe.ttf',pointsize:8,origin:'48 32 0',opaquebackground:true,backgroundcolor:'0 0 1'});}return value;}"}}]});
    let mut model = dynamic_text_model(&scene);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let before = group.read_canvas().unwrap().2;
    group.compose(1.0, 1.0).unwrap();
    let after = group.read_canvas().unwrap().2;
    assert_ne!(before, after);
    for (before, after) in before.chunks_exact(64 * 4).zip(after.chunks_exact(64 * 4)) {
        assert_eq!(&before[..32 * 4], &after[..32 * 4]);
    }
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, after);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

pub(super) fn dynamic_sprite_model(scene: &serde_json::Value, divergent: bool) -> SceneModel {
    const FRAME_TAG: &[u8] = b"TEXS0003\0";
    const FRAME_HEADER: usize = 12;
    const FRAME_BYTES: usize = 32;
    const TIME_OFFSET: usize = 4;
    const LEFT_OFFSET: usize = 8;
    const WIDTH_OFFSET: usize = 16;
    const TEXTURE: &str = "materials/textures/ordinary.tex";
    let path = std::env::var_os("SKWD_DYNAMIC_IMAGE_PACKAGE").expect("pinned image package");
    let package = paper_scene::pkg::Package::open(std::path::Path::new(&path)).unwrap();
    let mut texture = package.find(TEXTURE).unwrap().to_vec();
    if divergent {
        let start = texture.windows(FRAME_TAG.len()).position(|bytes| bytes == FRAME_TAG).unwrap()
            + FRAME_TAG.len()
            + FRAME_HEADER;
        for (index, (left, duration)) in [(0.0_f32, 0.25_f32), (4.0, 0.75)].into_iter().enumerate()
        {
            for (offset, value) in
                [(TIME_OFFSET, duration), (LEFT_OFFSET, left), (WIDTH_OFFSET, 4.0)]
            {
                let offset = start + index * FRAME_BYTES + offset;
                texture[offset..offset + std::mem::size_of::<f32>()]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    let text = scene.to_string();
    let mut files: Vec<_> = package
        .entries()
        .iter()
        .filter(|entry| entry.path != "scene.json" && entry.path != TEXTURE)
        .map(|entry| (entry.path.as_str(), package.find(&entry.path).unwrap()))
        .collect();
    files.push(("scene.json", text.as_bytes()));
    files.push((TEXTURE, &texture));
    model_files(&files)
}

#[test]
#[ignore = "requires Vulkan"]
fn material_property_pixels() {
    let script = r"
let effect, material, phase = 0;
export function init(value) {
    effect = thisScene.getLayer('target').getEffect(0);
    material = effect.getMaterial(0);
    if (effect.getMaterialCount() !== 3 || material.probeLevel !== 0.25) throw Error('material shape');
    return value;
}
export function update(value) {
    if (engine.runtime >= 3 && phase < 3) {
        effect.setMaterialProperty('probeLevel', 0.25);
        phase = 3;
    } else if (engine.runtime >= 2 && phase < 2) {
        material.probeLevel = 0.5;
        phase = 2;
    } else if (engine.runtime >= 1 && phase < 1) {
        effect.setMaterialProperty('probeLevel', 0.75);
        phase = 1;
    }
    if (material.probeLevel !== [0.25, 0.75, 0.5, 0.25][phase]) throw Error('material readback');
    return value;
}";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64",
                "effects":[{"file":"effects/method.json","passes":[{"constantshadervalues":{"u_probeLevel":0.25}}]}]},
            {"id":2,"alpha":{"value":0,"script":script}}
        ]
    });
    let source = scene.to_string();
    let mut model = model_files(&[
        ("scene.json", source.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/method.json", br#"{"fbos":[{"name":"_rt_A","fit":64,"format":"rgba8888","clear":"0 0 0 0","unique":true},{"name":"_rt_B","fit":64,"format":"rgba8888","clear":"0 0 0 0","unique":true}],"passes":[{"material":"materials/method.json","target":"_rt_A"},{"command":"swap","source":"_rt_A","target":"_rt_B"},{"material":"materials/read.json","bind":[{"name":"_rt_B","index":0}]}]}"#),
        ("materials/method.json", br#"{"passes":[{"shader":"method"}]}"#),
        ("materials/read.json", br#"{"passes":[{"shader":"read","textures":["util/white"]}]}"#),
        ("shaders/method.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/method.frag", br#"uniform float u_probeLevel; // {"material":"probeLevel","default":0.25}
void main(){gl_FragColor=vec4(u_probeLevel,1.0-u_probeLevel,0.0,1.0);}"#),
        ("shaders/read.vert", b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}"),
        ("shaders/read.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
    ]);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    for (time, expected) in [
        (0.0, [64, 191, 0, 255]),
        (1.0, [191, 64, 0, 255]),
        (2.0, [127, 127, 0, 255]),
        (3.0, [64, 191, 0, 255]),
    ] {
        group.compose(time, 0.1).unwrap();
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(center(&mut group), expected);
    }
    group.destroy();
}

fn clear_function_model(script: &str, alpha: &serde_json::Value) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"name":"target","image":"models/flat.json","origin":"32 32 0","size":"64 64","alpha":alpha,
                "effects":[{"file":"effects/cache.json","passes":[{"constantshadervalues":{"u_probeAlpha":1}}]}]},
            {"id":2,"alpha":{"value":0,"script":script}}
        ]
    });
    let source = scene.to_string();
    model_files(&[
        ("scene.json", source.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/cache.json", br#"{"fbos":[{"name":"_rt_A","fit":64,"format":"rgba8888","clear":"0 0 0 0","unique":true},{"name":"_rt_B","fit":64,"format":"rgba8888","clear":"0 0 0 0","unique":true}],"functions":{"clearProbe":{"action":"clear","fbos":["_rt_A","_rt_B"]}},"passes":[{"material":"materials/paint.json","target":"_rt_A"},{"command":"swap","source":"_rt_A","target":"_rt_B"},{"material":"materials/read.json","bind":[{"name":"_rt_B","index":0}]}]}"#),
        ("materials/paint.json", br#"{"passes":[{"shader":"paint","blending":"translucent"}]}"#),
        ("materials/read.json", br#"{"passes":[{"shader":"read","textures":["util/white"]}]}"#),
        ("shaders/paint.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/paint.frag", br#"uniform float u_probeAlpha; // {"material":"probeAlpha","default":1}
void main(){gl_FragColor=vec4(1.0,0.0,0.0,u_probeAlpha);}"#),
        ("shaders/read.vert", b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}"),
        ("shaders/read.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
    ])
}

#[test]
#[ignore = "requires Vulkan"]
fn clear_function_pixels() {
    let script = r"
let effect, material, phase = 0;
export function init(value) {
    effect = thisScene.getLayer('target').getEffect(0);
    material = effect.getMaterial(0);
    if (typeof effect.executeMaterialFunction !== 'function') throw Error('function missing');
    return value;
}
export function update(value) {
    if (engine.runtime >= 3 && phase < 3) {
        material.probeAlpha = 1;
        phase = 3;
    } else if (engine.runtime >= 2 && phase < 2) {
        effect.executeMaterialFunction('clearProbe');
        phase = 2;
    } else if (engine.runtime >= 1 && phase < 1) {
        material.probeAlpha = 0;
        phase = 1;
    }
    return value;
}";
    let mut model = clear_function_model(script, &serde_json::json!(1));
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let painted = group.read_canvas().unwrap().2;
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, painted);
    group.compose(2.0, 1.0).unwrap();
    let cleared = group.read_canvas().unwrap().2;
    assert_ne!(cleared, painted);
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert!(group.fx[0].pending_clears.is_empty());
    group.compose(2.5, 0.5).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, cleared);
    group.compose(3.0, 0.5).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, painted);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn hidden_clear_queue() {
    let script = r"
let effect, material, phase = 0;
export function init(value) {
    effect = thisScene.getLayer('target').getEffect(0);
    material = effect.getMaterial(0);
    return value;
}
export function update(value) {
    if (engine.runtime >= 1 && phase === 0) {
        material.probeAlpha = 0;
        phase = 1;
    }
    if (engine.runtime >= 1 && engine.runtime < 3)
        for (let call = 0; call < 8; call++) effect.executeMaterialFunction('clearProbe');
    if (engine.runtime >= 4 && phase === 1) {
        material.probeAlpha = 1;
        phase = 2;
    }
    return value;
}";
    let alpha = serde_json::json!({"value":1,"script":"export function update(value){return engine.runtime<1||engine.runtime>=3?1:0;}"});
    let mut model = clear_function_model(script, &alpha);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.fx[0].sampled_targets.is_empty());
    group.compose(0.0, 0.0).unwrap();
    let painted = group.read_canvas().unwrap().2;
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    let allocation = group.effect_target_allocation_bytes();
    for time in [1.0, 2.0, 2.5] {
        group.compose(time, 0.5).unwrap();
        assert_eq!(group.quads[group.fx[0].quad].tint[3], 0.0);
        assert_eq!(group.fx[0].pending_clears.len(), group.fx[0].fbos.len());
        assert_eq!(group.effect_target_allocation_bytes(), allocation);
    }
    let pending = group.fx[0].pending_clears.clone();
    assert!(!group.fx[0].queue_clear_function(0, "missing"));
    assert_eq!(group.fx[0].pending_clears, pending);
    group.compose(3.0, 0.5).unwrap();
    let cleared = group.read_canvas().unwrap().2;
    assert_ne!(cleared, painted);
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    assert!(group.fx[0].pending_clears.is_empty());
    assert!(group.fx[0].pending_clears.capacity() >= pending.len());
    group.compose(3.5, 0.5).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, cleared);
    group.compose(4.0, 0.5).unwrap();
    assert_eq!(group.read_canvas().unwrap().2, painted);
    assert_eq!(group.effect_target_allocation_bytes(), allocation);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn scene_event_pixels() {
    let script = r"
scene.on('userShortcut', () => { thisLayer.color = new Vec3(0,1,0); });
scene.on('cursorClick', () => engine.openUserShortcut('probe'));
scene.on('update', () => {
    if (engine.runtime >= 2) { thisLayer.color = new Vec3(0,0,1); scene.off('update'); }
});
";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "color":{"value":"1 0 0","script":script}}]
    });
    let mut model = model_scene(scene, b"void main(){gl_FragColor=vec4(1.0);}");
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let allocations = vk::Renderer::scene_target_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    group.mouse.position = [0.5; 2];
    group.mouse.buttons = [true, false, false];
    group.compose(0.5, 0.5).unwrap();
    group.mouse.buttons = [false; 3];
    group.compose(1.0, 0.5).unwrap();
    assert_eq!(center(&mut group), [0, 255, 0, 255]);
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(center(&mut group), [0, 0, 255, 255]);
    assert!(!group.animated());
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), allocations);
}

#[test]
#[ignore = "requires Vulkan"]
fn particle_light_tracks_script() {
    let scene = serde_json::json!({
        "general": {"orthogonalprojection":{"width":64,"height":64},
            "clearcolor":"0 0 0","lightconfig":{"point":1}},
        "objects": [
            {"id":1,"particle":"particles/lit.json","origin":"32 32 0","visible":true},
            {"id":2,"light":"lpoint","origin":"32 32 20","intensity":1,
                "radius":100,"exponent":2,"color":{"value":"1 0 0","script":
                    "export function update(value){return engine.runtime >= 1 ? new Vec3(0,0,0) : value;}"}}
        ]
    });
    let scene = scene.to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("particles/lit.json", br#"{"renderer":[{"name":"sprite"}],"maxcount":1,
            "emitter":[{"name":"boxrandom","rate":0,"instantaneous":1}],
            "initializer":[{"name":"lifetimerandom","min":10,"max":10},
                {"name":"sizerandom","min":32,"max":32}],
            "material":"materials/lit.json"}"#),
        ("materials/lit.json", br#"{"passes":[{"shader":"particle_light_probe",
            "blending":"translucent","textures":["util/white"]}]}"#),
        ("shaders/particle_light_probe.vert", b"attribute vec3 a_Position;\nattribute vec4 a_TexCoordVec4;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){vec2 p=a_Position.xy+(a_TexCoordVec4.xy-0.5)*a_TexCoordVec4.w*2.0;gl_Position=g_ModelViewProjectionMatrix*vec4(p,a_Position.z,1.0);}"),
        ("shaders/particle_light_probe.frag", b"uniform vec4 g_LPoint_Color[1];\nuniform vec4 g_LShadow_Size;\nuniform sampler2D g_Texture6;\nvoid main(){float shadow=texSample2D(g_Texture6,vec2(0.5)).r;gl_FragColor=vec4(g_LPoint_Color[0].rgb*(1.0-shadow*g_LShadow_Size.x*0.1),1.0);}"),
    ]);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.particles[0].engines.first().unwrap().shadow_slot, Some(6));
    group.compose(0.0, 0.0).unwrap();
    let lit = center(&mut group);
    assert!(lit[0] >= 200 && lit[1] == 0 && lit[2] == 0);
    group.compose(2.0, 0.0).unwrap();
    assert_eq!(group.particles[0].sim.particles.len(), 1);
    assert_eq!(center(&mut group), [0, 0, 0, 255]);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn particle_cookie_binds() {
    let scene = serde_json::json!({
        "general": {"orthogonalprojection":{"width":64,"height":64},
            "clearcolor":"0 0 0","lightconfig":{"spot":1,"spotcookie":1}},
        "objects": [
            {"id":1,"particle":"particles/cookie.json","origin":"32 32 0","visible":true},
            {"id":2,"light":"lspot","origin":"32 32 20","angles":"0 1.5707963267948966 0",
                "intensity":1,"radius":100,"exponent":2,"innercone":55,"outercone":60,
                "color":"1 1 1","castshadow":false,"usecookie":true,"cookie":"cookie-probe"}
        ]
    });
    let cookie = texture_levels(&[paper_scene::tex::MipLevel {
        width: 1,
        height: 1,
        data: vec![0, 255, 0, 255],
    }]);
    let scene = scene.to_string();
    let mut model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("particles/cookie.json", br#"{"renderer":[{"name":"sprite"}],"maxcount":1,
            "emitter":[{"name":"boxrandom","rate":0,"instantaneous":1}],
            "initializer":[{"name":"lifetimerandom","min":10,"max":10},
                {"name":"sizerandom","min":32,"max":32}],
            "material":"materials/cookie.json"}"#),
        ("materials/cookie.json", br#"{"passes":[{"shader":"particle_cookie_probe",
            "blending":"translucent","textures":["util/white"]}]}"#),
        ("materials/cookie-probe.tex", &cookie),
        ("shaders/particle_cookie_probe.vert", b"attribute vec3 a_Position;\nattribute vec4 a_TexCoordVec4;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){vec2 p=a_Position.xy+(a_TexCoordVec4.xy-0.5)*a_TexCoordVec4.w*2.0;gl_Position=g_ModelViewProjectionMatrix*vec4(p,a_Position.z,1.0);}"),
        ("shaders/particle_cookie_probe.frag", b"uniform sampler2D g_Texture7;\nuniform vec4 g_LSpot_Cookie[1];\nvoid main(){vec3 color=texSample2D(g_Texture7,vec2(0.5)).rgb;gl_FragColor=vec4(color*g_LSpot_Cookie[0].z,1.0);}"),
    ]);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.cookie_slot.is_some());
    assert_eq!(group.particles[0].engines.first().unwrap().cookie_slot, Some(7));
    group.compose(0.0, 0.0).unwrap();
    let [red, green, blue, _] = center(&mut group);
    assert!(green > red + 20 && green > blue + 20, "{red} {green} {blue}");
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn disabled_material_effect() {
    let shared = crate::shared::test_device();
    let targets = vk::Renderer::scene_target_live_allocations();
    let allocations = vk::Renderer::effect_live_allocations();
    let mut expected = None;
    for effects in [false, true] {
        let mut model = source_model(b"void main(){gl_FragColor=vec4(1.0);}", false);
        let layer = &mut model.layers[0];
        layer.color = [0.25, 0.75, 0.5];
        layer.texture.img_width = 24;
        layer.texture.img_height = 24;
        layer.size = (24.0, 24.0);
        layer.geometry.as_mut().unwrap().size = [24.0, 24.0];
        assert!(layer.material.is_some());
        if !effects {
            layer.effects.clear();
        }
        let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
        group.scripts = None;
        if effects {
            assert_eq!(group.fx.len(), 1);
            assert!(group.materials[0].target_bytes() > 0);
            let active = group.read_canvas().unwrap().2;
            for visible in [false, true, false] {
                group.fx[0].effects[0].visible = visible;
                group.compose(1.0, 0.0).unwrap();
                let pixels = group.read_canvas().unwrap().2;
                assert_eq!(&pixels, if visible { &active } else { expected.as_ref().unwrap() });
            }
        } else {
            expected = Some(group.read_canvas().unwrap().2);
        }
        group.destroy();
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
        assert_eq!(vk::Renderer::effect_live_allocations(), allocations);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn oversized_chain_admitted() {
    let extent = paper_scene::effects::FBO_MAX_EXTENT;
    let mut model = source_model(b"void main(){gl_FragColor=vec4(1.0);}", false);
    model.layers[0].texture.img_width = extent;
    model.layers[0].texture.img_height = extent;
    let shared = crate::shared::test_device();
    let group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(model.skipped.is_empty());
    assert_eq!(group.fx.len(), 1);
    let ping = group.fx[0].ping.get(&group.fx_pool).extent;
    assert_eq!((ping.width, ping.height), (extent, extent));
    assert!(group.effect_target_allocation_bytes() > 512 << 20);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn failed_target_skips_chain() {
    let shared = crate::shared::test_device();
    let models: [fn() -> SceneModel; 4] = [
        || source_model(b"void main(){gl_FragColor=vec4(1.0);}", false),
        || tinted_model(false),
        || tinted_model(true),
        || text_effect_model(TEXT_EFFECT_SCENE),
    ];
    let (mut built, mut leased) = (0, 0);
    for model_of in models {
        let mut model = model_of();
        let mut group =
            build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit).unwrap();
        group.compose(1.0, 0.1).unwrap();
        let reference = center(&mut group);
        group.destroy();
        for created in 0.. {
            let mut model = model_of();
            let targets = vk::Renderer::scene_target_live_allocations();
            let resources = vk::Renderer::effect_live_allocations();
            vk::Renderer::fail_scene_target_after(Some(created));
            let built_group = build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit);
            let pending = vk::Renderer::scene_target_failure_pending();
            vk::Renderer::fail_scene_target_after(None);
            let Ok(mut group) = built_group else {
                assert!(!pending, "failure {created}");
                assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
                continue;
            };
            if pending {
                group.destroy();
                break;
            }
            assert_eq!(model.skipped.len(), 1, "failure {created}: {:?}", model.skipped);
            if group.fx.is_empty() {
                built += 1;
                assert!(model.skipped[0].contains("injected scene target allocation failure"));
            } else {
                leased += 1;
                assert!(model.skipped[0].ends_with(STARVED_CHAIN));
                assert!(group.fx[0].starved.is_some() && group.fx[0].output.is_none());
                group.compose(1.0, 0.1).unwrap();
                assert!(group.fx[0].starved.is_none() && group.fx[0].output.is_some());
                assert_eq!(center(&mut group), reference, "failure {created}");
            }
            group.destroy();
            assert_eq!(vk::Renderer::scene_target_live_allocations(), targets, "failure {created}");
            assert_eq!(vk::Renderer::effect_live_allocations(), resources, "failure {created}");
        }
    }
    assert!(built >= 4 && leased >= 5, "built {built} leased {leased}");
}

fn tinted_model(solid: bool) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64","color":"0.5 0.25 1","effects":[{"file":"effects/test.json"}]}
        ]
    });
    let mut model = model_scene(
        scene,
        b"uniform float g_AudioSpectrum16Left[16];\nvoid main(){\ngl_FragColor=vec4(1.0,1.0,1.0,1.0+g_AudioSpectrum16Left[0]*0.0);\n}",
    );
    model.layers[0].solid = solid;
    assert!(model.scripts.is_none());
    model
}

#[test]
#[ignore = "requires Vulkan"]
fn failed_target_strict() {
    let shared = crate::shared::test_device();
    let mut model = source_model(b"void main(){gl_FragColor=vec4(1.0);}", false);
    let targets = vk::Renderer::scene_target_live_allocations();
    vk::Renderer::fail_scene_target_after(Some(2));
    let error = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).err().unwrap();
    assert!(format!("{error:#}").ends_with(STARVED_CHAIN));
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}

const TEXT_FONT: &[u8] = include_bytes!("../../../../paper-scene/src/fixtures/fonts/latin.ttf");

pub(super) const TEXT_EFFECT_SCENE: &str = r#"{"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":1,"text":"A","font":"fonts/Latin.ttf","pointsize":{"value":16,"script":"export function update(){return engine.runtime>=1?24:16;}"},"origin":"32 32 0","opaquebackground":true,"effects":[{"file":"effects/text.json"}]}]}"#;

pub(super) fn text_effect_model(scene: &str) -> SceneModel {
    model_files(&[
        ("scene.json", scene.as_bytes()),
        ("fonts/Latin.ttf", TEXT_FONT),
        ("effects/text.json", br#"{"fbos":[{"name":"cache","width":16,"height":16,"format":"rgba8888"}],"passes":[{"material":"materials/capture.json","target":"cache"},{"material":"materials/show.json","bind":[{"name":"cache","index":0}]}]}"#),
        ("materials/capture.json", br#"{"passes":[{"shader":"capture","blending":"normal"}]}"#),
        ("shaders/capture.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/capture.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
        ("materials/show.json", br#"{"passes":[{"shader":"show","blending":"normal"}]}"#),
        ("shaders/show.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/show.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
    ])
}

#[test]
#[ignore = "requires Vulkan"]
fn failed_resize_keeps_targets() {
    let mut model = text_effect_model(TEXT_EFFECT_SCENE);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.fx.len(), 1, "{:?}", model.skipped);
    let layer = group.fx[0].quad;
    let ping = group.fx[0].ping.get(&group.fx_pool);
    let (view, extent) = (ping.view, (ping.extent.width, ping.extent.height));
    let bytes = group.effect_target_allocation_bytes();
    let images = group.renderer.scene_image_allocations();
    let targets = vk::Renderer::scene_target_live_allocations();
    for created in 0..group.fx[0].target_ids().count() {
        vk::Renderer::fail_scene_target_after(Some(created));
        let size = (extent.0 / 2, extent.1);
        assert!(group.resize_text_effect(layer, size).is_err(), "failure {created}");
        let ping = group.fx[0].ping.get(&group.fx_pool);
        assert_eq!((ping.view, (ping.extent.width, ping.extent.height)), (view, extent));
        assert_eq!(group.effect_target_allocation_bytes(), bytes);
        if created == 0 {
            assert_eq!(group.renderer.scene_image_allocations(), images);
        }
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets, "failure {created}");
    }
    vk::Renderer::fail_scene_target_after(None);
    group.resize_text_effect(layer, (extent.0 / 2, extent.1)).unwrap();
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn frametime_effect() {
    let mut model = model(serde_json::json!(true), b"uniform float g_Frametime;\nvoid main(){\nfloat active=step(0.02,g_Frametime);\ngl_FragColor=vec4(1.0-active,active,0.0,1.0);\n}");
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.animated());
    assert_eq!(group.fx.len(), 1);
    for (dt, expected) in
        [(0.0, [255, 0, 0, 255]), (0.03125, [0, 255, 0, 255]), (0.0, [255, 0, 0, 255])]
    {
        group.compose(1.0, dt).unwrap();
        let (width, height, pixels) = group.read_canvas().unwrap();
        let center = ((height / 2 * width + width / 2) * 4) as usize;
        assert_eq!(&pixels[center..center + 4], &expected);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn puppet_clock_scheduling() {
    let shared = crate::shared::test_device();
    for (visible, rate, blend, spring, empty, animated) in [
        (true, 0.0, 1.0, false, true, false),
        (true, 0.0, 1.0, false, false, false),
        (true, 1.0, 0.0, false, false, false),
        (false, 1.0, 1.0, false, false, false),
        (true, 1.0, 1.0, false, false, true),
        (true, 0.0, 0.0, true, false, true),
    ] {
        let mut source = model(serde_json::json!(true), b"void main(){gl_FragColor=vec4(1.0);}");
        let layer = &mut source.layers[0];
        layer.effects.clear();
        layer.material = None;
        let mut mesh = paper_scene::puppet::parse(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../paper-scene/src/fixtures/a02/clock-step.mdl"
        )))
        .unwrap();
        if spring {
            mesh.springs.push(paper_scene::puppet::spring::Definition {
                bone: 0,
                translation: Some(paper_scene::puppet::spring::Translation {
                    stiffness: 1.0,
                    friction: 1.0,
                    inertia: 1.0,
                    maximum: 0.0,
                }),
                rotation: None,
                gravity: None,
            });
        }
        layer.puppet = Some(paper_scene::model::Puppet {
            cull: false,
            cpu: false,
            planar_owner: true,
            world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
            mesh,
            layers: vec![paper_scene::puppet::AnimationLayer {
                id: 220,
                rate,
                blend,
                additive: false,
                visible,
            }],
            size: (64.0, 64.0),
            mask_textures: Vec::new(),
        });
        if empty {
            layer.puppet.as_mut().unwrap().layers.clear();
        }
        let mut group =
            build_group(&shared, &mut source, true, &[(64, 64)], FillMode::Fill).unwrap();
        assert!(group.materials.is_empty());
        assert_eq!(group.animated(), animated);
        if animated {
            assert_eq!(group.puppets.len(), 1);
            assert_eq!(group.puppets[0].spring.is_some(), spring);
        }
        group.compose(0.0, 0.0).unwrap();
        let initial = group.read_canvas().unwrap().2;
        group.compose(2.0, 0.0).unwrap();
        if !animated {
            assert_eq!(group.read_canvas().unwrap().2, initial);
        }
        group.destroy();
    }
}

fn pooled_pair() -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64",
                "effects":[{"file":"effects/local.json","passes":[{"constantshadervalues":{"color":"1 0 0"}},{}]}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64",
                "effects":[{"file":"effects/local.json","passes":[{"constantshadervalues":{"color":"0 1 0"}},{}]}]},
            {"id":3,"alpha":{"value":1,"script":"export function update(value){return value;}"}}
        ]
    })
    .to_string();
    let vertex = b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";
    model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/local.json", br#"{"fbos":[{"name":"local","format":"rgba8888"}],"passes":[{"material":"materials/color.json","target":"local"},{"material":"materials/copy.json","bind":[{"index":0,"name":"local"}]}]}"#),
        ("materials/color.json", br#"{"passes":[{"shader":"color"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/color.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/copy.vert", vertex),
        ("shaders/color.frag", b"uniform vec3 g_Color; // {\"material\":\"color\"}\nvoid main(){gl_FragColor=vec4(g_Color,1.0);}"),
        ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5));}"),
    ])
}

fn halves(group: &mut Group) -> [[u8; 4]; 2] {
    let (width, _, rgba) = group.read_canvas().unwrap();
    [16, 48].map(|x| {
        let offset = ((32 * width + x) * 4) as usize;
        rgba[offset..offset + 4].try_into().unwrap()
    })
}

#[test]
#[ignore = "requires Vulkan"]
fn pooled_chains_keep_outputs() {
    let shared = crate::shared::test_device();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut model = pooled_pair();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.fx.len(), 2);
    assert!(group.fx.iter().all(|fx| matches!(fx.fbos[0].1, FxTargetStorage::Pooled(_))));
    assert_eq!(group.fx.iter().map(LayerFx::allocation_bytes).sum::<u64>(), 0);
    group.compose(1.0, 0.1).unwrap();
    assert_eq!(halves(&mut group), [[255, 0, 0, 255], [0, 255, 0, 255]]);
    let owned = 3 * group.fx.len();
    assert!(group.fx_pool.len() < owned, "pool {}", group.fx_pool.len());
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}

#[test]
#[ignore = "requires Vulkan"]
fn bloom_off_trims_pool() {
    let shared = crate::shared::test_device();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut model = pooled_pair();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(1.0, 0.1).unwrap();
    group.fx[0].layer_id = paper_scene::model::bloom::ID.to_owned();
    let pooled = group.fx_pool.len();
    let live = vk::Renderer::scene_target_live_allocations();
    let host = group.scripts.as_ref().and_then(|scripts| scripts.host.as_ref()).unwrap();
    let (scene, properties) = (host.scene.clone(), host.properties.clone());
    let resources = host.image_resources().unwrap();
    group.sync_bloom(&scene, &properties, &resources, &mut Vec::new()).unwrap();
    assert_eq!(group.fx.len(), 1);
    let trimmed = group.fx_pool.len();
    assert!(trimmed < pooled, "pool {trimmed}");
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live - (pooled - trimmed));
    group.compose(1.1, 0.1).unwrap();
    assert_eq!(halves(&mut group)[1], [0, 255, 0, 255]);
    assert_eq!(group.fx_pool.len(), trimmed);
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}

const POISON: [f32; 4] = [1.0; 4];

#[test]
#[ignore = "requires Vulkan"]
fn fresh_lease_cleared() {
    let shared = crate::shared::test_device();
    let mut model = pooled_pair();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    for batched in [false, true] {
        let mut pool = FxPool::default();
        let first = pool.lease(&mut group.renderer, &group.fx[0].pooled_classes()).unwrap();
        for lease in &first {
            let target = pool.target(*lease).unwrap();
            group.renderer.render_scene(target, POISON, &[], &[]).unwrap();
        }
        pool.release(&first);
        if batched {
            group.renderer.begin_scene_batch().unwrap();
        }
        fx_pool::lease_chain(&mut pool, &mut group.renderer, &mut group.fx[0], batched).unwrap();
        if batched {
            group.renderer.submit_scene_batch().unwrap();
        }
        let fx = &group.fx[0];
        let leased: Vec<_> = fx.storages().filter_map(|storage| storage.lease()).collect();
        assert_eq!(leased.len(), pool.len());
        assert!(matches!(fx.fbos[0].1, FxTargetStorage::Pooled(_)));
        for lease in leased {
            let target = pool.target(lease).unwrap();
            let (_, _, pixels) = group.renderer.read_scene_target(target).unwrap();
            assert!(
                pixels.iter().all(|byte| *byte == 0),
                "batched {batched} lease {}",
                lease.index
            );
        }
        pool.destroy(&mut group.renderer);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn scripted_tint_resumes() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64","color":"0.5 0.25 1","effects":[{"file":"effects/test.json"}]},
            {"id":2,"alpha":{"value":1,"script":"export function update(value){return value;}"}}
        ]
    });
    let mut model = model_scene(
        scene,
        b"uniform float g_Time;\nvoid main(){\ngl_FragColor=vec4(1.0+g_Time*0.0);\n}",
    );
    assert!(model.scripts.is_some());
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(1.0, 0.1).unwrap();
    let quad = group.fx[0].quad;
    let tint = group.quads[quad].tint;
    assert_ne!(tint, group.fx[0].fallback_tint);
    group.fx[0].starved = Some(Starvation { wait: 1, backoff: 1 });
    group.compose(1.1, 0.1).unwrap();
    assert_eq!(group.quads[quad].tint, group.fx[0].fallback_tint);
    group.compose(1.2, 0.1).unwrap();
    assert!(group.fx[0].starved.is_none());
    group.compose(1.3, 0.1).unwrap();
    assert_eq!(group.quads[quad].tint, tint);
    group.destroy();
}

fn layer_pair(effect: &[u8], color: &[u8], show: &[u8]) -> SceneModel {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64",
                "effects":[{"file":"effects/pair.json","passes":[{"constantshadervalues":{"color":"1 0 0"}},{},{}]}]},
            {"id":2,"image":"models/flat.json","origin":"48 32 0","size":"32 64",
                "effects":[{"file":"effects/pair.json","passes":[{"constantshadervalues":{"color":"0 1 0"}},{},{}]}]},
            {"id":3,"origin":{"value":"0 0 0","animation":{"options":{"fps":30,"length":30,"mode":"single"},"c0":[{"frame":0,"value":0},{"frame":30,"value":1}]}}}
        ]
    })
    .to_string();
    let vertex = b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";
    let direct = b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
    let copy = b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5));}";
    model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/pair.json", effect),
        ("materials/color.json", br#"{"passes":[{"shader":"color"}]}"#),
        ("materials/store.json", br#"{"passes":[{"shader":"store"}]}"#),
        ("materials/show.json", br#"{"passes":[{"shader":"show"}]}"#),
        ("shaders/color.vert", direct),
        ("shaders/store.vert", direct),
        ("shaders/show.vert", vertex),
        ("shaders/color.frag", color),
        ("shaders/store.frag", copy),
        ("shaders/show.frag", show),
    ])
}

#[test]
#[ignore = "requires Vulkan"]
fn feedback_layers_isolated() {
    let mut model = layer_pair(
        br#"{"fbos":[{"name":"history","format":"rgba8888"},{"name":"sum","format":"rgba8888"}],"passes":[{"material":"materials/color.json","target":"sum","bind":[{"index":0,"name":"history"}]},{"material":"materials/store.json","target":"history","bind":[{"index":0,"name":"sum"}]},{"material":"materials/show.json","bind":[{"index":0,"name":"history"}]}]}"#,
        b"uniform sampler2D g_Texture0;\nuniform vec3 g_Color; // {\"material\":\"color\"}\nvoid main(){gl_FragColor=vec4(texSample2D(g_Texture0,vec2(0.5)).rgb+g_Color*0.25,1.0);}",
        b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5));}",
    );
    let shared = crate::shared::test_device();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.fx.len(), 2);
    assert!(group.fx.iter().all(|fx| fx.fbos[0].1.owned().is_some()));
    group.compose(1.0, 0.1).unwrap();
    let [first, second] =
        [&group.fx[0], &group.fx[1]].map(|fx| fx.fbos[1].1.lease().map(|lease| lease.index));
    assert!(first.is_some() && first == second);
    let [red, green] = halves(&mut group);
    assert!(red[0].abs_diff(128) <= 1 && red[1] == 0, "{red:?}");
    assert!(green[1].abs_diff(128) <= 1 && green[0] == 0, "{green:?}");
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}

#[test]
#[ignore = "requires Vulkan"]
fn discarding_fbo_owned() {
    let mut model = layer_pair(
        br#"{"fbos":[{"name":"local","format":"rgba8888","clear":"0 0 1 1"},{"name":"out","format":"rgba8888"}],"passes":[{"material":"materials/color.json","target":"local"},{"material":"materials/store.json","target":"out","bind":[{"index":0,"name":"local"}]},{"material":"materials/show.json","bind":[{"index":0,"name":"out"}]}]}"#,
        b"uniform vec3 g_Color; // {\"material\":\"color\"}\nvoid main(){\nif(g_Color.g>0.5) discard;\ngl_FragColor=vec4(g_Color,1.0);\n}",
        b"uniform sampler2D g_Texture0;\nuniform float g_Time;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5))+vec4(g_Time*0.0);}",
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.fx.iter().all(|fx| fx.fbos[0].1.owned().is_some()));
    for time in [1.0, 2.0] {
        group.compose(time, 0.1).unwrap();
        assert_eq!(halves(&mut group), [[255, 0, 0, 255], [0, 0, 255, 255]]);
    }
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn hidden_layer_detached() {
    let mut model = model(
        serde_json::json!({"value":true,"script":"export function update(v){return engine.runtime<1.0;}"}),
        b"uniform float g_Time;\nvoid main(){\ngl_FragColor=vec4(1.0+g_Time*0.0);\n}",
    );
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(group.fx[0].output.is_some());
    group.compose(1.5, 0.1).unwrap();
    let fx = &group.fx[0];
    assert!(fx.output.is_none() && group.textures[fx.slot].effect.is_none());
    assert_eq!(group.quads[fx.quad].texture, fx.base_quad.texture);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn hidden_chain_strict() {
    let shared = crate::shared::test_device();
    let mut starved = 0;
    for created in 0.. {
        let mut model = model(
            serde_json::json!({"value":false,"script":"export function update(v){return engine.runtime>=1.0;}"}),
            b"void main(){\ngl_FragColor=vec4(1.0);\n}",
        );
        let targets = vk::Renderer::scene_target_live_allocations();
        vk::Renderer::fail_scene_target_after(Some(created));
        let built = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit);
        let pending = vk::Renderer::scene_target_failure_pending();
        vk::Renderer::fail_scene_target_after(None);
        match built {
            Ok(group) => {
                group.destroy();
                assert!(pending, "failure {created} hidden");
                break;
            }
            Err(error) => starved += usize::from(format!("{error:#}").ends_with(STARVED_CHAIN)),
        }
        assert_eq!(vk::Renderer::scene_target_live_allocations(), targets, "failure {created}");
    }
    assert!(starved >= 1);
}

#[test]
#[ignore = "requires Vulkan"]
fn crop_never_presented() {
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[
            {"id":1,"image":"models/flat.json","origin":"16 32 0","size":"32 64","effects":[{"file":"effects/test.json"}]},
            {"id":2,"origin":{"value":"0 0 0","animation":{"options":{"fps":30,"length":30,"mode":"single"},"c0":[{"frame":0,"value":0},{"frame":30,"value":1}]}}}
        ]
    });
    let mut model = model_scene(
        scene,
        b"uniform sampler2D g_Texture0;\nvoid main(){\ngl_FragColor=vec4(texSample2D(g_Texture0,vec2(0.5)).rgb+vec3(0.5,0.0,0.0),1.0);\n}",
    );
    model.clear = [0.0, 0.25, 0.0];
    model.layers[0].effects[0].passes[0].binds = vec![(0, EffectBind::SceneUnderLayer)];
    let shared = crate::shared::test_device();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert!(!group.frozen && group.fx.is_empty());
    assert!(group.baked_fx_draws.is_empty());
    assert_eq!(group.baked_fx_outputs.len(), 1);
    for time in [0.5, 1.0] {
        group.compose(time, 0.1).unwrap();
        let [pixel, _] = halves(&mut group);
        assert_eq!((pixel[0].abs_diff(128) <= 1, pixel[1]), (true, 64), "{pixel:?}");
    }
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}
