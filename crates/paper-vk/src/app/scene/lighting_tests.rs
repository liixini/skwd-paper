use super::*;

fn native_pixels(root: &std::path::Path, fixture: &str) -> Result<image::RgbImage> {
    use sha2::Digest;
    use std::io::Read;
    let folder = root.join(format!("{fixture}-native"));
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("at12s.window.json"))?)?;
    assert_eq!(identity["private_prefix"], true);
    assert_eq!(identity["private_engine"], true);
    assert!(identity["pid"].as_u64().is_some_and(|pid| pid > 0));
    let early: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("at8s.window.json"))?)?;
    assert_eq!(early["pid"], identity["pid"]);
    assert_eq!(early["process_start_ticks"], identity["process_start_ticks"]);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("run.json"))?)?;
    let mut package = std::fs::File::open(root.join(fixture).join("scene.pkg"))?;
    let mut hash = sha2::Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = package.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    assert_eq!(receipt["project_sha256"], format!("{:x}", hash.finalize()));
    Ok(image::open(folder.join("at12s.png"))?.to_rgb8())
}

fn native_shadow_frame(
    root: &std::path::Path,
    fixture: &str,
    image: &image::RgbaImage,
) -> Result<()> {
    let native = native_pixels(root, fixture)?;
    assert_eq!(image.dimensions(), native.dimensions());
    for (actual, expected) in image.pixels().zip(native.pixels()) {
        for channel in 0..3 {
            assert!(
                actual[channel].abs_diff(expected[channel]) <= 6,
                "{fixture}: {actual:?} {expected:?}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native volume fog fixtures"]
fn volume_fog() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    for fixture in ["fog-distance", "fog-height", "fog-both"] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        assert!(image.pixels().any(|pixel| pixel[0] > 0), "{fixture}");
        let native = native_pixels(&root, fixture)?;
        for y in [440, 540, 640] {
            for x in [800, 960, 1120] {
                let expected = native.get_pixel(x as u32, y as u32);
                let actual = image.get_pixel(x as u32, y as u32);
                for channel in 0..3 {
                    assert!(
                        actual[channel].abs_diff(expected[channel]) <= 6,
                        "{fixture} {x},{y}: {actual:?} {expected:?}"
                    );
                }
            }
        }
        group.destroy();
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and live cookie fixtures"]
fn cookie_toggle() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    for fixture in ["cookie-toggle", "volume-cookie-toggle"] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let textures = vk::Renderer::scene_texture_live_allocations();
        let slots = group.textures.len();
        for _ in 0..3 {
            for (time, cookie) in [(0.5, 0), (1.5, 1), (2.5, 2), (3.5, 0)] {
                group.compose(time, 1.0)?;
                let (_, _, pixels) = group.read_canvas()?;
                let pixel = &pixels[(540 * 1920 + 600) * 4..][..3];
                match cookie {
                    1 => assert!(pixel[0] > pixel[2] + 5, "{fixture} {time}: {pixel:?}"),
                    2 => assert!(pixel[2] > pixel[0] + 5, "{fixture} {time}: {pixel:?}"),
                    _ => assert_eq!(pixel[0], pixel[2], "{fixture} {time}: {pixel:?}"),
                }
                assert_eq!(group.textures.len(), slots);
                assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
            }
        }
        assert!(group.cookie_current.is_none());
        group.destroy();
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and live shadow fixtures"]
fn shadow_toggle() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    for fixture in ["shadow-toggle", "volume-shadow-toggle"] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let idle = group.shadows.as_ref().unwrap().allocation_bytes();
        for (time, shadowed) in [(0.5, false), (1.5, true), (2.5, false)] {
            group.compose(time, 1.0)?;
            let (_, _, pixels) = group.read_canvas()?;
            let value = pixels[(540 * 1920 + 1200) * 4];
            assert_eq!(value < 2, shadowed, "{fixture} {time}: {value}");
            let allocated = group.shadows.as_ref().unwrap().allocation_bytes();
            if shadowed {
                assert!(allocated > idle);
            } else {
                assert_eq!(allocated, idle);
            }
        }
        group.destroy();
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and alpha caster fixtures"]
fn shadow_alpha() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    let mut samples = Vec::new();
    for fixture in ["coverage-tangent-opaque", "coverage-tangent-clear"] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        native_shadow_frame(&root, fixture, &image)?;
        samples.push(image.get_pixel(1200, 540)[0]);
        image.save(root.join(format!("{fixture}-paper.png")))?;
        group.destroy();
    }
    assert!(samples[0] < 2 && samples[1] > 10, "{samples:?}");
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and live volume fixtures"]
fn volume_toggle() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let shared = crate::shared::test_device();
    for fixture in ["volume-toggle", "volume-toggle-bloom"] {
        let package = paper_scene::pkg::Package::open(&root.join(fixture).join("scene.pkg"))?;
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some(&assets)),
        )?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let idle = vk::Renderer::effect_live_allocations();
        for (time, visible) in [(0.5, false), (1.5, true), (2.5, false)] {
            group.compose(time, 1.0)?;
            let (_, _, pixels) = group.read_canvas()?;
            assert_eq!(pixels[(540 * 1920 + 960) * 4] > 0, visible, "{fixture}");
            assert_eq!(group.volumes.is_some(), visible);
        }
        assert_eq!(vk::Renderer::effect_live_allocations(), idle);
        group.destroy();
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and lighting resource fixture"]
fn lighting_resources() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let fixture = std::env::var("SKWD_WE_LIGHT_RESOURCE")?;
    let directory = root.join(&fixture);
    let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
    let shared = crate::shared::test_device();
    let live = vk::Renderer::scene_target_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
    for frame in 0..10 {
        group.compose(frame as f32 / 60.0, 1.0 / 60.0)?;
    }
    std::fs::write(root.join("resource.pid"), std::process::id().to_string())?;
    eprintln!("resource ready {fixture}");
    std::thread::sleep(std::time::Duration::from_secs(10));
    let started = std::time::Instant::now();
    for frame in 10..130 {
        group.compose(frame as f32 / 60.0, 1.0 / 60.0)?;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let status = std::fs::read_to_string("/proc/self/status")?;
    let resident = status
        .lines()
        .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "fixture":fixture,"pid":std::process::id(),"frames":120,"elapsed_seconds":elapsed,
        "main_target_bytes":group.target.allocation_bytes,
        "shadow_bytes":group.shadows.as_ref().map_or(0,shadow::Shadows::allocation_bytes),
        "volume_target_bytes":group.volumes.as_ref().map_or(0,volume::Volumes::allocation_bytes),
        "cookie_bytes":group.cookie_slot.and_then(|slot| group.textures.get(slot)).map_or(0, |texture| texture.allocation_bytes),
        "scene_target_count":vk::Renderer::scene_target_live_allocations()-live,"resident":resident
    });
    std::fs::write(
        root.join(format!("{fixture}-resources.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native spot cookie fixtures"]
fn native_cookies() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let shared = crate::shared::test_device();
    let mut differences = Vec::new();
    let mut allocations = Vec::new();
    for fixture in [
        "cookie-surface",
        "cookie-surface-shadow",
        "cookie-default",
        "cookie-volume",
        "cookie-volume-shadow",
        "cookie-multiple",
        "cookie-multiple-reverse",
    ] {
        let package = paper_scene::pkg::Package::open(&root.join(fixture).join("scene.pkg"))?;
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some(&assets)),
        )?;
        for material in model.meshes.iter().flat_map(|mesh| &mesh.materials) {
            let hlsl = material.pass.hlsl.as_ref().context("cookie HLSL source missing")?;
            paper_scene::hlsl::compile(&hlsl.vertex, paper_scene::shader::Stage::Vertex, fixture)?;
            paper_scene::hlsl::compile(
                &hlsl.fragment,
                paper_scene::shader::Stage::Fragment,
                fixture,
            )?;
        }
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let cookie = model.cookies.as_ref().unwrap();
        let source_bytes: usize =
            cookie.texture.pixels.levels.iter().map(|level| level.data.len()).sum();
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        allocations.push(serde_json::json!({
            "fixture":fixture,
            "source_bytes":source_bytes,
            "cookie_bytes":group.cookie_slot.and_then(|slot| group.textures.get(slot)).map_or(0, |texture| texture.allocation_bytes),
        }));
        let native = native_pixels(&root, fixture)?;
        for x in [600, 700, 800, 1200, 1300] {
            let expected = native.get_pixel(x as u32, 540);
            let actual = image.get_pixel(x as u32, 540);
            for channel in 0..3 {
                if actual[channel].abs_diff(expected[channel]) > 6 {
                    differences.push(format!("{fixture} x{x}: {actual:?} {expected:?}"));
                }
            }
        }
        group.destroy();
    }
    std::fs::write(root.join("cookie-allocations.json"), serde_json::to_vec_pretty(&allocations)?)?;
    assert!(differences.is_empty(), "{differences:?}");
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and volumetric bloom fixtures"]
fn volume_bloom() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let assets = std::env::var("SKWD_WE_ENGINE_ASSETS")?;
    let shared = crate::shared::test_device();
    let mut lit = Vec::new();
    for fixture in ["volume-bloom-control", "volume-bloom"] {
        let package = paper_scene::pkg::Package::open(&root.join(fixture).join("scene.pkg"))?;
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some(&assets)),
        )?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        group.compose(0.0, 0.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        lit.push(image.pixels().map(|pixel| u64::from(pixel[0])).sum::<u64>());
        group.destroy();
    }
    assert!(lit[1] > lit[0], "{lit:?}");
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and live light fixtures"]
fn surface_live_light() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let directory = root.join("surface-live");
    let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
    let mut values = Vec::new();
    for time in [0.5, 1.5] {
        group.compose(time, 0.5)?;
        let (_, _, pixels) = group.read_canvas()?;
        values.push(pixels[(540 * 1920 + 800) * 4]);
    }
    group.destroy();
    assert!(values[0] > 10, "{values:?}");
    assert_eq!(values[1], 0, "{values:?}");
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and shadow fixtures"]
fn volume_shadow_occluder() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    let mut samples = Vec::new();
    for fixture in ["shadow-control", "shadow-volume", "spot-volume-control", "spot-volume-shadow"]
    {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        native_shadow_frame(&root, fixture, &image)?;
        samples.push(image.get_pixel(1100, 540)[0]);
        group.destroy();
    }
    for pair in samples.chunks_exact(2) {
        assert!(pair[0] > 20, "{samples:?}");
        assert!(pair[1] < 2, "{samples:?}");
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and shadow fixtures"]
fn surface_shadow_occluder() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    let mut samples = Vec::new();
    for fixture in
        ["surface-control", "surface-shadow", "spot-surface-control", "spot-surface-shadow"]
    {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        assert_eq!(model.meshes.len(), 2, "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        assert!(group.meshes.iter().all(mesh::MeshActor::shadow_hlsl));
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        native_shadow_frame(&root, fixture, &image)?;
        samples.push(image.get_pixel(1200, 540)[0]);
        group.destroy();
    }
    for pair in samples.chunks_exact(2) {
        assert!(pair[0] > 10, "{samples:?}");
        assert!(pair[1] < 2, "{samples:?}");
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native shifted caster fixture"]
fn undeformed_shadow_caster() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let directory = root.join("surface-shifted");
    let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
    assert!(group.meshes.iter().all(mesh::MeshActor::shadow_hlsl));
    let (width, height, pixels) = group.read_canvas()?;
    let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
    image.save(root.join("surface-shifted-paper.png"))?;
    let native = native_pixels(&root, "surface-shifted")?;
    assert!(image.get_pixel(950, 540)[0] > 70);
    assert!(image.get_pixel(1400, 540)[0] < 2);
    assert!(image.get_pixel(1600, 540)[0] > 20);
    for (actual, expected) in image.pixels().zip(native.pixels()) {
        for channel in 0..3 {
            assert!(actual[channel].abs_diff(expected[channel]) <= 2);
        }
    }
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan"]
fn shadow_atlas_depth() -> Result<()> {
    use ash::vk as raw;
    use paper_scene::shader::{self, Stage};
    let shared = crate::shared::test_device();
    let mut renderer = shared_renderer(&shared, 64, 64)?;
    renderer.ensure_scene_pool(8)?;
    let mut atlas = vk::ShadowAtlas::new(&renderer, raw::Extent2D { width: 64, height: 64 })?;
    let positions = [-1.0_f32, -1.0, 0.75, 0.0, -1.0, 0.75, -1.0, 1.0, 0.75, 0.0, 1.0, 0.75];
    let indices = [0_u32, 1, 2, 2, 1, 3];
    let vertices = renderer
        .create_dyn_buffer(size_of_val(&positions), raw::BufferUsageFlags::VERTEX_BUFFER)?;
    let elements =
        renderer.create_dyn_buffer(size_of_val(&indices), raw::BufferUsageFlags::INDEX_BUFFER)?;
    renderer.write_dyn_buffer(
        &vertices,
        &positions.into_iter().flat_map(f32::to_ne_bytes).collect::<Vec<_>>(),
    );
    renderer.write_dyn_buffer(
        &elements,
        &indices.into_iter().flat_map(u32::to_ne_bytes).collect::<Vec<_>>(),
    );
    let vertex = shader::translate(
        "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
        Stage::Vertex,
        &BTreeMap::new(),
    );
    let fragment = shader::translate(
        "uniform sampler2D g_Texture0;\nvoid main(){float depth=texSample2D(g_Texture0,gl_FragCoord.xy/64.0).r;gl_FragColor=vec4(depth,depth,depth,1.0);}",
        Stage::Fragment,
        &BTreeMap::new(),
    );
    let pipeline = renderer.create_effect_pipeline(&vertex, &fragment, None, "shadow probe")?;
    let quad = renderer.create_quad_buffer_ndc()?;
    let target = renderer.create_scene_target(64, 64)?;
    renderer.begin_scene_batch()?;
    atlas.record(
        &renderer,
        &[vk::ShadowDraw {
            program: None,
            geometry: vk::MeshGeometry {
                vertices: vertices.buffer,
                indices: elements.buffer,
                index_type: raw::IndexType::UINT32,
                count: 6,
            },
            stride: 12,
            alpha: None,
            projection: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
            tile: raw::Rect2D { offset: raw::Offset2D::default(), extent: target.extent },
            revision: 0,
        }],
    )?;
    renderer.record_effect_pass(
        &pipeline,
        &quad,
        vk::EffectDraw { target: &target, preserve: false, final_vertices: false },
        &[atlas.sample()],
        &[],
    )?;
    renderer.submit_scene_batch()?;
    let (_, _, pixels) = renderer.read_scene_target(&target)?;
    assert_eq!(pixels[(32 * 64 + 16) * 4], 191);
    assert_eq!(pixels[(32 * 64 + 48) * 4], 0);
    assert!(atlas.allocation_bytes() >= 64 * 64 * 4);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native volumetric probe captures"]
fn native_volumes() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    let allocations = vk::Renderer::scene_target_live_allocations();
    for fixture in [
        "point",
        "point-steep",
        "spot",
        "occluder-front",
        "occluder-middle",
        "occluder-back",
        "volume-multiple",
        "volume-mixed",
    ] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        group.compose(0.0, 0.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        assert!(image.pixels().any(|pixel| pixel[0] > 0), "{fixture}");
        let native = native_pixels(&root, fixture)?;
        for offset in [-200, -100, 100, 200] {
            let expected = native.get_pixel((960 + offset) as u32, 540);
            let actual = image.get_pixel((960 + offset) as u32, 540);
            for channel in 0..3 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 6,
                    "{fixture}: {actual:?} {expected:?}"
                );
            }
        }
        group.destroy();
        assert_eq!(vk::Renderer::scene_target_live_allocations(), allocations);
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native spotlight probe captures"]
fn native_spot_uniforms() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_SPOT_PROBES")?);
    let live = vk::Renderer::scene_target_live_allocations();
    let shared = crate::shared::test_device();
    for (fixture, capture) in [
        ("query", "query-enabled"),
        ("defaults", "defaults-native"),
        ("rotate-y", "rotate-y-native"),
        ("rotate-z", "rotate-z-native"),
    ] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fit)?;
        group.compose(0.0, 0.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        let native = image::open(root.join(capture).join("at4s.png"))?.to_rgba8();
        for quarter in 0..4 {
            let actual = image.get_pixel((quarter * 2 + 1) * width / 8, height / 2);
            let expected =
                native.get_pixel((quarter * 2 + 1) * native.width() / 8, native.height() / 2);
            for channel in 0..3 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 1,
                    "{fixture}: {actual:?} vs {expected:?}"
                );
            }
        }
        group.destroy();
    }
    assert_eq!(vk::Renderer::scene_target_live_allocations(), live);
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native image caster controls"]
fn image_shadow_controls() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    for fixture in ["image-shadow", "puppet-flat"] {
        let mut captures = Vec::new();
        let mut references = Vec::new();
        for variant in ["off", "on"] {
            let name = format!("{fixture}-{variant}");
            let directory = root.join(&name);
            let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
            let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
            assert!(model.skipped.is_empty(), "{:?}", model.skipped);
            let mut group =
                build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
            group.compose(1.0, 1.0 / 60.0)?;
            let (width, height, pixels) = group.read_canvas()?;
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > 0));
            image::RgbaImage::from_raw(width, height, pixels.clone())
                .unwrap()
                .save(root.join(format!("{name}-paper.png")))?;
            let receiver = (540 * width as usize + 1500) * 4;
            assert!(pixels[receiver] > 10, "Paper receiver unlit: {name}");
            let native = native_pixels(&root, &name)?;
            assert!(native.get_pixel(1500, 540)[0] > 10, "native receiver unlit: {name}");
            captures.push(pixels);
            references.push(native);
            group.destroy();
        }
        assert_eq!(captures[0], captures[1], "Paper {fixture}");
        assert_eq!(references[0], references[1], "native {fixture}");
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native point projection fixtures"]
fn native_shadow_uniforms() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_WE_VOLUME_PROBES")?);
    let shared = crate::shared::test_device();
    for (fixture, expected) in [
        ("comparison-edges-v2", [0, 127, 255]),
        ("comparison-clear-v2", [0, 255, 255]),
        ("point-atlas-faces", [0, 0, 255]),
        ("point-coeff-default", [170, 13, 255]),
        ("point-coeff-small", [170, 13, 255]),
        ("point-coeff-large", [170, 13, 255]),
        ("spot-projection-shadow", [137, 132, 89]),
        ("spot-projection-cookie", [137, 132, 89]),
    ] {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        let native = native_pixels(&root, fixture)?;
        assert_eq!(native.get_pixel(600, 540).0, expected);
        for (actual, expected) in image.pixels().zip(native.pixels()) {
            for channel in 0..3 {
                assert!(actual[channel].abs_diff(expected[channel]) <= 1, "{fixture}");
            }
        }
        group.destroy();
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native skeletal fixtures"]
fn skeletal_shadow_caster() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_SKELETAL_PROBES")?);
    let shared = crate::shared::test_device();
    let fixtures = [
        "static-on",
        "static-off",
        "chest-rest-on",
        "chest-rest-off",
        "chest-pose-on",
        "chest-pose-off",
    ];
    for fixture in fixtures {
        let directory = root.join(fixture);
        let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
        assert_eq!(model.meshes.len(), 2, "{:?}", model.skipped);
        let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
        assert!(group.meshes.iter().all(mesh::MeshActor::shadow_hlsl));
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{fixture}-paper.png")))?;
        group.destroy();
    }
    for fixture in fixtures {
        let image = image::open(root.join(format!("{fixture}-paper.png")))?.to_rgba8();
        native_shadow_frame(&root, fixture, &image)?;
    }
    Ok(())
}
#[test]
#[ignore = "requires Vulkan and genuine Windmill fixtures"]
fn skeletal_playback() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("SKWD_SKELETAL_PROBES")?);
    let directory = root.join("windmill-rate1-on-layer-at2s-paper-input");
    let package = paper_scene::pkg::Package::open(&directory.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &directory)?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
    assert!(group.animated());
    assert_eq!(group.meshes.iter().filter(|actor| actor.animated()).count(), 2);
    let textures = vk::Renderer::scene_texture_live_allocations();
    let shadow_bytes = group.shadows.as_ref().unwrap().allocation_bytes();
    let (_, _, first) = group.read_canvas()?;
    group.compose(1.0, 1.0)?;
    let (_, _, moved) = group.read_canvas()?;
    assert_ne!(first, moved);
    for frame in 0..160 {
        group.compose(frame as f32 / 30.0, 1.0 / 30.0)?;
    }
    group.compose(160.0 / 30.0, 1.0 / 30.0)?;
    let (_, _, looped) = group.read_canvas()?;
    assert!(first.iter().zip(&looped).all(|(left, right)| left.abs_diff(*right) <= 6));
    assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
    assert_eq!(group.shadows.as_ref().unwrap().allocation_bytes(), shadow_bytes);
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native spring replay fixture"]
fn spring_native_replay() -> Result<()> {
    let package =
        paper_scene::pkg::Package::parse(std::fs::read(std::env::var("SKWD_SPRING_SCENE")?)?)?;
    let assets =
        paper_scene::effects::Assets::discover(Some(&std::env::var("SKWD_WE_ENGINE_ASSETS")?));
    let mut model = paper_scene::model::load_with(&package, &assets)?;
    assert!(
        model
            .layers
            .iter()
            .filter_map(|layer| layer.puppet.as_ref())
            .any(|puppet| !puppet.mesh.springs.is_empty())
    );
    let timings: Vec<[f32; 2]> =
        serde_json::from_slice(&std::fs::read(std::env::var("SKWD_SPRING_CLOCK")?)?)?;
    assert!(!timings.is_empty());
    let scriptless = std::env::var("SKWD_SPRING_ROUTE").ok();
    if scriptless.is_some() {
        assert!(model.scripts.is_none());
    }
    let before = vk::Renderer::scene_target_live_allocations();
    let shared = crate::shared::test_device();
    let mut group = build_group(&shared, &mut model, true, &[(1920, 1080)], FillMode::Fill)?;
    if let Some(route) = &scriptless {
        assert!(group.animated());
        match route.as_str() {
            "cpu" => {
                assert_eq!(group.puppets.len(), 1);
                assert!(group.puppets[0].spring.is_some());
                assert!(group.materials.is_empty());
            }
            "material" => {
                assert!(group.puppets.is_empty());
                assert_eq!(group.materials.len(), 1);
                assert!(group.materials[0].has_spring());
            }
            _ => panic!("unknown spring route"),
        }
    }
    let mut targets = None;
    for [time, dt] in timings {
        assert!(time.is_finite() && dt.is_finite() && time >= 0.0 && dt >= 0.0);
        group.compose(time, dt)?;
        let current = vk::Renderer::scene_target_live_allocations();
        assert_eq!(*targets.get_or_insert(current), current);
    }
    if scriptless.is_none() {
        let host = group
            .scripts
            .as_ref()
            .and_then(|scripts| scripts.host.as_ref())
            .context("spring replay ScriptHost missing")?;
        std::fs::write(
            std::env::var("SKWD_SPRING_DIAGNOSTICS")?,
            serde_json::to_vec_pretty(&host.diagnostics)?,
        )?;
    }
    if let Ok(path) = std::env::var("SKWD_SPRING_CAPTURE") {
        let (width, height, rgba) = group.read_canvas()?;
        image::RgbaImage::from_raw(width, height, rgba)
            .context("spring canvas size")?
            .save(path)?;
    }
    group.destroy();
    assert_eq!(vk::Renderer::scene_target_live_allocations(), before);
    Ok(())
}

fn sun_pixel(intensity: f32) -> [u8; 3] {
    const FRAGMENT: &[u8] = b"#include \"common.h\"\nvec3 ComputePBRLightShadowInfinite(vec3 n, vec3 l, vec3 v, vec3 a, vec3 c, vec3 s, vec3 b, float r, float m, float f) { return c; }\n#require LightingV1\nvoid main() { gl_FragColor = vec4(PerformLighting_V1(CAST3(0.0), CAST3(1.0), CAST3(0.0), CAST3(0.0), CAST3(1.0), CAST3(0.0), 0.5, 0.0), 1.0); }\n";
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0","lightconfig":{"directional":1}},
        "objects":[
            {"id":1,"light":"ldirectional","color":"1 0 0","intensity":intensity},
            {"id":2,"image":"models/flat.json","origin":"32 32 0","size":"64 64","effects":[{"file":"effects/sun.json"}]}
        ]
    })
    .to_string();
    let mut model = super::effect_lifetime_tests::model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/sun.json", br#"{"passes":[{"material":"materials/sun.json"}]}"#),
        ("materials/sun.json", br#"{"passes":[{"shader":"sun","combos":{"LIGHTING":1}}]}"#),
        ("shaders/sun.vert", super::effect_space_tests::PASS_VERTEX),
        ("shaders/sun.frag", FRAGMENT),
    ]);
    let shared = crate::shared::test_device();
    let mut group =
        build_group(&shared, &mut model, true, &[(64, 64)], paper_geom::FillMode::Fill).unwrap();
    group.compose(0.0, 1.0 / 60.0).unwrap();
    let (width, _, rgba) = group.read_canvas().unwrap();
    group.destroy();
    let offset = ((32 * width + 32) as usize) * RGBA_CHANNELS;
    [rgba[offset], rgba[offset + 1], rgba[offset + 2]]
}

#[test]
#[ignore = "requires Vulkan"]
fn sun_lights_output() {
    assert_eq!(sun_pixel(1.0), [255, 0, 0]);
    assert_eq!(sun_pixel(0.0), [0, 0, 0]);
}

const SUN_SIZE: u32 = 64;
const SHADED_PIXEL: (usize, usize) = (26, 35);
const LIT_PIXEL: (usize, usize) = (53, 18);

pub(in crate::app::scene) fn scripted_intensity() -> serde_json::Value {
    serde_json::json!({"value":1.0,"script":"export function update(value) { return value; }"})
}

pub(in crate::app::scene) fn sun_shadow_model(
    caster_shadow: bool,
    intensity: serde_json::Value,
) -> SceneModel {
    const VERTEX: &[u8] = b"uniform mat4 g_ModelMatrix;\nuniform mat4 g_ViewProjectionMatrix;\nattribute vec3 a_Position;\nvarying vec3 v_World;\nvoid main(){vec4 world=mul(vec4(a_Position,1.0),g_ModelMatrix);v_World=world.xyz;gl_Position=mul(world,g_ViewProjectionMatrix);}";
    const FRAGMENT: &[u8] = b"#include \"common.h\"\nvec3 ComputePBRLightShadowInfinite(vec3 n, vec3 l, vec3 v, vec3 a, vec3 c, vec3 s, vec3 b, float r, float m, float f) { return c * f; }\n#require LightingV1\nvarying vec3 v_World;\nvoid main(){gl_FragColor=vec4(PerformLighting_V1(v_World,CAST3(1.0),vec3(0.0,0.0,1.0),vec3(0.0,0.0,1.0),CAST3(0.0),CAST3(0.0),0.5,0.0),1.0);}";
    let scene = serde_json::json!({
        "camera":{"eye":"0 0 4","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":100,"lightconfig":{"directional":1,"directionalshadow":1}},
        "objects":[
            {"id":1,"light":"ldirectional","angles":"0 1.2 0","color":"1 0 0","castshadow":true,"intensity":intensity},
            {"id":2,"model":"models/mesh.mdl","origin":"5 5 0","scale":"10 10 1","castshadow":false},
            {"id":3,"model":"models/mesh.mdl","origin":"-1 0 2","scale":"0.5 0.5 1","castshadow":caster_shadow}
        ]
    })
    .to_string();
    let mesh = super::tests::triangle_mdl();
    let model = super::effect_lifetime_tests::model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/mesh.mdl", &mesh),
        ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","depthtest":"enabled","depthwrite":"enabled","cullmode":"nocull","blending":"normal","combos":{"LIGHTING":1}}]}"#),
        ("shaders/mesh.vert", VERTEX),
        ("shaders/mesh.frag", FRAGMENT),
    ]);
    assert_eq!(model.meshes.len(), 2, "{:?}", model.skipped);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    model
}

pub(in crate::app::scene) fn sun_group(
    shared: &crate::shared::SharedDevice,
    model: &mut SceneModel,
) -> Group {
    build_group(shared, model, true, &[(SUN_SIZE, SUN_SIZE)], FillMode::Fill).unwrap()
}

fn red_at(rgba: &[u8], (x, y): (usize, usize)) -> u8 {
    rgba[(y * SUN_SIZE as usize + x) * RGBA_CHANNELS]
}

#[test]
#[ignore = "requires Vulkan"]
fn sun_caster_darkens() {
    let shared = crate::shared::test_device();
    for intensity in [scripted_intensity(), serde_json::json!(1.0)] {
        let mut samples = Vec::new();
        for caster_shadow in [true, false] {
            let mut model = sun_shadow_model(caster_shadow, intensity.clone());
            let mut group = sun_group(&shared, &mut model);
            group.compose(0.0, 1.0 / 60.0).unwrap();
            let (_, _, rgba) = group.read_canvas().unwrap();
            samples.push((red_at(&rgba, SHADED_PIXEL), red_at(&rgba, LIT_PIXEL)));
            group.destroy();
        }
        assert_eq!(samples, [(0, 255), (255, 255)]);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn idle_sun_atlas() {
    let shared = crate::shared::test_device();
    let mut model = sun_shadow_model(true, scripted_intensity());
    let mut group = sun_group(&shared, &mut model);
    for frame in 0..4 {
        group.compose(frame as f32 / 60.0, 1.0 / 60.0).unwrap();
    }
    assert_eq!(group.shadows.as_ref().unwrap().renders, 1);
    group.orient_camera([3.0, 0.0, 4.0], [3.0, 0.0, 0.0], [0.0, 1.0, 0.0], None);
    group.compose(4.0 / 60.0, 1.0 / 60.0).unwrap();
    assert_eq!(group.shadows.as_ref().unwrap().renders, 2);
    group.destroy();
}

#[test]
#[ignore = "requires Vulkan"]
fn sun_atlas_freed() {
    let shared = crate::shared::test_device();
    let atlases = vk::ShadowAtlas::live_allocations();
    let textures = vk::Renderer::scene_texture_live_allocations();
    let targets = vk::Renderer::scene_target_live_allocations();
    let mut model = sun_shadow_model(true, scripted_intensity());
    let mut group = sun_group(&shared, &mut model);
    group.compose(0.0, 1.0 / 60.0).unwrap();
    assert_eq!(vk::ShadowAtlas::live_allocations(), atlases + 1);
    group.destroy();
    assert_eq!(vk::ShadowAtlas::live_allocations(), atlases);
    assert_eq!(vk::Renderer::scene_texture_live_allocations(), textures);
    assert_eq!(vk::Renderer::scene_target_live_allocations(), targets);
}
