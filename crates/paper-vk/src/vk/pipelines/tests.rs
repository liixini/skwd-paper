use crate::vk::{Renderer, Src};

#[test]
#[ignore = "requires a Vulkan device; run explicitly on the renderer qualification host"]
fn limited_bt709_shader_preserves_primary_and_neutral_pixels() -> anyhow::Result<()> {
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

fn translucent_puppet() -> anyhow::Result<paper_scene::puppet::Mesh> {
    let mut bytes = b"MDLV0019\0".to_vec();
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
fn puppet_intermediate_preserves_straight_alpha_colors() -> anyhow::Result<()> {
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
    renderer.ensure_scene_pool(1)?;
    let texture = renderer.create_scene_texture(1, 1, &[200, 100, 50, 128])?;
    let target = renderer.create_scene_target(4, 4)?;
    let mesh = renderer.create_scene_mesh(&translucent_puppet()?, (4.0, 4.0))?;
    let result = (|| {
        renderer.render_scene_mesh(&target, &texture, &mesh)?;
        let (width, height, pixels) = renderer.read_scene_target(&target)?;
        anyhow::ensure!((width, height) == (4, 4), "unexpected dimensions");
        for pixel in pixels.chunks_exact(4) {
            let actual = [pixel[0], pixel[1], pixel[2], pixel[3]];
            anyhow::ensure!(actual == [200, 100, 50, 128], "puppet RGBA changed: {actual:?}");
        }
        Ok(())
    })();
    renderer.destroy_scene_mesh(mesh);
    renderer.destroy_scene_target(target);
    renderer.destroy_scene_texture(texture);
    result
}
