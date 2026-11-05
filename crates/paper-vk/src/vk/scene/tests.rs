use super::{EffectProjection, MipSampling, timestamp_delta_ns};

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device"]
#[expect(clippy::too_many_lines, reason = "one MSAA target recreation lifecycle test")]
fn msaa_edges_recreate() -> anyhow::Result<()> {
    use super::{Renderer, SceneBlend, SceneQuad, SceneTargetSpec};
    use ash::vk;

    const SIDE: u32 = 32;
    const SMALL: u32 = 16;
    const HALF: f32 = 16.0;
    const EDGE: u8 = 255;
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
        SIDE,
        SIDE,
    )?;
    anyhow::ensure!(
        renderer.scene_sample_count() == vk::SampleCountFlags::TYPE_4,
        "qualification GPU does not support scene MSAA4"
    );
    renderer.ensure_scene_pool(1)?;
    let texture = renderer.create_scene_texture(1, 1, &[EDGE; 4])?;
    let baseline = Renderer::scene_target_live_allocations();
    let quad = SceneQuad {
        rect: [HALF, HALF, HALF, HALF],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [1.0; 4],
        angle: std::f32::consts::FRAC_PI_4,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Copy,
    };
    let result = (|| {
        for side in [SIDE, SMALL] {
            let extent = vk::Extent2D { width: side, height: side };
            let single = renderer.create_scene_target(side, side)?;
            let multisample = renderer.create_scene_target_opts(SceneTargetSpec {
                depth_attachment: true,
                extent,
                format: renderer.scene_format(),
                repeat: false,
                samples: vk::SampleCountFlags::TYPE_4,
            })?;
            anyhow::ensure!(
                multisample.allocation_bytes > single.allocation_bytes,
                "MSAA color storage is missing"
            );
            anyhow::ensure!(
                Renderer::scene_target_live_allocations() == baseline + 2,
                "scene targets were not tracked"
            );
            let draw = if side == SIDE {
                quad.clone()
            } else {
                SceneQuad { rect: [HALF / 2.0; 4], ..quad.clone() }
            };
            renderer.render_scene(
                &single,
                [0.0; 4],
                std::slice::from_ref(&draw),
                std::slice::from_ref(&texture),
            )?;
            renderer.render_scene(
                &multisample,
                [0.0; 4],
                std::slice::from_ref(&draw),
                std::slice::from_ref(&texture),
            )?;
            let single_pixels = renderer.read_scene_target(&single)?.2;
            let msaa_pixels = renderer.read_scene_target(&multisample)?.2;
            let edge_pixels = |pixels: &[u8]| {
                pixels.chunks_exact(4).filter(|pixel| pixel[0] > 0 && pixel[0] < EDGE).count()
            };
            anyhow::ensure!(
                edge_pixels(&single_pixels) == 0,
                "single sample image has fractional edge coverage"
            );
            anyhow::ensure!(
                edge_pixels(&msaa_pixels) > 0,
                "MSAA resolve has no fractional edge coverage"
            );
            renderer.begin_scene_batch()?;
            renderer.record_scene_append_with_canvas(
                &multisample,
                [side as f32; 2],
                [0.0; 4],
                &[],
                &[],
                &[],
            )?;
            renderer.submit_scene_batch()?;
            anyhow::ensure!(
                renderer.read_scene_target(&multisample)?.2 == msaa_pixels,
                "MSAA append changed prior coverage"
            );
            renderer.destroy_scene_target(multisample);
            renderer.destroy_scene_target(single);
            anyhow::ensure!(
                Renderer::scene_target_live_allocations() == baseline,
                "scene targets remained live after recreation"
            );
        }
        Ok(())
    })();
    renderer.destroy_scene_texture(texture);
    result
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device"]
fn ordered_base_draws() -> anyhow::Result<()> {
    use super::{EffectGeometry, Renderer, SceneBlend, SceneEffect, SceneQuad};
    use paper_scene::shader::{Stage, translate};

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
    renderer.ensure_scene_pool(1)?;
    let target = renderer.create_scene_target(4, 4)?;
    let mut texture = renderer.create_scene_texture(1, 1, &[255; 4])?;
    let vertices = renderer.create_quad_buffer((4, 4), [0.0, 0.0, 1.0, 1.0], (2.0, 2.0))?;
    let vertex = translate(
        "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
        Stage::Vertex,
        &std::collections::BTreeMap::new(),
    );
    let mut pipelines = Vec::new();
    for color in ["1.0,0.0,0.0", "0.0,0.0,1.0"] {
        let fragment = translate(
            &format!("void main(){{gl_FragColor=vec4({color},0.5);}}"),
            Stage::Fragment,
            &std::collections::BTreeMap::new(),
        );
        pipelines.push(renderer.create_effect_pipeline_for(
            &vertex,
            &fragment,
            None,
            "ordered base",
            super::super::EffectPipelineSpec::quad(target.format, SceneBlend::Alpha, None),
        )?);
    }
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
        blend: SceneBlend::Copy,
    };
    let result = (|| {
        for order in [[0, 1], [1, 0]] {
            texture.effect = Some(SceneEffect::for_geometry(
                &pipelines[order[0]],
                EffectGeometry::Quad {
                    vertices: vertices.buffer,
                    offset: crate::vk::effect::MODEL_QUAD_OFFSET,
                },
                None,
            )?);
            texture.effect_tail = vec![SceneEffect::for_geometry(
                &pipelines[order[1]],
                EffectGeometry::Quad {
                    vertices: vertices.buffer,
                    offset: crate::vk::effect::MODEL_QUAD_OFFSET,
                },
                None,
            )?];
            renderer.render_scene(
                &target,
                [0.0; 4],
                std::slice::from_ref(&quad),
                std::slice::from_ref(&texture),
            )?;
            let frame = renderer.read_scene_target(&target)?.2;
            let pixel = &frame[40..44];
            assert!(pixel[order[0] * 2] <= 65 && pixel[order[0] * 2] >= 63);
            assert!(pixel[order[1] * 2] <= 129 && pixel[order[1] * 2] >= 127);
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_texture(texture);
    for pipeline in pipelines {
        renderer.destroy_effect_pipeline(pipeline);
    }
    renderer.destroy_quad_buffer(vertices);
    result
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn rotated_atlas_axes() -> anyhow::Result<()> {
    use super::{Renderer, SceneBlend, SceneQuad};
    use paper_scene::tex::Pixels;

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
    renderer.ensure_scene_pool(3)?;
    let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]];
    let textures = [[0, 1, 2, 3], [2, 0, 3, 1], [1, 3, 0, 2]]
        .into_iter()
        .map(|order| {
            let pixels =
                Pixels::rgba(2, 2, order.into_iter().flat_map(|index| colors[index]).collect());
            renderer.create_scene_texture_pixels(&pixels, true, true, MipSampling::Disabled)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let target = renderer.create_scene_target(4, 4)?;
    let mut quad = SceneQuad {
        rect: [2.0, 2.0, 4.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [1.0; 4],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Copy,
    };
    let result = (|| {
        renderer.render_scene(&target, [0.0; 4], std::slice::from_ref(&quad), &textures)?;
        let expected = renderer.read_scene_target(&target)?.2;
        for (slot, uv, axes) in [
            (1, [1.0, 0.0, 1.0, 1.0], [0.0, 1.0, -1.0, 0.0]),
            (2, [0.0, 1.0, 1.0, 1.0], [0.0, -1.0, 1.0, 0.0]),
        ] {
            quad.texture = slot;
            quad.uv = uv;
            quad.uv_axes = Some(axes);
            renderer.render_scene(&target, [0.0; 4], std::slice::from_ref(&quad), &textures)?;
            assert_eq!(renderer.read_scene_target(&target)?.2, expected);
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    for texture in textures {
        renderer.destroy_scene_texture(texture);
    }
    result
}

#[test]
fn text_mips_stay_linear() {
    assert!(!MipSampling::Disabled.has_mips());
    assert!(MipSampling::Linear.has_mips());
    assert!(MipSampling::Anisotropic.has_mips());
    assert!(!MipSampling::Linear.uses_anisotropy(false, true));
    assert!(!MipSampling::Anisotropic.uses_anisotropy(true, true));
    assert!(!MipSampling::Anisotropic.uses_anisotropy(false, false));
    assert!(MipSampling::Anisotropic.uses_anisotropy(false, true));
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device"]
fn modern_texture_uploads() -> anyhow::Result<()> {
    use super::{Renderer, SceneBlend, SceneQuad};
    use paper_scene::tex::{MipLevel, PixelFormat, Pixels};

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
    renderer.ensure_scene_pool(1)?;
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
        blend: SceneBlend::Copy,
    };
    for format in [
        PixelFormat::Rgba16f,
        PixelFormat::Rgba16,
        PixelFormat::Rg16f,
        PixelFormat::R16f,
        PixelFormat::Bc7,
        PixelFormat::Bc6h,
        PixelFormat::Rgb565,
        PixelFormat::Rgba1010102,
    ] {
        let pixel = match format {
            PixelFormat::Rgba16f => vec![0, 0x3c, 0, 0x38, 0, 0, 0, 0x3c],
            PixelFormat::Rgba16 => vec![0xff, 0xff, 0, 0x80, 0, 0, 0xff, 0xff],
            PixelFormat::Rg16f => vec![0, 0x3c, 0, 0x38],
            PixelFormat::R16f => vec![0, 0x3c],
            PixelFormat::Rgb565 => vec![0, 0xf8],
            PixelFormat::Rgba1010102 => vec![0xff, 0x03, 0, 0xc0],
            PixelFormat::Bc7 => {
                let mut block = vec![0; 16];
                block[0] = 1;
                block
            }
            PixelFormat::Bc6h => vec![0; 16],
            _ => unreachable!(),
        };
        let data = if format.compressed() { pixel } else { pixel.repeat(16) };
        let pixels = Pixels { format, levels: vec![MipLevel { width: 4, height: 4, data }] };
        let expected = pixels.base_rgba().unwrap();
        let texture =
            renderer.create_scene_texture_pixels(&pixels, true, false, MipSampling::Disabled)?;
        renderer.render_scene(
            &target,
            [0.0; 4],
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        let actual = renderer.read_scene_target(&target)?.2;
        for (actual, expected) in actual.chunks_exact(4).zip(expected.chunks_exact(4)) {
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| actual.abs_diff(*expected) <= 1),
                "{format:?}: {actual:?} != {expected:?}"
            );
        }
        renderer.destroy_scene_texture(texture);
    }
    renderer.destroy_scene_target(target);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device"]
fn hdr_texture_precision() -> anyhow::Result<()> {
    use super::{Renderer, SceneBlend, SceneQuad};
    use ash::vk;
    use paper_scene::shader::{Stage, translate};
    use paper_scene::tex::{MipLevel, PixelFormat, Pixels};

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
    renderer.configure_scene(super::SceneConfig {
        format: vk::Format::R16G16B16A16_SFLOAT,
        depth: None,
    })?;
    renderer.ensure_scene_pool(1)?;
    let texture = renderer.create_scene_texture_pixels(
        &Pixels {
            format: PixelFormat::Rgba16f,
            levels: vec![MipLevel {
                width: 1,
                height: 1,
                data: vec![0, 0x40, 0, 0, 0, 0, 0, 0x3c],
            }],
        },
        true,
        true,
        MipSampling::Disabled,
    )?;
    let target = renderer.create_scene_target(4, 4)?;
    let output = renderer.create_scene_target_fmt(4, 4, false, vk::Format::R8G8B8A8_UNORM)?;
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
        blend: SceneBlend::Copy,
    };
    let vertex = translate(
        "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_Uv;\nvoid main(){v_Uv=a_TexCoord;gl_Position=vec4(a_Position,1.0);}",
        Stage::Vertex,
        &std::collections::BTreeMap::new(),
    );
    let fragment = translate(
        "varying vec2 v_Uv;\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=vec4(texSample2D(g_Texture0,v_Uv).rgb*0.25,1.0);}",
        Stage::Fragment,
        &std::collections::BTreeMap::new(),
    );
    let pipeline = renderer.create_effect_pipeline_for(
        &vertex,
        &fragment,
        None,
        "HDR texture readback",
        super::super::EffectPipelineSpec::quad(vk::Format::R8G8B8A8_UNORM, SceneBlend::Copy, None),
    )?;
    let vertices = renderer.create_quad_buffer_ndc()?;
    let result = (|| {
        renderer.render_scene(
            &target,
            [0.0; 4],
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        renderer.run_effect_pass(
            &pipeline,
            &vertices,
            crate::vk::EffectDraw { target: &output, preserve: false, final_vertices: false },
            &[(target.view, target.sampler)],
            &[],
        )?;
        let rgba = renderer.read_scene_target(&output)?.2;
        assert!(rgba.chunks_exact(4).all(|pixel| (127..=128).contains(&pixel[0])));
        Ok(())
    })();
    renderer.destroy_quad_buffer(vertices);
    renderer.destroy_effect_pipeline(pipeline);
    renderer.destroy_scene_target(output);
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_texture(texture);
    result
}

#[test]
fn authored_mips_preserved() {
    use paper_scene::tex::{MipLevel, Pixels};

    const EDGE: u32 = 32;
    const CHANNELS: usize = 4;
    let mut pixels = Pixels::rgba(EDGE, EDGE, vec![u8::MAX; (EDGE * EDGE) as usize * CHANNELS]);
    assert_eq!(MipSampling::Anisotropic.mip_layout(&pixels, true), (1, false));
    assert!(MipSampling::Anisotropic.uses_anisotropy(false, true));
    assert!(!MipSampling::Anisotropic.uses_anisotropy(false, false));
    assert!(!MipSampling::Anisotropic.uses_anisotropy(true, true));
    assert_eq!(MipSampling::Linear.mip_layout(&pixels, true), (6, true));
    assert_eq!(MipSampling::Disabled.mip_layout(&pixels, true), (1, false));
    assert_eq!(MipSampling::Linear.mip_layout(&pixels, false), (1, false));

    pixels.levels.push(MipLevel {
        width: EDGE / 2,
        height: EDGE / 2,
        data: vec![u8::MAX; (EDGE * EDGE) as usize * CHANNELS / 4],
    });
    assert_eq!(MipSampling::Anisotropic.mip_layout(&pixels, true), (2, false));
    assert_eq!(MipSampling::Linear.mip_layout(&pixels, true), (2, false));
}

#[test]
fn timestamp_delta_applies_device_period() {
    assert_eq!(timestamp_delta_ns(100, 140, 64, 2.5), 100);
}

#[test]
fn timestamp_delta_wraps_at_valid_bit_width() {
    assert_eq!(timestamp_delta_ns(250, 4, 8, 1.0), 10);
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn scene_append_preserves_sampled_prefix_and_grab_pixels() -> anyhow::Result<()> {
    use super::{Renderer, SceneBlend, SceneQuad};
    use ash::vk;

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
    renderer.ensure_scene_pool(2)?;
    let textures = [renderer.create_scene_texture(1, 1, &[255; 4])?, renderer.create_view_slot()?];
    let target = renderer.create_scene_target(4, 4)?;
    let expected = renderer.create_scene_target(4, 4)?;
    let sampled = renderer.create_scene_target(4, 4)?;
    let grab = renderer.create_scene_target(4, 4)?;
    renderer.point_slot_at(&textures[1], target.view);
    let canvas = [4.0, 4.0];
    let clear = [0.125, 0.25, 0.5, 1.0];
    let quads = [
        SceneQuad {
            uv_axes: None,
            uv_padding: false,
            rect: [1.0, 2.0, 2.0, 4.0],
            uv: [0.0, 0.0, 1.0, 1.0],
            tint: [1.0, 0.0, 0.0, 0.5],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: 0,
            blend: SceneBlend::Alpha,
        },
        SceneQuad {
            uv_axes: None,
            uv_padding: false,
            rect: [2.0, 1.0, 4.0, 2.0],
            uv: [0.0, 0.0, 1.0, 1.0],
            tint: [0.0, 1.0, 0.0, 0.5],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: 0,
            blend: SceneBlend::Alpha,
        },
    ];
    let sample = SceneQuad {
        uv_axes: None,
        uv_padding: false,
        rect: [2.0, 2.0, 4.0, 4.0],
        tint: [1.0; 4],
        texture: 1,
        blend: SceneBlend::Copy,
        ..quads[0].clone()
    };
    let result = (|| {
        renderer.render_scene(&expected, clear, &quads[..1], &textures)?;
        let (_, _, prefix_pixels) = renderer.read_scene_target(&expected)?;
        renderer.render_scene(&expected, clear, &quads, &textures)?;
        let (_, _, complete_pixels) = renderer.read_scene_target(&expected)?;
        anyhow::ensure!(prefix_pixels != complete_pixels, "append fixture did not alter pixels");
        let append_pass =
            renderer.scene_pass_append_for(target.format, ash::vk::SampleCountFlags::TYPE_1)?;
        let grab_pass =
            renderer.scene_pass_load_for(target.format, ash::vk::SampleCountFlags::TYPE_1)?;
        renderer.begin_scene_batch()?;
        renderer.record_scene_with_canvas(&target, canvas, clear, &quads[..1], &textures, &[]);
        renderer.record_scene_with_canvas(&sampled, canvas, [0.0; 4], &[sample], &textures, &[]);
        renderer.record_scene_append_with_canvas(&target, canvas, [1.0; 4], &[], &textures, &[])?;
        unsafe {
            renderer.device.cmd_begin_render_pass(
                renderer.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(append_pass)
                    .framebuffer(target.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: target.extent,
                    }),
                vk::SubpassContents::INLINE,
            );
        }
        renderer.grab_scene(&target, grab.grab(), grab_pass);
        unsafe { renderer.device.cmd_end_render_pass(renderer.cmd) };
        renderer.record_scene_append_with_canvas(
            &target,
            canvas,
            [1.0; 4],
            &quads[1..],
            &textures,
            &[],
        )?;
        renderer.submit_scene_batch()?;
        anyhow::ensure!(
            renderer.read_scene_target(&target)?.2 == complete_pixels,
            "append changed composition"
        );
        anyhow::ensure!(
            renderer.read_scene_target(&sampled)?.2 == prefix_pixels,
            "sampled prefix changed"
        );
        anyhow::ensure!(
            renderer.read_scene_target(&grab)?.2 == prefix_pixels,
            "grab did not preserve prefix"
        );
        Ok(())
    })();
    for target in [target, expected, sampled, grab] {
        renderer.destroy_scene_target(target);
    }
    for texture in textures {
        renderer.destroy_scene_texture(texture);
    }
    result
}

#[test]
fn effect_projection_translation() {
    let matrix = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let mut model = matrix;
    model[12] = 32.0;
    model[13] = 32.0;
    let projection = EffectProjection::for_layer(matrix, model, (64.0, 64.0), 0);
    for (flipped, y) in [(false, 0.5), (true, -0.5)] {
        let changed = projection.at([48.0, 48.0], [64.0, 64.0], flipped);
        assert_eq!([changed[12], changed[13]], [0.5, y]);
        assert_eq!(changed[0], 1.0);
    }
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device"]
fn float_scene_preserves_emission() -> anyhow::Result<()> {
    float_composition(false)
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device"]
fn depth_preserves_color_passes() -> anyhow::Result<()> {
    float_composition(true)
}

#[cfg(feature = "shared-device")]
#[expect(clippy::too_many_lines, reason = "one float-scene composition GPU transaction")]
fn float_composition(depth: bool) -> anyhow::Result<()> {
    use super::{Renderer, SceneBlend, SceneQuad};
    use ash::vk;
    use paper_scene::shader::{Stage, translate};
    use paper_scene::tex::Pixels;

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
    renderer.configure_scene(super::SceneConfig {
        format: vk::Format::R16G16B16A16_SFLOAT,
        depth: depth.then_some(crate::vk::DepthOrder::Less),
    })?;
    renderer.ensure_scene_pipelines()?;
    renderer.ensure_scene_pool(1)?;
    let texture = renderer.create_scene_texture_pixels(
        &Pixels::rgba(1, 1, vec![255; 4]),
        true,
        true,
        MipSampling::Disabled,
    )?;
    let target = renderer.create_scene_target(4, 4)?;
    let output = renderer.create_scene_target_fmt(4, 4, false, vk::Format::R8G8B8A8_UNORM)?;
    let quad = SceneQuad {
        rect: [2.0, 2.0, 4.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [4.0, 2.0, 0.5, 1.0],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Copy,
    };
    let vertex = translate(
        "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_Uv;\nvoid main(){\nv_Uv=a_TexCoord;\ngl_Position=vec4(a_Position,1.0);\n}",
        Stage::Vertex,
        &std::collections::BTreeMap::new(),
    );
    let fragment = translate(
        "varying vec2 v_Uv;\nuniform sampler2D g_Texture0;\nvoid main(){\ngl_FragColor=vec4(texSample2D(g_Texture0,v_Uv).rgb*0.25,1.0);\n}",
        Stage::Fragment,
        &std::collections::BTreeMap::new(),
    );
    let pipeline = renderer.create_effect_pipeline_for(
        &vertex,
        &fragment,
        None,
        "float emission",
        super::super::EffectPipelineSpec::quad(vk::Format::R8G8B8A8_UNORM, SceneBlend::Copy, None),
    )?;
    let additive = renderer.create_effect_pipeline_for(
        &vertex,
        &fragment,
        None,
        "float additive",
        super::super::EffectPipelineSpec::quad(
            vk::Format::R8G8B8A8_UNORM,
            SceneBlend::Add,
            Some(SceneBlend::Alpha),
        ),
    )?;
    let vertices = renderer.create_quad_buffer_ndc()?;
    let result = (|| {
        renderer.render_scene(
            &target,
            [0.0; 4],
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        assert!(
            renderer
                .read_scene_target(&target)?
                .2
                .chunks_exact(4)
                .all(|pixel| pixel[..2] == [255, 255]
                    && (127..=128).contains(&pixel[2])
                    && pixel[3] == 255)
        );
        renderer.run_effect_pass(
            &pipeline,
            &vertices,
            crate::vk::EffectDraw { target: &output, preserve: false, final_vertices: false },
            &[(target.view, target.sampler)],
            &[],
        )?;
        let sampled = renderer.read_scene_target(&output)?.2;
        assert!(
            sampled.chunks_exact(4).all(|pixel| pixel[0] == 255
                && (127..=128).contains(&pixel[1])
                && pixel[2..] == [32, 255]),
            "{sampled:?}"
        );
        renderer.run_effect_pass(
            &additive,
            &vertices,
            crate::vk::EffectDraw { target: &output, preserve: true, final_vertices: false },
            &[(target.view, target.sampler)],
            &[],
        )?;
        assert!(
            renderer.read_scene_target(&output)?.2.chunks_exact(4).all(|pixel| pixel[0] == 255
                && (254..=255).contains(&pixel[1])
                && pixel[2..] == [64, 255])
        );
        assert!(renderer.configure_scene(super::SceneConfig::default()).is_err());
        Ok(())
    })();
    renderer.destroy_quad_buffer(vertices);
    renderer.destroy_effect_pipeline(pipeline);
    renderer.destroy_effect_pipeline(additive);
    renderer.destroy_scene_target(output);
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_texture(texture);
    result
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires Vulkan timestamps"]
fn segmented_scene_timing() -> anyhow::Result<()> {
    use super::Renderer;

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
    assert!(renderer.scene_timestamp_valid_bits > 0);
    if renderer.scene_query_pool.is_none() {
        renderer.scene_query_pool = Some(unsafe {
            renderer.device.create_query_pool(
                &ash::vk::QueryPoolCreateInfo::default()
                    .query_type(ash::vk::QueryType::TIMESTAMP)
                    .query_count(2),
                None,
            )?
        });
        renderer.scene_timestamp_active = true;
    }
    renderer.begin_scene_batch()?;
    renderer.scene_gpu_time_ns = Some(1_000_000_000);
    renderer.flush_scene_batch()?;
    let first = renderer.scene_gpu_time_ns.unwrap();
    assert!(first >= 1_000_000_000);
    renderer.submit_scene_batch()?;
    let total = renderer.take_scene_gpu_time_ns().unwrap();
    assert!(total >= first);
    assert_eq!(renderer.take_scene_gpu_time_ns(), None);
    renderer.scene_gpu_time_ns = Some(total);
    renderer.begin_scene_batch()?;
    assert_eq!(renderer.scene_gpu_time_ns, None);
    renderer.submit_scene_batch()?;
    Ok(())
}

#[test]
fn copy_alpha_masks() {
    let copy = super::scene_blend_state(super::SceneBlend::Copy);
    let rgb = super::scene_blend_state(super::SceneBlend::CopyRgb);
    assert_eq!(copy.blend_enable, ash::vk::FALSE);
    assert_eq!(rgb.blend_enable, ash::vk::FALSE);
    assert!(copy.color_write_mask == ash::vk::ColorComponentFlags::RGBA);
    assert!(
        rgb.color_write_mask
            == (ash::vk::ColorComponentFlags::R
                | ash::vk::ColorComponentFlags::G
                | ash::vk::ColorComponentFlags::B)
    );
}

#[test]
fn packs_homogeneous_depth() {
    let matrix =
        [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
    assert_eq!(
        super::LayerProjection::from(matrix).pack(),
        [[1.0, 5.0, 3.0, 13.0], [2.0, 6.0, 7.0, 14.0], [4.0, 8.0, 15.0, 16.0],]
    );
    assert_eq!(std::mem::size_of::<super::LayerPush>(), 128);
}

#[test]
fn packs_zero_constant_w() {
    let matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.1, 0.0];
    let [_, _, w] = super::LayerProjection::from(matrix).pack();
    assert_eq!(w, [1.0, -1.0, 0.1, 0.0]);
    assert!(w.iter().any(|value| *value != 0.0));
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn rejects_mesh_layouts() -> anyhow::Result<()> {
    use super::{Renderer, SceneIndices};
    let shared = crate::shared::test_device();
    let renderer = Renderer::new_shared_headless(
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
    let indices = [0, 1, 2];
    assert!(
        renderer.create_scene_mesh([[0.0]; 3].into_iter(), SceneIndices::U16(&indices)).is_err()
    );
    assert!(
        renderer.create_scene_mesh([[0.0; 2]; 3].into_iter(), SceneIndices::U16(&indices)).is_err()
    );
    assert!(
        renderer.create_scene_mesh([[0.0; 3]; 3].into_iter(), SceneIndices::U16(&indices)).is_err()
    );
    assert!(
        renderer.create_scene_mesh([[0.0; 4]; 3].into_iter(), SceneIndices::U16(&indices)).is_err()
    );
    let text =
        renderer.create_scene_mesh([[0.25; 12]; 3].into_iter(), SceneIndices::U16(&indices))?;
    let before = unsafe {
        std::slice::from_raw_parts(
            text.mapped.cast::<u8>(),
            text.index_offset as usize + std::mem::size_of_val(&indices),
        )
        .to_vec()
    };
    assert!(renderer.update_scene_mesh(&text, &[[1.0; 3]; 3], &[], (4.0, 4.0)).is_err());
    assert_eq!(before, unsafe {
        std::slice::from_raw_parts(text.mapped.cast::<u8>(), before.len())
    });
    let puppet =
        renderer.create_scene_mesh([[0.25; 5]; 3].into_iter(), SceneIndices::U16(&indices))?;
    assert!(
        renderer.update_scene_mesh(&puppet, &[[1.0, 2.0, 0.0]; 3], &[0.5], (4.0, 4.0)).is_err()
    );
    renderer.update_scene_mesh(&puppet, &[[1.0, 2.0, 0.0]; 3], &[0.5, 1.0, 1.0], (4.0, 4.0))?;
    assert_eq!(
        unsafe { std::slice::from_raw_parts(puppet.mapped, 15) },
        [
            [0.25, -0.5, 0.25, 0.25, 0.5],
            [0.25, -0.5, 0.25, 0.25, 1.0],
            [0.25, -0.5, 0.25, 0.25, 1.0]
        ]
        .concat()
    );
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(
                puppet.mapped.cast::<u8>().add(puppet.index_offset as usize).cast::<u16>(),
                indices.len(),
            )
        },
        indices
    );
    Ok(())
}

#[test]
fn resource_owners_stay_compact() {
    use super::super::effect::{DynBuffer, EffectPipeline, QuadBuffer};
    use std::mem::size_of;

    const MAX_INLINE_RESOURCE_BYTES: usize = 256;
    for bytes in [
        size_of::<super::SceneTarget>(),
        size_of::<EffectPipeline>(),
        size_of::<QuadBuffer>(),
        size_of::<DynBuffer>(),
    ] {
        assert!(bytes <= MAX_INLINE_RESOURCE_BYTES);
        assert!(bytes < size_of::<ash::Device>());
    }
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn targets_share_device_storage() -> anyhow::Result<()> {
    use std::sync::Arc;

    let shared = crate::shared::test_device();
    let mut renderer = super::Renderer::new_shared_headless(
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
    let baseline = super::Renderer::scene_target_live_allocations();
    let first = renderer.create_scene_target(4, 4)?;
    let second = renderer.create_scene_target(4, 4)?;
    assert_eq!(super::Renderer::scene_target_live_allocations(), baseline + 2);
    assert!(Arc::ptr_eq(
        first.device.as_ref().expect("target device"),
        second.device.as_ref().expect("target device"),
    ));
    assert!(Arc::ptr_eq(
        first.device.as_ref().expect("target device"),
        &renderer.resource_device(),
    ));
    drop(second);
    drop(first);
    assert_eq!(super::Renderer::scene_target_live_allocations(), baseline);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn lazy_depth_composition() -> anyhow::Result<()> {
    depth_composition(ash::vk::SampleCountFlags::TYPE_1)
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn msaa_depth_composition() -> anyhow::Result<()> {
    depth_composition(ash::vk::SampleCountFlags::TYPE_4)
}

#[cfg(feature = "shared-device")]
#[expect(clippy::too_many_lines, reason = "depth promotion and composition lifecycle")]
fn depth_composition(samples: ash::vk::SampleCountFlags) -> anyhow::Result<()> {
    use super::{EffectGeometry, Renderer, SceneBlend, SceneEffect, SceneQuad};
    use crate::vk::{DepthOrder, EffectDepth, EffectPipelineSpec};
    use paper_scene::shader::{Stage, translate};

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
    if samples == ash::vk::SampleCountFlags::TYPE_4 {
        anyhow::ensure!(
            renderer.scene_sample_count() == samples,
            "qualification GPU does not support scene MSAA4"
        );
    }
    renderer.ensure_scene_pool(3)?;
    let spec = super::SceneTargetSpec {
        depth_attachment: true,
        extent: ash::vk::Extent2D { width: 4, height: 4 },
        format: renderer.scene_format(),
        repeat: false,
        samples,
    };
    let mut target = renderer.create_scene_target_opts(spec)?;
    let mut snapshot = renderer.create_scene_target_opts(spec)?;
    let grab = renderer.create_scene_target(4, 4)?;
    let color_bytes = target.allocation_bytes;
    assert!(target.promoted.is_none());
    renderer.render_scene(&target, [0.0, 1.0, 0.0, 1.0], &[], &[])?;
    renderer.enable_scene_depth(&mut target, DepthOrder::Greater)?;
    renderer.enable_scene_depth(&mut snapshot, DepthOrder::Greater)?;
    assert!(target.allocation_bytes > color_bytes);
    if samples == ash::vk::SampleCountFlags::TYPE_1 {
        assert_eq!(grab.allocation_bytes, color_bytes);
    } else {
        assert!(grab.allocation_bytes < color_bytes);
    }
    let promoted_bytes = target.allocation_bytes;
    renderer.enable_scene_depth(&mut target, DepthOrder::Greater)?;
    assert_eq!(target.allocation_bytes, promoted_bytes);
    let vertices = renderer.create_quad_buffer((4, 4), [0.0, 0.0, 1.0, 1.0], (2.0, 2.0))?;
    let mut textures = Vec::new();
    let mut pipelines = Vec::new();
    for (z, color) in [(0.75, [255, 0, 0, 255]), (0.25, [0, 0, 255, 255])] {
        let vertex = translate(
            &format!(
                "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_Uv;\nvoid main(){{v_Uv=a_TexCoord;gl_Position=vec4(a_Position.xy,{z},1.0);}}"
            ),
            Stage::Vertex,
            &std::collections::BTreeMap::new(),
        );
        let fragment = translate(
            "varying vec2 v_Uv;\nuniform sampler2D g_Texture0;\nvoid main(){if(v_Uv.x<0.5)discard;gl_FragColor=texSample2D(g_Texture0,v_Uv);}",
            Stage::Fragment,
            &std::collections::BTreeMap::new(),
        );
        let mut spec = EffectPipelineSpec::quad(target.format, SceneBlend::Copy, None);
        spec.samples = samples;
        spec.depth =
            EffectDepth { test: true, write: true, cull: false, order: DepthOrder::Greater };
        let pipeline =
            renderer.create_effect_pipeline_for(&vertex, &fragment, None, "lazy depth", spec)?;
        let mut texture = renderer.create_scene_texture(1, 1, &color)?;
        renderer.write_effect_inputs(&pipeline, &[(texture.view, texture.sampler)], &[]);
        texture.effect = Some(SceneEffect::for_geometry(
            &pipeline,
            EffectGeometry::Quad {
                vertices: vertices.buffer,
                offset: crate::vk::effect::MODEL_QUAD_OFFSET,
            },
            None,
        )?);
        textures.push(texture);
        pipelines.push(pipeline);
    }
    textures.push(renderer.create_scene_texture(1, 1, &[0, 255, 0, 255])?);
    let near = SceneQuad {
        rect: [2.0, 2.0, 4.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [1.0; 4],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Copy,
    };
    renderer.begin_scene_batch()?;
    renderer.record_scene_append_with_canvas(
        &target,
        [4.0; 2],
        [1.0; 4],
        std::slice::from_ref(&near),
        &textures,
        &[],
    )?;
    renderer.submit_scene_batch()?;
    let appended = renderer.read_scene_target(&target)?.2;
    assert_eq!(&appended[..4], &[0, 255, 0, 255]);
    assert_eq!(&appended[40..44], &[255, 0, 0, 255]);
    let far = SceneQuad { texture: 1, ..near.clone() };
    let color = SceneQuad { texture: 2, rect: [0.5, 0.5, 1.0, 1.0], ..near.clone() };
    renderer.render_scene(
        &target,
        [0.0; 4],
        &[near.clone(), color.clone(), far.clone()],
        &textures,
    )?;
    let composed = renderer.read_scene_target(&target)?.2;
    assert_eq!(&composed[40..44], &[255, 0, 0, 255]);
    let append_pass = renderer.scene_pass_append_for(snapshot.format, samples)?;
    let grab_pass = renderer.scene_pass_load_for(snapshot.format, samples)?;
    renderer.begin_scene_batch()?;
    renderer.record_scene(&snapshot, [0.0; 4], &[near.clone(), color], &textures);
    unsafe {
        renderer.device.cmd_begin_render_pass(
            renderer.cmd,
            &ash::vk::RenderPassBeginInfo::default()
                .render_pass(append_pass)
                .framebuffer(snapshot.framebuffer)
                .render_area(ash::vk::Rect2D {
                    offset: ash::vk::Offset2D::default(),
                    extent: snapshot.extent,
                }),
            ash::vk::SubpassContents::INLINE,
        );
    }
    renderer.grab_scene(&snapshot, grab.grab(), grab_pass);
    unsafe { renderer.device.cmd_end_render_pass(renderer.cmd) };
    renderer.record_scene_append_with_canvas(
        &snapshot,
        [4.0; 2],
        [1.0; 4],
        std::slice::from_ref(&far),
        &textures,
        &[],
    )?;
    renderer.submit_scene_batch()?;
    assert_eq!(renderer.read_scene_target(&snapshot)?.2, composed);
    assert_eq!(renderer.read_scene_target(&grab)?.2, composed);
    renderer.render_scene(&snapshot, [0.0; 4], &[far], &textures)?;
    assert_eq!(&renderer.read_scene_target(&snapshot)?.2[40..44], &[0, 0, 255, 255]);
    renderer.disable_scene_depth(&mut target)?;
    renderer.disable_scene_depth(&mut snapshot)?;
    assert_eq!(target.allocation_bytes, color_bytes);
    assert_eq!(snapshot.allocation_bytes, color_bytes);
    assert!(target.promoted.is_none());
    renderer.render_scene(&target, [0.0; 4], &[SceneQuad { texture: 2, ..near }], &textures)?;
    assert_eq!(&renderer.read_scene_target(&target)?.2[40..44], &[0, 255, 0, 255]);
    let mut resized = renderer.create_scene_target_opts(super::SceneTargetSpec {
        extent: ash::vk::Extent2D { width: 8, height: 8 },
        ..spec
    })?;
    let resized_color_bytes = resized.allocation_bytes;
    renderer.enable_scene_depth(&mut resized, DepthOrder::Greater)?;
    assert!(resized.allocation_bytes > resized_color_bytes);
    renderer.disable_scene_depth(&mut resized)?;
    assert_eq!(resized.allocation_bytes, resized_color_bytes);
    renderer.destroy_scene_target(resized);
    for texture in textures {
        renderer.destroy_scene_texture(texture);
    }
    for pipeline in pipelines {
        renderer.destroy_effect_pipeline(pipeline);
    }
    renderer.destroy_quad_buffer(vertices);
    for target in [target, snapshot, grab] {
        renderer.destroy_scene_target(target);
    }
    Ok(())
}
