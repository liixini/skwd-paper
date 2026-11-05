use crate::effects::Assets;
use crate::model::{Properties, Texture};
use crate::pkg::Package;
use serde_json::Value;

pub mod batch;
mod font;
pub mod script;
pub(crate) mod system_font;

const EM_PER_POINT_RATIO: f64 = 300.0 / 72.0;
pub const EM_PER_POINT: f32 = EM_PER_POINT_RATIO as f32;

fn em_from_points(pointsize: f32) -> f32 {
    (f64::from(pointsize) * EM_PER_POINT_RATIO) as f32
}

pub const FALLBACK_FONT: &str = "fonts/NotoSans-Regular.ttf";
const TEXT_INPUT_KIND: &str = "textinput";
const PROPERTIES_PATH: &str = "/general/properties";
const OBJECTS_FIELD: &str = "objects";
const TEXT_FIELD: &str = "text";
const USER_FIELD: &str = "user";
const TYPE_FIELD: &str = "type";
const VALUE_FIELD: &str = "value";
const LIMIT_WIDTH_FIELD: &str = "limitwidth";
const MAX_WIDTH_FIELD: &str = "maxwidth";
const LIMIT_ROWS_FIELD: &str = "limitrows";
const MAX_ROWS_FIELD: &str = "maxrows";
const LINE_BREAK: &str = "\n";
const RASTER_GUARD: f32 = 1.0;
const MAX_TEXT_EDGE: u32 = paper_text::MAX_EDGE;
const MAX_RASTER_BYTES: usize = 16 * 1024 * 1024;
const DIRECT_PADDING: f32 = 2.0 * RASTER_GUARD;
const FIT_MARGIN: f32 = 0.95;
const MAX_TEXT_CHARS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_height: f32,
    pub lines: usize,
}

impl Metrics {
    #[must_use]
    pub fn height(&self) -> f32 {
        self.ascent + self.descent + self.line_height * (self.lines.max(1) - 1) as f32
    }
}

#[must_use]
pub fn box_offset(metrics: &Metrics, halign: HAlign, valign: VAlign) -> (f32, f32) {
    let x = match halign {
        HAlign::Left => 0.0,
        HAlign::Center => -metrics.width * 0.5,
        HAlign::Right => -metrics.width,
    };
    let y = match valign {
        VAlign::Top => 0.0,
        VAlign::Center => {
            -metrics.ascent.ceil() * 0.5
                - metrics.line_height * (metrics.lines.max(1) - 1) as f32 * 0.5
        }
        VAlign::Bottom => -metrics.height(),
    };
    (x, y)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub size: [f32; 2],
    pub offset: [f32; 2],
}

pub struct Rendered {
    pub bounds: Bounds,
    pub texture: Texture,
    pub metrics: Metrics,
    pub offset: (f32, f32),
    pub live: Option<Prepared>,
    pub extent: Option<[f32; 2]>,
}

impl Rendered {
    #[must_use]
    pub fn layout(&self) -> ([f32; 2], [f32; 2]) {
        let size = self.extent.unwrap_or([self.texture.width as f32, self.texture.height as f32]);
        let offset = [
            self.offset.0 + size[0] * 0.5 - RASTER_GUARD,
            self.offset.1 + size[1] * 0.5 - RASTER_GUARD,
        ];
        (size, offset)
    }
}

fn direct_extent(metrics: &Metrics) -> [f32; 2] {
    [metrics.width.ceil() + DIRECT_PADDING, metrics.height().ceil() + DIRECT_PADDING]
}

fn direct_fits([width, height]: [f32; 2]) -> bool {
    width.max(height) <= MAX_TEXT_EDGE as f32
        && crate::tex::PixelFormat::Rgba8
            .level_bytes(width as u32, height as u32)
            .is_some_and(|bytes| bytes <= MAX_RASTER_BYTES)
}

fn direct_scale(metrics: &Metrics) -> f32 {
    let extent = direct_extent(metrics);
    if direct_fits(extent) {
        return 1.0;
    }
    let [width, height] = extent;
    let pixels = MAX_RASTER_BYTES as f32 / crate::tex::RGBA8_PIXEL_BYTES as f32;
    let area = (pixels / (width * height)).sqrt();
    let edge = MAX_TEXT_EDGE as f32 / width.max(height);
    area.min(edge) * FIT_MARGIN
}

pub struct Prepared {
    face: std::cell::RefCell<Face>,
    assets: Assets,
    halign: HAlign,
    width: u32,
    height: u32,
    placeholder: String,
    raster: Raster,
    weekday: Option<Vec<String>>,
    clock: Option<crate::dynamic_text::clock::Clock>,
    date: Option<crate::dynamic_text::date::Date>,
}

impl Prepared {
    #[must_use]
    pub fn value(&self, now: crate::dynamic_text::LocalTime) -> String {
        if let Some(clock) = &self.clock {
            return clock.value(now);
        }
        if let Some(date) = &self.date {
            return date.value(now);
        }
        crate::dynamic_text::substitute_with(&self.placeholder, now, self.weekday.as_deref())
            .unwrap_or_else(|| self.placeholder.clone())
    }

    #[must_use]
    pub fn cadence(&self) -> crate::dynamic_text::Cadence {
        self.clock.as_ref().map_or_else(
            || crate::dynamic_text::cadence(&self.placeholder),
            crate::dynamic_text::clock::Clock::cadence,
        )
    }

    #[must_use]
    pub fn rasterize(&self, text: &str) -> Option<Texture> {
        let mut face = self.face.borrow_mut();
        face.load_fallbacks(&self.assets, text);
        face.rasterize_in(text, self.halign, self.width, self.height, self.raster)
    }
}

const LIVE_HEADROOM: f32 = 1.35;

fn dynamic_placeholder(object: &Value) -> Option<String> {
    let Value::Object(map) = object.get("text")? else {
        return None;
    };
    map.get("script")?;
    let placeholder = text_value(map.get("value"))?;
    let now = crate::dynamic_text::local_now()?;
    crate::dynamic_text::clock::Clock::from_text(object.get("text")?)
        .map(|clock| clock.value(now))
        .or_else(|| {
            crate::dynamic_text::date::Date::from_text(object.get("text")?)
                .map(|date| date.value(now))
        })
        .or_else(|| crate::dynamic_text::substitute(&placeholder, now))
        .map(|_| placeholder)
}

fn halign_of(value: Option<&Value>) -> HAlign {
    match value.and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        Some("left") => HAlign::Left,
        Some("right") => HAlign::Right,
        _ => HAlign::Center,
    }
}

fn valign_of(value: Option<&Value>) -> VAlign {
    match value.and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        Some("top") => VAlign::Top,
        Some("bottom") => VAlign::Bottom,
        _ => VAlign::Center,
    }
}

#[must_use]
pub fn text_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Object(map) => text_value(map.get("value")),
        _ => None,
    }
}

pub(crate) fn bind_user_values(scene: &mut Value, project: &Value) {
    let Some(declarations) = project.pointer(PROPERTIES_PATH).and_then(Value::as_object) else {
        return;
    };
    let Some(objects) = scene.get_mut(OBJECTS_FIELD).and_then(Value::as_array_mut) else {
        return;
    };
    for object in objects {
        let Some(text) = object.get_mut(TEXT_FIELD).and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(name) = text.get(USER_FIELD).and_then(Value::as_str) else {
            continue;
        };
        let selected = declarations
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, entry)| entry)
            .filter(|entry| entry.get(TYPE_FIELD).and_then(Value::as_str) == Some(TEXT_INPUT_KIND))
            .and_then(|entry| entry.get(VALUE_FIELD))
            .and_then(Value::as_str);
        if let Some(value) = selected {
            text.insert(
                VALUE_FIELD.into(),
                Value::String(value.chars().take(MAX_TEXT_CHARS).collect()),
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct TextLimits {
    width: Option<f32>,
    rows: Option<usize>,
}

impl TextLimits {
    fn of(object: &Value, props: &Properties) -> Self {
        let width = object
            .get(LIMIT_WIDTH_FIELD)
            .and_then(Value::as_bool)
            .filter(|enabled| *enabled)
            .map(|_| crate::model::number(object.get(MAX_WIDTH_FIELD), props, MAX_TEXT_EDGE as f32))
            .filter(|width| width.is_finite() && *width > 0.0)
            .map(|width| width.min(MAX_TEXT_EDGE as f32));
        let rows = object
            .get(LIMIT_ROWS_FIELD)
            .and_then(Value::as_bool)
            .filter(|enabled| *enabled)
            .map(|_| crate::model::number(object.get(MAX_ROWS_FIELD), props, MAX_TEXT_CHARS as f32))
            .filter(|rows| rows.is_finite() && *rows > 0.0)
            .map(|rows| rows.min(MAX_TEXT_CHARS as f32) as usize);
        Self { width, rows }
    }

    fn apply(self, face: &Face, text: &str) -> Option<String> {
        wrap_text(text, self.width, self.rows, |line| {
            face.line_box(line).map(|(left, right)| right - left)
        })
    }
}

fn wrap_text(
    text: &str,
    width: Option<f32>,
    rows: Option<usize>,
    measure: impl Fn(&str) -> Option<f32>,
) -> Option<String> {
    if rows == Some(0) {
        return Some(String::new());
    }
    let mut lines = Vec::new();
    for source in text.split('\n') {
        let Some(width) = width else {
            if push_line(&mut lines, source.to_owned(), rows) {
                return Some(lines.join(LINE_BREAK));
            }
            continue;
        };
        let mut line = String::new();
        for word in source.split_inclusive(' ') {
            let before = line.len();
            line.push_str(word);
            if before > 0 && measure(line.trim_end())? > width {
                let remainder = line.split_off(before);
                if push_line(&mut lines, line.trim_end().to_owned(), rows) {
                    return Some(lines.join(LINE_BREAK));
                }
                remainder.trim_start().clone_into(&mut line);
            }
            while measure(line.trim_end())? > width {
                let mut split = None;
                for (index, ch) in line.char_indices() {
                    let end = index + ch.len_utf8();
                    if measure(line[..end].trim_end())? > width {
                        break;
                    }
                    split = Some(end);
                }
                let split = split.unwrap_or_else(|| line.chars().next().map_or(0, char::len_utf8));
                if split == line.len() {
                    break;
                }
                if push_line(&mut lines, line[..split].trim_end().to_owned(), rows) {
                    return Some(lines.join(LINE_BREAK));
                }
                line = line[split..].trim_start().to_owned();
            }
        }
        if push_line(&mut lines, line.trim_end().to_owned(), rows) {
            return Some(lines.join(LINE_BREAK));
        }
    }
    Some(lines.join(LINE_BREAK))
}

fn push_line(lines: &mut Vec<String>, line: String, rows: Option<usize>) -> bool {
    lines.push(line);
    rows.is_some_and(|rows| lines.len() >= rows)
}

fn font_bytes(pkg: &Package, assets: &Assets, name: &str) -> Option<Vec<u8>> {
    if let Some(family) = name.strip_prefix("systemfont_") {
        return assets
            .system_fonts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .resolve(assets, family);
    }
    if !name.is_empty() {
        if let Some(bytes) = pkg.find(name) {
            return Some(bytes.to_vec());
        }
        if let Some(bytes) = assets.read_bytes(name) {
            return Some(bytes);
        }
    }
    assets.read_bytes(FALLBACK_FONT)
}

pub struct Face {
    font: paper_text::Font,
    fallbacks: Vec<paper_text::Font>,
    fallback_loaded: bool,
}

impl Face {
    pub fn parse(bytes: &[u8], em: f32) -> Option<Self> {
        Some(Self {
            font: paper_text::Font::parse(bytes, em)?,
            fallbacks: Vec::new(),
            fallback_loaded: false,
        })
    }

    fn line_box(&self, line: &str) -> Option<(f32, f32)> {
        let mut cursor = 0.0f32;
        let mut ink_min = 0.0f32;
        let mut ink_max = 0.0f32;
        let mut failed = false;
        self.shape(line, |font, shaped| {
            let Some(glyph) = font.metrics(shaped.id) else {
                failed = true;
                return;
            };
            if glyph.width > 0 {
                let left = cursor + shaped.x + glyph.xmin as f32;
                ink_min = ink_min.min(left);
                ink_max = ink_max.max(left + glyph.width as f32);
            } else {
                ink_max = ink_max.max(cursor + shaped.advance);
            }
            cursor += shaped.advance;
        });
        (!failed).then_some((ink_min, ink_max))
    }

    #[must_use]
    pub fn measure(&self, text: &str) -> Option<Metrics> {
        let line = self.font.line_metrics();
        let lines: Vec<&str> = text.split('\n').collect();
        let width = lines.iter().try_fold(0.0f32, |width, line| {
            self.line_box(line).map(|(left, right)| width.max(right - left))
        })?;
        Some(Metrics {
            width,
            ascent: line.ascent.ceil().max(0.0),
            descent: (-line.descent).max(0.0),
            line_height: line.height.max(1.0),
            lines: lines.len(),
        })
    }

    #[must_use]
    fn draw(
        &self,
        text: &str,
        halign: HAlign,
        metrics: &Metrics,
        width: u32,
        height: u32,
        box_width: f32,
        origin: [f32; 2],
    ) -> Option<(Vec<u8>, batch::Batch)> {
        let mut glyphs = Vec::new();
        let mut data = vec![0u8; width as usize * height as usize * 4];
        for (index, line) in text.split('\n').enumerate() {
            let (left, right) = self.line_box(line)?;
            let line_width = right - left;
            let mut cursor = origin[0] - left
                + match halign {
                    HAlign::Left => 0.0,
                    HAlign::Center => (box_width - line_width) * 0.5,
                    HAlign::Right => box_width - line_width,
                };
            let baseline = origin[1] + metrics.ascent + metrics.line_height * index as f32;
            let mut failed = false;
            self.shape(line, |font, shaped| {
                let Some((glyph, bitmap)) = font.rasterize(shaped.id) else {
                    failed = true;
                    return;
                };
                let [x0, y0] = paper_text::place(cursor, baseline, &shaped, glyph);
                for row in 0..glyph.height {
                    let y = y0 + row as i64;
                    if y < 0 || y >= i64::from(height) {
                        continue;
                    }
                    for col in 0..glyph.width {
                        let x = x0 + col as i64;
                        if x < 0 || x >= i64::from(width) {
                            continue;
                        }
                        let coverage = bitmap[row * glyph.width + col];
                        let at = ((y as usize) * width as usize + x as usize) * 4;
                        data[at] = 255;
                        data[at + 1] = 255;
                        data[at + 2] = 255;
                        let maximum = u16::from(u8::MAX);
                        let foreground = u16::from(coverage);
                        let background = u16::from(data[at + 3]);
                        data[at + 3] = (foreground
                            + (background * (maximum - foreground) + maximum / 2) / maximum)
                            as u8;
                    }
                }
                glyphs.push(batch::Glyph {
                    bounds: [x0, y0, glyph.width as i64, glyph.height as i64],
                    bitmap,
                });
                cursor += shaped.advance;
            });
            if failed {
                return None;
            }
        }
        Some((data, batch::Batch::pack([width, height], glyphs)?))
    }

    fn bounds(&self, metrics: &Metrics, halign: HAlign, valign: VAlign) -> Bounds {
        let height = self.font.line_metrics().height;
        let logical = Metrics { descent: height - metrics.ascent, line_height: height, ..*metrics };
        let size = [logical.width, logical.height()];
        let (x, y) = box_offset(&logical, halign, valign);
        Bounds { size, offset: [x + size[0] * 0.5, y + size[1] * 0.5] }
    }

    fn texture_of(width: u32, height: u32, raster: (Vec<u8>, batch::Batch)) -> Texture {
        let (data, batch) = raster;
        Texture {
            video: None,
            text_batch: Some(batch),
            pages: Vec::new(),
            system_texture: None,
            image_count: 1,
            width,
            height,
            img_width: width,
            img_height: height,
            pixels: std::sync::Arc::new(crate::tex::Pixels::rgba(width, height, data)),
            frames: Vec::new(),
            stream: None,
            clamp: true,
            nearest: false,
            component_mask: 0,
            format: crate::tex::TexFormat::Rgba8888,
        }
    }

    pub fn rasterize(&self, text: &str, halign: HAlign) -> Option<(Metrics, Texture)> {
        self.rasterize_mode(text, halign, Raster::of(RasterMode::Direct, &Value::Null))
    }

    fn raster_size(&self, metrics: &Metrics, raster: Raster) -> Option<[u32; 2]> {
        let padding = raster.padding?;
        if raster.mode == RasterMode::Direct {
            return Some(direct_extent(metrics).map(|edge| (edge as u32).clamp(1, MAX_TEXT_EDGE)));
        }
        let size = [
            metrics.width + padding[0] * 2.0,
            self.font.line_metrics().height * metrics.lines.max(1) as f32 + padding[1] * 2.0,
        ];
        if size.iter().any(|edge| !edge.is_finite() || *edge > MAX_TEXT_EDGE as f32) {
            return None;
        }
        let size = size.map(|edge| (edge as u32).max(1));
        raster.check_size(size)?;
        Some(size)
    }

    fn rasterize_mode(
        &self,
        text: &str,
        halign: HAlign,
        raster: Raster,
    ) -> Option<(Metrics, Texture)> {
        let metrics = self.measure(text)?;
        let [width, height] = self.raster_size(&metrics, raster)?;
        let origin = raster.padding?.map(|padding| padding + raster.mode.guard());
        let data = self.draw(text, halign, &metrics, width, height, metrics.width, origin)?;
        Some((metrics, Self::texture_of(width, height, data)))
    }

    fn rasterize_in(
        &self,
        text: &str,
        halign: HAlign,
        width: u32,
        height: u32,
        raster: Raster,
    ) -> Option<Texture> {
        raster.check_size([width, height])?;
        let metrics = self.measure(text)?;
        let measured = match raster.mode {
            RasterMode::Direct => metrics.height(),
            RasterMode::Effect => self.raster_size(&metrics, raster)?[1] as f32,
        };
        let padding = raster.padding?;
        let guard = raster.mode.guard();
        let left = guard + padding[0];
        let top = ((height as f32 - measured) * 0.5).max(guard) + padding[1];
        let data = self.draw(
            text,
            halign,
            &metrics,
            width,
            height,
            width as f32 - 2.0 * left,
            [left, top],
        )?;
        Some(Self::texture_of(width, height, data))
    }
}

pub fn render(
    pkg: &Package,
    assets: &Assets,
    object: &Value,
    props: &Properties,
    mode: RasterMode,
) -> Option<Rendered> {
    let mut text = crate::dynamic_text::local_now()
        .and_then(|now| crate::dynamic_text::resolve(object.get("text"), now))
        .or_else(|| text_value(object.get("text")))?;
    if text.chars().count() > MAX_TEXT_CHARS {
        text = text.chars().take(MAX_TEXT_CHARS).collect();
    }
    let pointsize = crate::model::number(object.get("pointsize"), props, 30.0).clamp(1.0, 1024.0);
    let em = em_from_points(pointsize);
    let font_name = object.get("font").and_then(Value::as_str).unwrap_or("");
    let bytes = font_bytes(pkg, assets, font_name)?;
    let mut face = Face::parse(&bytes, em)?;
    face.load_fallbacks(assets, &text);
    let text = TextLimits::of(object, props).apply(&face, &text)?;
    let halign = halign_of(object.get("horizontalalign"));
    let valign = valign_of(object.get("verticalalign"));
    let raster = Raster::of(mode, object);
    let (metrics, texture) = face.rasterize_mode(&text, halign, raster)?;
    let offset = render_offset(&metrics, halign, valign, raster);
    let bounds = face.bounds(&metrics, halign, valign);
    let Some(placeholder) = dynamic_placeholder(object) else {
        return Some(Rendered { texture, metrics, offset, bounds, live: None, extent: None });
    };
    let width = ((texture.width as f32 * LIVE_HEADROOM).ceil() as u32).clamp(1, MAX_TEXT_EDGE);
    let weekday = object
        .get("text")
        .and_then(|text| text.get("script"))
        .and_then(Value::as_str)
        .and_then(crate::dynamic_text::weekday_table);
    let clock = object.get("text").and_then(crate::dynamic_text::clock::Clock::from_text);
    let date = object.get("text").and_then(crate::dynamic_text::date::Date::from_text);
    let live = Prepared {
        face: std::cell::RefCell::new(face),
        assets: assets.snapshot(),
        halign,
        width,
        height: texture.height,
        placeholder,
        raster,
        weekday,
        clock,
        date,
    };
    let Some(padded) = live.rasterize(&text) else {
        return Some(Rendered { texture, metrics, offset, bounds, live: None, extent: None });
    };
    let slack = (padded.width as f32 - texture.width as f32) * 0.5;
    let offset = (offset.0 - slack, offset.1);
    Some(Rendered { texture: padded, metrics, offset, bounds, live: Some(live), extent: None })
}

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterMode {
    Direct,
    Effect,
}

impl RasterMode {
    fn guard(self) -> f32 {
        match self {
            Self::Direct => RASTER_GUARD,
            Self::Effect => 0.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Raster {
    mode: RasterMode,
    padding: Option<[f32; 2]>,
}

impl Raster {
    fn check_size(self, size: [u32; 2]) -> Option<()> {
        if self.mode == RasterMode::Effect {
            let bytes = crate::tex::PixelFormat::Rgba8.level_bytes(size[0], size[1])?;
            if bytes > MAX_RASTER_BYTES {
                return None;
            }
        }
        Some(())
    }

    fn of(mode: RasterMode, object: &Value) -> Self {
        let padding = if mode == RasterMode::Direct {
            Some([0.0; 2])
        } else {
            script::padding_of(object)
                .filter(|padding| padding.iter().all(|value| value.is_finite() && *value >= 0.0))
        };
        Self { mode, padding }
    }
}

fn render_offset(metrics: &Metrics, halign: HAlign, valign: VAlign, raster: Raster) -> (f32, f32) {
    let (x, y) = box_offset(metrics, halign, valign);
    let guard = RASTER_GUARD - raster.mode.guard();
    let [horizontal, vertical] = raster.padding.expect("validated raster padding");
    (x + guard - horizontal, y + guard - vertical)
}
