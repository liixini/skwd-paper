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
