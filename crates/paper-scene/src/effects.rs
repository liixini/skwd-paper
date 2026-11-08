pub mod customshader;
pub mod material;

use crate::pkg::Package;
use crate::shader::{self, Stage, Translated};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const SCENE_TEXTURE_KIND: &str = "scenetexture";
const PRESET_FIELD: &str = "preset";
const SCENE_TEXTURE_TYPE_FIELD: &str = "type";
const SCENE_TEXTURE_VALUE_FIELD: &str = "value";
const GENERAL_FIELD: &str = "general";
const PROPERTIES_FIELD: &str = "properties";
pub(crate) const USER_SHADER_VALUES: &str = "usershadervalues";
pub(crate) const CONSTANT_SHADER_VALUES: &str = "constantshadervalues";
pub(crate) const EFFECTS_FIELD: &str = "effects";
const VISIBLE_FIELD: &str = "visible";
const FUNCTIONS_FIELD: &str = "functions";
const FUNCTION_ACTION_FIELD: &str = "action";
const FUNCTION_TARGETS_FIELD: &str = "fbos";
const FUNCTION_CLEAR_ACTION: &str = "clear";
const FBO_NAME_FIELD: &str = "name";
const MAX_EFFECT_FUNCTIONS: usize = 64;
const MAX_FUNCTION_TARGETS: usize = 32;
pub(crate) const MAX_EFFECT_FUNCTION_NAME_BYTES: usize = 128;
pub(crate) const SPRITESHEET_COMBO: &str = "SPRITESHEET";
pub(crate) const COMBOS_FIELD: &str = "combos";
const GEOMETRY_COMBO: &str = "GS_ENABLED";
pub const MAX_TEXTURE_SLOTS: u32 = 16;
const TEXTURE_UNIFORM_PREFIX: &str = "g_Texture";
const TEXTURE_RESOLUTION: &str = "Resolution";
const TEXTURE_ROTATION: &str = "Rotation";
const TEXTURE_TRANSLATION: &str = "Translation";
const IDENTITY_ROTATION: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const ZERO_TRANSLATION: [f32; 2] = [0.0; 2];

fn texture_uniform(name: &str) -> Option<(usize, &str)> {
    let text = name.strip_prefix(TEXTURE_UNIFORM_PREFIX)?;
    let split = text.find(|character: char| !character.is_ascii_digit())?;
    let slot = text[..split].parse::<usize>().ok()?;
    (slot < MAX_TEXTURE_SLOTS as usize).then_some((slot, &text[split..]))
}

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
    read_bounded(&confined_path(root, relative)?, max)
}

fn read_bounded(path: &Path, max: usize) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > max as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes).ok()?;
    crate::source::file(path, &bytes);
    (bytes.len() <= max).then_some(bytes)
}

pub struct Assets {
    root: Option<PathBuf>,
    effect_root: Option<PathBuf>,
    scene_textures: BTreeMap<String, (PathBuf, String)>,
    custom_shaders: BTreeMap<String, String>,
    pub(crate) system_fonts: std::sync::Mutex<crate::text::system_font::Cache>,
    pub properties: BTreeMap<String, Vec<f32>>,
    property_dependencies:
        std::sync::Arc<std::sync::Mutex<Option<std::collections::BTreeSet<String>>>>,
    texture_cache:
        Option<std::sync::Arc<std::sync::Mutex<crate::model::texture_cache::TextureCache>>>,
    context: SceneContext,
}

#[derive(Clone, Default)]
pub(crate) struct SceneContext {
    pub projection: Option<crate::model::ProjectionMode>,
    pub lighting: crate::model::lighting::Config,
    pub fog: crate::model::fog::Config,
}

pub(crate) fn property_dependencies(value: &Value) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    match value {
        Value::Object(map) => {
            if let Some(user) = map.get("user")
                && let Some(name) = user.as_str().or_else(|| user.get("name")?.as_str())
            {
                names.insert(name.to_ascii_lowercase());
            }
            if let Some(bindings) = map.get(USER_SHADER_VALUES).and_then(Value::as_object) {
                names.extend(bindings.keys().map(|name| name.to_ascii_lowercase()));
            }
            for child in map.values() {
                names.extend(property_dependencies(child));
            }
        }
        Value::Array(values) => {
            for child in values {
                names.extend(property_dependencies(child));
            }
        }
        _ => {}
    }
    names
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
    pub(crate) fn snapshot(&self) -> Self {
        Self {
            root: self.root.clone(),
            effect_root: self.effect_root.clone(),
            scene_textures: self.scene_textures.clone(),
            custom_shaders: self.custom_shaders.clone(),
            system_fonts: std::sync::Mutex::default(),
            properties: self.properties.clone(),
            property_dependencies: std::sync::Arc::default(),
            texture_cache: None,
            context: self.context.clone(),
        }
    }

    pub(crate) fn with_scene(&self, context: SceneContext) -> Self {
        let mut assets = self.snapshot();
        assets.context = context;
        assets.property_dependencies = self.property_dependencies.clone();
        assets.texture_cache.clone_from(&self.texture_cache);
        assets
    }

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
        Self {
            root,
            effect_root: None,
            scene_textures: BTreeMap::new(),
            custom_shaders: BTreeMap::new(),
            system_fonts: std::sync::Mutex::default(),
            properties: BTreeMap::new(),
            property_dependencies: std::sync::Arc::new(std::sync::Mutex::new(Some(
                std::collections::BTreeSet::new(),
            ))),
            texture_cache: None,
            context: SceneContext::default(),
        }
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

    pub(crate) fn with_scene_textures(
        mut self,
        document: &Value,
        source: &Path,
        directories: &[PathBuf],
    ) -> std::io::Result<Self> {
        let Some(declarations) = document
            .get(GENERAL_FIELD)
            .and_then(|general| general.get(PROPERTIES_FIELD))
            .and_then(Value::as_object)
        else {
            return Ok(self);
        };
        let declarations: Vec<_> = declarations
            .iter()
            .filter(|(_, entry)| {
                entry.get(SCENE_TEXTURE_TYPE_FIELD).and_then(Value::as_str)
                    == Some(SCENE_TEXTURE_KIND)
            })
            .collect();
        if declarations.is_empty() {
            return Ok(self);
        }
        let documents = directories
            .iter()
            .map(|dir| {
                paper_control::we_project::read(dir).map(|document| {
                    crate::source::declaration(dir, &document);
                    (dir, document)
                })
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        for (name, entry) in declarations {
            let Some(path) = entry
                .get(SCENE_TEXTURE_VALUE_FIELD)
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
            else {
                continue;
            };
            let root = documents
                .iter()
                .find(|(_, document)| {
                    document.get(PRESET_FIELD).and_then(|preset| preset.get(name)).is_some()
                })
                .map_or(source, |(dir, _)| dir.as_path());
            self.scene_textures
                .insert(name.to_ascii_lowercase(), (root.to_path_buf(), path.to_owned()));
        }
        Ok(self)
    }

    pub(crate) fn scene_texture_bytes(&self, name: &str, max: usize) -> Option<Vec<u8>> {
        let (root, path) = self.scene_textures.get(&name.to_ascii_lowercase())?;
        read_confined_bytes(root, path, max)
    }

    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn available(&self) -> bool {
        self.root.is_some()
    }

    pub fn read(&self, relative: &str) -> Option<String> {
        let text =
            String::from_utf8(self.read_scoped_bytes(relative, crate::json::MAX_JSON_BYTES)?)
                .ok()?;
        if relative.ends_with(".json") {
            let mut dependencies = self
                .property_dependencies
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(names) = &mut *dependencies {
                match crate::json::parse(text.as_bytes()) {
                    Ok(value) => names.extend(property_dependencies(&value)),
                    Err(_) => *dependencies = None,
                }
            }
        }
        Some(text)
    }

    pub(crate) fn with_effect(&self, file: &str) -> Self {
        let mut scoped = self.snapshot();
        scoped.property_dependencies = self.property_dependencies.clone();
        scoped.texture_cache.clone_from(&self.texture_cache);
        scoped.effect_root = self
            .root
            .as_deref()
            .and_then(|root| confined_path(root, file))
            .and_then(|path| path.parent().map(Path::to_path_buf));
        scoped
    }

    fn read_scoped_bytes(&self, relative: &str, max: usize) -> Option<Vec<u8>> {
        self.effect_root
            .as_deref()
            .and_then(|root| read_confined_bytes(root, relative, max))
            .or_else(|| read_confined_bytes(self.root.as_deref()?, relative, max))
    }

    pub(crate) fn with_texture_cache(mut self) -> Self {
        self.texture_cache = Some(std::sync::Arc::default());
        self
    }

    pub(crate) fn read_asset(&self, path: &Path) -> Option<Vec<u8>> {
        let confined = [self.effect_root.as_deref(), self.root.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .any(|root| path.starts_with(root));
        confined.then(|| read_bounded(path, crate::pkg::MAX_PACKAGE_ENTRY_BYTES)).flatten()
    }

    pub(crate) fn asset_path(&self, relative: &str) -> Option<PathBuf> {
        self.effect_root
            .as_deref()
            .and_then(|root| confined_path(root, relative))
            .or_else(|| confined_path(self.root.as_deref()?, relative))
    }

    pub(crate) fn cached_texture(
        &self,
        key: crate::model::texture_cache::Key,
        load: impl FnOnce() -> Option<crate::model::Texture>,
    ) -> Option<crate::model::Texture> {
        let Some(cache) = &self.texture_cache else {
            return load();
        };
        let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(texture) = cache.get(&key) {
            return Some(texture);
        }
        let texture = load()?;
        cache.insert(key, &texture);
        Some(texture)
    }

    pub(crate) fn texture_charge(&self, texture: &crate::model::Texture) -> usize {
        self.texture_cache.as_ref().map_or_else(
            || texture.payload_bytes(),
            |cache| cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).charge(texture),
        )
    }

    pub(crate) fn depend(&self, name: &str) {
        let mut dependencies =
            self.property_dependencies.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(names) = &mut *dependencies {
            names.insert(name.to_ascii_lowercase());
        }
    }

    pub(crate) fn property_dependencies(&self) -> Option<std::collections::BTreeSet<String>> {
        self.property_dependencies.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

const BLENDING_FIELD: &str = "blending";
const BLEND_COPY: &str = "normal";
const BLEND_COVERAGE: &str = "alphatocoverage";
const ALPHA_COVERAGE_COMBO: &str = "ALPHATOCOVERAGE";
pub(crate) const SHADER_VERSION_COMBO: &str = "SHADERVERSION";
pub(crate) const SHADER_VERSION: i64 = 69;
const BLEND_ALPHA: &str = "translucent";
const BLEND_ADD: &str = "additive";
const BLEND_SCREEN: &str = "screen";
pub(crate) const USER_TEXTURES_FIELD: &str = "usertextures";
pub(crate) const TEXTURES_FIELD: &str = "textures";
const TEXTURE_NAME_FIELD: &str = "name";

pub(crate) fn texture_name(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| value.get(TEXTURE_NAME_FIELD)?.as_str())
}

pub(crate) fn merge_texture_slots(base: Option<&[Value]>, updates: &[Value]) -> Vec<Value> {
    let mut slots = base.map_or_else(Vec::new, <[Value]>::to_vec);
    slots.resize(slots.len().max(updates.len()), Value::Null);
    for (slot, value) in updates.iter().enumerate() {
        let name = texture_name(value);
        if name.is_some_and(|name| !name.is_empty()) {
            slots[slot] = value.clone();
        }
    }
    slots
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PassBlend {
    AlphaCoverage,
    #[default]
    Copy,
    Alpha,
    Add,
    Screen,
}

impl PassBlend {
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some(BLEND_COPY) => Some(Self::Copy),
            Some(BLEND_COVERAGE) => Some(Self::AlphaCoverage),
            Some(BLEND_ALPHA) => Some(Self::Alpha),
            Some(BLEND_ADD) => Some(Self::Add),
            Some(BLEND_SCREEN) => Some(Self::Screen),
            Some(_) => None,
        }
    }
}

pub struct EffectPass {
    pub blend: PassBlend,
    pub property_source: Option<(usize, usize)>,
    pub user_uniforms: BTreeSet<String>,
    pub name: String,
    pub vertex: Translated,
    pub geometry: Option<Translated>,
    pub fragment: Translated,
    pub hlsl: Option<crate::hlsl::HlslPair>,
    pub textures: Vec<Option<crate::model::Texture>>,
    pub values: BTreeMap<String, Vec<f32>>,
    pub target: Option<String>,
    pub binds: Vec<(usize, EffectBind)>,
}

const TIME_UNIFORM: &str = "g_Time";
const FRAME_TIME_UNIFORM: &str = "g_Frametime";
const DAY_TIME_UNIFORM: &str = "g_Daytime";
const POINTER_UNIFORM: &str = "g_PointerPosition";
const LAST_POINTER_UNIFORM: &str = "g_PointerPositionLast";
const POINTER_STATE_UNIFORM: &str = "g_PointerState";
const PARALLAX_UNIFORM: &str = "g_ParallaxPosition";

#[derive(PartialEq)]
enum FrameInput {
    Time,
    Clock,
    Pointer,
    Audio,
}

fn frame_input(name: &str) -> Option<FrameInput> {
    match name {
        TIME_UNIFORM | FRAME_TIME_UNIFORM => Some(FrameInput::Time),
        DAY_TIME_UNIFORM => Some(FrameInput::Clock),
        POINTER_UNIFORM | LAST_POINTER_UNIFORM | POINTER_STATE_UNIFORM | PARALLAX_UNIFORM => {
            Some(FrameInput::Pointer)
        }
        name if name.starts_with(AUDIO_PREFIX) => Some(FrameInput::Audio),
        _ => None,
    }
}

impl EffectPass {
    #[must_use]
    pub fn clocked(&self) -> bool {
        self.vertex
            .uniforms
            .iter()
            .chain(self.geometry.iter().flat_map(|stage| &stage.uniforms))
            .chain(&self.fragment.uniforms)
            .any(|uniform| {
                matches!(frame_input(&uniform.name), Some(FrameInput::Time | FrameInput::Clock))
            })
    }

    #[must_use]
    pub fn frame_dependent(&self) -> bool {
        self.vertex
            .uniforms
            .iter()
            .chain(self.geometry.iter().flat_map(|stage| &stage.uniforms))
            .chain(&self.fragment.uniforms)
            .any(|uniform| frame_input(&uniform.name).is_some())
    }

    pub fn bind_texture(&mut self, bind: &EffectBind, texture: fn() -> crate::model::Texture) {
        let slots: Vec<usize> =
            self.binds.iter().filter(|(_, bound)| bound == bind).map(|(slot, _)| *slot).collect();
        for slot in slots {
            if slot >= self.textures.len() {
                self.textures.resize_with(slot + 1, || None);
            }
            self.textures[slot] = Some(texture());
        }
        self.binds.retain(|(_, bound)| bound != bind);
    }

    pub fn samples_reflection(&self) -> bool {
        self.samples(|bind| *bind == EffectBind::Reflection)
    }

    pub fn samples(&self, wanted: impl Fn(&EffectBind) -> bool) -> bool {
        let glsl = [&self.vertex.source, &self.fragment.source];
        let hlsl = self.hlsl.iter().flat_map(|pair| [&pair.vertex, &pair.fragment]);
        self.binds.iter().filter(|(_, bind)| wanted(bind)).any(|(slot, _)| {
            self.vertex.samplers.iter().chain(&self.fragment.samplers).any(|sampler| {
                let name = &sampler.name;
                sampler.index as usize == *slot
                    && (glsl
                        .iter()
                        .any(|source| shader::reads_sampler(source, &BTreeMap::new(), name))
                        || hlsl.clone().any(|source| crate::hlsl::reads_texture(source, name)))
            })
        })
    }

    fn filter_binds(&mut self) {
        self.binds.retain(|(slot, _)| {
            self.vertex
                .samplers
                .iter()
                .chain(&self.fragment.samplers)
                .any(|sampler| sampler.index as usize == *slot)
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompositeBuffer {
    A,
    B,
}

pub const REFLECTION_BUFFER: &str = "_rt_Reflection";
const RENDER_TARGET_PREFIX: &str = "_rt_";
pub const COOKIE_BUFFER: &str = "_alias_lightCookie";
const SHADOW_BUFFER: &str = "_rt_shadowAtlas";

pub const MIP_FRAME_BUFFER: &str = "_rt_MipMappedFrameBuffer";
pub const SOURCE_COLOR_UNIFORM: &str = "g_Color4";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectBind {
    Previous,
    Named(String),
    LayerComposite { layer: String, buffer: CompositeBuffer },
    SceneSoFar,
    SceneUnderLayer,
    SceneHistory,
    Reflection,
    ShadowAtlas,
    LightCookie,
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
        if name.eq_ignore_ascii_case(REFLECTION_BUFFER) {
            return Self::Reflection;
        }
        if name.eq_ignore_ascii_case(COOKIE_BUFFER) {
            return Self::LightCookie;
        }
        if name.eq_ignore_ascii_case(SHADOW_BUFFER) {
            return Self::ShadowAtlas;
        }
        if name.eq_ignore_ascii_case(MIP_FRAME_BUFFER) {
            return Self::SceneHistory;
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
    ScaleFloor(f32),
    Fit(u32),
    Fixed { width: u32, height: u32 },
}

const FBO_FORMAT_NAMES: [(&str, FboFormat); 14] = [
    ("", FboFormat::Rgba8),
    ("rgba8888", FboFormat::Rgba8),
    ("rgb888", FboFormat::Rgba8),
    ("rgba_backbuffer", FboFormat::Rgba8),
    ("rgb_backbuffer", FboFormat::Rgba8),
    ("r8", FboFormat::R8),
    ("rg88", FboFormat::Rg8),
    ("r16f", FboFormat::R16f),
    ("rg1616f", FboFormat::Rg16f),
    ("rgba16f", FboFormat::Rgba16f),
    ("rgba16161616f", FboFormat::Rgba16f),
    ("rgb161616f", FboFormat::Rgba16f),
    ("rgba16161616", FboFormat::Rgba16),
    ("rgb161616", FboFormat::Rgba16),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FboFormat {
    Rgba8,
    R8,
    Rg8,
    R16f,
    Rg16f,
    Rgba16f,
    Rgba16,
}

impl FboFormat {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let name = text.trim().to_ascii_lowercase();
        FBO_FORMAT_NAMES.iter().find(|(known, _)| *known == name).map(|(_, format)| *format)
    }

    #[must_use]
    pub fn bytes_per_pixel(self) -> u64 {
        (match self {
            Self::R8 => size_of::<u8>(),
            Self::Rg8 | Self::R16f => size_of::<[u8; 2]>(),
            Self::Rgba8 | Self::Rg16f => size_of::<[u8; 4]>(),
            Self::Rgba16f | Self::Rgba16 => size_of::<[u16; 4]>(),
        }) as u64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fbo {
    pub name: String,
    pub size: FboSize,
    pub format: FboFormat,
    pub repeat: Option<bool>,
    pub clear: [f32; 4],
    pub clear_on_setup: bool,
    pub unique: bool,
}

pub const FBO_MAX_EXTENT: u32 = 8192;
const FBO_MIN_SCALE: f32 = 1e-3;

impl Fbo {
    #[must_use]
    pub fn extent(&self, base: (u32, u32)) -> (u32, u32) {
        let (w, h) = (base.0.max(1) as f32, base.1.max(1) as f32);
        let (out_w, out_h) = match self.size {
            FboSize::Scale(scale) => (w / scale.max(FBO_MIN_SCALE), h / scale.max(FBO_MIN_SCALE)),
            FboSize::ScaleFloor(scale) => {
                ((w / scale.max(FBO_MIN_SCALE)).floor(), (h / scale.max(FBO_MIN_SCALE)).floor())
            }
            FboSize::Fit(edge) => {
                let edge = (edge.max(1) as f32).min(w.max(h));
                let maximum = w.max(h);
                return (
                    ((w / maximum * edge) as u32).clamp(1, FBO_MAX_EXTENT),
                    ((h / maximum * edge) as u32).clamp(1, FBO_MAX_EXTENT),
                );
            }
            FboSize::Fixed { width, height } => (width as f32, height as f32),
        };
        (
            (out_w.round() as u32).clamp(1, FBO_MAX_EXTENT),
            (out_h.round() as u32).clamp(1, FBO_MAX_EXTENT),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Swap {
    pub after_pass: Option<usize>,
    pub source: String,
    pub target: String,
}

#[derive(Clone, Copy)]
pub struct EffectState {
    pub source: Option<usize>,
    pub visible: bool,
    pub available: bool,
}

impl Default for EffectState {
    fn default() -> Self {
        Self { source: None, visible: true, available: true }
    }
}

impl EffectState {
    pub fn apply_object(&mut self, object: &Value, properties: &BTreeMap<String, Vec<f32>>) {
        if let Some(index) = self.source
            && let Some(effect) = object.get(EFFECTS_FIELD).and_then(|effects| effects.get(index))
        {
            self.visible = is_visible(effect.get(VISIBLE_FIELD), properties);
        }
    }
}

pub struct Effect {
    pub state: EffectState,
    pub name: String,
    pub passes: Vec<EffectPass>,
    pub fbos: Vec<Fbo>,
    pub swaps: Vec<Swap>,
    pub clear_functions: BTreeMap<String, Vec<String>>,
}

impl Effect {
    #[must_use]
    pub fn scene_capture(&self) -> bool {
        let [pass] = self.passes.as_slice() else {
            return false;
        };
        self.fbos.is_empty()
            && self.swaps.is_empty()
            && pass.target.is_none()
            && matches!(pass.binds.as_slice(), [(0, EffectBind::SceneSoFar)])
    }
}

pub(crate) fn clear_functions(definition: &Value) -> BTreeMap<String, Vec<String>> {
    let Some(functions) = definition.get(FUNCTIONS_FIELD).and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    if functions.len() > MAX_EFFECT_FUNCTIONS {
        return BTreeMap::new();
    }
    let available: BTreeSet<_> = definition
        .get(FUNCTION_TARGETS_FIELD)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|fbo| fbo.get(FBO_NAME_FIELD).and_then(Value::as_str))
        .collect();
    functions
        .iter()
        .filter_map(|(name, function)| {
            if name.is_empty()
                || name.len() > MAX_EFFECT_FUNCTION_NAME_BYTES
                || function.get(FUNCTION_ACTION_FIELD).and_then(Value::as_str)
                    != Some(FUNCTION_CLEAR_ACTION)
            {
                return None;
            }
            let targets = function.get(FUNCTION_TARGETS_FIELD)?.as_array()?;
            if targets.is_empty() || targets.len() > MAX_FUNCTION_TARGETS {
                return None;
            }
            let names: Vec<_> = targets.iter().map(Value::as_str).collect::<Option<Vec<_>>>()?;
            if names.iter().any(|name| !available.contains(*name)) {
                return None;
            }
            let names: Vec<_> = names.into_iter().map(str::to_owned).collect();
            let unique: BTreeSet<_> = names.iter().collect();
            (unique.len() == names.len()).then_some((name.clone(), names))
        })
        .collect()
}

pub(crate) fn conditions_hold(value: Option<&Value>, combos: &BTreeMap<String, i64>) -> bool {
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
        _ => FboSize::ScaleFloor(
            entry.get("scale").and_then(Value::as_f64).map_or(1.0, |v| v as f32).max(FBO_MIN_SCALE),
        ),
    };
    let format_text = entry.get("format").and_then(Value::as_str).unwrap_or("");
    let format = FboFormat::parse(format_text).unwrap_or_else(|| {
        skipped.push(format!("{effect}: fbo {name} format {format_text} unknown, using rgba8"));
        FboFormat::Rgba8
    });
    let repeat =
        entry.get("uvs").and_then(Value::as_str).map(|uvs| uvs.eq_ignore_ascii_case("repeat"));
    let clear = entry.get("clear").and_then(json_numbers);
    let clear_on_setup = clear.is_some();
    let clear = clear.map_or([0.0; 4], |parts| {
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
    Some(Fbo { name, size, format, repeat, clear, clear_on_setup, unique })
}

pub(crate) fn instance_combos(
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

pub fn json_numbers(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Number(num) => Some(vec![num.as_f64()? as f32]),
        Value::Bool(flag) => Some(vec![f32::from(u8::from(*flag))]),
        Value::String(text) => {
            let parts: Vec<f32> =
                text.split_whitespace().filter_map(|part| part.parse().ok()).collect();
            (!parts.is_empty()).then_some(parts)
        }
        Value::Array(values) => {
            values.iter().map(|value| value.as_f64().map(|number| number as f32)).collect()
        }
        Value::Object(obj) => obj.get("value").and_then(json_numbers),
        Value::Null => None,
    }
}

pub(crate) fn combos_of(value: Option<&Value>) -> BTreeMap<String, i64> {
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
    obj.get(CONSTANT_SHADER_VALUES).or_else(|| obj.get("constants"))
}

pub(crate) fn material_values<'a>(
    pass: &Value,
    overrides: Option<&Value>,
    assets: &Assets,
    uniforms: &(impl Iterator<Item = (&'a str, Option<&'a str>)> + Clone),
) -> BTreeMap<String, Vec<f32>> {
    let mut constants = constants_of(shader_values(Some(pass)), &assets.properties);
    constants.extend(constants_of(shader_values(overrides), &assets.properties));
    let mut values = resolve_material_names(&constants, uniforms.clone());
    values.extend(user_values(pass, overrides, &assets.properties, uniforms));
    values
}

fn user_values<'a>(
    pass: &Value,
    overrides: Option<&Value>,
    properties: &BTreeMap<String, Vec<f32>>,
    uniforms: &(impl Iterator<Item = (&'a str, Option<&'a str>)> + Clone),
) -> BTreeMap<String, Vec<f32>> {
    let mut values = BTreeMap::new();
    let bindings: BTreeMap<_, _> = std::iter::once(pass)
        .chain(overrides)
        .filter_map(|pass| pass.get(USER_SHADER_VALUES).and_then(Value::as_object))
        .flat_map(|bindings| bindings.iter())
        .collect();
    for (property, uniform) in bindings {
        if let Some(uniform) = uniform.as_str()
            && let Some(value) = properties.get(&property.to_ascii_lowercase())
            && let Some((name, _)) = uniforms.clone().find(|(_, alias)| *alias == Some(uniform))
        {
            values.insert(name.to_owned(), value.clone());
        }
    }
    values
}

fn resolve_material_names<'a>(
    values: &BTreeMap<String, Vec<f32>>,
    uniforms: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> BTreeMap<String, Vec<f32>> {
    let mut out = BTreeMap::new();
    for (name, material) in uniforms {
        if let Some(found) = material.and_then(|material| values.get(material)) {
            out.insert(name.to_owned(), found.clone());
        }
    }
    out
}

pub const COLOR_BLEND_EFFECT: &str = "colorblend";
pub const COLOR_BLEND_SHADER: &str = "skwd/colorblend";
pub const COLOR_BLEND_MESH_EFFECT: &str = "colorblend-mesh";
pub const COLOR_BLEND_MESH_SHADER: &str = "skwd/colorblend-mesh";
pub const COLOR_BLEND_BACKDROP_SLOT: usize = 4;

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

const COLOR_BLEND_MESH_VERT: &str = "uniform mat4 g_ModelViewProjectionMatrix;
attribute vec2 a_Position;
attribute vec2 a_TexCoord;
attribute float a_VertexAlpha;
varying vec2 v_TexCoord;
varying float v_VertexAlpha;
void main() {
	gl_Position = mul(vec4(a_Position, 0.0, 1.0), g_ModelViewProjectionMatrix);
	v_TexCoord = a_TexCoord;
	v_VertexAlpha = a_VertexAlpha;
}
";

const COLOR_BLEND_MESH_FRAG: &str = "#include \"common_blending.h\"
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture4;
uniform vec4 g_Texture4Resolution;
uniform vec4 g_Color4;
varying vec2 v_TexCoord;
varying float v_VertexAlpha;
void main() {
	vec4 layer = texSample2D(g_Texture0, v_TexCoord) * g_Color4;
	layer.a *= v_VertexAlpha;
	if (layer.a <= 0.0) {
		discard;
	}
	vec4 screen = texSample2D(g_Texture4, gl_FragCoord.xy / g_Texture4Resolution.xy);
	gl_FragColor = vec4(ApplyBlending(BLENDMODE, screen.rgb, layer.rgb, layer.a), screen.a);
}
";

pub const ADDITIVE_COLOR_BLEND: u32 = 31;
const PASSTHROUGH_TRANSFORM: &str = "TRANSFORM";
const WHITE_SOURCE_COLOR: [f32; 4] = [1.0; 4];

#[must_use]
pub fn shader_blend(mode: u32) -> bool {
    !matches!(mode, 0 | ADDITIVE_COLOR_BLEND)
}

pub fn color_blend_effect(
    pkg: &Package,
    assets: &Assets,
    mode: u32,
    own_id: Option<&str>,
    mesh: bool,
) -> Option<Effect> {
    let (name, shader) = if mesh {
        (COLOR_BLEND_MESH_EFFECT, COLOR_BLEND_MESH_SHADER)
    } else {
        (COLOR_BLEND_EFFECT, COLOR_BLEND_SHADER)
    };
    let material = serde_json::json!({
        "passes": [{"shader": shader, "combos": {"BLENDMODE": mode}}]
    });
    let mut passes = build_material(pkg, assets, &material, None, own_id, None, None)?;
    let pass = passes.last_mut()?;
    pass.binds.retain(|(slot, _)| *slot != COLOR_BLEND_BACKDROP_SLOT);
    if mesh {
        pass.hlsl = None;
    } else {
        pass.binds.push((COLOR_BLEND_BACKDROP_SLOT, EffectBind::SceneUnderLayer));
    }
    pass.values.insert(SOURCE_COLOR_UNIFORM.into(), WHITE_SOURCE_COLOR.to_vec());
    Some(Effect {
        state: EffectState::default(),
        name: name.into(),
        passes,
        fbos: Vec::new(),
        swaps: Vec::new(),
        clear_functions: BTreeMap::new(),
    })
}

pub fn passthrough_effect(pkg: &Package, assets: &Assets) -> Option<Effect> {
    let passes = passthrough_passes(pkg, assets, None, true)?;
    Some(Effect {
        state: EffectState::default(),
        name: material::PASSTHROUGH.into(),
        passes,
        fbos: Vec::new(),
        swaps: Vec::new(),
        clear_functions: BTreeMap::new(),
    })
}

pub fn particle_pass(
    pkg: &Package,
    assets: &Assets,
    material: &Value,
    combos: &BTreeMap<String, i64>,
) -> Option<EffectPass> {
    let overrides = Value::Array(vec![serde_json::json!({ "combos": combos })]);
    let mut passes = build_material(pkg, assets, material, Some(&overrides), None, None, None)?;
    let mut pass = passes.pop()?;
    pass.filter_binds();
    Some(pass)
}

const VERTEX_SHADER_EXT: &str = "vert";
const GEOMETRY_SHADER_EXT: &str = "geom";
const FRAGMENT_SHADER_EXT: &str = "frag";

pub(crate) fn load_shader(pkg: &Package, assets: &Assets, name: &str, ext: &str) -> Option<String> {
    const MAX_SHADER_BYTES: usize = 4 * 1024 * 1024;
    if name == COLOR_BLEND_SHADER {
        return match ext {
            VERTEX_SHADER_EXT => Some(COLOR_BLEND_VERT.to_string()),
            FRAGMENT_SHADER_EXT => Some(COLOR_BLEND_FRAG.to_string()),
            _ => None,
        };
    }
    if name == COLOR_BLEND_MESH_SHADER {
        return match ext {
            VERTEX_SHADER_EXT => Some(COLOR_BLEND_MESH_VERT.to_string()),
            FRAGMENT_SHADER_EXT => Some(COLOR_BLEND_MESH_FRAG.to_string()),
            _ => None,
        };
    }
    let rel = format!("shaders/{name}.{ext}");
    pkg.find(&rel)
        .filter(|bytes| bytes.len() <= MAX_SHADER_BYTES)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .or_else(|| {
            (ext != GEOMETRY_SHADER_EXT
                || ![VERTEX_SHADER_EXT, FRAGMENT_SHADER_EXT]
                    .iter()
                    .any(|stage| pkg.find(&format!("shaders/{name}.{stage}")).is_some()))
            .then(|| assets.read(&rel))
            .flatten()
        })
}

fn texture_slot(
    pkg: &Package,
    assets: &Assets,
    raw: Option<&str>,
) -> Option<crate::model::Texture> {
    let name = raw.filter(|text| !text.is_empty())?;
    crate::model::load_texture_or_asset(pkg, assets, name)
        .filter(|texture| texture.stream.is_none())
}

fn texture_shipped(pkg: &Package, assets: &Assets, name: &str) -> bool {
    name.starts_with(RENDER_TARGET_PREFIX)
        || crate::model::tex_candidates(name)
            .iter()
            .any(|path| pkg.find(path).is_some() || assets.asset_path(path).is_some())
}

fn texture_components(pkg: &Package, assets: &Assets, name: &str) -> Option<u8> {
    let candidates = crate::model::tex_candidates(name);
    for path in &candidates {
        if let Some(bytes) = pkg.find(path) {
            return crate::tex::parse_meta(bytes).ok().map(|meta| meta.component_mask());
        }
    }
    let bytes = candidates.iter().find_map(|path| assets.read_bytes(path))?;
    crate::tex::parse_meta(&bytes).ok().map(|meta| meta.component_mask())
}

impl Assets {
    pub fn read_bytes(&self, relative: &str) -> Option<Vec<u8>> {
        self.read_scoped_bytes(relative, crate::pkg::MAX_PACKAGE_ENTRY_BYTES)
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
    property_source: (usize, usize),
) -> Option<Vec<EffectPass>> {
    let material = pkg.find_json(material_path).ok().flatten().or_else(|| {
        assets.read(material_path).and_then(|text| crate::json::parse(text.as_bytes()).ok())
    })?;
    build_material(pkg, assets, &material, overrides, own_id, Some(property_source), None)
}

fn passthrough_passes(
    pkg: &Package,
    assets: &Assets,
    own_id: Option<&str>,
    transform: bool,
) -> Option<Vec<EffectPass>> {
    let mut pass = serde_json::json!({(material::SHADER): material::PASSTHROUGH});
    if transform {
        pass[COMBOS_FIELD] = serde_json::json!({(PASSTHROUGH_TRANSFORM): 1});
    }
    let material = serde_json::json!({(material::PASSES): [pass]});
    build_material(pkg, assets, &material, None, own_id, None, None)
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
    let mut built = passthrough_passes(pkg, assets, own_id, false)?;
    let pass = built.last_mut()?;
    pass.name = "copy".to_string();
    pass.target = Some(target.to_string());
    pass.binds.push((0, EffectBind::parse(source, own_id)));
    Some(built)
}

const SCENE_ORTHO: &str = "SCENE_ORTHO";

fn dialect(combos: &BTreeMap<String, i64>, name: &str) -> BTreeMap<String, i64> {
    let mut out = combos.clone();
    out.insert(name.to_string(), 1);
    if name == "HLSL" {
        out.insert("HLSL_SM40".to_string(), 1);
    }
    out
}

#[derive(Clone, Copy)]
struct MaterialInput {
    bones: Option<usize>,
    component_mask: u8,
}

fn build_material(
    pkg: &Package,
    assets: &Assets,
    material: &Value,
    overrides: Option<&Value>,
    own_id: Option<&str>,
    property_source: Option<(usize, usize)>,
    input: Option<MaterialInput>,
) -> Option<Vec<EffectPass>> {
    let passes = material.get("passes")?.as_array()?;
    let mut out = Vec::new();
    for (index, pass) in passes.iter().enumerate() {
        let shader_name = customshader::resolve(pass, assets)?;
        let mut combos = combos_of(pass.get("combos"));
        if let Some(over) =
            overrides.and_then(|value| value.as_array()).and_then(|arr| arr.get(index))
        {
            for (key, value) in combos_of(over.get("combos")) {
                combos.insert(key, value);
            }
        }
        if pass.get(BLENDING_FIELD).and_then(Value::as_str) == Some(BLEND_COVERAGE) {
            combos.insert(ALPHA_COVERAGE_COMBO.into(), 1);
        }
        let vert_src = load_shader(pkg, assets, shader_name, VERTEX_SHADER_EXT)?;
        let geom_src = load_shader(pkg, assets, shader_name, GEOMETRY_SHADER_EXT);
        if geom_src.is_some() {
            combos.insert(GEOMETRY_COMBO.into(), 1);
        }
        combos.insert(SHADER_VERSION_COMBO.into(), SHADER_VERSION);
        combos.insert(crate::model::lighting::COMBO.into(), assets.context.lighting.count as i64);
        combos.insert(
            crate::model::lighting::COOKIE_COMBO.into(),
            i64::from(assets.context.lighting.cookies),
        );
        combos.insert(
            crate::model::lighting::SHADOW_COMBO.into(),
            i64::from(assets.context.lighting.shadows),
        );
        combos.insert(
            crate::model::lighting::spot::COMBO.into(),
            assets.context.lighting.spots as i64,
        );
        combos.insert(
            crate::model::lighting::directional::COMBO.into(),
            assets.context.lighting.directionals as i64,
        );
        let frag_src = load_shader(pkg, assets, shader_name, FRAGMENT_SHADER_EXT)?;
        let loader = |name: &str| -> Option<String> {
            pkg.find(&format!("shaders/{name}"))
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .or_else(|| assets.read(&format!("shaders/{name}")))
        };
        let vert = shader::relax_portability(
            &assets.context.lighting.source(&shader::resolve_includes(&vert_src, &loader, 0).ok()?),
        );
        let frag = shader::relax_portability(
            &assets.context.lighting.source(&shader::resolve_includes(&frag_src, &loader, 0).ok()?),
        );
        let geom = if let Some(source) = geom_src {
            Some(shader::relax_portability(
                &assets
                    .context
                    .lighting
                    .source(&shader::resolve_includes(&source, &loader, 0).ok()?),
            ))
        } else {
            None
        };
        for (name, value) in shader::combo_defaults(&vert)
            .into_iter()
            .chain(geom.iter().flat_map(|source| shader::combo_defaults(source)))
            .chain(shader::combo_defaults(&frag))
        {
            combos.entry(name).or_insert(value);
        }
        assets.context.fog.combos(&mut combos);
        let base = pass.get(TEXTURES_FIELD).and_then(Value::as_array);
        let updates = overrides
            .and_then(Value::as_array)
            .and_then(|arr| arr.get(index))
            .and_then(|value| value.get(TEXTURES_FIELD))
            .and_then(Value::as_array);
        let texture_slots = updates.map_or_else(
            || base.cloned().unwrap_or_default(),
            |updates| merge_texture_slots(base.map(Vec::as_slice), updates),
        );
        let mut names: Vec<Option<String>> =
            texture_slots.iter().map(|entry| texture_name(entry).map(str::to_owned)).collect();
        let mut probe = shader::translate(&frag, Stage::Fragment, &combos);
        probe.samplers.extend(shader::translate(&vert, Stage::Vertex, &combos).samplers);
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
        if assets.context.lighting.shadows
            && probe.uniforms.iter().any(|uniform| {
                uniform.name.starts_with(crate::model::lighting::SHADOW_PREFIX)
                    || uniform.name.starts_with(crate::model::lighting::SHADOW_FEATURE_PREFIX)
            })
            && probe
                .samplers
                .iter()
                .any(|sampler| sampler.index as usize == crate::model::lighting::SHADOW_SLOT)
        {
            self_binds.push((crate::model::lighting::SHADOW_SLOT, EffectBind::ShadowAtlas));
        }
        if assets.context.lighting.cookies
            && probe
                .uniforms
                .iter()
                .any(|uniform| uniform.name.starts_with(crate::model::lighting::COOKIE_PREFIX))
            && probe
                .samplers
                .iter()
                .any(|sampler| sampler.index as usize == crate::model::lighting::COOKIE_SLOT)
        {
            self_binds.push((crate::model::lighting::COOKIE_SLOT, EffectBind::LightCookie));
        }
        for sampler in &probe.samplers {
            let slot = sampler.index as usize;
            if slot == 0
                || self_binds.iter().any(|(bound, _)| *bound == slot)
                || (sampler.combo.is_some()
                    && !shader::reads_sampler(&frag, &combos, &sampler.name)
                    && !shader::reads_sampler(&vert, &combos, &sampler.name))
            {
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
            if default.eq_ignore_ascii_case(SHADOW_BUFFER) && !assets.context.lighting.shadows {
                continue;
            }
            if default.starts_with(RENDER_TARGET_PREFIX) {
                self_binds.push((slot, EffectBind::parse(default, own_id)));
            } else {
                names[slot] = Some(default.to_string());
            }
        }
        let usertextures = overrides
            .and_then(Value::as_array)
            .and_then(|array| array.get(index))
            .and_then(|value| value.get(USER_TEXTURES_FIELD))
            .and_then(Value::as_array)
            .map(|updates| {
                merge_texture_slots(
                    pass.get(USER_TEXTURES_FIELD).and_then(Value::as_array).map(Vec::as_slice),
                    updates,
                )
            })
            .or_else(|| pass.get(USER_TEXTURES_FIELD).and_then(Value::as_array).cloned());
        names.resize(names.len().max(usertextures.as_ref().map_or(0, Vec::len)), None);
        let textures: Vec<Option<crate::model::Texture>> = names
            .iter()
            .enumerate()
            .map(|(slot, name)| {
                if slot == 0 {
                    return None;
                }
                usertextures
                    .as_ref()
                    .and_then(|slots| slots.get(slot))
                    .and_then(crate::model::MediaTexture::of)
                    .map(crate::model::MediaTexture::texture)
                    .or_else(|| texture_slot(pkg, assets, name.as_deref()))
            })
            .collect();
        for (slot, texture) in textures.iter().enumerate() {
            let Some(texture) = texture else {
                continue;
            };
            combos.entry(format!("TEX{slot}FORMAT")).or_insert(texture.we_format());
        }
        for sampler in &probe.samplers {
            if sampler.components.iter().all(Option::is_none) {
                continue;
            }
            let slot = sampler.index as usize;
            let mask = if slot == 0 {
                input.map(|input| input.component_mask).or_else(|| {
                    names
                        .first()
                        .and_then(Option::as_deref)
                        .and_then(|name| texture_components(pkg, assets, name))
                })
            } else {
                textures.get(slot).and_then(Option::as_ref).map(|texture| texture.component_mask)
            }
            .unwrap_or(0);
            for (index, combo) in sampler.components.iter().enumerate() {
                if let Some(combo) = combo
                    && mask & (1 << index) != 0
                {
                    combos.insert(combo.clone(), 1);
                }
            }
        }
        if let Some(bones) = input.and_then(|input| input.bones) {
            let morph = combos.get(crate::puppet::morph::COMBO) == Some(&1);
            material::puppet::combos(&mut combos, bones, morph).ok()?;
        }
        combos.insert(
            SCENE_ORTHO.into(),
            i64::from(matches!(
                assets.context.projection,
                Some(
                    crate::model::ProjectionMode::Orthographic
                        | crate::model::ProjectionMode::Automatic
                )
            )),
        );
        let hlsl_symbols = dialect(&combos, "HLSL");
        let glsl_symbols = dialect(&combos, "GLSL");
        let vert_hlsl = shader::preprocess(&vert, &hlsl_symbols);
        let frag_hlsl = shader::preprocess(&frag, &hlsl_symbols);
        let vert = shader::preprocess(&vert, &glsl_symbols);
        let geom = geom.map(|source| shader::preprocess(&source, &glsl_symbols));
        let frag = shader::preprocess(&frag, &glsl_symbols);
        let (inputs, outputs) = geom
            .as_deref()
            .map_or_else(|| (String::new(), String::new()), shader::geometry_interfaces);
        let vertex_varyings =
            shader::varying_map(&vert, if geom.is_some() { &inputs } else { &frag }, &combos);
        let fragment_varyings =
            shader::varying_map(if geom.is_some() { &outputs } else { &vert }, &frag, &combos);
        let mut vertex =
            shader::translate_with(&vert, Stage::Vertex, &combos, Some(&vertex_varyings));
        let mut geometry = geom
            .as_deref()
            .map(|source| {
                shader::translate_geometry(source, &combos, &vertex_varyings, &fragment_varyings)
            })
            .transpose()
            .ok()?;
        let mut fragment =
            shader::translate_with(&frag, Stage::Fragment, &combos, Some(&fragment_varyings));
        let hlsl_available = geometry.is_none()
            && !crate::hlsl::fragment_indexes_varying_arrays_dynamically(&frag_hlsl, &combos)
            && crate::hlsl::varying_slots(&vert_hlsl, &frag_hlsl, &combos)
                <= crate::hlsl::MAX_VARYING_SLOTS;
        let present = textures.iter().map(Option::is_some).collect::<Vec<_>>();
        shader::omit_unused_samplers(
            &mut vertex,
            &combos,
            &present,
            hlsl_available.then_some(vert_hlsl.as_str()),
        );
        shader::omit_unused_samplers(
            &mut fragment,
            &combos,
            &present,
            hlsl_available.then_some(frag_hlsl.as_str()),
        );
        if let Some(stage) = geometry.as_mut() {
            shader::omit_unused_samplers(stage, &combos, &present, None);
            shader::unify_uniform_stages(&mut [&mut vertex, stage, &mut fragment]);
        } else {
            shader::unify_uniforms(&mut vertex, &mut fragment);
        }
        let mut sampler_union = vertex.samplers.clone();
        for sampler in geometry.iter().flat_map(|stage| &stage.samplers).chain(&fragment.samplers) {
            if !sampler_union.iter().any(|known| known.name == sampler.name) {
                sampler_union.push(sampler.clone());
            }
        }
        let hlsl = hlsl_available.then(|| {
            crate::hlsl::rewrite(&vert_hlsl, &frag_hlsl, &combos, &vertex.uniforms, &sampler_union)
        });
        let uniforms = fragment
            .uniforms
            .iter()
            .map(|uniform| (uniform.name.as_str(), uniform.material.as_deref()));
        let overrides = overrides.and_then(Value::as_array).and_then(|passes| passes.get(index));
        let mut values = material_values(pass, overrides, assets, &uniforms);
        let user_uniforms =
            user_values(pass, overrides, &assets.properties, &uniforms).into_keys().collect();
        values.extend(assets.context.lighting.values.clone());
        for uniform in &fragment.uniforms {
            if let Some(value) = assets.context.fog.uniform(&uniform.name) {
                values.insert(uniform.name.clone(), value);
            }
        }
        for (slot, name) in names.iter().enumerate() {
            let Some(name) = name else {
                continue;
            };
            if !name.starts_with(RENDER_TARGET_PREFIX) {
                continue;
            }
            let binding = EffectBind::parse(name, own_id);
            if slot != 0 || matches!(binding, EffectBind::SceneHistory | EffectBind::Reflection) {
                self_binds.push((slot, binding));
            }
        }
        out.push(EffectPass {
            blend: PassBlend::parse(pass.get(BLENDING_FIELD).and_then(Value::as_str))?,
            property_source: property_source.filter(|_| index == 0),
            user_uniforms,
            name: shader_name.to_string(),
            vertex,
            geometry,
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
    paper_runtime::clock::local_now()
        .map_or(0.0, |now| daytime(now.hour, now.minute, now.second, now.millisecond))
}

#[must_use]
pub fn daytime(hour: i32, minute: i32, second: i32, millisecond: i32) -> f32 {
    (f64::from(hour) / 24.0
        + f64::from(minute) / 1440.0
        + f64::from(second) / 86_400.0
        + f64::from(millisecond) / 86_400_000.0) as f32
}

const REDUCTION_SCALE_UNIFORM: &str = "g_TextureReductionScale";
const SCREEN_UNIFORM: &str = "g_Screen";
const ALPHA_UNIFORM: &str = "g_Alpha";
const USER_ALPHA_UNIFORM: &str = "g_UserAlpha";
const BRIGHTNESS_UNIFORM: &str = "g_Brightness";
const COLOR_UNIFORM: &str = "g_Color";
const TEXEL_UNIFORM: &str = "g_TexelSize";
const TEXEL_HALF_UNIFORM: &str = "g_TexelSizeHalf";
const EFFECT_MVP_UNIFORM: &str = "g_EffectModelViewProjectionMatrix";
const EFFECT_MVP_INVERSE_UNIFORM: &str = "g_EffectModelViewProjectionMatrixInverse";
const MVP_UNIFORM: &str = "g_ModelViewProjectionMatrix";
const MVP_INVERSE_UNIFORM: &str = "g_ModelViewProjectionMatrixInverse";
const VIEW_PROJECTION_UNIFORM: &str = "g_ViewProjectionMatrix";
const RESOLUTION_UNIFORM: &str = "g_Resolution";
const MAT4_LANES: usize = 16;
const CENTER: [f32; 2] = [0.5; 2];
const RELEASED: [f32; 4] = [0.0; 4];
const UNIT: [f32; 1] = [1.0];
const WHITE: [f32; 3] = [1.0; 3];
const OPAQUE: [f32; 4] = [1.0; 4];
const ALIASED_ARRAYS: [&str; 7] = [
    AUDIO_PREFIX,
    crate::model::lighting::UNIFORM_PREFIX,
    crate::model::lighting::SHADOW_PREFIX,
    crate::model::lighting::SHADOW_FEATURE_PREFIX,
    crate::model::lighting::spot::PREFIX,
    crate::model::lighting::directional::PREFIX,
    crate::model::lighting::LEGACY_UNIFORM_PREFIX,
];

struct FrameShape {
    clock: FrameClock,
    quad: Option<(u32, u32)>,
    width: u32,
    height: u32,
    screen: (u32, u32),
    d3d_clip: bool,
}

struct FrameUniforms<'a> {
    meta: &'a PassMeta,
    clock: [[f32; 1]; 3],
    screen: [f32; 3],
    texel: [f32; 2],
    texel_half: [f32; 2],
    projection: Option<[[f32; MAT4_LANES]; 2]>,
    view_projection: [f32; MAT4_LANES],
    resolution: [f32; 4],
    slots: [[f32; 4]; MAX_TEXTURE_SLOTS as usize],
}

impl<'a> FrameUniforms<'a> {
    fn new(meta: &'a PassMeta, shape: &FrameShape, slot_sizes: &[Option<[f32; 4]>]) -> Self {
        let (sw, sh) = (shape.screen.0.max(1) as f32, shape.screen.1.max(1) as f32);
        let (w, h) = (shape.width as f32, shape.height as f32);
        let ys = if shape.d3d_clip { 1.0 } else { -1.0 };
        let projection = shape.quad.map(|(quad_w, quad_h)| {
            let (qw, qh) = (quad_w.max(1) as f32, quad_h.max(1) as f32);
            [
                [
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
                [
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
            ]
        });
        Self {
            meta,
            clock: [[shape.clock.time], [shape.clock.dt], [shape.clock.daytime]],
            screen: [sw, sh, sw / sh],
            texel: [1.0 / sw, 1.0 / sh],
            texel_half: [0.5 / sw, 0.5 / sh],
            projection,
            view_projection: [
                2.0 / sw,
                0.0,
                0.0,
                0.0,
                0.0,
                ys * 2.0 / sh,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                -1.0,
                -ys,
                0.5,
                1.0,
            ],
            resolution: [w, h, 1.0 / w.max(1.0), 1.0 / h.max(1.0)],
            slots: std::array::from_fn(|slot| {
                slot_sizes
                    .get(slot)
                    .copied()
                    .flatten()
                    .or_else(|| meta.resolutions.get(slot).copied().flatten())
                    .unwrap_or([w, h, w, h])
            }),
        }
    }

    fn value<'b>(
        &'b self,
        key: &str,
        overrides: &'b BTreeMap<String, Vec<f32>>,
    ) -> Option<&'b [f32]> {
        if let Some(value) = overrides.get(key) {
            return Some(value);
        }
        match texture_uniform(key) {
            Some((slot, TEXTURE_RESOLUTION)) => Some(&self.slots[slot]),
            Some((_, TEXTURE_ROTATION)) => Some(self.derived(key).unwrap_or(&IDENTITY_ROTATION)),
            Some((_, TEXTURE_TRANSLATION)) => Some(self.derived(key).unwrap_or(&ZERO_TRANSLATION)),
            _ => self.derived(key),
        }
    }

    fn derived(&self, key: &str) -> Option<&[f32]> {
        match key {
            RESOLUTION_UNIFORM => Some(&self.resolution),
            VIEW_PROJECTION_UNIFORM => Some(self.projected(key).unwrap_or(&self.view_projection)),
            _ => self.projected(key),
        }
    }

    fn projected(&self, key: &str) -> Option<&[f32]> {
        match (key, &self.projection) {
            (EFFECT_MVP_UNIFORM | MVP_UNIFORM, Some([mvp, _])) => Some(mvp),
            (EFFECT_MVP_INVERSE_UNIFORM | MVP_INVERSE_UNIFORM, Some([_, inverse])) => Some(inverse),
            (TEXEL_UNIFORM, _) => Some(&self.texel),
            (TEXEL_HALF_UNIFORM, _) => Some(&self.texel_half),
            _ => self.clocked(key),
        }
    }

    fn clocked(&self, key: &str) -> Option<&[f32]> {
        let [time, dt, daytime] = &self.clock;
        match key {
            TIME_UNIFORM => Some(time),
            FRAME_TIME_UNIFORM => Some(dt),
            DAY_TIME_UNIFORM => Some(daytime),
            PARALLAX_UNIFORM | POINTER_UNIFORM | LAST_POINTER_UNIFORM => Some(&CENTER),
            POINTER_STATE_UNIFORM => Some(&RELEASED),
            _ => self.meta.constants.get(key).map(Vec::as_slice).or_else(|| self.fallback(key)),
        }
    }

    fn fallback(&self, key: &str) -> Option<&[f32]> {
        match key {
            REDUCTION_SCALE_UNIFORM | ALPHA_UNIFORM | USER_ALPHA_UNIFORM | BRIGHTNESS_UNIFORM => {
                Some(&UNIT)
            }
            SCREEN_UNIFORM => Some(&self.screen),
            COLOR_UNIFORM => Some(&WHITE),
            SOURCE_COLOR_UNIFORM => Some(&OPAQUE),
            _ => None,
        }
    }
}

pub struct PassMeta {
    property_source: Option<(usize, usize)>,
    user_uniforms: BTreeSet<String>,
    pub name: String,
    uniforms: Vec<crate::shader::Uniform>,
    constants: BTreeMap<String, Vec<f32>>,
    resolutions: Vec<Option<[f32; 4]>>,
    ndc: bool,
}

impl PassMeta {
    pub fn source(&self) -> Option<(usize, usize)> {
        self.property_source
    }

    pub fn apply_object_values(&mut self, object: &Value, props: &BTreeMap<String, Vec<f32>>) {
        if let Some((effect, pass)) = self.property_source
            && let Some(values) = object
                .get("effects")
                .and_then(|effects| effects.get(effect))
                .and_then(|effect| effect.get("passes"))
                .and_then(|passes| passes.get(pass))
                .and_then(|pass| pass.get(CONSTANT_SHADER_VALUES))
        {
            let values = constants_of(Some(values), props);
            self.constants.extend(
                resolve_material_names(
                    &values,
                    self.uniforms
                        .iter()
                        .map(|uniform| (uniform.name.as_str(), uniform.material.as_deref())),
                )
                .into_iter()
                .filter(|(name, _)| !self.user_uniforms.contains(name)),
            );
        }
    }

    pub fn apply_script_values(
        &mut self,
        values: &Value,
        props: &BTreeMap<String, Vec<f32>>,
    ) -> BTreeMap<String, Vec<f32>> {
        let values = constants_of(Some(values), props);
        let mut resolved = values.clone();
        resolved.extend(resolve_material_names(
            &values,
            self.uniforms
                .iter()
                .map(|uniform| (uniform.name.as_str(), uniform.material.as_deref())),
        ));
        self.constants.extend(resolved.clone());
        resolved
    }

    pub fn apply_resolved_values(&mut self, values: &BTreeMap<String, Vec<f32>>) {
        self.constants.extend(values.clone());
    }

    pub fn set_source_tint(&mut self, color: [f32; 4]) {
        self.constants.insert(SOURCE_COLOR_UNIFORM.into(), color.to_vec());
    }

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
            property_source: pass.property_source,
            user_uniforms: pass.user_uniforms.clone(),
            name: pass.name.clone(),
            uniforms: pass.fragment.uniforms.clone(),
            constants: pass.values.clone(),
            resolutions,
            ndc: !pass.vertex.uniforms.iter().any(|uniform| uniform.name == MVP_UNIFORM),
        }
    }

    pub fn matrix_offset(&self, name: &str) -> Option<usize> {
        self.uniforms
            .iter()
            .find(|uniform| {
                uniform.name == name
                    && uniform.kind == crate::shader::UniformKind::Mat4
                    && uniform.count == 1
            })
            .map(|uniform| uniform.offset)
    }

    #[must_use]
    pub fn ndc(&self) -> bool {
        self.ndc
    }

    #[must_use]
    pub fn time_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| frame_input(&uniform.name) == Some(FrameInput::Time))
    }

    #[must_use]
    pub fn pointer_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| frame_input(&uniform.name) == Some(FrameInput::Pointer))
    }

    pub fn audio_dependent(&self) -> bool {
        self.uniforms.iter().any(|uniform| frame_input(&uniform.name) == Some(FrameInput::Audio))
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
        let shape = FrameShape { clock, quad, width, height, screen, d3d_clip };
        let frame = FrameUniforms::new(self, &shape, slot_sizes);
        crate::shader::pack_with(&self.uniforms, |name| {
            name.split_once('[')
                .map(|(base, _)| base)
                .filter(|base| ALIASED_ARRAYS.iter().any(|prefix| base.starts_with(prefix)))
                .and_then(|base| frame.value(base, overrides))
                .or_else(|| frame.value(name, overrides))
        })
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

pub(crate) fn definition(
    pkg: &Package,
    assets: &Assets,
    file: &str,
    entry: &Value,
) -> Option<Value> {
    pkg.find_json(file).ok().flatten().or_else(|| {
        assets
            .read(file)
            .and_then(|text| crate::json::parse(text.as_bytes()).ok())
            .or_else(|| builtin_effect(file))
            .or_else(|| crate::model::hdr::definition(file, entry))
    })
}

pub fn load_effects(
    pkg: &Package,
    assets: &Assets,
    object: &Value,
    retain_hidden: bool,
) -> (Vec<Effect>, Vec<String>) {
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
    for (effect_index, entry) in entries.iter().enumerate() {
        let Some(file) = entry.get("file").and_then(Value::as_str) else {
            continue;
        };
        let visible = is_visible(entry.get(VISIBLE_FIELD), &assets.properties);
        if !visible && !retain_hidden {
            continue;
        }
        let name = file.rsplit('/').nth(1).unwrap_or(file).to_string();
        let skipped_offset = skipped.len();
        let loaded = 'load_effect: {
            let Some(definition) = definition(pkg, assets, file, entry) else {
                skipped.push(format!("{name}: effect json missing"));
                break 'load_effect None;
            };
            let Some(defs) = definition.get("passes").and_then(Value::as_array) else {
                skipped.push(format!("{name}: no passes"));
                break 'load_effect None;
            };
            let effect_assets = assets.with_effect(file);
            let overrides = entry.get("passes");
            let combos = instance_combos(pkg, &effect_assets, defs, overrides);
            let mut fbos: Vec<Fbo> = definition
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
            crate::model::hdr::configure_targets(file, &mut fbos);
            let mut clear_functions = clear_functions(&definition);
            clear_functions.retain(|_, names| {
                names.iter().all(|name| fbos.iter().any(|fbo| fbo.name.as_str() == name.as_str()))
            });
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
                    match build_copy_pass(pkg, &effect_assets, def, own_id) {
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
                if let Some(mut built) = build_pass(
                    pkg,
                    &effect_assets,
                    material,
                    slot_override.as_ref(),
                    own_id,
                    (effect_index, index),
                ) {
                    if let Some(first) = built.first_mut() {
                        crate::model::hdr::configure_pass(file, entry, index, first);
                    }
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
                                        if crate::effect_lifetime::resolve_fbo_index(
                                            fbos.iter().map(|fbo| fbo.name.as_str()),
                                            name,
                                        )
                                        .is_some()
                                        {
                                            EffectBind::Named(name.to_string())
                                        } else {
                                            EffectBind::Previous
                                        },
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
                break 'load_effect None;
            }
            Some(Effect {
                state: EffectState { source: Some(effect_index), visible, available: true },
                name: name.clone(),
                passes,
                fbos,
                swaps,
                clear_functions,
            })
        };
        if !visible && loaded.is_none() && !assets.has_custom_shaders() {
            let reasons = skipped.split_off(skipped_offset);
            tracing::warn!(effect = effect_index, %name, ?reasons, "hidden effect unavailable");
            out.push(Effect {
                state: EffectState { source: Some(effect_index), visible, available: false },
                name,
                passes: Vec::new(),
                fbos: Vec::new(),
                swaps: Vec::new(),
                clear_functions: BTreeMap::new(),
            });
        } else if let Some(effect) = loaded {
            out.push(effect);
        }
    }
    (out, skipped)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod uniform_tests;

#[cfg(test)]
mod projection_tests;
