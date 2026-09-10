use read_fonts::TableProvider;
use read_fonts::tables::kern::SubtableKind;
use std::cell::RefCell;
use std::collections::HashMap;
use swash::scale::{Render, ScaleContext, Source};
use swash::{CacheKey, FontRef};

#[derive(Clone, Copy, Default)]
pub(super) struct Glyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    pub advance_width: f32,
}

pub(super) struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub new_line_size: f32,
}

pub(super) struct Font {
    bytes: Vec<u8>,
    offset: u32,
    key: CacheKey,
    em: f32,
    kern: HashMap<(u16, u16), i16>,
    scale: RefCell<ScaleContext>,
    metrics: RefCell<HashMap<char, Glyph>>,
}

impl Font {
    pub fn parse(bytes: &[u8], em: f32) -> Option<Self> {
        if !em.is_finite() || em <= 0.0 {
            return None;
        }
        let parsed = read_fonts::FontRef::from_index(bytes, 0).ok()?;
        parsed.head().ok()?;
        parsed.hhea().ok()?;
        parsed.hmtx().ok()?;
        parsed.maxp().ok()?;
        let font = FontRef::from_index(bytes, 0)?;
        let metrics = font.metrics(&[]);
        if metrics.units_per_em == 0 || metrics.glyph_count == 0 {
            return None;
        }
        let mut kern = HashMap::new();
        if let Ok(table) = parsed.kern() {
            for subtable in table.subtables().flatten() {
                if !subtable.is_horizontal() || subtable.is_cross_stream() || subtable.is_variable()
                {
                    continue;
                }
                if let Ok(SubtableKind::Format0(pairs)) = subtable.kind() {
                    for pair in pairs.pairs() {
                        kern.insert((pair.left().to_u16(), pair.right().to_u16()), pair.value());
                    }
                }
            }
        }
        Some(Self {
            bytes: bytes.to_vec(),
            offset: font.offset,
            key: font.key,
            em,
            kern,
            scale: RefCell::new(ScaleContext::new()),
            metrics: RefCell::new(HashMap::new()),
        })
    }

    fn face(&self) -> FontRef<'_> {
        FontRef { data: &self.bytes, offset: self.offset, key: self.key }
    }

    pub fn line_metrics(&self) -> LineMetrics {
        let metrics = self.face().metrics(&[]).scale(self.em);
        LineMetrics {
            ascent: metrics.ascent,
            descent: -metrics.descent,
            new_line_size: metrics.ascent + metrics.descent + metrics.leading,
        }
    }

    pub fn kern(&self, left: char, right: char) -> f32 {
        let face = self.face();
        let map = face.charmap();
        let pair = (map.map(left), map.map(right));
        f32::from(self.kern.get(&pair).copied().unwrap_or(0)) * self.em
            / f32::from(face.metrics(&[]).units_per_em)
    }

    pub fn metrics(&self, ch: char) -> Glyph {
        if let Some(metrics) = self.metrics.borrow().get(&ch) {
            return *metrics;
        }
        let (metrics, _) = self.rasterize(ch);
        metrics
    }

    pub fn rasterize(&self, ch: char) -> (Glyph, Vec<u8>) {
        let face = self.face();
        let id = face.charmap().map(ch);
        let advance_width = face.glyph_metrics(&[]).scale(self.em).advance_width(id);
        let mut context = self.scale.borrow_mut();
        let mut scaler = context.builder(face).size(self.em).hint(false).build();
        let mut metrics = Glyph { advance_width, ..Glyph::default() };
        let bitmap = if let Some(image) = Render::new(&[Source::Outline])
            .format(swash::zeno::Format::Alpha)
            .render(&mut scaler, id)
        {
            let placement = image.placement;
            metrics.width = placement.width as usize;
            metrics.height = placement.height as usize;
            metrics.xmin = placement.left;
            metrics.ymin = placement.top - placement.height as i32;
            image.data
        } else {
            Vec::new()
        };
        self.metrics.borrow_mut().insert(ch, metrics);
        (metrics, bitmap)
    }
}
