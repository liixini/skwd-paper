use super::timestamp_delta_ns;

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

    let shared = crate::shared::create(std::ptr::null_mut())?;
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
        let append_pass = renderer.scene_pass_append_for(target.format)?;
        let grab_pass = renderer.scene_pass_load_for(target.format)?;
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
        renderer.grab_scene(&target, &grab, grab_pass);
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
