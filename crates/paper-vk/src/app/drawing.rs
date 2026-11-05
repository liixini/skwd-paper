use super::shared_support::FadeState;
use crate::vk;
use anyhow::{Result, ensure};
use std::time::Duration;

const TEXTURE_POOL: u32 = 16;
const TEXTURE_LIMIT: u64 = 128 * 1024 * 1024;

pub(crate) struct DrawingTexture {
    pub(crate) texture: vk::SceneTexture,
    pub(crate) strokes: vk::DrawingStrokes,
    pub(crate) focus: [f32; 2],
    pub(crate) colours: paper_control::DrawingColours,
}

impl DrawingTexture {
    pub(crate) fn source<'a>(&'a self, photo: ash::vk::ImageView) -> vk::Src<'a> {
        vk::Src::Drawing {
            photo,
            ink: self.texture.view,
            strokes: &self.strokes,
            focus: self.focus,
            colours: self.colours,
        }
    }
}

pub(super) fn prepare(renderer: &mut vk::Renderer, fade: &mut FadeState) -> Result<()> {
    let Some(drawing) = &fade.drawing else {
        return Ok(());
    };
    ensure!(fade.still_b, "drawing requires a still image");
    let source = std::env::var_os(paper_control::DRAWING_SOURCE_ENV)
        .map_or_else(|| fade.path.clone().into(), std::path::PathBuf::from);
    fade.drawing_texture =
        Some(load(renderer, drawing, &source, Duration::from_millis(fade.dur_ms))?);
    Ok(())
}

pub(crate) fn load(
    renderer: &mut vk::Renderer,
    drawing: &paper_control::DrawingTransition,
    source: &std::path::Path,
    duration: Duration,
) -> Result<DrawingTexture> {
    let (manifest, path, strokes_path) = drawing.load(source)?;
    renderer.ensure_scene_pool(TEXTURE_POOL)?;
    let texture = load_texture(renderer, &path, (manifest.width, manifest.height))?;
    let strokes = renderer.create_drawing_strokes(
        &paper_control::DrawingStroke::read(&strokes_path)?,
        vk::StrokeSpec {
            extent: ash::vk::Extent2D {
                width: manifest.timing_width,
                height: manifest.timing_height,
            },
            ink: drawing.ink,
            seconds: duration.as_secs_f32(),
        },
    )?;
    Ok(DrawingTexture { texture, strokes, focus: manifest.focus, colours: drawing.colours })
}

pub(super) fn replace_photo(
    renderer: &mut vk::Renderer,
    current: &mut super::dmabuf_helpers::RgbaStillSource,
    path: &std::path::Path,
    dims: &[(u32, u32)],
) -> Result<()> {
    ensure!(std::fs::metadata(path)?.len() <= TEXTURE_LIMIT, "drawing photo is too large");
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(TEXTURE_LIMIT);
    reader.limits(limits);
    let pixels = reader.decode()?.into_rgba8();
    let texture =
        renderer.create_scene_texture(pixels.width(), pixels.height(), pixels.as_raw())?;
    let uvs = dims
        .iter()
        .map(|&(width, height)| {
            crate::fill::mode_uv(pixels.width(), pixels.height(), width, height)
        })
        .collect();
    renderer.destroy_scene_texture(std::mem::replace(&mut current.texture, texture));
    current.uvs = uvs;
    Ok(())
}

fn load_texture(
    renderer: &mut vk::Renderer,
    path: &std::path::Path,
    size: (u32, u32),
) -> Result<vk::SceneTexture> {
    ensure!(std::fs::metadata(path)?.len() <= TEXTURE_LIMIT, "drawing texture is too large");
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    ensure!(reader.format() == Some(image::ImageFormat::Png), "drawing texture must be PNG");
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(size.0);
    limits.max_image_height = Some(size.1);
    limits.max_alloc = Some(TEXTURE_LIMIT);
    reader.limits(limits);
    let pixels = reader.decode()?.into_rgba8();
    ensure!(pixels.dimensions() == size, "drawing texture size mismatch");
    renderer.create_scene_texture(size.0, size.1, pixels.as_raw())
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
