use super::super::ALPHA_COMPONENT;
use super::*;

fn pixels(rows: &[&str]) -> Vec<u8> {
    rows.iter()
        .flat_map(|row| row.bytes().flat_map(|pixel| [u8::from(pixel == b'#') * u8::MAX, 0, 0, 0]))
        .collect()
}

#[test]
fn preserves_support_union() {
    let rows = [".##...##.", ".#######.", ".#######.", "...###..."];
    let rects = support(&pixels(&rows), rows[0].len()).unwrap();
    let mut coverage = vec![0; rows.len() * rows[0].len()];
    for rect in rects {
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                coverage[y * rows[0].len() + x] += 1;
            }
        }
    }
    assert_eq!(
        coverage,
        rows.iter()
            .flat_map(|row| row.bytes().map(|pixel| u8::from(pixel == b'#')))
            .collect::<Vec<_>>()
    );
}

#[test]
fn merges_equal_runs() {
    assert_eq!(
        support(&pixels(&[".##.", ".##."]), 4).unwrap(),
        [Rect { left: 1, top: 0, right: 3, bottom: 2 }]
    );
}

#[test]
fn retains_transparent_support() {
    let rgba = pixels(&["##"]);
    assert!(rgba.chunks_exact(RGBA_CHANNELS).all(|pixel| pixel[ALPHA_COMPONENT] == 0));
    assert_eq!(support(&rgba, 2).unwrap().len(), 1);
}

#[test]
fn maps_cropped_geometry() {
    let rects = [Rect { left: 1, top: 2, right: 3, bottom: 4 }];
    let (vertices, indices) = geometry(&rects, [4.0, 8.0]).unwrap();
    assert_eq!(
        vertices,
        [
            [-0.25, -0.25, 0.25, 0.25, 1.0],
            [0.25, -0.25, 0.75, 0.25, 1.0],
            [-0.25, 0.0, 0.25, 0.5, 1.0],
            [0.25, 0.0, 0.75, 0.5, 1.0]
        ]
    );
    assert_eq!(indices, [0, 1, 2, 2, 1, 3]);
}

#[test]
fn empty_text_support() {
    assert!(support(&pixels(&["...", "..."]), 3).unwrap().is_empty());
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn clips_fractional_support() -> Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let mut mismatches = 0;
    let mut renderer = headless(16)?;
    let target = renderer.create_scene_target(16, 16)?;
    let mut rgba = pixels(&["........", ".###....", ".#####..", "...###..", "........"]);
    for pixel in rgba.chunks_exact_mut(RGBA_CHANNELS) {
        if pixel[0] != 0 {
            pixel[..ALPHA_COMPONENT].fill(u8::MAX);
            pixel[ALPHA_COMPONENT] = 128;
        }
    }
    let texture = renderer.create_text_texture(
        &Pixels::rgba(8, 5, rgba.clone()),
        MipSampling::Disabled,
        None,
    )?;
    for pixel in rgba.chunks_exact_mut(RGBA_CHANNELS) {
        pixel[..ALPHA_COMPONENT].fill(u8::MAX);
    }
    let reference = renderer.create_scene_texture_pixels(
        &Pixels::rgba(8, 5, rgba),
        true,
        false,
        MipSampling::Disabled,
    )?;
    let rects = [[1.0, 1.0, 3.0, 1.0], [1.0, 2.0, 5.0, 1.0], [3.0, 3.0, 3.0, 1.0]];
    let mut cases = Vec::new();
    for center in [[8.0, 8.0], [8.25, 8.5]] {
        let mut quad = SceneQuad {
            rect: [center[0], center[1], 8.0, 5.0],
            uv: [0.0, 0.0, 1.0, 1.0],
            uv_axes: None,
            uv_padding: false,
            tint: [0.25, 0.5, 1.0, 0.75],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: 0,
            blend: SceneBlend::Alpha,
        };
        cases.push(quad.clone());
        quad.angle = 0.31;
        quad.rect[2..].copy_from_slice(&[12.0, 7.5]);
        cases.push(quad.clone());
        quad.blend = SceneBlend::Copy;
        quad.projection = Some([
            [0.75, 0.125, 0.0, 0.03125],
            [-0.125, 0.5, 0.0, 0.0625],
            [0.0625, -0.03125, 0.0, 1.0],
        ]);
        cases.push(quad.clone());
        quad.uv = [0.125, 0.25, 0.5, 0.5];
        quad.uv_axes = Some([0.5, 0.125, -0.125, 0.5]);
        quad.uv_padding = true;
        cases.push(quad);
    }
    for (variant, quad) in cases.into_iter().enumerate() {
        renderer.render_scene(
            &target,
            [0.0; 4],
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        let (_, _, actual) = renderer.read_scene_target(&target)?;
        let cropped: Vec<_> = rects
            .iter()
            .map(|[x, y, width, height]| {
                cropped_quad(&quad, [x / 8.0, y / 5.0, width / 8.0, height / 5.0], [8.0, 5.0])
            })
            .collect();
        renderer.render_scene(&target, [0.0; 4], &cropped, std::slice::from_ref(&reference))?;
        let (_, _, expected) = renderer.read_scene_target(&target)?;
        mismatches += usize::from(!compare_frame(
            ("clips_fractional_support", &format!("case-{variant}")),
            16,
            (&actual, &expected),
            &quad,
        )?);
    }
    renderer.destroy_scene_texture(texture);
    renderer.destroy_scene_texture(reference);
    renderer.destroy_scene_target(target);
    assert_eq!(mismatches, 0, "differing rendered variants");
    Ok(())
}

#[test]
fn widens_text_indices() {
    const RECTS: usize = 16_385;
    let row = [u8::MAX, 0, 0, 0, 0, 0, 0, 0];
    let rgba = row.repeat(RECTS);
    let rects = support(&rgba, RECTS * 2).unwrap();
    let (vertices, indices) = geometry(&rects, [(RECTS * 2) as f32, 1.0]).unwrap();
    assert_eq!(rects.len(), RECTS);
    assert_eq!(vertices.len(), RECTS * RECT_VERTICES);
    assert_eq!(indices.iter().copied().max(), Some(65_539));
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn text_transform_sampling() -> Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let mut mismatches = 0;
    let mut renderer = headless(32)?;
    renderer.tex_mips = true;
    let target = renderer.create_scene_target(32, 32)?;
    let rgba: Vec<_> =
        (0..64).flat_map(|index| [255, 255, 255, if index % 3 == 0 { 0 } else { 255 }]).collect();
    let pixels = Pixels::rgba(8, 8, rgba);
    for sampling in [MipSampling::Linear, MipSampling::Disabled] {
        let mut text = renderer.create_text_texture(&pixels, sampling, None)?;
        let reference = renderer.create_scene_texture_pixels(&pixels, true, false, sampling)?;
        assert_eq!(text.sampler, reference.sampler);
        assert_eq!(text.sampler == renderer.sampler_mip, sampling == MipSampling::Linear);
        let mut quad = SceneQuad {
            rect: [16.25, 14.5, 13.75, 8.25],
            uv: [0.125, 0.25, 0.5, 0.625],
            uv_axes: None,
            uv_padding: false,
            tint: [0.25, 0.5, 1.0, 0.75],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: 0,
            blend: SceneBlend::Alpha,
        };
        let mut cases = vec![quad.clone()];
        quad.angle = 0.31;
        cases.push(quad.clone());
        quad.uv_axes = Some([0.5, 0.125, -0.25, 0.5]);
        quad.uv_padding = true;
        cases.push(quad.clone());
        quad.rect = [16.25, 14.5, 2.75, 3.25];
        cases.push(quad.clone());
        let geometry = paper_scene::model::Geometry {
            spatial: None,
            depth_scale: 1.0,
            transform: paper_scene::affine::Affine::new([14.25, 17.5], [0.75, 1.25], 0.23),
            size: [13.75, 8.25],
            offset: [0.25, -0.5],
        };
        quad.projection = Some(geometry.projection((32.0, 32.0)));
        cases.push(quad);
        for (variant, quad) in cases.into_iter().enumerate() {
            renderer.render_scene(
                &target,
                [0.0; 4],
                std::slice::from_ref(&quad),
                std::slice::from_ref(&text),
            )?;
            let (_, _, actual) = renderer.read_scene_target(&target)?;
            renderer.render_scene(
                &target,
                [0.0; 4],
                std::slice::from_ref(&quad),
                std::slice::from_ref(&reference),
            )?;
            let (_, _, expected) = renderer.read_scene_target(&target)?;
            mismatches += usize::from(!compare_frame(
                (
                    "text_transform_sampling",
                    &format!(
                        "sampling-{}-case-{variant}",
                        usize::from(sampling == MipSampling::Linear)
                    ),
                ),
                32,
                (&actual, &expected),
                &quad,
            )?);
            let replacement = renderer.create_text_texture(&pixels, sampling, None)?;
            renderer.destroy_scene_texture(std::mem::replace(&mut text, replacement));
        }
        renderer.destroy_scene_texture(text);
        renderer.destroy_scene_texture(reference);
    }
    renderer.destroy_scene_target(target);
    assert_eq!(mismatches, 0, "differing rendered variants");
    Ok(())
}

#[cfg(feature = "shared-device")]
fn headless(size: u32) -> Result<Renderer> {
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
        size,
        size,
    )?;
    renderer.ensure_scene_pipelines()?;
    renderer.ensure_scene_pool(4)?;
    Ok(renderer)
}

#[cfg(feature = "shared-device")]
fn cropped_quad(
    quad: &crate::vk::SceneQuad,
    rect: [f32; 4],
    texture: [f32; 2],
) -> crate::vk::SceneQuad {
    let [u, v, w, h] = rect;
    let offset = [u + w * 0.5 - 0.5, v + h * 0.5 - 0.5];
    let dx = offset[0] * quad.rect[2];
    let dy = offset[1] * quad.rect[3];
    let (sin, cos) = quad.angle.sin_cos();
    let mut cropped = quad.clone();
    cropped.rect = [
        quad.rect[0] + dx * cos - dy * sin,
        quad.rect[1] + dx * sin + dy * cos,
        quad.rect[2] * w,
        quad.rect[3] * h,
    ];
    cropped.uv =
        [quad.uv[0] + u * quad.uv[2], quad.uv[1] + v * quad.uv[3], quad.uv[2] * w, quad.uv[3] * h];
    if let Some([ux, vx, uy, vy]) = quad.uv_axes {
        let inset = if quad.uv_padding {
            texture.map(|edge| paper_scene::model::IMAGE_UV_INSET / edge)
        } else {
            [0.0; 2]
        };
        let scale = inset.map(|inset| 1.0 - 2.0 * inset);
        let left = inset[0] + u * scale[0];
        let top = inset[1] + v * scale[1];
        cropped.uv =
            [quad.uv[0] + left * ux + top * uy, quad.uv[1] + left * vx + top * vy, 1.0, 1.0];
        cropped.uv_axes =
            Some([ux * w * scale[0], vx * w * scale[0], uy * h * scale[1], vy * h * scale[1]]);
        cropped.uv_padding = false;
    }
    cropped.projection = quad.projection.map(|rows| {
        rows.map(|row| {
            [row[0] * w, row[1] * h, row[2], row[3] + row[0] * offset[0] + row[1] * offset[1]]
        })
    });
    cropped
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn puppet_uv_precompose() -> Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let mut renderer = headless(4)?;
    let target = renderer.create_scene_target(4, 4)?;
    let texture = renderer.create_scene_texture(
        2,
        2,
        &[200, 100, 50, 128, 20, 160, 80, 192, 40, 80, 240, 64, 100, 40, 200, 255],
    )?;
    let vertices = [
        [-0.5, -0.5, 0.0, 0.0, 1.0],
        [0.5, -0.5, 1.0, 0.0, 1.0],
        [-0.5, 0.5, 0.0, 1.0, 1.0],
        [0.5, 0.5, 1.0, 1.0, 1.0],
    ];
    let mesh =
        renderer.create_scene_mesh(vertices.into_iter(), SceneIndices::U16(&[0, 1, 2, 2, 1, 3]))?;
    renderer.render_scene_mesh(&target, &texture, &mesh)?;
    let (_, _, actual) = renderer.read_scene_target(&target)?;
    let batch = paper_scene::text::batch::Batch {
        canvas: [2, 2],
        pixels: Pixels::rgba(2, 2, vec![255; 16]),
        quads: vec![paper_scene::text::batch::Quad { bounds: [0, 0, 2, 2], uv: [0, 0, 2, 2] }],
    };
    let flat = Pixels::rgba(
        2,
        2,
        vec![200, 100, 50, 128, 20, 160, 80, 192, 40, 80, 240, 64, 100, 40, 200, 255],
    );
    let text = renderer.create_text_texture(&flat, MipSampling::Disabled, Some(&batch))?;
    renderer.render_scene_mesh(&target, &text, &mesh)?;
    let (_, _, logical) = renderer.read_scene_target(&target)?;
    assert_eq!(logical, actual);
    renderer.destroy_scene_texture(text);
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
    let (_, _, expected) = renderer.read_scene_target(&target)?;
    assert_eq!(actual, expected);
    assert_ne!(&actual[..4], &actual[12..16]);
    renderer.destroy_scene_texture(texture);
    renderer.destroy_scene_target(target);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn replaces_text_support() -> Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let mut mismatches = 0;
    let mut renderer = headless(16)?;
    let target = renderer.create_scene_target(16, 16)?;
    let original = ["........", ".###....", ".#####..", "...###..", "........"];
    let changed = ["........", ".....##.", ".....##.", "........", "........"];
    let empty = ["........"; 5];
    let original_rects = vec![[1.0, 1.0, 3.0, 1.0], [1.0, 2.0, 5.0, 1.0], [3.0, 3.0, 3.0, 1.0]];
    let quad = SceneQuad {
        rect: [8.25, 8.5, 8.0, 5.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [0.25, 0.5, 1.0, 0.75],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Alpha,
    };
    let mut texture = renderer.create_text_texture(
        &Pixels::rgba(8, 5, pixels(&empty)),
        MipSampling::Disabled,
        None,
    )?;
    let mut frames = Vec::new();
    for (variant, (rows, rects)) in [
        (original, original_rects.clone()),
        (changed, vec![[5.0, 1.0, 2.0, 2.0]]),
        (empty, Vec::new()),
        (original, original_rects),
    ]
    .into_iter()
    .enumerate()
    {
        let mut rgba = pixels(&rows);
        for pixel in rgba.chunks_exact_mut(RGBA_CHANNELS) {
            if pixel[0] != 0 {
                pixel[..ALPHA_COMPONENT].fill(u8::MAX);
                pixel[ALPHA_COMPONENT] = 128;
            }
        }
        let replacement = renderer.create_text_texture(
            &Pixels::rgba(8, 5, rgba.clone()),
            MipSampling::Disabled,
            None,
        )?;
        renderer.destroy_scene_texture(std::mem::replace(&mut texture, replacement));
        renderer.render_scene(
            &target,
            [0.0; 4],
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        let (_, _, actual) = renderer.read_scene_target(&target)?;
        for pixel in rgba.chunks_exact_mut(RGBA_CHANNELS) {
            pixel[..ALPHA_COMPONENT].fill(u8::MAX);
        }
        let reference = renderer.create_scene_texture_pixels(
            &Pixels::rgba(8, 5, rgba),
            true,
            false,
            MipSampling::Disabled,
        )?;
        let cropped: Vec<_> = rects
            .into_iter()
            .map(|[x, y, w, h]| {
                cropped_quad(&quad, [x / 8.0, y / 5.0, w / 8.0, h / 5.0], [8.0, 5.0])
            })
            .collect();
        renderer.render_scene(&target, [0.0; 4], &cropped, std::slice::from_ref(&reference))?;
        let (_, _, expected) = renderer.read_scene_target(&target)?;
        mismatches += usize::from(!compare_frame(
            ("replaces_text_support", &format!("case-{variant}")),
            16,
            (&actual, &expected),
            &quad,
        )?);
        frames.push(actual);
        renderer.destroy_scene_texture(reference);
    }
    assert_ne!(frames[0], frames[1]);
    assert!(frames[2].iter().all(|byte| *byte == 0));
    assert!(frames[0] == frames[3], "restored frame differs");
    renderer.destroy_scene_texture(texture);
    renderer.destroy_scene_target(target);
    assert_eq!(mismatches, 0, "differing rendered variants");
    Ok(())
}

#[cfg(feature = "shared-device")]
fn compare_frame(
    case: (&str, &str),
    size: u32,
    frames: (&[u8], &[u8]),
    quad: &crate::vk::SceneQuad,
) -> Result<bool> {
    let (actual, expected) = frames;
    assert_eq!(actual.len(), expected.len());
    let mut changed = 0;
    let mut channels = [0_usize; RGBA_CHANNELS];
    let mut maximum = [0_u8; RGBA_CHANNELS];
    let mut bounds = [size, size, 0, 0];
    for (index, (left, right)) in
        actual.chunks_exact(RGBA_CHANNELS).zip(expected.chunks_exact(RGBA_CHANNELS)).enumerate()
    {
        if left != right {
            changed += 1;
            let index = u32::try_from(index)?;
            let (x, y) = (index % size, index / size);
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        }
        for channel in 0..RGBA_CHANNELS {
            channels[channel] += usize::from(left[channel] != right[channel]);
            maximum[channel] = maximum[channel].max(left[channel].abs_diff(right[channel]));
        }
    }
    let report = serde_json::json!({
        "test": case.0, "variant": case.1, "changed_pixels": changed,
        "changed_channels": channels, "maximum_channel_difference": maximum,
        "bbox_inclusive": (changed != 0).then_some(bounds),
        "size": [size, size], "rect": quad.rect, "uv": quad.uv,
        "uv_axes": quad.uv_axes, "uv_padding": quad.uv_padding,
        "angle": quad.angle, "projection": quad.projection,
    });
    eprintln!("{report}");
    if let Some(root) = std::env::var_os("SKWD_WE_TEXT_DIAGNOSTICS") {
        let directory = std::path::PathBuf::from(root).join(case.0).join(case.1);
        std::fs::create_dir_all(directory.parent().unwrap())?;
        std::fs::create_dir(&directory)?;
        for (name, bytes) in [("actual", actual), ("expected", expected)] {
            std::fs::write(directory.join(format!("{name}.rgba")), bytes)?;
            image::save_buffer(
                directory.join(format!("{name}.png")),
                bytes,
                size,
                size,
                image::ColorType::Rgba8,
            )?;
        }
        std::fs::write(directory.join("comparison.json"), serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(changed == 0)
}

fn overlap_batch() -> paper_scene::text::batch::Batch {
    use paper_scene::text::batch::{Batch, Quad};
    let mut atlas = vec![0; 6 * 4 * RGBA_CHANNELS];
    for y in 1..3 {
        for x in 1..5 {
            let start = (y * 6 + x) * RGBA_CHANNELS;
            atlas[start..start + RGBA_CHANNELS].copy_from_slice(&[255, 255, 255, 128]);
        }
    }
    Batch {
        canvas: [8, 4],
        pixels: Pixels::rgba(6, 4, atlas),
        quads: vec![
            Quad { bounds: [1, 1, 4, 2], uv: [1, 1, 4, 2] },
            Quad { bounds: [3, 1, 4, 2], uv: [1, 1, 4, 2] },
        ],
    }
}

#[test]
fn preserves_glyph_overlap() {
    let batch = overlap_batch();
    let (vertices, indices) = glyph_geometry(&batch).unwrap();
    assert_eq!(vertices.len(), 8);
    assert_eq!(indices, [0, 1, 2, 2, 1, 3, 4, 5, 6, 6, 5, 7]);
    assert_eq!(vertices[0], [0.0, 0.0, 1.0, 1.0, 4.0, 2.0, 1.0, 1.0, 4.0, 2.0, 8.0, 4.0]);
    assert_eq!(&vertices[4][2..6], &[3.0, 1.0, 4.0, 2.0]);
}

#[test]
fn accumulates_text_input() {
    use ash::vk::BlendFactor;
    let state = blend_state(super::super::SceneBlend::Copy);
    assert!(state.src_color_blend_factor == BlendFactor::SRC_ALPHA);
    assert!(state.dst_color_blend_factor == BlendFactor::ONE_MINUS_SRC_ALPHA);
    assert!(state.src_alpha_blend_factor == BlendFactor::SRC_ALPHA);
    assert!(state.dst_alpha_blend_factor == BlendFactor::ONE_MINUS_SRC_ALPHA);
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn samples_glyph_overlap() -> Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let mut renderer = headless(16)?;
    let target = renderer.create_scene_target(16, 16)?;
    let batch = overlap_batch();
    let flat = Pixels::rgba(8, 4, vec![0; 8 * 4 * RGBA_CHANNELS]);
    let texture = renderer.create_text_texture(&flat, MipSampling::Disabled, Some(&batch))?;
    let mut quad = SceneQuad {
        rect: [8.0, 8.0, 8.0, 4.0],
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_axes: None,
        uv_padding: false,
        tint: [1.0, 1.0, 1.0, 0.5],
        angle: 0.0,
        projection: None,
        order_bias: 0,
        texture: 0,
        blend: SceneBlend::Alpha,
    };
    for (blend, clear, expected) in [
        (SceneBlend::Alpha, [0.0; 4], [112, 112, 112, 112]),
        (SceneBlend::Copy, [0.0; 4], [112, 112, 112, 28]),
        (SceneBlend::Copy, [0.0, 0.0, 0.0, 0.5], [112, 112, 112, 100]),
        (SceneBlend::Copy, [0.25, 0.5, 0.75, 0.0], [148, 184, 219, 28]),
        (SceneBlend::CopyRgb, [0.0, 0.0, 0.0, 0.5], [255, 255, 255, 128]),
    ] {
        quad.blend = blend;
        renderer.render_scene(
            &target,
            clear,
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        let (_, _, frame) = renderer.read_scene_target(&target)?;
        let start = (7 * 16 + 7) * RGBA_CHANNELS;
        for (actual, expected) in frame[start..start + RGBA_CHANNELS].iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 1, "overlap channel");
        }
    }
    quad.blend = SceneBlend::Alpha;
    quad.rect[0] = 8.25;
    renderer.render_scene(
        &target,
        [0.0; 4],
        std::slice::from_ref(&quad),
        std::slice::from_ref(&texture),
    )?;
    let (_, _, fractional) = renderer.read_scene_target(&target)?;
    assert!(fractional[(7 * 16 + 7) * RGBA_CHANNELS].abs_diff(100) <= 1, "filtered overlap");
    quad.rect[0] = 8.0;
    quad.uv = [0.5, 0.5, 0.0, 0.0];
    renderer.render_scene(
        &target,
        [0.0; 4],
        std::slice::from_ref(&quad),
        std::slice::from_ref(&texture),
    )?;
    let (_, _, frame) = renderer.read_scene_target(&target)?;
    for y in 6..10 {
        for x in 4..12 {
            assert!(frame[(y * 16 + x) * RGBA_CHANNELS].abs_diff(112) <= 1, "singular uv overlap");
        }
    }
    renderer.destroy_scene_texture(texture);
    renderer.destroy_scene_target(target);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn empty_glyphs_preserve_target() -> Result<()> {
    use crate::vk::{SceneBlend, SceneQuad};
    let mut renderer = headless(16)?;
    let target = renderer.create_scene_target(16, 16)?;
    let mut batch = overlap_batch();
    let flat = Pixels::rgba(8, 4, vec![0; 8 * 4 * RGBA_CHANNELS]);
    let mut texture = renderer.create_text_texture(&flat, MipSampling::Disabled, Some(&batch))?;
    let mut quad = SceneQuad {
        rect: [8.0, 8.0, 8.0, 4.0],
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
    let clear = [0.25, 0.5, 0.75, 0.5];
    renderer.render_scene(&target, clear, &[], &[])?;
    let (_, _, expected) = renderer.read_scene_target(&target)?;
    batch.quads.clear();
    for blend in SceneBlend::ALL {
        let replacement =
            renderer.create_text_texture(&flat, MipSampling::Disabled, Some(&batch))?;
        renderer.destroy_scene_texture(std::mem::replace(&mut texture, replacement));
        quad.blend = blend;
        renderer.render_scene(
            &target,
            clear,
            std::slice::from_ref(&quad),
            std::slice::from_ref(&texture),
        )?;
        let (_, _, actual) = renderer.read_scene_target(&target)?;
        assert_eq!(actual, expected);
    }
    let replacement =
        renderer.create_text_texture(&flat, MipSampling::Disabled, Some(&overlap_batch()))?;
    renderer.destroy_scene_texture(std::mem::replace(&mut texture, replacement));
    quad.blend = SceneBlend::Alpha;
    renderer.render_scene(
        &target,
        clear,
        std::slice::from_ref(&quad),
        std::slice::from_ref(&texture),
    )?;
    let (_, _, restored) = renderer.read_scene_target(&target)?;
    assert_ne!(restored, expected);
    renderer.destroy_scene_texture(texture);
    renderer.destroy_scene_target(target);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn glyph_texture_lifetimes() -> Result<()> {
    let mut renderer = headless(16)?;
    let baseline = crate::vk::Renderer::scene_texture_live_allocations();
    let flat = Pixels::rgba(8, 4, vec![0; 8 * 4 * RGBA_CHANNELS]);
    for empty in [false, true] {
        for explicit in [false, true] {
            let mut batch = overlap_batch();
            if empty {
                batch.quads.clear();
            }
            let texture =
                renderer.create_text_texture(&flat, MipSampling::Disabled, Some(&batch))?;
            assert_eq!(
                crate::vk::Renderer::scene_texture_live_allocations(),
                baseline + if empty { 1 } else { 2 }
            );
            if explicit {
                renderer.destroy_scene_texture(texture);
            } else {
                drop(texture);
            }
            assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), baseline);
        }
    }
    let slot = renderer.create_view_slot()?;
    assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), baseline);
    renderer.destroy_scene_texture(slot);
    assert_eq!(crate::vk::Renderer::scene_texture_live_allocations(), baseline);
    Ok(())
}
