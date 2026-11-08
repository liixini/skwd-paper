use super::{
    Assets, Face, HAlign, Package, Properties, Rendered, VAlign, Value, font_bytes, halign_of,
    valign_of,
};
use std::path::PathBuf;
use std::sync::Arc;

const POINT_SIZE_FIELD: &str = "pointsize";
const H_ALIGN_FIELD: &str = "horizontalalign";
const V_ALIGN_FIELD: &str = "verticalalign";
const FONT_FIELD: &str = "font";
const BACKGROUND_COLOR_FIELD: &str = "backgroundcolor";
const BACKGROUND_BRIGHTNESS_FIELD: &str = "backgroundbrightness";
const OPAQUE_BACKGROUND_FIELD: &str = "opaquebackground";
const PADDING_FIELD: &str = "padding";
const RGB_COMPONENTS: usize = 3;
const DEFAULT_POINT_SIZE: f32 = 30.0;
const MAX_POINT_SIZE: f32 = 1024.0;

pub struct Backdrop {
    pub color: [f32; 3],
    pub padding: [f32; 2],
    pub visible: bool,
}

impl Backdrop {
    pub fn of(object: &Value, props: &Properties) -> Self {
        let color = object
            .get(BACKGROUND_COLOR_FIELD)
            .and_then(crate::effects::json_numbers)
            .filter(|values| values.len() >= RGB_COMPONENTS)
            .map_or([0.0; RGB_COMPONENTS], |values| [values[0], values[1], values[2]]);
        let brightness = crate::model::number(object.get(BACKGROUND_BRIGHTNESS_FIELD), props, 1.0);
        let padding = padding_of(object).unwrap_or([0.0; 2]);
        Self {
            color: color.map(|value| value * brightness),
            padding,
            visible: object.get(OPAQUE_BACKGROUND_FIELD).and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

pub(super) fn padding_of(object: &Value) -> Option<[f32; 2]> {
    fn pair(value: &Value) -> Option<[f32; 2]> {
        match value {
            Value::Number(number) => Some([number.as_f64()? as f32; 2]),
            Value::Bool(flag) => Some([f32::from(u8::from(*flag)); 2]),
            Value::String(text) => {
                let mut values = text.split_whitespace().filter_map(|part| part.parse().ok());
                let first = values.next()?;
                Some([first, values.next().unwrap_or(first)])
            }
            Value::Array(values) => {
                let first = values.first()?.as_f64()? as f32;
                let second = values.get(1).unwrap_or(&values[0]).as_f64()? as f32;
                values.iter().all(|value| value.as_f64().is_some()).then_some([first, second])
            }
            Value::Object(object) => object.get(super::VALUE_FIELD).and_then(pair),
            Value::Null => None,
        }
    }
    object.get(PADDING_FIELD).map_or(Some([0.0; 2]), pair)
}

fn em_of(object: &Value, props: &Properties) -> f32 {
    let pointsize = crate::model::number(object.get(POINT_SIZE_FIELD), props, DEFAULT_POINT_SIZE)
        .clamp(1.0, MAX_POINT_SIZE);
    super::em_from_points(pointsize)
}

enum PackageSource {
    Path(PathBuf),
    Memory(Package),
}

pub struct Source {
    package: PackageSource,
    assets: Assets,
}

impl Source {
    pub fn new(pkg: &Package, assets: &Assets) -> Self {
        let package = pkg.source_path().map_or_else(
            || PackageSource::Memory(pkg.clone()),
            |path| PackageSource::Path(path.to_path_buf()),
        );
        Self { package, assets: assets.snapshot() }
    }

    fn font(&self, name: &str) -> Option<Vec<u8>> {
        match &self.package {
            PackageSource::Path(path) => {
                Package::open(path).ok().and_then(|pkg| font_bytes(&pkg, &self.assets, name))
            }
            PackageSource::Memory(pkg) => font_bytes(pkg, &self.assets, name),
        }
    }
}

pub struct ScriptText {
    pub bounds: super::Bounds,
    pub font: Arc<[u8]>,
    face: std::cell::RefCell<Option<(Arc<[u8]>, Face)>>,
    font_name: String,
    source: Arc<Source>,
    em: f32,
    halign: HAlign,
    valign: VAlign,
    limits: super::TextLimits,
    raster: super::Raster,
    pub shown: String,
    pub needs_render: bool,
    rejected: Option<(String, Style, Option<u64>)>,
}

pub struct Style {
    raster: super::Raster,
    face: Option<(Arc<[u8]>, Face)>,
    font: Arc<[u8]>,
    font_name: String,
    em: f32,
    halign: HAlign,
    valign: VAlign,
    limits: super::TextLimits,
    needs_render: bool,
}

impl ScriptText {
    pub fn set_mode(&mut self, mode: super::RasterMode) {
        self.needs_render |= self.raster.mode != mode;
        self.raster.mode = mode;
    }

    pub fn update_style(&mut self, object: &Value, props: &Properties) -> Option<Style> {
        let font_name = object.get(FONT_FIELD).and_then(Value::as_str).unwrap_or("");
        let em = em_of(object, props);
        let halign = halign_of(object.get(H_ALIGN_FIELD));
        let valign = valign_of(object.get(V_ALIGN_FIELD));
        let limits = super::TextLimits::of(object, props);
        let raster = super::Raster::of(self.raster.mode, object);
        if font_name == self.font_name
            && em == self.em
            && halign == self.halign
            && valign == self.valign
            && limits == self.limits
            && raster == self.raster
        {
            return None;
        }
        let mut previous = Style {
            raster: self.raster,
            face: None,
            font: self.font.clone(),
            font_name: self.font_name.clone(),
            em: self.em,
            halign: self.halign,
            valign: self.valign,
            limits: self.limits,
            needs_render: self.needs_render,
        };
        if font_name != self.font_name {
            if let Some(font) = self.source.font(font_name) {
                self.needs_render |= self.font.as_ref() != font;
                self.font = font.into();
            }
            font_name.clone_into(&mut self.font_name);
        }
        let changed = self.em != em
            || self.halign != halign
            || self.valign != valign
            || self.limits != limits
            || self.raster != raster;
        if self.em != em || !Arc::ptr_eq(&self.font, &previous.font) {
            previous.face = self.face.get_mut().take();
        }
        self.em = em;
        self.halign = halign;
        self.valign = valign;
        self.limits = limits;
        self.raster = raster;
        self.needs_render |= changed;
        Some(previous)
    }

    pub fn has_rejection(&self) -> bool {
        self.rejected.is_some()
    }

    pub fn rejected(
        &self,
        wanted: &str,
        object: &Value,
        props: &Properties,
        revision: u64,
    ) -> bool {
        self.rejected.as_ref().is_some_and(|(text, style, previous)| {
            previous.is_none_or(|previous| previous == revision)
                && style.raster == super::Raster::of(self.raster.mode, object)
                && text.chars().eq(wanted.chars().take(super::MAX_TEXT_CHARS))
                && style.font_name == object.get(FONT_FIELD).and_then(Value::as_str).unwrap_or("")
                && style.em == em_of(object, props)
                && style.halign == halign_of(object.get(H_ALIGN_FIELD))
                && style.valign == valign_of(object.get(V_ALIGN_FIELD))
                && style.limits == super::TextLimits::of(object, props)
        })
    }

    pub fn reject(&mut self, wanted: &str, revision: Option<u64>) {
        self.rejected = Some((
            wanted.chars().take(super::MAX_TEXT_CHARS).collect(),
            Style {
                raster: self.raster,
                face: None,
                font: self.font.clone(),
                font_name: self.font_name.clone(),
                em: self.em,
                halign: self.halign,
                valign: self.valign,
                limits: self.limits,
                needs_render: self.needs_render,
            },
            revision,
        ));
    }

    pub fn accepted(&mut self, bounds: super::Bounds) {
        self.bounds = bounds;
        self.rejected = None;
    }

    pub fn restore(&mut self, style: Style) {
        if self.em != style.em || !Arc::ptr_eq(&self.font, &style.font) {
            *self.face.get_mut() = style.face;
        }
        self.font = style.font;
        self.font_name = style.font_name;
        self.em = style.em;
        self.halign = style.halign;
        self.valign = style.valign;
        self.limits = style.limits;
        self.raster = style.raster;
        self.needs_render = style.needs_render;
    }

    pub fn effect_padding(&self) -> Option<[f32; 2]> {
        (self.raster.mode == super::RasterMode::Effect).then_some(self.raster.padding).flatten()
    }

    pub fn render(&self, text: &str) -> Option<Rendered> {
        let text: String = text.chars().take(super::MAX_TEXT_CHARS).collect();
        let text = if text.trim().is_empty() { " " } else { &text };
        let mut cached = self.face.borrow_mut();
        if cached.as_ref().is_none_or(|(font, _)| !Arc::ptr_eq(font, &self.font)) {
            *cached = Some((self.font.clone(), Face::parse(&self.font, self.em)?));
        }
        let (_, face) = cached.as_mut()?;
        face.load_fallbacks(&self.source.assets, text);
        let text = self.limits.apply(face, text)?;
        if self.raster.mode == super::RasterMode::Direct {
            let metrics = face.measure(&text)?;
            let scale = super::direct_scale(&metrics);
            if scale < 1.0 {
                let mut fitted = Face::parse(&self.font, self.em * scale)?;
                fitted.load_fallbacks(&self.source.assets, &text);
                if !super::direct_fits(super::direct_extent(&fitted.measure(&text)?)) {
                    return None;
                }
                let (_, texture) = fitted.rasterize_mode(&text, self.halign, self.raster)?;
                return Some(Rendered {
                    texture,
                    offset: super::render_offset(&metrics, self.halign, self.valign, self.raster),
                    bounds: face.bounds(&metrics, self.halign, self.valign),
                    extent: Some(super::direct_extent(&metrics)),
                    metrics,
                    live: None,
                });
            }
        }
        let (metrics, texture) = face.rasterize_mode(&text, self.halign, self.raster)?;
        let offset = super::render_offset(&metrics, self.halign, self.valign, self.raster);
        let bounds = face.bounds(&metrics, self.halign, self.valign);
        Some(Rendered { texture, metrics, offset, bounds, live: None, extent: None })
    }
}

pub fn render(
    pkg: &Package,
    assets: &Assets,
    object: &Value,
    props: &Properties,
    source: Arc<Source>,
    mode: super::RasterMode,
) -> Option<(Rendered, ScriptText)> {
    let text = super::text_value(object.get("text")).unwrap_or_default();
    let font_name = object.get(FONT_FIELD).and_then(Value::as_str).unwrap_or("");
    let mut prepared = ScriptText {
        bounds: super::Bounds::default(),
        font: font_bytes(pkg, assets, font_name)?.into(),
        face: std::cell::RefCell::new(None),
        font_name: font_name.to_owned(),
        source,
        em: em_of(object, props),
        halign: halign_of(object.get(H_ALIGN_FIELD)),
        valign: valign_of(object.get(V_ALIGN_FIELD)),
        limits: super::TextLimits::of(object, props),
        raster: super::Raster::of(mode, object),
        shown: text.clone(),
        needs_render: false,
        rejected: None,
    };
    let rendered = prepared.render(&text)?;
    prepared.accepted(rendered.bounds);
    Some((rendered, prepared))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_assets() -> (tempfile::TempDir, Assets) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("shaders")).unwrap();
        std::fs::create_dir(directory.path().join("fonts")).unwrap();
        for (name, bytes) in [
            (super::super::FALLBACK_FONT, include_bytes!("../fixtures/fonts/latin.ttf").as_slice()),
            (
                "fonts/RobotoMono-Regular.ttf",
                include_bytes!("../fixtures/fonts/mono.ttf").as_slice(),
            ),
            (
                "fonts/TwemojiMozilla.ttf",
                include_bytes!("../fixtures/fonts/fallback.ttf").as_slice(),
            ),
        ] {
            std::fs::write(directory.path().join(name), bytes).unwrap();
        }
        let assets = Assets::discover(directory.path().to_str());
        (directory, assets)
    }

    #[test]
    fn backdrop_reads_live_style() {
        let props = Properties::new();
        let plain = Backdrop::of(&json!({"opaquebackground":false}), &props);
        assert!(!plain.visible);
        let styled = Backdrop::of(
            &json!({"opaquebackground":true,"backgroundcolor":"0 1 0",
                "backgroundbrightness":0.5,"padding":"40 24"}),
            &props,
        );
        assert!(styled.visible);
        assert_eq!(styled.color, [0.0, 0.5, 0.0]);
        assert_eq!(styled.padding, [40.0, 24.0]);
    }

    fn script_text(assets: &Assets, points: f32) -> ScriptText {
        let font = assets.read_bytes(super::super::FALLBACK_FONT).unwrap();
        ScriptText {
            bounds: super::super::Bounds::default(),
            font: font.into(),
            face: std::cell::RefCell::new(None),
            font_name: String::new(),
            source: Arc::new(Source::new(
                &Package::parse(crate::tests::build_pkg(&[])).unwrap(),
                assets,
            )),
            em: points * super::super::EM_PER_POINT,
            halign: HAlign::Center,
            valign: VAlign::Center,
            limits: super::super::TextLimits { width: None, rows: None },
            raster: super::super::Raster::of(super::super::RasterMode::Direct, &Value::Null),
            shown: "Same text".into(),
            needs_render: false,
            rejected: None,
        }
    }

    #[test]
    fn oversized_text_scales() {
        let (_directory, assets) = test_assets();
        let text = script_text(&assets, 1500.0);
        let face = Face::parse(&text.font, text.em).unwrap();
        let full = face.measure("Huge headline").unwrap();
        let rendered = text.render("Huge headline").unwrap();
        let texture = &rendered.texture;
        assert!(texture.pixels.bytes() <= super::super::MAX_RASTER_BYTES);
        assert!(texture.width.max(texture.height) <= super::super::MAX_TEXT_EDGE);
        assert!(texture.width as f32 * 2.0 < full.width);
        assert_eq!(rendered.layout().0, super::super::direct_extent(&full));
        assert_eq!(rendered.bounds, face.bounds(&full, HAlign::Center, VAlign::Center));
    }

    #[test]
    fn style_updates_raster() {
        let (_directory, assets) = test_assets();
        let mut text = script_text(&assets, 30.0);
        let before = text.render(&text.shown).unwrap();
        let style = json!({"pointsize":60,"horizontalalign":"left","verticalalign":"top"});
        let _ = text.update_style(&style, &Properties::new());
        assert!(text.needs_render);
        let after = text.render(&text.shown).unwrap();
        assert!(after.metrics.width > before.metrics.width);
        assert_eq!(after.offset, (0.0, 0.0));
        text.needs_render = false;
        let _ = text.update_style(&style, &Properties::new());
        assert!(!text.needs_render);

        let limited = json!({"pointsize":60,"horizontalalign":"left","verticalalign":"top",
            "limitwidth":true,"maxwidth":100,"limitrows":true,"maxrows":1});
        let _ = text.update_style(&limited, &Properties::new());
        assert!(text.needs_render);
        assert_eq!(text.render("Same text").unwrap().metrics.lines, 1);

        text.font = Arc::from([]);
        assert!(text.render("Same text").is_none());
        assert!(text.needs_render);
    }

    #[test]
    fn font_switch_rerenders() {
        let (_directory, assets) = test_assets();
        let noto = assets.read_bytes("fonts/NotoSans-Regular.ttf").unwrap();
        let mono = assets.read_bytes("fonts/RobotoMono-Regular.ttf").unwrap();
        let pkg = Package::parse(crate::tests::build_pkg(&[
            ("fonts/NotoSans-Regular.ttf", &noto),
            ("fonts/RobotoMono-Regular.ttf", &mono),
        ]))
        .unwrap();
        let mut text = ScriptText {
            bounds: super::super::Bounds::default(),
            font: noto.into(),
            face: std::cell::RefCell::new(None),
            font_name: "fonts/NotoSans-Regular.ttf".into(),
            source: Arc::new(Source::new(&pkg, &assets)),
            em: em_of(&json!({"pointsize":72}), &Properties::new()),
            halign: HAlign::Center,
            valign: VAlign::Center,
            limits: super::super::TextLimits { width: None, rows: None },
            raster: super::super::Raster::of(super::super::RasterMode::Direct, &Value::Null),
            shown: "WWW iii 123".into(),
            needs_render: false,
            rejected: None,
        };
        let before = text.render(&text.shown).unwrap();
        let _ = text.update_style(
            &json!({"font":"fonts/RobotoMono-Regular.ttf","pointsize":72}),
            &Properties::new(),
        );
        assert!(text.needs_render);
        assert_eq!(text.font.as_ref(), mono);
        let after = text.render(&text.shown).unwrap();
        assert_ne!(before.metrics.width, after.metrics.width);
    }

    #[test]
    fn script_preserves_font_caches() {
        let (_directory, assets) = test_assets();
        let pkg = Package::parse(crate::tests::build_pkg(&[])).unwrap();
        let source = Arc::new(Source::new(&pkg, &assets));
        let object = json!({"text":"AאבB", "pointsize":24});
        let (first, text) = render(
            &pkg,
            &assets,
            &object,
            &Properties::new(),
            source,
            super::super::RasterMode::Direct,
        )
        .unwrap();
        let key = text.face.borrow().as_ref().unwrap().1.fallbacks[0].cache_key();
        let repeated = text.render("AאבB").unwrap();
        assert_eq!(key, text.face.borrow().as_ref().unwrap().1.fallbacks[0].cache_key());
        assert_eq!(first.texture.pixels.levels[0].data, repeated.texture.pixels.levels[0].data);
    }

    #[test]
    #[ignore = "requires pinned dynamic text package"]
    fn restore_keeps_face() {
        let path = std::env::var_os("SKWD_DYNAMIC_TEXT_PACKAGE").expect("pinned text package");
        let package = Package::open(std::path::Path::new(&path)).unwrap();
        let assets = Assets::discover(Some("/nonexistent"));
        let source = Arc::new(Source::new(&package, &assets));
        let mut object = serde_json::json!({"text":"A","font":"fonts/Probe.ttf","pointsize":8});
        let (_, mut text) = super::render(
            &package,
            &assets,
            &object,
            &Properties::new(),
            source,
            super::super::RasterMode::Direct,
        )
        .unwrap();
        assert!(text.face.borrow().is_some());
        object["pointsize"] = serde_json::json!(1024);
        let prior = text.update_style(&object, &Properties::new()).unwrap();
        assert!(prior.face.is_some());
        assert!(text.render(&"W".repeat(4096)).is_none());
        text.reject(&"W".repeat(4096), None);
        text.restore(prior);
        assert!(text.face.borrow().is_some());
        assert!(text.rejected(&"W".repeat(4096), &object, &Properties::new(), 0));
        assert!(text.rejected(&"W".repeat(4096), &object, &Properties::new(), 1));
        assert!(text.render("A").is_some());
    }
}
