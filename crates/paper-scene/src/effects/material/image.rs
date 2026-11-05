use super::super::{
    COMBOS_FIELD, USER_SHADER_VALUES, USER_TEXTURES_FIELD, combos_of, merge_texture_slots,
};
use super::{Assets, CONSTANTS, MATERIAL, PASSES, Package, TEXTURES, Value};

const IMAGE: &str = "image";
const INSTANCED: &str = "instanced";
pub(super) const INSTANCE: &str = "instance";

pub(crate) struct Image {
    pub model: Value,
    pub material: Value,
    pub path: String,
}

impl Image {
    pub fn load(pkg: &Package, assets: &Assets, object: &Value) -> Result<Option<Self>, String> {
        let model_path = object[IMAGE].as_str().ok_or("image model path missing")?;
        let Some(model) = super::json(pkg, assets, model_path) else { return Ok(None) };
        let Some(path) = model.get(MATERIAL).and_then(Value::as_str) else { return Ok(None) };
        let mut material = super::json(pkg, assets, path)
            .ok_or_else(|| format!("missing base material {path}"))?;
        if model[INSTANCED] == Value::Bool(true)
            && let Some(instance) = object.get(INSTANCE)
            && let Some(pass) = material
                .get_mut(PASSES)
                .and_then(Value::as_array_mut)
                .and_then(|passes| passes.first_mut())
        {
            overlay(pass, instance);
        }
        let path = path.to_owned();
        Ok(Some(Self { model, material, path }))
    }
}

fn overlay(pass: &mut Value, instance: &Value) {
    if !pass.is_object() {
        return;
    }
    if instance.get(COMBOS_FIELD).is_some() {
        let mut combos = combos_of(pass.get(COMBOS_FIELD));
        combos.extend(combos_of(instance.get(COMBOS_FIELD)));
        pass[COMBOS_FIELD] = serde_json::to_value(combos).expect("material combos");
    }
    for field in [CONSTANTS, USER_SHADER_VALUES] {
        let Some(values) = instance.get(field).and_then(Value::as_object) else { continue };
        if !pass[field].is_object() {
            pass[field] = Value::Object(serde_json::Map::default());
        }
        let target = pass[field].as_object_mut().expect("material map");
        for (key, value) in values {
            target.insert(key.clone(), value.clone());
        }
    }
    for field in [TEXTURES, USER_TEXTURES_FIELD] {
        let Some(values) = instance.get(field).and_then(Value::as_array) else { continue };
        pass[field] = Value::Array(merge_texture_slots(
            pass.get(field).and_then(Value::as_array).map(Vec::as_slice),
            values,
        ));
    }
}

#[cfg(test)]
mod tests;
