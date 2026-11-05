use super::{
    MipSampling, PUPPET_VERTEX_FLOATS, PixelFormat, Pixels, RGBA_CHANNELS, Renderer, SceneIndices,
    SceneTexture,
};
use anyhow::{Context, Result, ensure};

pub(in crate::vk) const TEXT_VERTEX_FLOATS: usize = 12;
pub(in crate::vk) const TEXT_VERTEX_BYTES: u32 =
    std::mem::size_of::<[f32; TEXT_VERTEX_FLOATS]>() as u32;

const RECT_VERTICES: usize = 4;
const RECT_INDICES: [u32; 6] = [0, 1, 2, 2, 1, 3];

#[derive(Debug, PartialEq, Eq)]
struct Rect {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

fn support(pixels: &[u8], width: usize) -> Result<Vec<Rect>> {
    ensure!(width > 0 && pixels.len().is_multiple_of(width * RGBA_CHANNELS), "invalid text pixels");
    let mut rects: Vec<Rect> = Vec::new();
    let mut previous: Vec<((usize, usize), usize)> = Vec::new();
    let mut current = Vec::new();
    for (y, row) in pixels.chunks_exact(width * RGBA_CHANNELS).enumerate() {
        current.clear();
        let mut x = 0;
        while x < width {
            if row[x * RGBA_CHANNELS] == 0 {
                x += 1;
                continue;
            }
            let left = x;
            while x < width && row[x * RGBA_CHANNELS] != 0 {
                x += 1;
            }
            let range = (left, x);
            let index = if let Ok(slot) = previous.binary_search_by_key(&range, |entry| entry.0) {
                let index = previous[slot].1;
                rects[index].bottom = y + 1;
                index
            } else {
                rects.push(Rect { left, top: y, right: x, bottom: y + 1 });
                rects.len() - 1
            };
            current.push((range, index));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    Ok(rects)
}

fn geometry(
    rects: &[Rect],
    size: [f32; 2],
) -> Result<(Vec<[f32; PUPPET_VERTEX_FLOATS]>, Vec<u32>)> {
    let mut vertices = Vec::with_capacity(rects.len() * RECT_VERTICES);
    let mut indices = Vec::with_capacity(rects.len() * RECT_INDICES.len());
    for rect in rects {
        let base = u32::try_from(vertices.len())?;
        for [x, y] in [
            [rect.left, rect.top],
            [rect.right, rect.top],
            [rect.left, rect.bottom],
            [rect.right, rect.bottom],
        ] {
            let u = x as f32 / size[0];
            let v = y as f32 / size[1];
            vertices.push([u - 0.5, v - 0.5, u, v, 1.0]);
        }
        indices.extend(RECT_INDICES.map(|index| base + index));
    }
    Ok((vertices, indices))
}

impl Renderer {
    pub fn create_text_texture(
        &mut self,
        pixels: &Pixels,
        sampling: MipSampling,
        batch: Option<&paper_scene::text::batch::Batch>,
    ) -> Result<SceneTexture> {
        if let Some(batch) = batch {
            return self.create_glyph_texture(pixels, batch);
        }
        ensure!(
            pixels.format == PixelFormat::Rgba8 && pixels.levels.len() == 1,
            "invalid text format"
        );
        let level = pixels.levels.first().context("text pixels unavailable")?;
        let rects = support(&level.data, level.width as usize)?;
        if rects.is_empty() {
            return self.create_scene_texture_pixels(pixels, true, false, sampling);
        }
        let (vertices, indices) =
            geometry(&rects, [pixels.width() as f32, pixels.height() as f32])?;
        let mesh = self.create_scene_mesh(vertices.into_iter(), SceneIndices::U32(&indices))?;
        let mut texture = self.create_scene_texture_pixels(pixels, true, false, sampling)?;
        texture.use_scene_mesh(&mesh);
        texture.allocation_bytes += mesh.allocation_bytes;
        texture.text_mesh = Some(mesh);
        Ok(texture)
    }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;

fn glyph_geometry(
    batch: &paper_scene::text::batch::Batch,
) -> Result<(Vec<[f32; TEXT_VERTEX_FLOATS]>, Vec<u32>)> {
    let mut vertices = Vec::with_capacity(batch.quads.len() * RECT_VERTICES);
    let mut indices = Vec::with_capacity(batch.quads.len() * RECT_INDICES.len());
    for quad in &batch.quads {
        let base = u32::try_from(vertices.len())?;
        let [x, y, width, height] = quad.bounds.map(|value| value as f32);
        let [atlas_x, atlas_y, atlas_width, atlas_height] = quad.uv.map(|value| value as f32);
        for [u, v] in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
            vertices.push([
                u,
                v,
                x,
                y,
                width,
                height,
                atlas_x,
                atlas_y,
                atlas_width,
                atlas_height,
                batch.canvas[0] as f32,
                batch.canvas[1] as f32,
            ]);
        }
        indices.extend(RECT_INDICES.map(|index| base + index));
    }
    Ok((vertices, indices))
}

impl Renderer {
    fn create_glyph_texture(
        &mut self,
        pixels: &Pixels,
        batch: &paper_scene::text::batch::Batch,
    ) -> Result<SceneTexture> {
        ensure!(batch.canvas == [pixels.width(), pixels.height()], "text canvas mismatch");
        let limit = unsafe { self.instance.get_physical_device_properties(self.phys) }
            .limits
            .max_image_dimension2_d;
        ensure!(
            batch.pixels.width() <= limit && batch.pixels.height() <= limit,
            "text atlas exceeds device extent"
        );
        let mut texture =
            self.create_scene_texture_pixels(pixels, true, false, MipSampling::Disabled)?;
        if batch.quads.is_empty() {
            texture.empty_text = true;
            return Ok(texture);
        }
        let atlas =
            self.create_scene_texture_pixels(&batch.pixels, true, false, MipSampling::Disabled)?;
        let (vertices, indices) = glyph_geometry(batch)?;
        let mesh = self.create_scene_mesh(vertices.into_iter(), SceneIndices::U32(&indices))?;
        texture.use_scene_mesh(&mesh);
        texture.allocation_bytes += mesh.allocation_bytes + atlas.allocation_bytes;
        texture.text_mesh = Some(mesh);
        texture.text_atlas = Some(Box::new(atlas));
        Ok(texture)
    }
}

pub(in crate::vk) fn vertex_attributes() -> [ash::vk::VertexInputAttributeDescription; 4] {
    const ATTRIBUTES: [(ash::vk::Format, u32); 4] = [
        (ash::vk::Format::R32G32_SFLOAT, 0),
        (ash::vk::Format::R32G32B32A32_SFLOAT, 2),
        (ash::vk::Format::R32G32B32A32_SFLOAT, 6),
        (ash::vk::Format::R32G32_SFLOAT, 10),
    ];
    std::array::from_fn(|index| {
        ash::vk::VertexInputAttributeDescription::default()
            .location(index as u32)
            .binding(0)
            .format(ATTRIBUTES[index].0)
            .offset(ATTRIBUTES[index].1 * std::mem::size_of::<f32>() as u32)
    })
}

pub(in crate::vk) fn blend_state(
    blend: super::SceneBlend,
) -> ash::vk::PipelineColorBlendAttachmentState {
    let state = super::scene_blend_state(blend);
    if blend != super::SceneBlend::Copy {
        return state;
    }
    state
        .blend_enable(true)
        .src_color_blend_factor(ash::vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(ash::vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .src_alpha_blend_factor(ash::vk::BlendFactor::SRC_ALPHA)
        .dst_alpha_blend_factor(ash::vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(ash::vk::BlendOp::ADD)
        .alpha_blend_op(ash::vk::BlendOp::ADD)
}
