use crate::pkg::Package;
use crate::shader::{self, Stage, Translated};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

fn confined_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    let root = std::fs::canonicalize(root).ok()?;
    let candidate = std::fs::canonicalize(root.join(relative)).ok()?;
    candidate.starts_with(&root).then_some(candidate)
}

pub(crate) fn read_confined_bytes(root: &Path, relative: &str, max: usize) -> Option<Vec<u8>> {
    let path = confined_path(root, relative)?;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > max as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= max).then_some(bytes)
}

pub struct Assets {
    root: Option<PathBuf>,
    pub properties: BTreeMap<String, Vec<f32>>,
}

#[must_use]
pub fn parse_properties(project: &Value) -> BTreeMap<String, Vec<f32>> {
    let mut out = BTreeMap::new();
    let Some(props) =
        project.get("general").and_then(|top| top.get("properties")).and_then(Value::as_object)
    else {
        return out;
    };
    for (name, entry) in props {
        let kind = entry.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(kind, "group" | "text" | "Text" | "textinput" | "usershortcut") {
            continue;
        }
        let Some(values) = entry.get("value").and_then(json_numbers) else {
            continue;
        };
        if !values.is_empty() {
            out.insert(name.to_ascii_lowercase(), values);
        }
    }
    out
}

#[must_use]
pub fn parse_property_overrides(
    raw: &serde_json::Map<String, Value>,
) -> BTreeMap<String, Vec<f32>> {
    let mut out = BTreeMap::new();
    for (name, entry) in raw {
        if let Some(values) = json_numbers(entry).filter(|values| !values.is_empty()) {
            out.insert(name.to_ascii_lowercase(), values);
        }
    }
    out
}

impl Assets {
    pub fn discover(configured: Option<&str>) -> Self {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(path) = configured.filter(|text| !text.is_empty()) {
            candidates.push(PathBuf::from(path));
        }
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(
                Path::new(&home)
                    .join(".local/share/Steam/steamapps/common/wallpaper_engine/assets"),
            );
            candidates.push(
                Path::new(&home).join(".steam/steam/steamapps/common/wallpaper_engine/assets"),
            );
        }
        let root = candidates
            .into_iter()
            .find(|path| path.join("shaders").is_dir())
            .and_then(|path| std::fs::canonicalize(path).ok());
        Self { root, properties: BTreeMap::new() }
    }

    #[must_use]
    pub fn with_properties(mut self, properties: BTreeMap<String, Vec<f32>>) -> Self {
        self.properties = properties;
        self
    }

    #[must_use]
    pub fn with_overrides(mut self, overrides: BTreeMap<String, Vec<f32>>) -> Self {
        self.properties.extend(overrides);
        self
    }

    pub fn available(&self) -> bool {
        self.root.is_some()
    }

    pub fn read(&self, relative: &str) -> Option<String> {
        String::from_utf8(read_confined_bytes(
            self.root.as_ref()?,
            relative,
            crate::json::MAX_JSON_BYTES,
        )?)
        .ok()
    }
}

pub struct EffectPass {
    pub name: String,
    pub vertex: Translated,
    pub fragment: Translated,
    pub textures: Vec<Option<crate::model::Texture>>,
    pub values: BTreeMap<String, Vec<f32>>,
    pub target: Option<String>,
    pub binds: Vec<(usize, EffectBind)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompositeBuffer {
    A,
    B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectBind {
    Previous,
    Named(String),
    LayerComposite { layer: String, buffer: CompositeBuffer },
    SceneSoFar,
}

impl EffectBind {
    #[must_use]
    pub fn parse(name: &str, own_id: Option<&str>) -> Self {
        if name == "previous" {
            return Self::Previous;
        }
        if name.eq_ignore_ascii_case("_rt_FullFrameBuffer") {
            return Self::SceneSoFar;
        }
        if let Some((layer, suffix)) =
            name.strip_prefix("_rt_imageLayerComposite_").and_then(|rest| rest.rsplit_once('_'))
        {
            let buffer = match suffix {
                "a" | "A" => Some(CompositeBuffer::A),
                "b" | "B" => Some(CompositeBuffer::B),
                _ => None,
            };
            if let Some(buffer) = buffer.filter(|_| !layer.is_empty()) {
                if own_id == Some(layer) {
                    return Self::Previous;
                }
                return Self::LayerComposite { layer: layer.to_string(), buffer };
            }
        }
        Self::Named(name.to_string())
    }
}

pub struct Effect {
    pub name: String,
    pub passes: Vec<EffectPass>,
    pub fbos: Vec<(String, u32)>,
}

pub(crate) fn json_numbers(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Number(num) => Some(vec![num.as_f64()? as f32]),
        Value::Bool(flag) => Some(vec![f32::from(u8::from(*flag))]),
        Value::String(text) => {
            let parts: Vec<f32> =
                text.split_whitespace().filter_map(|part| part.parse().ok()).collect();
            (!parts.is_empty()).then_some(parts)
        }
        Value::Object(obj) => obj.get("value").and_then(json_numbers),
        _ => None,
    }
}

fn combos_of(value: Option<&Value>) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(obj)) = value {
        for (key, raw) in obj {
            let number = match raw {
                Value::Number(num) => num.as_i64(),
                Value::Bool(flag) => Some(i64::from(*flag)),
                Value::String(text) => text.parse().ok(),
                _ => None,
            };
            if let Some(number) = number {
                out.insert(key.clone(), number);
            }
        }
    }
    out
}

pub(crate) fn bound_value(
    raw: &Value,
    properties: &BTreeMap<String, Vec<f32>>,
) -> Option<Vec<f32>> {
    let Value::Object(obj) = raw else {
        return json_numbers(raw);
    };
    match obj.get("user") {
        Some(Value::String(key)) => properties
            .get(&key.to_ascii_lowercase())
            .cloned()
            .or_else(|| obj.get("value").and_then(json_numbers)),
        Some(Value::Object(binding)) => {
            let name = binding.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
            let condition = binding.get("condition").and_then(Value::as_str)?;
            let wanted: f32 = condition.parse().ok()?;
            properties
                .get(&name)
                .and_then(|values| values.first())
                .map(|selected| {
                    vec![f32::from(u8::from((*selected - wanted).abs() < f32::EPSILON))]
                })
                .or_else(|| obj.get("value").and_then(json_numbers))
        }
        _ => json_numbers(raw),
    }
}

fn is_visible(value: Option<&Value>, properties: &BTreeMap<String, Vec<f32>>) -> bool {
    let Some(value) = value else {
        return true;
    };
    if let Value::String(text) = value {
        let trimmed = text.trim();
        return !(trimmed.eq_ignore_ascii_case("false") || trimmed == "0");
    }
    bound_value(value, properties)
        .and_then(|values| values.first().copied())
        .is_none_or(|value| value != 0.0)
}

fn constants_of(
    value: Option<&Value>,
    properties: &BTreeMap<String, Vec<f32>>,
) -> BTreeMap<String, Vec<f32>> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(obj)) = value {
        for (key, raw) in obj {
            if let Some(values) = bound_value(raw, properties) {
                out.insert(key.clone(), values);
            }
        }
    }
    out
}

fn shader_values(value: Option<&Value>) -> Option<&Value> {
    let obj = value?.as_object()?;
    obj.get("constantshadervalues").or_else(|| obj.get("constants"))
}

fn resolve_material_names(
    values: &BTreeMap<String, Vec<f32>>,
    uniforms: &[crate::shader::Uniform],
) -> BTreeMap<String, Vec<f32>> {
    let mut out = BTreeMap::new();
    for uniform in uniforms {
        let Some(material) = uniform.material.as_deref() else {
            continue;
        };
        if let Some(found) = values.get(material).or_else(|| {
            values.iter().find(|(key, _)| key.eq_ignore_ascii_case(material)).map(|(_, hit)| hit)
        }) {
            out.insert(uniform.name.clone(), found.clone());
        }
    }
    for (key, value) in values {
        out.entry(key.clone()).or_insert_with(|| value.clone());
    }
    out
}

fn load_shader(pkg: &Package, assets: &Assets, name: &str, ext: &str) -> Option<String> {
    const MAX_SHADER_BYTES: usize = 4 * 1024 * 1024;
    let rel = format!("shaders/{name}.{ext}");
    pkg.find(&rel)
        .filter(|bytes| bytes.len() <= MAX_SHADER_BYTES)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .or_else(|| assets.read(&rel))
}

fn texture_slot(
    pkg: &Package,
    assets: &Assets,
    raw: Option<&str>,
) -> Option<crate::model::Texture> {
    let name = raw.filter(|text| !text.is_empty())?;
    crate::model::load_texture_named(pkg, name).or_else(|| {
        crate::model::load_texture_bytes(&assets.read_bytes(&format!("materials/{name}.tex"))?)
    })
}

impl Assets {
    pub fn read_bytes(&self, relative: &str) -> Option<Vec<u8>> {
        read_confined_bytes(self.root.as_ref()?, relative, crate::pkg::MAX_PACKAGE_ENTRY_BYTES)
    }

    #[must_use]
    pub fn preset_root(&self, name: &str) -> Option<PathBuf> {
        let relative = Path::new("presets").join(name);
        let root = confined_path(self.root.as_ref()?, relative.to_str()?)?;
        root.is_dir().then_some(root)
    }
}

fn build_pass(
    pkg: &Package,
    assets: &Assets,
    material_path: &str,
    overrides: Option<&Value>,
    own_id: Option<&str>,
) -> Option<Vec<EffectPass>> {
    let material = pkg.find_json(material_path).ok().flatten().or_else(|| {
        assets.read(material_path).and_then(|text| crate::json::parse(text.as_bytes()).ok())
    })?;
    let passes = material.get("passes")?.as_array()?;
    let mut out = Vec::new();
    for (index, pass) in passes.iter().enumerate() {
        let Some(shader_name) = pass.get("shader").and_then(Value::as_str) else {
            continue;
        };
        let props = &assets.properties;
        let mut combos = combos_of(pass.get("combos"));
        let mut values = constants_of(shader_values(Some(pass)), props);
        if let Some(over) =
            overrides.and_then(|value| value.as_array()).and_then(|arr| arr.get(index))
        {
            for (key, value) in combos_of(over.get("combos")) {
                combos.insert(key, value);
            }
            for (key, value) in constants_of(shader_values(Some(over)), props) {
                values.insert(key, value);
            }
        }
        let vert_src = load_shader(pkg, assets, shader_name, "vert")?;
        let frag_src = load_shader(pkg, assets, shader_name, "frag")?;
        let loader = |name: &str| -> Option<String> {
            pkg.find(&format!("shaders/{name}"))
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .or_else(|| assets.read(&format!("shaders/{name}")))
        };
        let vert = shader::resolve_includes(&vert_src, &loader, 0).ok()?;
        let frag = shader::resolve_includes(&frag_src, &loader, 0).ok()?;
        let (vert, frag) = shader::unify_varying_types(&vert, &frag);
        for (name, value) in
            shader::combo_defaults(&vert).into_iter().chain(shader::combo_defaults(&frag))
        {
            combos.entry(name).or_insert(value);
        }
        let mut names: Vec<Option<String>> = pass
            .get("textures")
            .and_then(Value::as_array)
            .map(|slots| slots.iter().map(|entry| entry.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if let Some(over) = overrides
            .and_then(Value::as_array)
            .and_then(|arr| arr.get(index))
            .and_then(|value| value.get("textures"))
            .and_then(Value::as_array)
        {
            if over.len() > names.len() {
                names.resize(over.len(), None);
            }
            for (slot, entry) in over.iter().enumerate() {
                if let Some(name) = entry.as_str().filter(|text| !text.is_empty()) {
                    names[slot] = Some(name.to_string());
                }
            }
        }
        let probe = shader::translate(&frag, Stage::Fragment, &combos);
        for sampler in &probe.samplers {
            let Some(combo) = sampler.combo.as_deref() else {
                continue;
            };
            let present = names
                .get(sampler.index as usize)
                .and_then(Option::as_ref)
                .is_some_and(|name| !name.is_empty());
            if present {
                combos.insert(combo.to_string(), 1);
            }
        }
        let mut self_binds = Vec::new();
        for sampler in &probe.samplers {
            let slot = sampler.index as usize;
            if slot == 0 || sampler.combo.is_some() {
                continue;
            }
            let Some(default) = sampler.default.as_deref().filter(|text| !text.is_empty()) else {
                continue;
            };
            if slot >= names.len() {
                names.resize(slot + 1, None);
            }
            if names[slot].as_deref().is_some_and(|name| !name.is_empty()) {
                continue;
            }
            if default.starts_with("_rt_") {
                self_binds.push((slot, EffectBind::parse(default, own_id)));
            } else {
                names[slot] = Some(default.to_string());
            }
        }
        let textures: Vec<Option<crate::model::Texture>> = names
            .iter()
            .enumerate()
            .map(|(slot, name)| {
                if slot == 0 {
                    return None;
                }
                texture_slot(pkg, assets, name.as_deref())
            })
            .collect();
        for (slot, texture) in textures.iter().enumerate() {
            let format = match texture.as_ref().map(|texture| texture.format) {
                Some(crate::tex::TexFormat::Rg88) => 8,
                Some(crate::tex::TexFormat::R8) => 9,
                _ => continue,
            };
            combos.entry(format!("TEX{slot}FORMAT")).or_insert(format);
        }
        let varyings = shader::varying_map(&vert, &frag);
        let mut vertex = shader::translate_with(&vert, Stage::Vertex, &combos, Some(&varyings));
        let mut fragment = shader::translate_with(&frag, Stage::Fragment, &combos, Some(&varyings));
        shader::unify_uniforms(&mut vertex, &mut fragment);
        let values = resolve_material_names(&values, &fragment.uniforms);
        for (slot, name) in names.iter().enumerate().skip(1) {
            let Some(name) = name else {
                continue;
            };
            if !name.starts_with("_rt_") {
                continue;
            }
            self_binds.push((slot, EffectBind::parse(name, own_id)));
        }
        out.push(EffectPass {
            name: shader_name.to_string(),
            vertex,
            fragment,
            textures,
            values,
            target: None,
            binds: self_binds.clone(),
        });
    }
    (!out.is_empty()).then_some(out)
}

pub struct PassMeta {
    uniforms: Vec<crate::shader::Uniform>,
    constants: BTreeMap<String, Vec<f32>>,
    resolutions: Vec<Option<[f32; 4]>>,
}

impl PassMeta {
    #[must_use]
    pub fn of(pass: &EffectPass) -> Self {
        let resolutions = pass
            .textures
            .iter()
            .map(|slot| {
                slot.as_ref().map(|tex| {
                    [
                        tex.width as f32,
                        tex.height as f32,
                        tex.img_width as f32,
                        tex.img_height as f32,
                    ]
                })
            })
            .collect();
        Self {
            uniforms: pass.fragment.uniforms.clone(),
            constants: pass.values.clone(),
            resolutions,
        }
    }

    #[must_use]
    pub fn time_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| uniform.name == "g_Time")
    }

    #[must_use]
    pub fn uniform_bytes(
        &self,
        time: f32,
        quad: Option<(u32, u32)>,
        width: u32,
        height: u32,
        slot_sizes: &[Option<[f32; 4]>],
    ) -> Vec<u8> {
        let mut values = self.constants.clone();
        values.insert("g_Time".into(), vec![time]);
        values.insert("g_Daytime".into(), vec![0.5]);
        values.insert("g_ParallaxPosition".into(), vec![0.5, 0.5]);
        values.insert("g_PointerPosition".into(), vec![0.5, 0.5]);
        values.entry("g_Screen".into()).or_insert_with(|| {
            vec![width as f32, height as f32, width as f32 / height.max(1) as f32]
        });
        values.insert("g_Alpha".into(), vec![1.0]);
        values.insert("g_UserAlpha".into(), vec![1.0]);
        values.entry("g_Brightness".into()).or_insert_with(|| vec![1.0]);
        values.insert("g_Color".into(), vec![1.0, 1.0, 1.0]);
        values.insert("g_Color4".into(), vec![1.0, 1.0, 1.0, 1.0]);
        let (tw, th) = (width.max(1) as f32, height.max(1) as f32);
        values.insert("g_TexelSize".into(), vec![1.0 / tw, 1.0 / th]);
        values.insert("g_TexelSizeHalf".into(), vec![0.5 / tw, 0.5 / th]);
        if let Some((quad_w, quad_h)) = quad {
            let (qw, qh) = ((quad_w.max(1)) as f32, (quad_h.max(1)) as f32);
            values.insert(
                "g_ModelViewProjectionMatrix".into(),
                vec![
                    2.0 / qw,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    -2.0 / qh,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    -1.0,
                    1.0,
                    0.0,
                    1.0,
                ],
            );
            values.insert(
                "g_ModelViewProjectionMatrixInverse".into(),
                vec![
                    qw / 2.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    -qh / 2.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    qw / 2.0,
                    qh / 2.0,
                    0.0,
                    1.0,
                ],
            );
        }
        let (w, h) = (width as f32, height as f32);
        values.insert("g_Resolution".into(), vec![w, h, 1.0 / w.max(1.0), 1.0 / h.max(1.0)]);
        for slot in 0..8 {
            let res = slot_sizes
                .get(slot)
                .copied()
                .flatten()
                .or_else(|| self.resolutions.get(slot).copied().flatten())
                .unwrap_or([w, h, w, h]);
            values.insert(format!("g_Texture{slot}Resolution"), res.to_vec());
        }
        crate::shader::pack_uniforms(&self.uniforms, &values)
    }
}

pub fn load_effects(pkg: &Package, assets: &Assets, object: &Value) -> (Vec<Effect>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let Some(entries) = object.get("effects").and_then(Value::as_array) else {
        return (out, skipped);
    };
    let own = object.get("id").map(|id| match id {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    });
    let own_id = own.as_deref();
    for entry in entries {
        let Some(file) = entry.get("file").and_then(Value::as_str) else {
            continue;
        };
        if !is_visible(entry.get("visible"), &assets.properties) {
            continue;
        }
        let name = file.rsplit('/').nth(1).unwrap_or(file).to_string();
        let definition = match pkg.find_json(file) {
            Ok(Some(value)) => Some(value),
            _ => assets.read(file).and_then(|text| crate::json::parse(text.as_bytes()).ok()),
        };
        let Some(definition) = definition else {
            skipped.push(format!("{name}: effect json missing"));
            continue;
        };
        let Some(defs) = definition.get("passes").and_then(Value::as_array) else {
            skipped.push(format!("{name}: no passes"));
            continue;
        };
        let overrides = entry.get("passes");
        let mut passes = Vec::new();
        let mut failed = false;
        for (index, def) in defs.iter().enumerate() {
            let Some(material) = def.get("material").and_then(Value::as_str) else {
                continue;
            };
            let slot_override = overrides
                .and_then(Value::as_array)
                .and_then(|arr| arr.get(index))
                .map(|value| Value::Array(vec![value.clone()]));
            if let Some(mut built) =
                build_pass(pkg, assets, material, slot_override.as_ref(), own_id)
            {
                let target = def.get("target").and_then(Value::as_str).map(str::to_string);
                let binds: Vec<(usize, EffectBind)> = def
                    .get("bind")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| {
                                let name = entry.get("name").and_then(Value::as_str)?;
                                let index = entry.get("index").and_then(Value::as_u64)?;
                                Some((
                                    usize::try_from(index).ok()?,
                                    EffectBind::parse(name, own_id),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(last) = built.last_mut() {
                    last.target = target;
                    last.binds.extend(binds);
                }
                passes.extend(built);
            } else {
                failed = true;
                skipped.push(format!("{name}: pass {index} ({material}) unavailable"));
                break;
            }
        }
        if failed || passes.is_empty() {
            continue;
        }
        let fbos = definition
            .get("fbos")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let name = entry.get("name").and_then(Value::as_str)?;
                        let scale = entry
                            .get("scale")
                            .and_then(Value::as_u64)
                            .and_then(|raw| u32::try_from(raw).ok())
                            .unwrap_or(1)
                            .max(1);
                        Some((name.to_string(), scale))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Effect { name, passes, fbos });
    }
    (out, skipped)
}

#[cfg(test)]
mod tests;
