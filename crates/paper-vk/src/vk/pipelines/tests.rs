use crate::vk::{Renderer, Src};

#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn limited_bt709_shader_preserves_primary_and_neutral_pixels() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    let upload = renderer.create_upload_path(4, 4)?;
    let target = renderer.create_video_texture_target(4, 4, true, true)?;
    let fixtures = [
        ([63, 102, 240], [255, 0, 0]),
        ([173, 42, 26], [0, 255, 0]),
        ([32, 240, 118], [0, 0, 255]),
        ([16, 128, 128], [0, 0, 0]),
        ([126, 128, 128], [128, 128, 128]),
        ([235, 128, 128], [255, 255, 255]),
    ];
    let result = (|| {
        for ([y, u, v], expected) in fixtures {
            renderer.upload_nv12(&upload, &[y; 16], 4, &[u, v, u, v, u, v, u, v], 4)?;
            renderer
                .render_video_texture(&target, &Src::Views(upload.luma_view, upload.chroma_view))?;
            let (width, height, pixels) = renderer.read_scene_target(&target)?;
            anyhow::ensure!((width, height) == (4, 4), "unexpected readback dimensions");
            anyhow::ensure!(pixels.len() == 4 * 4 * 4, "truncated pixel readback");
            for pixel in pixels.chunks_exact(4) {
                let actual = [pixel[2], pixel[1], pixel[0]];
                anyhow::ensure!(
                    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                    "NV12 {y},{u},{v}: expected RGB {expected:?}, got {actual:?}"
                );
            }
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    renderer.destroy_upload_path(upload);
    result
}

fn puppet_rig(
    shared: &crate::shared::SharedDevice,
) -> anyhow::Result<(
    Renderer,
    crate::vk::SceneTarget,
    crate::vk::SceneTexture,
    paper_scene::puppet::Mesh,
)> {
    let mut renderer = crate::shared::renderer(shared, 4, 4)?;
    renderer.ensure_scene_pipelines()?;
    renderer.ensure_scene_pool(2)?;
    let target = renderer.create_scene_target(4, 4)?;
    let texture = renderer.create_scene_texture(1, 1, &[255; 4])?;
    Ok((renderer, target, texture, translucent_puppet()?))
}

fn puppet_mesh(
    renderer: &Renderer,
    puppet: &paper_scene::puppet::Mesh,
    indices: &[u16],
) -> anyhow::Result<crate::vk::SceneMesh> {
    renderer.create_scene_mesh(
        puppet.vertices.iter().map(|vertex| {
            [vertex.position[0] / 4.0, -vertex.position[1] / 4.0, vertex.uv[0], vertex.uv[1], 1.0]
        }),
        crate::vk::SceneIndices::U16(indices),
    )
}

fn translucent_puppet() -> anyhow::Result<paper_scene::puppet::Mesh> {
    let mut bytes = b"MDLV0019\0".to_vec();
    bytes.extend([0x0180_0009_u32, 1, 1].into_iter().flat_map(u32::to_le_bytes));
    bytes.extend(b"materials/puppet.json\0");
    bytes.extend([0_u32; 7].into_iter().flat_map(u32::to_le_bytes));
    bytes.extend([0x0f, 0x00, 0x80, 0x01]);
    bytes.extend(320_u32.to_le_bytes());
    for (x, y, u, v) in [
        (-2.0_f32, 2.0, 0.0, 0.0),
        (2.0, 2.0, 1.0, 0.0),
        (2.0, -2.0, 1.0, 1.0),
        (-2.0, -2.0, 0.0, 1.0),
    ] {
        let start = bytes.len();
        bytes.extend([x, y, 0.0].into_iter().flat_map(f32::to_le_bytes));
        bytes.resize(start + 56, 0);
        bytes.extend([1.0_f32, 0.0, 0.0, 0.0, u, v].into_iter().flat_map(f32::to_le_bytes));
    }
    bytes.extend(12_u32.to_le_bytes());
    bytes.extend([0_u16, 1, 2, 2, 3, 0].into_iter().flat_map(u16::to_le_bytes));
    bytes.extend(b"MDLS0002\0");
    let animation_offset = bytes.len();
    bytes.extend([0_u32, 1].into_iter().flat_map(u32::to_le_bytes));
    bytes.push(0);
    bytes.extend([1_u32, u32::MAX, 64].into_iter().flat_map(u32::to_le_bytes));
    for index in 0..16 {
        bytes.extend(if index % 5 == 0 { 1.0_f32 } else { 0.0 }.to_le_bytes());
    }
    bytes.push(0);
    let animation = bytes.len() as u32;
    bytes[animation_offset..animation_offset + 4].copy_from_slice(&animation.to_le_bytes());
    bytes.extend(b"MDLA0005\0");
    let end_offset = bytes.len();
    bytes.extend([0_u32, 1, 1, 0].into_iter().flat_map(u32::to_le_bytes));
    bytes.extend(b"idle\0loop\0");
    bytes.extend(30.0_f32.to_le_bytes());
    bytes.extend([1_u32, 0, 1, 0, 72].into_iter().flat_map(u32::to_le_bytes));
    for _ in 0..2 {
        bytes.extend(
            [0.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
    }
    bytes.resize(bytes.len() + 34, 0);
    let end = bytes.len() as u32;
    bytes[end_offset..end_offset + 4].copy_from_slice(&end.to_le_bytes());
    paper_scene::puppet::parse(&bytes)
}

#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn puppet_alpha_roundtrip() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    renderer.ensure_scene_pipelines()?;
    renderer.ensure_scene_pool(5)?;
    let composite = renderer.create_scene_target(4, 4)?;
    let target = renderer.create_scene_target(4, 4)?;
    let mut slot = renderer.create_view_slot()?;
    slot.premultiplied = true;
    renderer.point_slot_at(&slot, composite.view);
    let quad = crate::vk::SceneQuad {
        uv_axes: None,
        uv_padding: false,
        rect: [2.0, 2.0, 4.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: [1.0; 4],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: crate::vk::SceneBlend::Copy,
    };
    let cases = [
        ([200, 100, 50, 0, 40, 80, 240, 0], [0, 0, 0, 0]),
        ([200, 100, 50, 128, 40, 80, 240, 0], [200, 100, 50, 128]),
        ([200, 100, 50, 255, 40, 80, 240, 0], [200, 100, 50, 255]),
        ([200, 100, 50, 128, 40, 80, 240, 128], [93, 86, 177, 192]),
    ];
    let mut puppet = translucent_puppet()?;
    for vertex in &mut puppet.vertices {
        vertex.uv = [0.25, 0.5];
    }
    let overlay = puppet
        .vertices
        .iter()
        .map(|vertex| {
            let mut vertex = *vertex;
            vertex.uv = [0.75, 0.5];
            vertex
        })
        .collect::<Vec<_>>();
    puppet.vertices.extend(overlay);
    puppet.indices.extend([4, 5, 6, 6, 7, 4]);
    let mesh = puppet_mesh(&renderer, &puppet, &puppet.indices)?;
    let result = (|| {
        for (pixels, expected) in cases {
            let texture = renderer.create_scene_texture(2, 1, &pixels)?;
            let rendered = renderer.render_scene_mesh(&composite, &texture, &mesh);
            renderer.destroy_scene_texture(texture);
            rendered?;
            renderer.render_scene(
                &target,
                [0.0; 4],
                std::slice::from_ref(&quad),
                std::slice::from_ref(&slot),
            )?;
            let (width, height, pixels) = renderer.read_scene_target(&target)?;
            anyhow::ensure!((width, height) == (4, 4), "unexpected dimensions");
            for pixel in pixels.chunks_exact(4) {
                anyhow::ensure!(
                    pixel
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(expected) <= 2),
                    "puppet RGBA {pixel:?} != {expected:?}"
                );
            }
        }
        Ok(())
    })();
    drop(mesh);
    renderer.destroy_scene_target(composite);
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_texture(slot);
    result
}

#[test]
#[ignore = "requires a Vulkan device"]
fn puppet_index_rewrite() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    renderer.ensure_scene_pipelines()?;
    renderer.ensure_scene_pool(5)?;
    let target = renderer.create_scene_target(4, 4)?;
    let mut puppet = translucent_puppet()?;
    for vertex in &mut puppet.vertices {
        vertex.uv = [0.25, 0.5];
    }
    let overlay = puppet
        .vertices
        .iter()
        .map(|vertex| paper_scene::puppet::Vertex { uv: [0.75, 0.5], ..*vertex })
        .collect::<Vec<_>>();
    puppet.vertices.extend(overlay);
    puppet.indices.extend([4, 5, 6, 6, 7, 4]);
    let mesh = puppet_mesh(&renderer, &puppet, &puppet.indices)?;
    let texture = renderer.create_scene_texture(2, 1, &[200, 0, 0, 255, 0, 0, 200, 255])?;
    let reordered: Vec<u8> =
        [4_u16, 5, 6, 6, 7, 4, 0, 1, 2, 2, 3, 0].iter().flat_map(|i| i.to_ne_bytes()).collect();
    let result = (|| {
        for (bytes, expected) in [(None, [0, 0, 200, 255]), (Some(&reordered), [200, 0, 0, 255])] {
            if let Some(bytes) = bytes {
                renderer.update_scene_indices(&mesh, bytes)?;
            }
            renderer.render_scene_mesh(&target, &texture, &mesh)?;
            let (_, _, pixels) = renderer.read_scene_target(&target)?;
            anyhow::ensure!(pixels[..4] == expected, "top quad {:?}", &pixels[..4]);
        }
        anyhow::ensure!(renderer.update_scene_indices(&mesh, &reordered[1..]).is_err());
        let wide = [0_u32; 3];
        let wide = renderer.create_scene_mesh(
            puppet.vertices.iter().map(|vertex| {
                [vertex.position[0], vertex.position[1], vertex.uv[0], vertex.uv[1], 1.0]
            }),
            crate::vk::SceneIndices::U32(&wide),
        )?;
        anyhow::ensure!(renderer.update_scene_indices(&wide, &[0; 12]).is_err());
        Ok(())
    })();
    drop(mesh);
    renderer.destroy_scene_texture(texture);
    renderer.destroy_scene_target(target);
    result
}

#[test]
#[ignore = "requires a Vulkan device"]
fn puppet_direct_composition() -> anyhow::Result<()> {
    direct_composition(crate::vk::SceneConfig::default())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn puppet_direct_float() -> anyhow::Result<()> {
    direct_composition(crate::vk::SceneConfig {
        format: ash::vk::Format::R16G16B16A16_SFLOAT,
        depth: Some(crate::vk::DepthOrder::Less),
    })
}

#[expect(clippy::too_many_lines, reason = "one ordered puppet composition GPU transaction")]
fn direct_composition(config: crate::vk::SceneConfig) -> anyhow::Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        16,
        16,
    )?;
    renderer.configure_scene(config)?;
    renderer.ensure_scene_pool(2)?;
    let source = renderer.create_scene_texture(
        2,
        2,
        &[200, 100, 50, 128, 20, 160, 80, 192, 40, 80, 240, 64, 100, 40, 200, 255],
    )?;
    let mut slot = renderer.create_view_slot()?;
    slot.sampler = source.sampler;
    slot.view = source.view;
    let puppet = translucent_puppet()?;
    let mesh = puppet_mesh(&renderer, &puppet, &puppet.indices)?;
    let positions = puppet
        .vertices
        .iter()
        .map(|vertex| [vertex.position[0] + 4.0, vertex.position[1], vertex.position[2]])
        .collect::<Vec<_>>();
    renderer.update_scene_mesh(&mesh, &positions, &[], (4.0, 4.0))?;
    slot.use_scene_mesh(&mesh);
    renderer.point_slot_at(&slot, source.view);
    let mut textures = vec![source, slot];
    let target = renderer.create_scene_target(16, 16)?;
    let reference = renderer.create_scene_target(16, 16)?;
    let quad = SceneQuad {
        rect: [4.0, 8.0, 4.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [0.5, 0.75, 0.25, 0.5],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 1,
        blend: SceneBlend::Alpha,
    };
    let result = (|| {
        for premultiplied in [false, true] {
            for texture in &mut textures {
                texture.premultiplied = premultiplied;
            }
            for blend in SceneBlend::ALL {
                for (angle, projection) in [
                    (0.0_f32, None),
                    (std::f32::consts::FRAC_PI_2, None),
                    (
                        0.0,
                        Some([[0.5, 0.0, 0.0, -0.5], [0.0, 0.5, 0.0, 0.0], [0.25, 0.0, 0.0, 1.0]]),
                    ),
                ] {
                    let mut direct = quad.clone();
                    direct.blend = blend;
                    direct.angle = angle;
                    direct.projection = projection;
                    let mut expected = direct.clone();
                    expected.texture = 0;
                    expected.rect[0] += 4.0 * angle.cos();
                    expected.rect[1] += 4.0 * angle.sin();
                    if let Some(projection) = &mut expected.projection {
                        for row in projection {
                            row[3] += row[0];
                        }
                    }
                    let mut backdrop = expected.clone();
                    backdrop.rect = [8.0, 8.0, 16.0, 16.0];
                    backdrop.projection = None;
                    backdrop.tint = [0.25; 4];
                    let mut overlay = backdrop.clone();
                    overlay.rect = [8.0, 10.0, 2.0, 2.0];
                    let actual = [backdrop.clone(), direct, overlay.clone()];
                    let expected = [backdrop, expected, overlay];
                    for count in [2, 3] {
                        renderer.render_scene(&target, [0.1; 4], &actual[..count], &textures)?;
                        renderer.render_scene(
                            &reference,
                            [0.1; 4],
                            &expected[..count],
                            &textures,
                        )?;
                        let pixels = renderer.read_scene_target(&target)?.2;
                        let expected = renderer.read_scene_target(&reference)?.2;
                        assert!(
                            pixels
                                .iter()
                                .zip(expected)
                                .all(|(actual, expected)| actual.abs_diff(expected) <= 1)
                        );
                    }
                }
            }
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_target(reference);
    for texture in textures {
        renderer.destroy_scene_texture(texture);
    }
    drop(mesh);
    result
}

#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn scene_creation_retries() -> anyhow::Result<()> {
    use crate::vk::{MipSampling, SceneBlend, SceneQuad};
    use ash::vk;
    use paper_scene::{
        tex::Pixels,
        text::batch::{Batch, Quad},
    };
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    for step in 0..6 {
        super::PIPELINE_FAILURE.with(|failure| failure.set(Some(step)));
        let result = renderer.ensure_rgba_pipelines();
        super::PIPELINE_FAILURE.with(|failure| failure.set(None));
        assert!(result.is_err(), "RGBA creation step {step}");
        assert_eq!(renderer.pipeline_rgba, vk::Pipeline::null());
        assert_eq!(renderer.pipeline_rgba_fade, vk::Pipeline::null());
    }
    renderer.ensure_rgba_pipelines()?;
    for step in 0..20 {
        super::PIPELINE_FAILURE.with(|failure| failure.set(Some(step)));
        let result = renderer.ensure_scene_pipelines();
        super::PIPELINE_FAILURE.with(|failure| failure.set(None));
        assert!(result.is_err(), "creation step {step}");
        assert_eq!(renderer.scene_pass, vk::RenderPass::null());
        assert_eq!(renderer.pipeline_layout_layer, vk::PipelineLayout::null());
        assert!(renderer.raster_pipelines[0].is_none());
    }
    renderer.ensure_scene_pipelines()?;
    assert!(
        renderer.raster_pipelines[0]
            .as_ref()
            .is_some_and(|set| set.text.iter().all(|pipeline| *pipeline != vk::Pipeline::null()))
    );
    renderer.ensure_scene_pool(2)?;
    let pixels = Pixels::rgba(4, 4, vec![255; 64]);
    let batch = Batch {
        canvas: [4, 4],
        pixels: pixels.clone(),
        quads: vec![Quad { bounds: [0, 0, 4, 4], uv: [0, 0, 4, 4] }],
    };
    let texture = renderer.create_text_texture(&pixels, MipSampling::Disabled, Some(&batch))?;
    let target = renderer.create_scene_target(4, 4)?;
    let quad = SceneQuad {
        rect: [2.0, 2.0, 4.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [1.0; 4],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Alpha,
    };
    renderer.render_scene(&target, [0.0; 4], &[quad], std::slice::from_ref(&texture))?;
    let (_, _, pixels) = renderer.read_scene_target(&target)?;
    assert!(pixels.chunks_exact(4).all(|pixel| pixel == [255; 4]));
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_texture(texture);
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn culled_puppet_winding() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let (mut renderer, target, texture, puppet) = puppet_rig(&shared)?;
    let reversed: Vec<u16> =
        puppet.indices.chunks_exact(3).flat_map(|tri| [tri[0], tri[2], tri[1]]).collect();
    let mut drawn = Vec::new();
    for cull in [false, true] {
        for indices in [&puppet.indices, &reversed] {
            let mut mesh = puppet_mesh(&renderer, &puppet, indices)?;
            mesh.cull = cull;
            renderer.render_scene_mesh(&target, &texture, &mesh)?;
            let (_, _, pixels) = renderer.read_scene_target(&target)?;
            drawn.push(pixels.iter().any(|value| *value != 0));
        }
    }
    renderer.destroy_scene_texture(texture);
    assert_eq!(drawn, [true, true, false, true]);
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn puppet_vertex_alpha() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let (mut renderer, target, texture, puppet) = puppet_rig(&shared)?;
    let mesh = puppet_mesh(&renderer, &puppet, &puppet.indices)?;
    let positions: Vec<_> = puppet.vertices.iter().map(|vertex| vertex.position).collect();
    let mut alphas = Vec::new();
    for alpha in [1.0, 0.5] {
        renderer.update_scene_mesh(&mesh, &positions, &vec![alpha; positions.len()], (4.0, 4.0))?;
        renderer.render_scene_mesh(&target, &texture, &mesh)?;
        let (_, _, pixels) = renderer.read_scene_target(&target)?;
        alphas.push(pixels.chunks_exact(4).map(|pixel| pixel[3]).max().unwrap_or(0));
    }
    renderer.destroy_scene_texture(texture);
    assert_eq!(alphas[0], 255);
    assert!(alphas[1].abs_diff(128) <= 1, "{alphas:?}");
    Ok(())
}
