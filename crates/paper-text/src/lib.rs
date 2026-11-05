use read_fonts::TableProvider;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use swash::shape::{Direction, ShapeContext, cluster::Glyph as ShapedGlyph};
use swash::text::{BidiClass, Codepoint, Script};
use swash::{CacheKey, FontRef};

mod raster;

use raster::RasterFace;

pub const MAX_EDGE: u32 = 8192;

#[derive(Clone, Copy, Default)]
pub struct Glyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
}

pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub height: f32,
}

pub struct Font {
    bytes: Arc<[u8]>,
    raster: RefCell<RasterFace>,
    offset: u32,
    key: CacheKey,
    em: f32,
    height: f32,
    shape: RefCell<ShapeContext>,
    metrics: RefCell<HashMap<u16, Glyph>>,
}

impl Font {
    pub fn parse(bytes: impl Into<Arc<[u8]>>, em: f32) -> Option<Self> {
        if !em.is_finite() || em <= 0.0 {
            return None;
        }
        let bytes = bytes.into();
        let parsed = read_fonts::FontRef::from_index(&bytes, 0).ok()?;
        parsed.head().ok()?;
        parsed.hhea().ok()?;
        parsed.hmtx().ok()?;
        parsed.maxp().ok()?;
        let font = FontRef::from_index(&bytes, 0)?;
        let metrics = font.metrics(&[]);
        if metrics.units_per_em == 0 || metrics.glyph_count == 0 {
            return None;
        }
        let (offset, key) = (font.offset, font.key);
        let raster = RasterFace::new(bytes.clone(), em)?;
        let height = raster.height()?;
        Some(Self {
            bytes,
            raster: RefCell::new(raster),
            offset,
            key,
            em,
            height,
            shape: RefCell::new(ShapeContext::new()),
            metrics: RefCell::new(HashMap::new()),
        })
    }

    pub fn face(&self) -> FontRef<'_> {
        FontRef { data: &self.bytes, offset: self.offset, key: self.key }
    }

    pub fn em(&self) -> f32 {
        self.em
    }

    pub fn line_metrics(&self) -> LineMetrics {
        let metrics = self.face().metrics(&[]).scale(self.em);
        LineMetrics { ascent: metrics.ascent, descent: -metrics.descent, height: self.height }
    }

    pub fn cache_key(&self) -> CacheKey {
        self.key
    }

    pub fn shape(&self, text: &str, mut visit: impl FnMut(ShapedGlyph)) {
        let mut emit = |glyphs: &[ShapedGlyph]| {
            let mut discarded = 0.0;
            for glyph in glyphs {
                if glyph.id == 0 {
                    continue;
                }
                let mut glyph = *glyph;
                let advance = glyph.advance.floor();
                glyph.x += discarded;
                discarded += glyph.advance - advance;
                glyph.advance = advance;
                visit(glyph);
            }
        };
        let rtl = text
            .chars()
            .find_map(|ch| match ch.properties().bidi_class() {
                BidiClass::R | BidiClass::AL => Some(true),
                BidiClass::L => Some(false),
                _ => None,
            })
            .unwrap_or(false);
        let direction = if rtl { Direction::RightToLeft } else { Direction::LeftToRight };
        let mut context = self.shape.borrow_mut();
        let mut clusters = Vec::new();
        {
            let mut shape_part = |part: &str, script| {
                let mirrored: String = if rtl {
                    part.chars().map(|ch| ch.mirror().unwrap_or(ch)).collect()
                } else {
                    String::new()
                };
                let mut shaper = context
                    .builder(self.face())
                    .script(script)
                    .direction(direction)
                    .size(self.em)
                    .build();
                shaper.add_str(if rtl { &mirrored } else { part });
                shaper.shape_with(|cluster| {
                    if rtl {
                        clusters.push(cluster.glyphs.to_vec());
                    } else {
                        emit(cluster.glyphs);
                    }
                });
            };
            for (range, script) in script_runs(text) {
                shape_part(&text[range], script);
            }
        }
        for cluster in clusters.into_iter().rev() {
            emit(&cluster);
        }
    }

    pub fn metrics(&self, id: u16) -> Option<Glyph> {
        if let Some(metrics) = self.metrics.borrow().get(&id) {
            return Some(*metrics);
        }
        self.rasterize(id).map(|(metrics, _)| metrics)
    }

    pub fn rasterize(&self, id: u16) -> Option<(Glyph, Vec<u8>)> {
        let (metrics, bitmap) = self.raster.borrow_mut().rasterize(id)?;
        self.metrics.borrow_mut().insert(id, metrics);
        Some((metrics, bitmap))
    }
}

pub fn place(cursor: f32, baseline: f32, shaped: &ShapedGlyph, glyph: Glyph) -> [i64; 2] {
    [
        (cursor + shaped.x.floor() + glyph.xmin as f32).round() as i64,
        (baseline - shaped.y.floor() - (glyph.ymin + glyph.height as i32) as f32).round() as i64,
    ]
}

pub fn script_runs(text: &str) -> Vec<(Range<usize>, Script)> {
    let mut runs = Vec::new();
    let mut script = text.chars().find_map(script_of).unwrap_or(Script::Latin);
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if let Some(next) = script_of(ch)
            && next != script
        {
            runs.push((start..index, script));
            start = index;
            script = next;
        }
    }
    runs.push((start..text.len(), script));
    runs
}

fn script_of(ch: char) -> Option<Script> {
    match ch.properties().script() {
        Script::Common | Script::Inherited | Script::Unknown => None,
        script => Some(script),
    }
}
