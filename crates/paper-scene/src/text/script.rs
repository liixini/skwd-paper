use super::{
    Assets, Face, HAlign, Package, Properties, Rendered, VAlign, Value, box_offset, font_bytes,
    halign_of, valign_of,
};
use std::sync::Arc;

pub struct ScriptText {
    pub font: Arc<[u8]>,
    em: f32,
    halign: HAlign,
    valign: VAlign,
    pub shown: String,
}

impl ScriptText {
    pub fn render(&self, text: &str) -> Option<Rendered> {
        let text: String = text.chars().take(super::MAX_TEXT_CHARS).collect();
        let text = if text.trim().is_empty() { " " } else { &text };
        let face = Face::parse(&self.font, self.em)?;
        let measured = face.measure(text)?;
        if (measured.width.ceil() + 2.0) * (measured.height().ceil() + 2.0) * 4.0
            > 16.0 * 1024.0 * 1024.0
        {
            return None;
        }
        let (metrics, texture) = face.rasterize(text, self.halign)?;
        let offset = box_offset(&metrics, self.halign, self.valign);
        Some(Rendered { texture, metrics, offset, live: None })
    }
}

pub fn render(
    pkg: &Package,
    assets: &Assets,
    object: &Value,
    props: &Properties,
) -> Option<(Rendered, ScriptText)> {
    let text = super::text_value(object.get("text")).unwrap_or_default();
    let font_name = object.get("font").and_then(Value::as_str).unwrap_or("");
    let prepared = ScriptText {
        font: font_bytes(pkg, assets, font_name)?.into(),
        em: crate::model::number(object.get("pointsize"), props, 30.0).clamp(1.0, 1024.0)
            * super::EM_PER_POINT,
        halign: halign_of(object.get("horizontalalign")),
        valign: valign_of(object.get("verticalalign")),
        shown: text.clone(),
    };
    Some((prepared.render(&text)?, prepared))
}
