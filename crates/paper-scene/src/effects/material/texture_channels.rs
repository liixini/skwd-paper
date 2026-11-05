use super::{Assets, EffectPass, Package, build_material};
use crate::puppet::{Mesh, channels};

const ALPHA_WRITING: &str = "alphawriting";
const ALPHA_DEFAULT: &str = "default";
const ALPHA_DISABLED: &str = "disabled";
const DOUBLE_BUFFERED: &str = "DOUBLEBUFFERED";

pub struct Channels {
    pub geometry: channels::Geometry,
    pub pass: EffectPass,
    pub cull: super::CullMode,
    pub alpha_write: bool,
}

fn alpha_write(pass: &serde_json::Value) -> Result<bool, String> {
    match pass.get(ALPHA_WRITING).map(serde_json::Value::as_str) {
        None | Some(Some(ALPHA_DEFAULT)) => Ok(true),
        Some(Some(ALPHA_DISABLED)) => Ok(false),
        Some(_) => Err("puppet channel alpha writing unsupported".into()),
    }
}

pub(super) fn load(
    pkg: &Package,
    assets: &Assets,
    mesh: &mut Mesh,
) -> Result<Option<Channels>, String> {
    let Some(geometry) = &mut mesh.texture_channels else { return Ok(None) };
    let mut material = pkg
        .find_json(&geometry.material)
        .map_err(|error| error.to_string())?
        .or_else(|| {
            assets
                .read(&geometry.material)
                .and_then(|text| crate::json::parse(text.as_bytes()).ok())
        })
        .ok_or_else(|| format!("puppet channel material {} missing", geometry.material))?;
    let validated = super::material_passes(&material, &geometry.material)?;
    let state = super::ImageState::parse(&validated[0], &geometry.material)?;
    if state.has_depth() {
        return Err("puppet channel material depth unsupported".into());
    }
    let cull = state.cull;
    let passes = material
        .get_mut(super::PASSES)
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("puppet channel material passes missing")?;
    let [pass] = passes.as_mut_slice() else {
        return Err("puppet channel material requires one pass".into());
    };
    let alpha_write = alpha_write(pass)?;
    let source = super::super::texture_slot(
        pkg,
        assets,
        pass.get(super::TEXTURES)
            .and_then(serde_json::Value::as_array)
            .and_then(|values| values.first())
            .and_then(serde_json::Value::as_str),
    )
    .ok_or("puppet channel source texture missing")?;
    let mut combos = super::combos_of(pass.get(super::super::COMBOS_FIELD));
    if combos.get(DOUBLE_BUFFERED).is_some_and(|value| *value != 0) {
        return Err("double-buffered puppet channels unsupported".into());
    }
    combos.insert(channels::COMBO.into(), geometry.rows as i64);
    pass[super::super::COMBOS_FIELD] = serde_json::to_value(combos).expect("channel combos");
    let mut passes = build_material(pkg, assets, &material, None, None, None, None)
        .ok_or("puppet channel material shader unavailable")?;
    let mut pass = passes.remove(0);
    if pass.textures.is_empty() {
        pass.textures.push(Some(source));
    } else {
        pass.textures[0] = Some(source);
    }
    if !pass.binds.is_empty()
        || pass.target.is_some()
        || pass.textures.first().and_then(Option::as_ref).is_none()
    {
        return Err("puppet channel material input unsupported".into());
    }
    Ok(Some(Channels {
        geometry: channels::Geometry {
            material: geometry.material.clone(),
            rows: geometry.rows,
            vertices: std::mem::take(&mut geometry.vertices),
            indices: std::mem::take(&mut geometry.indices),
        },
        pass,
        cull,
        alpha_write,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn alpha_write_modes() {
        assert_eq!(super::alpha_write(&json!({})), Ok(true));
        assert_eq!(super::alpha_write(&json!({"alphawriting":"default"})), Ok(true));
        assert_eq!(super::alpha_write(&json!({"alphawriting":"disabled"})), Ok(false));
        assert!(super::alpha_write(&json!({"alphawriting":"other"})).is_err());
        assert!(super::alpha_write(&json!({"alphawriting":1})).is_err());
    }
}
