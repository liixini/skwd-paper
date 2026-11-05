use super::{
    FLAG_IS_GIF, MAX_STREAMED_BYTES, MAX_TEX_PAYLOAD_BYTES, Span, Tex, TexFormat, TexFrame,
};
use anyhow::{Result, anyhow, bail};

const CHANNELS: usize = 4;

pub struct Cell {
    pub width: u32,
    pub height: u32,
    packed: Vec<u8>,
}

impl Cell {
    pub fn pack(width: u32, height: u32, rgba: &[u8]) -> Self {
        Self { width, height, packed: lz4_flex::block::compress(rgba) }
    }

    pub fn bytes(&self) -> usize {
        self.width as usize * self.height as usize * CHANNELS
    }

    pub fn packed_bytes(&self) -> usize {
        self.packed.len()
    }

    pub fn unpack(&self, out: &mut Vec<u8>) -> Result<()> {
        out.resize(self.bytes(), 0);
        let written = lz4_flex::block::decompress_into(&self.packed, out)
            .map_err(|err| anyhow!("lz4: {err}"))?;
        if written != out.len() {
            bail!("sprite cell holds {written} of {} bytes", out.len());
        }
        Ok(())
    }
}

pub struct Sprite {
    pub tex: Tex,
    pub cells: Vec<Cell>,
}

pub fn pack(data: &[u8], threshold: usize) -> Result<Option<Sprite>> {
    let (tex, spans) = super::locate(data)?;
    let unpacked: usize = spans.iter().filter_map(|spans| spans.first()).map(Span::size).sum();
    if !streamable(&tex, &spans) || unpacked <= threshold {
        return Ok(None);
    }
    if unpacked > MAX_STREAMED_BYTES {
        bail!("streamed sprite unpacks to {unpacked} bytes; limit is {MAX_STREAMED_BYTES}");
    }
    let mut cells: Vec<Option<Cell>> = tex.frames.iter().map(|_| None).collect();
    let mut packed = 0;
    for (image, spans) in spans.iter().enumerate() {
        let span = spans.first().ok_or_else(|| anyhow!("sprite sheet {image} has no level"))?;
        let sheet = span.read(data)?;
        for (cell, frame) in cells.iter_mut().zip(&tex.frames) {
            if usize::try_from(frame.image_id).ok() == Some(image) {
                let packed_cell = cut(&sheet, span, frame)?;
                packed += packed_cell.packed_bytes();
                if packed > MAX_TEX_PAYLOAD_BYTES {
                    bail!("streamed sprite packs past {MAX_TEX_PAYLOAD_BYTES} bytes");
                }
                *cell = Some(packed_cell);
            }
        }
    }
    let cells = cells
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow!("sprite frame names a missing sheet"))?;
    Ok(Some(Sprite { tex, cells }))
}

pub(super) fn streamable(tex: &Tex, spans: &[Vec<Span>]) -> bool {
    tex.meta.flags & FLAG_IS_GIF != 0
        && tex.meta.format == TexFormat::Rgba8888
        && tex.meta.free_image_format.is_none_or(|format| format < 0)
        && spans.iter().all(|spans| spans.len() == 1)
        && tex.frames.first().is_some_and(|first| {
            tex.frames.iter().all(|frame| whole(frame) && frame.size() == first.size())
        })
}

fn whole(frame: &TexFrame) -> bool {
    frame.frame_time.is_finite()
        && frame.width_y == 0.0
        && frame.height_x == 0.0
        && frame.width > 0.0
        && frame.height > 0.0
        && [frame.x, frame.y, frame.width, frame.height]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
}

impl Span {
    fn size(&self) -> usize {
        self.unpacked.unwrap_or(self.length)
    }

    fn read(&self, data: &[u8]) -> Result<Vec<u8>> {
        let bytes = self
            .offset
            .checked_add(self.length)
            .and_then(|end| data.get(self.offset..end))
            .ok_or_else(|| anyhow!("sprite sheet outside its texture"))?;
        let sheet = match self.unpacked {
            Some(length) => {
                lz4_flex::block::decompress(bytes, length).map_err(|err| anyhow!("lz4: {err}"))?
            }
            None => bytes.to_vec(),
        };
        let needed = self.width as usize * self.height as usize * CHANNELS;
        if sheet.len() < needed {
            bail!("sprite sheet holds {} of {needed} bytes", sheet.len());
        }
        Ok(sheet)
    }
}

fn cut(sheet: &[u8], span: &Span, frame: &TexFrame) -> Result<Cell> {
    let fits = |start: f32, length: f32, limit: u32| start + length <= limit as f32;
    if !fits(frame.x, frame.width, span.width) || !fits(frame.y, frame.height, span.height) {
        bail!("sprite frame outside its sheet");
    }
    let [x, y, width, height] =
        [frame.x, frame.y, frame.width, frame.height].map(|value| value as usize);
    let stride = span.width as usize * CHANNELS;
    let mut cell = Vec::with_capacity(width * height * CHANNELS);
    for row in y..y + height {
        let start = row * stride + x * CHANNELS;
        cell.extend_from_slice(&sheet[start..start + width * CHANNELS]);
    }
    Ok(Cell::pack(width as u32, height as u32, &cell))
}

#[cfg(test)]
#[path = "stream_tests.rs"]
pub(crate) mod tests;
