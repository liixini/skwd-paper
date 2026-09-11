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
    pub hlsl: Option<crate::hlsl::HlslPair>,
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
    SceneUnderLayer,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FboSize {
    Scale(f32),
    Fit(u32),
    Fixed { width: u32, height: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FboFormat {
    Rgba8,
    R8,
    Rg8,
    R16f,
    Rg16f,
    Rgba16f,
}

impl FboFormat {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "rgba8888" | "rgba_backbuffer" | "rgb_backbuffer" => Some(Self::Rgba8),
            "r8" => Some(Self::R8),
            "rg88" => Some(Self::Rg8),
            "r16f" => Some(Self::R16f),
            "rg1616f" => Some(Self::Rg16f),
            "rgba16f" | "rgba16161616f" => Some(Self::Rgba16f),
            _ => None,
        }
    }

    #[must_use]
    pub fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::R8 => 1,
            Self::Rg8 | Self::R16f => 2,
            Self::Rgba8 | Self::Rg16f => 4,
            Self::Rgba16f => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fbo {
    pub name: String,
    pub size: FboSize,
    pub format: FboFormat,
    pub repeat: Option<bool>,
    pub clear: [f32; 4],
    pub unique: bool,
}

impl Fbo {
    #[must_use]
    pub fn extent(&self, base: (u32, u32)) -> (u32, u32) {
        let (w, h) = (base.0.max(1) as f32, base.1.max(1) as f32);
        let (out_w, out_h) = match self.size {
            FboSize::Scale(scale) => (w / scale.max(1e-3), h / scale.max(1e-3)),
            FboSize::Fit(edge) => {
                let edge = edge.max(1) as f32;
                let k = edge / w.max(h);
                (w * k, h * k)
            }
            FboSize::Fixed { width, height } => (width as f32, height as f32),
        };
        ((out_w.round() as u32).clamp(1, 8192), (out_h.round() as u32).clamp(1, 8192))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Swap {
    pub after_pass: Option<usize>,
    pub source: String,
    pub target: String,
}

pub struct Effect {
    pub name: String,
    pub passes: Vec<EffectPass>,
    pub fbos: Vec<Fbo>,
    pub swaps: Vec<Swap>,
}

fn conditions_hold(value: Option<&Value>, combos: &BTreeMap<String, i64>) -> bool {
    let Some(entries) = value.and_then(Value::as_array) else {
        return true;
    };
    if entries.is_empty() {
        return true;
    }
    entries.iter().any(|entry| {
        entry.as_object().is_some_and(|object| {
            object.iter().all(|(key, expected)| {
                let expected = json_numbers(expected).and_then(|parts| parts.first().copied());
                let actual = combos.get(key).copied().unwrap_or(0);
                expected.is_some_and(|expected| (expected as i64) == actual)
            })
        })
    })
}

fn parse_fbo(entry: &Value, skipped: &mut Vec<String>, effect: &str) -> Option<Fbo> {
    let name = entry.get("name").and_then(Value::as_str)?.to_string();
    let width = entry.get("width").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
    let height = entry.get("height").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
    let size = match (width, height, entry.get("fit").and_then(Value::as_u64)) {
        (Some(width), Some(height), _) if width > 0 && height > 0 => {
            FboSize::Fixed { width, height }
        }
        (_, _, Some(fit)) if fit > 0 => FboSize::Fit(u32::try_from(fit).unwrap_or(u32::MAX)),
        _ => FboSize::Scale(
            entry.get("scale").and_then(Value::as_f64).map_or(1.0, |v| v as f32).max(1e-3),
        ),
    };
    let format_text = entry.get("format").and_then(Value::as_str).unwrap_or("");
    let format = FboFormat::parse(format_text).unwrap_or_else(|| {
        skipped.push(format!("{effect}: fbo {name} format {format_text} unknown, using rgba8"));
        FboFormat::Rgba8
    });
    let repeat =
        entry.get("uvs").and_then(Value::as_str).map(|uvs| uvs.eq_ignore_ascii_case("repeat"));
    let clear = entry.get("clear").and_then(json_numbers).map_or([0.0; 4], |parts| {
        let mut out = [0.0; 4];
        for (slot, value) in out.iter_mut().zip(parts.iter().chain(std::iter::repeat(&0.0))) {
            *slot = *value;
        }
        if parts.len() == 1 {
            out = [parts[0]; 4];
        }
        out
    });
    let unique = entry.get("unique").and_then(Value::as_bool).unwrap_or(false);
    Some(Fbo { name, size, format, repeat, clear, unique })
}

fn instance_combos(
    pkg: &Package,
    assets: &Assets,
    defs: &[Value],
    overrides: Option<&Value>,
) -> BTreeMap<String, i64> {
    let mut combos = BTreeMap::new();
    for (index, def) in defs.iter().enumerate() {
        if let Some(material) = def.get("material").and_then(Value::as_str) {
            let json = pkg.find_json(material).ok().flatten().or_else(|| {
                assets.read(material).and_then(|text| crate::json::parse(text.as_bytes()).ok())
            });
            if let Some(first) = json
                .as_ref()
                .and_then(|json| json.get("passes"))
                .and_then(Value::as_array)
                .and_then(|passes| passes.first())
            {
                combos.extend(combos_of(first.get("combos")));
            }
        }
        if let Some(over) = overrides.and_then(Value::as_array).and_then(|arr| arr.get(index)) {
            combos.extend(combos_of(over.get("combos")));
        }
    }
    combos
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
                out.insert(key.to_ascii_uppercase(), number);
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

pub const COLOR_BLEND_EFFECT: &str = "colorblend";
pub const COLOR_BLEND_SHADER: &str = "skwd/colorblend";

const COLOR_BLEND_VERT: &str = "uniform mat4 g_ModelViewProjectionMatrix;
attribute vec3 a_Position;
attribute vec2 a_TexCoord;
varying vec2 v_TexCoord;
void main() {
	gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
	v_TexCoord = a_TexCoord;
}
";

const COLOR_BLEND_FRAG: &str = "#include \"common_blending.h\"
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture4;
uniform vec4 g_Color4;
varying vec2 v_TexCoord;
void main() {
	vec4 layer = texSample2D(g_Texture0, v_TexCoord) * g_Color4;
	vec4 screen = texSample2D(g_Texture4, v_TexCoord);
	gl_FragColor = vec4(ApplyBlending(BLENDMODE, screen.rgb, layer.rgb, layer.a), screen.a);
}
";

#[must_use]
pub fn shader_blend(mode: u32) -> bool {
    !matches!(mode, 0 | 6 | 8)
}

pub fn color_blend_effect(
    pkg: &Package,
    assets: &Assets,
    mode: u32,
    color: [f32; 3],
    alpha: f32,
    own_id: Option<&str>,
) -> Option<Effect> {
    let material = serde_json::json!({
        "passes": [{"shader": COLOR_BLEND_SHADER, "combos": {"BLENDMODE": mode}}]
    });
    let mut passes = build_material(pkg, assets, &material, None, own_id)?;
    let pass = passes.last_mut()?;
    pass.binds.retain(|(slot, _)| *slot != 4);
    pass.binds.push((4, EffectBind::SceneUnderLayer));
    pass.values.insert("g_Color4".into(), vec![color[0], color[1], color[2], alpha]);
    Some(Effect { name: COLOR_BLEND_EFFECT.into(), passes, fbos: Vec::new(), swaps: Vec::new() })
}

pub fn particle_pass(
    pkg: &Package,
    assets: &Assets,
    material: &Value,
    combos: &BTreeMap<String, i64>,
) -> Option<EffectPass> {
    let overrides = Value::Array(vec![serde_json::json!({ "combos": combos })]);
    let mut passes = build_material(pkg, assets, material, Some(&overrides), None)?;
    passes.pop()
}

fn load_shader(pkg: &Package, assets: &Assets, name: &str, ext: &str) -> Option<String> {
    const MAX_SHADER_BYTES: usize = 4 * 1024 * 1024;
    if name == COLOR_BLEND_SHADER {
        let source = if ext == "vert" { COLOR_BLEND_VERT } else { COLOR_BLEND_FRAG };
        return Some(source.to_string());
    }
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
    build_material(pkg, assets, &material, overrides, own_id)
}

fn build_copy_pass(
    pkg: &Package,
    assets: &Assets,
    def: &Value,
    own_id: Option<&str>,
) -> Option<Vec<EffectPass>> {
    if def.get("command").and_then(Value::as_str) != Some("copy") {
        return None;
    }
    let source = def.get("source").and_then(Value::as_str)?;
    let target = def.get("target").and_then(Value::as_str)?;
    let material = serde_json::json!({"passes": [{"shader": "passthrough"}]});
    let mut built = build_material(pkg, assets, &material, None, own_id)?;
    let pass = built.last_mut()?;
    pass.name = "copy".to_string();
    pass.target = Some(target.to_string());
    pass.binds.push((0, EffectBind::parse(source, own_id)));
    Some(built)
}

const ENGINE_GLOBAL_TARGETS: [&str; 1] = ["_rt_shadowAtlas"];

fn dialect(combos: &BTreeMap<String, i64>, name: &str) -> BTreeMap<String, i64> {
    let mut out = combos.clone();
    out.insert(name.to_string(), 1);
    if name == "HLSL" {
        out.insert("HLSL_SM40".to_string(), 1);
    }
    out
}

fn build_material(
    pkg: &Package,
    assets: &Assets,
    material: &Value,
    overrides: Option<&Value>,
    own_id: Option<&str>,
) -> Option<Vec<EffectPass>> {
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
        let vert =
            shader::relax_portability(&shader::resolve_includes(&vert_src, &loader, 0).ok()?);
        let frag =
            shader::relax_portability(&shader::resolve_includes(&frag_src, &loader, 0).ok()?);
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
            if ENGINE_GLOBAL_TARGETS.contains(&default) {
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
            let Some(texture) = texture else {
                continue;
            };
            combos.entry(format!("TEX{slot}FORMAT")).or_insert(texture.we_format());
        }
        let hlsl_symbols = dialect(&combos, "HLSL");
        let glsl_symbols = dialect(&combos, "GLSL");
        let vert_hlsl = shader::preprocess(&vert, &hlsl_symbols);
        let frag_hlsl = shader::preprocess(&frag, &hlsl_symbols);
        let vert = shader::preprocess(&vert, &glsl_symbols);
        let frag = shader::preprocess(&frag, &glsl_symbols);
        let varyings = shader::varying_map(&vert, &frag, &combos);
        let mut vertex = shader::translate_with(&vert, Stage::Vertex, &combos, Some(&varyings));
        let mut fragment = shader::translate_with(&frag, Stage::Fragment, &combos, Some(&varyings));
        shader::unify_uniforms(&mut vertex, &mut fragment);
        let mut sampler_union = vertex.samplers.clone();
        for sampler in &fragment.samplers {
            if !sampler_union.iter().any(|known| known.name == sampler.name) {
                sampler_union.push(sampler.clone());
            }
        }
        let hlsl = (!crate::hlsl::fragment_indexes_varying_arrays_dynamically(&frag_hlsl)
            && crate::hlsl::varying_slots(&vert_hlsl) <= crate::hlsl::MAX_VARYING_SLOTS)
            .then(|| {
                crate::hlsl::rewrite(
                    &vert_hlsl,
                    &frag_hlsl,
                    &combos,
                    &vertex.uniforms,
                    &sampler_union,
                )
            });
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
            hlsl,
            textures,
            values,
            target: None,
            binds: self_binds.clone(),
        });
    }
    (!out.is_empty()).then_some(out)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameClock {
    pub time: f32,
    pub dt: f32,
    pub daytime: f32,
}

impl FrameClock {
    #[must_use]
    pub fn now(time: f32, dt: f32) -> Self {
        Self { time, dt, daytime: daytime_now() }
    }
}

#[must_use]
pub fn daytime_now() -> f32 {
    let mut spec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &raw mut spec) } != 0 {
        return 0.0;
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&raw const spec.tv_sec, &raw mut tm) }.is_null() {
        return 0.0;
    }
    daytime(tm.tm_hour, tm.tm_min, tm.tm_sec, (spec.tv_nsec / 1_000_000) as i32)
}

#[must_use]
pub fn daytime(hour: i32, minute: i32, second: i32, millisecond: i32) -> f32 {
    (f64::from(hour) / 24.0
        + f64::from(minute) / 1440.0
        + f64::from(second) / 86_400.0
        + f64::from(millisecond) / 86_400_000.0) as f32
}

pub struct PassMeta {
    pub name: String,
    uniforms: Vec<crate::shader::Uniform>,
    constants: BTreeMap<String, Vec<f32>>,
    resolutions: Vec<Option<[f32; 4]>>,
    ndc: bool,
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
            name: pass.name.clone(),
            uniforms: pass.fragment.uniforms.clone(),
            constants: pass.values.clone(),
            resolutions,
            ndc: !pass
                .vertex
                .uniforms
                .iter()
                .any(|uniform| uniform.name == "g_ModelViewProjectionMatrix"),
        }
    }

    #[must_use]
    pub fn ndc(&self) -> bool {
        self.ndc
    }

    #[must_use]
    pub fn time_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| uniform.name == "g_Time")
    }

    #[must_use]
    pub fn pointer_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| {
            matches!(
                uniform.name.as_str(),
                "g_PointerPosition"
                    | "g_PointerPositionLast"
                    | "g_PointerState"
                    | "g_ParallaxPosition"
            )
        })
    }

    pub fn audio_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| uniform.name.starts_with(AUDIO_PREFIX))
    }

    #[must_use]
    pub fn uniform_bytes(
        &self,
        clock: FrameClock,
        quad: Option<(u32, u32)>,
        width: u32,
        height: u32,
        screen: (u32, u32),
        slot_sizes: &[Option<[f32; 4]>],
    ) -> Vec<u8> {
        self.uniform_bytes_with(
            clock,
            quad,
            width,
            height,
            screen,
            slot_sizes,
            &BTreeMap::new(),
            false,
        )
    }

    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn uniform_bytes_with(
        &self,
        clock: FrameClock,
        quad: Option<(u32, u32)>,
        width: u32,
        height: u32,
        screen: (u32, u32),
        slot_sizes: &[Option<[f32; 4]>],
        overrides: &BTreeMap<String, Vec<f32>>,
        d3d_clip: bool,
    ) -> Vec<u8> {
        let mut values = self.constants.clone();
        values.insert("g_Time".into(), vec![clock.time]);
        values.insert("g_Frametime".into(), vec![clock.dt]);
        values.insert("g_Daytime".into(), vec![clock.daytime]);
        values.insert("g_ParallaxPosition".into(), vec![0.5, 0.5]);
        values.insert("g_PointerPosition".into(), vec![0.5, 0.5]);
        values.insert("g_PointerPositionLast".into(), vec![0.5, 0.5]);
        values.insert("g_PointerState".into(), vec![0.0, 0.0, 0.0, 0.0]);
        values.entry("g_TextureReductionScale".into()).or_insert_with(|| vec![1.0]);
        let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
        values.entry("g_Screen".into()).or_insert_with(|| vec![sw, sh, sw / sh]);
        values.entry("g_Alpha".into()).or_insert_with(|| vec![1.0]);
        values.entry("g_UserAlpha".into()).or_insert_with(|| vec![1.0]);
        values.entry("g_Brightness".into()).or_insert_with(|| vec![1.0]);
        values.entry("g_Color".into()).or_insert_with(|| vec![1.0, 1.0, 1.0]);
        values.entry("g_Color4".into()).or_insert_with(|| vec![1.0, 1.0, 1.0, 1.0]);
        values.entry("g_Texture0Rotation".into()).or_insert_with(|| vec![1.0, 0.0, 0.0, 1.0]);
        values.entry("g_Texture0Translation".into()).or_insert_with(|| vec![0.0, 0.0]);
        values.insert("g_TexelSize".into(), vec![1.0 / sw, 1.0 / sh]);
        values.insert("g_TexelSizeHalf".into(), vec![0.5 / sw, 0.5 / sh]);
        let ys = if d3d_clip { 1.0 } else { -1.0 };
        let projection = if let Some((quad_w, quad_h)) = quad {
            let (qw, qh) = ((quad_w.max(1)) as f32, (quad_h.max(1)) as f32);
            Some((
                vec![
                    2.0 / qw,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    ys * 2.0 / qh,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    -1.0,
                    -ys,
                    0.0,
                    1.0,
                ],
                vec![
                    qw / 2.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    ys * qh / 2.0,
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
            ))
        } else {
            None
        };
        if let Some((mvp, inverse)) = projection {
            values.insert("g_EffectModelViewProjectionMatrix".into(), mvp.clone());
            values.insert("g_EffectModelViewProjectionMatrixInverse".into(), inverse.clone());
            values.insert("g_ModelViewProjectionMatrix".into(), mvp);
            values.insert("g_ModelViewProjectionMatrixInverse".into(), inverse);
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
        for (name, value) in overrides {
            values.insert(name.clone(), value.clone());
        }
        for uniform in &self.uniforms {
            let Some((base, _)) = uniform.name.split_once('[') else {
                continue;
            };
            if !base.starts_with(AUDIO_PREFIX) {
                continue;
            }
            if let Some(bands) = overrides.get(base) {
                values.insert(uniform.name.clone(), bands.clone());
            }
        }
        crate::shader::pack_uniforms(&self.uniforms, &values)
    }
}

pub const AUDIO_PREFIX: &str = "g_AudioSpectrum";

pub const BLOOM_LDR_EFFECT_FILE: &str = "effects/skwd/bloom_ldr.json";

const BLOOM_LDR_EFFECT: &str = r#"{
  "fbos": [
    {"name": "_rt_4FrameBuffer", "scale": 4},
    {"name": "_rt_8FrameBuffer", "scale": 8},
    {"name": "_rt_Bloom", "scale": 8}
  ],
  "passes": [
    {"material": "materials/util/downsample_quarter_bloom.json", "target": "_rt_4FrameBuffer",
     "bind": [{"index": 0, "name": "_rt_FullFrameBuffer"}]},
    {"material": "materials/util/downsample_eighth_blur_v.json", "target": "_rt_8FrameBuffer",
     "bind": [{"index": 0, "name": "_rt_4FrameBuffer"}]},
    {"material": "materials/util/blur_h_bloom.json", "target": "_rt_Bloom",
     "bind": [{"index": 0, "name": "_rt_8FrameBuffer"}]},
    {"material": "materials/util/combine_ldr.json",
     "bind": [{"index": 0, "name": "_rt_FullFrameBuffer"}, {"index": 1, "name": "_rt_Bloom"}]}
  ]
}"#;

fn dedupe_binds(binds: &mut Vec<(usize, EffectBind)>) {
    let mut kept: Vec<(usize, EffectBind)> = Vec::with_capacity(binds.len());
    for (index, bind) in binds.drain(..) {
        kept.retain(|(known, _)| *known != index);
        kept.push((index, bind));
    }
    *binds = kept;
}

#[must_use]
pub fn builtin_effect(file: &str) -> Option<Value> {
    match file {
        BLOOM_LDR_EFFECT_FILE => crate::json::parse(BLOOM_LDR_EFFECT.as_bytes()).ok(),
        _ => None,
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
            _ => assets
                .read(file)
                .and_then(|text| crate::json::parse(text.as_bytes()).ok())
                .or_else(|| builtin_effect(file)),
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
        let combos = instance_combos(pkg, assets, defs, overrides);
        let mut passes = Vec::new();
        let mut swaps = Vec::new();
        let mut failed = false;
        for (index, def) in defs.iter().enumerate() {
            if !conditions_hold(def.get("conditions"), &combos) {
                continue;
            }
            let Some(material) = def.get("material").and_then(Value::as_str) else {
                if def.get("command").and_then(Value::as_str) == Some("swap") {
                    if let (Some(source), Some(target)) = (
                        def.get("source").and_then(Value::as_str),
                        def.get("target").and_then(Value::as_str),
                    ) {
                        swaps.push(Swap {
                            after_pass: passes.len().checked_sub(1),
                            source: source.to_string(),
                            target: target.to_string(),
                        });
                    } else {
                        skipped.push(format!("{name}: pass {index} swap incomplete"));
                    }
                    continue;
                }
                match build_copy_pass(pkg, assets, def, own_id) {
                    Some(built) => passes.extend(built),
                    None if def.get("command").is_some() => {
                        skipped.push(format!("{name}: pass {index} command unsupported"));
                    }
                    None => {}
                }
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
                                if !conditions_hold(entry.get("conditions"), &combos) {
                                    return None;
                                }
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
                    dedupe_binds(&mut last.binds);
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
        let fbos: Vec<Fbo> = definition
            .get("fbos")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|entry| conditions_hold(entry.get("conditions"), &combos))
                    .filter_map(|entry| parse_fbo(entry, &mut skipped, &name))
                    .collect()
            })
            .unwrap_or_default();
        out.push(Effect { name, passes, fbos, swaps });
    }
    (out, skipped)
}

#[cfg(test)]
mod tests;
