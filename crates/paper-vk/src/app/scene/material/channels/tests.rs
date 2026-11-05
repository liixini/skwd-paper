use super::*;

#[test]
#[ignore = "requires authored channel packages, video fixture and Vulkan"]
fn changing_channel_inputs() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_CHANNEL_DIR").unwrap());
    let video = std::fs::read(std::env::var_os("SKWD_CHANNEL_VIDEO").unwrap())?;
    let shared = crate::shared::test_device();
    for system in [false, true] {
        let dir = root.join("unit");
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let channel = model.layers[0]
            .material
            .as_mut()
            .unwrap()
            .skin
            .as_mut()
            .unwrap()
            .channels
            .as_mut()
            .unwrap();
        let source = channel.pass.textures[0].as_mut().unwrap();
        if system {
            *source = paper_scene::model::MediaTexture::Current.texture();
        } else {
            source.video = Some(paper_scene::model::Video::Bytes(video.clone().into()));
        }
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        let material = &mut group.materials[0];
        let channel = material.channels.as_mut().unwrap();
        assert!(channel.dynamic);
        let weights = material.palette.as_ref().unwrap().channels.clone();
        let cached = channel.sampled.clone();
        assert_eq!(cached.as_deref(), Some(weights.as_slice()));
        let slot = channel.inputs[0];
        let original = (group.textures[slot].view, group.textures[slot].sampler);
        for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
            let replacement = group.renderer.create_scene_texture(1, 1, &color)?;
            group.textures[slot].view = replacement.view;
            group.textures[slot].sampler = replacement.sampler;
            channel.render(
                &mut group.renderer,
                &group.textures,
                FrameClock { time: 1.0, dt: 0.0, daytime: 0.0 },
                &weights,
                false,
            )?;
            let (width, height, pixels) =
                group.renderer.read_scene_target(channel.gpu.target.as_ref().unwrap())?;
            let center = ((height / 2 * width + width / 2) * 4) as usize;
            assert_eq!(&pixels[center..center + 4], &color, "system={system}");
            assert_eq!(channel.sampled, cached);
            group.textures[slot].view = original.0;
            group.textures[slot].sampler = original.1;
            group.renderer.destroy_scene_texture(replacement);
        }
        group.destroy();
    }
    Ok(())
}
