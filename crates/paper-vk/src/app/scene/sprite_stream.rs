use super::vk;
use anyhow::{Context, Result};
use paper_scene::tex::stream::Cell;

pub(super) struct SpriteStream {
    cells: std::sync::Arc<[Cell]>,
    frame: vk::FrameStream,
    scratch: Vec<u8>,
}

impl SpriteStream {
    pub(super) fn create(renderer: &vk::Renderer, cells: std::sync::Arc<[Cell]>) -> Result<Self> {
        let first = cells.first().context("sprite stream has no frames")?;
        let frame = renderer.create_frame_stream(first.width, first.height)?;
        let mut stream = Self { cells, frame, scratch: Vec::new() };
        stream.show(renderer, 0)?;
        Ok(stream)
    }

    pub(super) fn frame(&self) -> &vk::FrameStream {
        &self.frame
    }

    pub(super) fn show(&mut self, renderer: &vk::Renderer, image: i32) -> Result<()> {
        let cell = usize::try_from(image)
            .ok()
            .and_then(|index| self.cells.get(index))
            .context("sprite stream frame missing")?;
        cell.unpack(&mut self.scratch)?;
        renderer.upload_frame(&self.frame, &self.scratch)
    }
}
