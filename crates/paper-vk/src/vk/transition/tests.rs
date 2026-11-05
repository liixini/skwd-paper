use super::*;
use crate::vk::Src;

const SIZE: [u32; 2] = [96, 64];
const UV: [f32; 4] = [1.0, 1.0, 0.0, 0.0];
const STEPS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn gradient(blue: u8) -> Vec<u8> {
    image::RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        image::Rgba([(x * 255 / SIZE[0]) as u8, (y * 255 / SIZE[1]) as u8, blue, u8::MAX])
    })
    .into_raw()
}

#[test]
#[ignore = "requires a Vulkan device"]
fn studio_endpoints_match() -> Result<()> {
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
        SIZE[0],
        SIZE[1],
    )?;
    let target = renderer.create_render_target(SIZE[0], SIZE[1])?;
    let mut export = renderer.create_export_image(SIZE[0], SIZE[1])?;
    let readback = renderer.create_readback_buf(u64::from(SIZE[0]) * u64::from(SIZE[1]) * 4)?;
    let first = renderer.create_scene_texture(SIZE[0], SIZE[1], &gradient(30))?;
    let second = renderer.create_scene_texture(SIZE[0], SIZE[1], &gradient(200))?;
    let (first_src, second_src) = (Src::Rgba(first.view), Src::Rgba(second.view));
    let textures = Renderer::scene_texture_live_allocations();
    let read = |renderer: &mut Renderer, export: &crate::vk::ExportImage| -> Result<Vec<u8>> {
        renderer.read_export_to(export, &readback, SIZE[0], SIZE[1])?;
        Ok(unsafe { std::slice::from_raw_parts(readback.ptr, readback.size as usize).to_vec() })
    };
    renderer.render_to(&target, &mut export, &first_src, UV)?;
    let start = read(&mut renderer, &export)?;
    renderer.render_to(&target, &mut export, &second_src, UV)?;
    let end = read(&mut renderer, &export)?;
    for spec in paper_control::transitions::STUDIO_TRANSITIONS {
        let effect = paper_shaders::effect_index(spec.id).expect("studio effect");
        for progress in STEPS {
            renderer.render_effect_to(
                &target,
                &mut export,
                &first_src,
                UV,
                &second_src,
                UV,
                progress,
                effect,
            )?;
            let pixels = read(&mut renderer, &export)?;
            if progress == 0.0 {
                assert_eq!(pixels, start, "{} start", spec.id);
            }
            if progress == 1.0 {
                assert_eq!(pixels, end, "{} end", spec.id);
            }
        }
        renderer.clear_transition()?;
        let cleared = renderer.transition.as_ref().is_none_or(|state| state.environment.is_none());
        assert!(cleared, "{} cleared", spec.id);
        assert_eq!(Renderer::scene_texture_live_allocations(), textures, "{} released", spec.id);
    }
    let effect = paper_control::transitions::STUDIO_TRANSITIONS
        .iter()
        .filter_map(|spec| paper_shaders::effect_index(spec.id))
        .find(|effect| paper_shaders::effect_requires_environment(*effect))
        .expect("environment effect");
    renderer.prepare_transition(effect)?;
    assert_eq!(Renderer::scene_texture_live_allocations(), textures + 1);
    renderer.transition = None;
    assert_eq!(Renderer::scene_texture_live_allocations(), textures, "dropped");
    renderer.destroy_scene_texture(first);
    renderer.destroy_scene_texture(second);
    Ok(())
}
