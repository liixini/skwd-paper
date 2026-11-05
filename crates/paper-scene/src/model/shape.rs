use super::{Texture, solid_texture};
use serde_json::Value;

pub(crate) const FIELD: &str = "shape";
pub(crate) const KIND: &str = "shape:quad";
const QUAD: &str = "quad";
const TRANSPARENT: [u8; 4] = [0; 4];

pub(crate) fn is_quad(object: &Value) -> bool {
    object.get(FIELD).and_then(Value::as_str) == Some(QUAD)
}

pub(super) fn texture() -> Texture {
    let mut texture = solid_texture();
    texture.pixels = std::sync::Arc::new(crate::tex::Pixels::rgba(1, 1, TRANSPARENT.to_vec()));
    texture
}
