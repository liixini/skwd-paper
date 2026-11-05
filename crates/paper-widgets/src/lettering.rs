use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use paper_text::{Font, Glyph};
use zeno::Placement;

use crate::canvas::{Canvas, Ink};

const PROBE_SIZE: f32 = 16.0;
const CACHED_GLYPH: usize = 64 * 1024;
pub const FONT_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub size: f32,
    pub tracking: f32,
    pub align: f32,
}

impl Line {
    pub const fn new(size: f32) -> Self {
        Self { size, tracking: 0.0, align: 0.0 }
    }

    #[must_use]
    pub const fn tracked(self, tracking: f32) -> Self {
        Self { tracking, ..self }
    }

    #[must_use]
    pub const fn aligned(self, align: f32) -> Self {
        Self { align, ..self }
    }
}

type Bitmap = Rc<(Glyph, Vec<u8>)>;

pub fn read_font(path: &Path) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "widget font is not a file"));
    }
    let mut bytes = Vec::new();
    file.take(FONT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FONT_LIMIT {
        return Err(io::Error::new(io::ErrorKind::FileTooLarge, "widget font is too large"));
    }
    Ok(bytes)
}

pub struct Lettering {
    bytes: Arc<[u8]>,
    fonts: RefCell<HashMap<u32, Rc<Font>>>,
    glyphs: RefCell<HashMap<(u32, u16), Option<Bitmap>>>,
    used: RefCell<HashSet<u32>>,
}

impl Lettering {
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Option<Self> {
        let bytes = bytes.into();
        Font::parse(Arc::clone(&bytes), PROBE_SIZE)?;
        Some(Self {
            bytes,
            fonts: RefCell::new(HashMap::new()),
            glyphs: RefCell::new(HashMap::new()),
            used: RefCell::new(HashSet::new()),
        })
    }

    fn font(&self, size: f32) -> Option<Rc<Font>> {
        self.used.borrow_mut().insert(size.to_bits());
        let mut fonts = self.fonts.borrow_mut();
        if let Some(font) = fonts.get(&size.to_bits()) {
            return Some(Rc::clone(font));
        }
        let font = Rc::new(Font::parse(Arc::clone(&self.bytes), size)?);
        fonts.insert(size.to_bits(), Rc::clone(&font));
        Some(font)
    }

    fn bitmap(&self, font: &Font, size: f32, id: u16) -> Option<Bitmap> {
        let key = (size.to_bits(), id);
        if let Some(cached) = self.glyphs.borrow().get(&key) {
            return cached.clone();
        }
        let bitmap = font.rasterize(id).map(Rc::new);
        if bitmap.as_ref().is_none_or(|bitmap| bitmap.1.len() <= CACHED_GLYPH) {
            self.glyphs.borrow_mut().insert(key, bitmap.clone());
        }
        bitmap
    }

    pub fn sweep(&self) {
        let used = std::mem::take(&mut *self.used.borrow_mut());
        self.glyphs.borrow_mut().retain(|(size, _), _| used.contains(size));
        self.fonts.borrow_mut().retain(|size, _| used.contains(size));
    }

    pub fn width(&self, text: &str, line: Line) -> f32 {
        let Some(font) = self.font(line.size) else {
            return 0.0;
        };
        let mut width = 0.0;
        let mut glyphs = 0usize;
        font.shape(text, |glyph| {
            width += glyph.advance;
            glyphs += 1;
        });
        width + line.tracking * glyphs.saturating_sub(1) as f32
    }

    pub fn draw(&self, canvas: &mut Canvas, text: &str, at: [f32; 2], line: Line, ink: Ink) -> f32 {
        let scale = canvas.scale;
        let physical = Line { size: line.size * scale, tracking: line.tracking * scale, ..line };
        let Some(font) = self.font(physical.size) else {
            return 0.0;
        };
        let lead = if line.align == 0.0 { 0.0 } else { self.width(text, physical) * line.align };
        let metrics = font.face().metrics(&[]).scale(physical.size);
        let [dx, dy] = canvas.shift;
        let baseline = at[1] * scale - dy + (metrics.ascent - metrics.descent) / 2.0;
        let start = at[0] * scale - dx - lead;
        let mut cursor = start;
        let mut last = start;
        font.shape(text, |shaped| {
            if let Some(bitmap) = self.bitmap(&font, physical.size, shaped.id) {
                let (glyph, coverage) = &*bitmap;
                let [left, top] = paper_text::place(cursor, baseline, &shaped, *glyph);
                let place = Placement {
                    left: left as i32,
                    top: top as i32,
                    width: glyph.width as u32,
                    height: glyph.height as u32,
                };
                canvas.cover(coverage, place, ink);
            }
            last = cursor + shaped.advance;
            cursor = last + physical.tracking;
        });
        (last - start) / scale
    }
}

#[cfg(test)]
#[path = "lettering_tests.rs"]
mod tests;
