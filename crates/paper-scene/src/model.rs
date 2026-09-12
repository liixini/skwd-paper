use crate::pkg::Package;
use crate::tex;
use anyhow::{Result, anyhow};
use serde_json::Value;

pub const MAX_SCENE_TEXTURE_BYTES: usize = 1024 * 1024 * 1024;
pub const MAX_SCENE_OBJECTS: usize = 16_384;

pub struct SceneModel {
    pub canvas: (f32, f32),
    pub mouse: crate::mouse::Parallax,
    pub camera_fov: f32,
    pub clear: [f32; 3],
    pub ambient: [f32; 3],
    pub skylight: [f32; 3],
    pub layers: Vec<Layer>,
    pub particles: Vec<ParticleLayer>,
    pub skipped: Vec<String>,
}

pub struct Layer {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub texture: Texture,
    pub puppet: Option<Puppet>,
    pub center: (f32, f32),
    pub size: (f32, f32),
    pub scale: (f32, f32),
    pub depth: f32,
    pub scene_order: usize,
    pub alpha: f32,
    pub angle: f32,
    pub color: [f32; 3],
    pub color_blend: u32,
    pub passthrough: bool,
    pub solid: bool,
    pub live_text: Option<crate::text::Prepared>,
    pub is_text: bool,
    pub mouse: crate::mouse::LayerMouse,
    pub effects: Vec<crate::effects::Effect>,
}

pub struct ParticleLayer {
    pub system: crate::particles::ParticleSystem,
    pub depth: f32,
    pub scene_order: usize,
}

pub struct Puppet {
    pub mesh: crate::puppet::Mesh,
    pub size: (f32, f32),
    pub layers: Vec<crate::puppet::AnimationLayer>,
}

pub type Properties = std::collections::BTreeMap<String, Vec<f32>>;

#[derive(Clone, Copy, Default)]
struct Transform {
    origin: (f32, f32, f32),
    scale: (f32, f32),
    angle: f32,
}

fn layer_alpha(object: &Value, props: &Properties) -> f32 {
    if crate::dynamic_text::media_scripted(object.get("alpha")) {
        return 0.0;
    }
    number(object.get("alpha"), props, 1.0).clamp(0.0, 1.0)
}

fn object_transform(object: &Value, props: &Properties) -> Transform {
    Transform {
        origin: vec3(object.get("origin"), props).unwrap_or((0.0, 0.0, 0.0)),
        scale: vec3(object.get("scale"), props).map_or((1.0, 1.0), |(x, y, _)| (x, y)),
        angle: vec3(object.get("angles"), props).map_or(0.0, |(_, _, z)| z),
    }
}

fn compose(parent: Transform, child: Transform) -> Transform {
    let (sin, cos) = parent.angle.sin_cos();
    let sx = child.origin.0 * parent.scale.0;
    let sy = child.origin.1 * parent.scale.1;
    Transform {
        origin: (
            parent.origin.0 + sx * cos - sy * sin,
            parent.origin.1 + sx * sin + sy * cos,
            parent.origin.2 + child.origin.2,
        ),
        scale: (parent.scale.0 * child.scale.0, parent.scale.1 * child.scale.1),
        angle: parent.angle + child.angle,
    }
}

struct Parallax {
    amount: f32,
    focus: (f32, f32),
}

impl Parallax {
    fn of(scene: &Value, canvas: (f32, f32), props: &Properties) -> Option<Self> {
        let general = scene.get("general");
        if !truthy(general.and_then(|top| top.get("cameraparallax")), props, false) {
            return None;
        }
        let amount = number(general.and_then(|top| top.get("cameraparallaxamount")), props, 0.5);
        let eye = vec3(scene.get("camera").and_then(|camera| camera.get("eye")), props)
            .unwrap_or((0.0, 0.0, 0.0));
        Some(Self { amount, focus: (canvas.0 * 0.5 + eye.0, canvas.1 * 0.5 + eye.1) })
    }

    fn offset(&self, root: &Value, props: &Properties) -> (f32, f32) {
        let depth = vec2_or(root.get("parallaxDepth"), props, (0.0, 0.0));
        let origin = vec3(root.get("origin"), props).unwrap_or((0.0, 0.0, 0.0));
        (
            (origin.0 - self.focus.0) * self.amount * depth.0,
            (origin.1 - self.focus.1) * self.amount * depth.1,
        )
    }
}

fn ancestor_chain<'a>(
    object: &'a Value,
    by_id: &std::collections::HashMap<String, &'a Value>,
) -> Vec<&'a Value> {
    let mut chain = vec![object];
    let mut cursor = object;
    for _ in 0..8 {
        let Some(parent_id) = cursor.get("parent").and_then(id_of) else {
            break;
        };
        let Some(parent) = by_id.get(&parent_id) else {
            break;
        };
        if chain.iter().any(|seen| std::ptr::eq(*seen, *parent)) {
            break;
        }
        chain.push(parent);
        cursor = parent;
    }
    chain
}

fn resolve_transform<'a>(
    object: &'a Value,
    by_id: &std::collections::HashMap<String, &'a Value>,
    props: &Properties,
    parallax: Option<&Parallax>,
) -> Transform {
    let chain = ancestor_chain(object, by_id);
    let mut out = Transform { scale: (1.0, 1.0), ..Transform::default() };
    for node in chain.iter().rev() {
        out = compose(out, object_transform(node, props));
    }
    if let Some(parallax) = parallax
        && let Some(root) = chain.last()
    {
        let (dx, dy) = parallax.offset(root, props);
        out.origin.0 += dx;
        out.origin.1 += dy;
    }
    out
}

fn layer_mouse(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value>,
    props: &Properties,
    parallax: crate::mouse::Parallax,
    transform: Transform,
    text: bool,
) -> crate::mouse::LayerMouse {
    let chain = ancestor_chain(object, by_id);
    let root = chain.last().copied().unwrap_or(object);
    let depth = vec2_or(root.get("parallaxDepth"), props, (0.0, 0.0));
    crate::mouse::LayerMouse {
        parallax: if parallax.amount != 0.0 && parallax.influence != 0.0 {
            [depth.0, depth.1]
        } else {
            [0.0; 2]
        },
        clock: text
            .then(|| {
                object.get("text").and_then(|value| {
                    crate::mouse::Clock3d::from_text(
                        value,
                        [transform.origin.0, transform.origin.1],
                    )
                })
            })
            .flatten(),
    }
}

fn id_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(num) => Some(num.to_string()),
        _ => None,
    }
}

fn collect_render_target_layer_ids(
    value: &Value,
    own_id: Option<&str>,
    targets: &mut std::collections::BTreeSet<String>,
) {
    match value {
        Value::String(text) => {
            let Some((layer, suffix)) = text
                .strip_prefix("_rt_imageLayerComposite_")
                .and_then(|rest| rest.rsplit_once('_'))
            else {
                return;
            };
            if !layer.is_empty() && own_id != Some(layer) && matches!(suffix, "a" | "A" | "b" | "B")
            {
                targets.insert(layer.to_string());
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_render_target_layer_ids(value, own_id, targets);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_render_target_layer_ids(value, own_id, targets);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteFrame {
    pub uv: [f32; 4],
    pub rotated: bool,
    pub time: f32,
    pub image: i32,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub img_width: u32,
    pub img_height: u32,
    pub pixels: tex::Pixels,
    pub video: Option<std::sync::Arc<[u8]>>,
    pub frames: Vec<SpriteFrame>,
    pub clamp: bool,
    pub nearest: bool,
    pub format: tex::TexFormat,
}

fn sprite_frames(parsed: &tex::Tex, width: u32, height: u32) -> Vec<SpriteFrame> {
    let (aw, ah) = (width.max(1) as f32, height.max(1) as f32);
    parsed
        .frames
        .iter()
        .filter_map(|frame| {
            let (fw, fh) = frame.size();
            if fw <= 0.0 || fh <= 0.0 {
                return None;
            }
            let rotated = frame.rotated();
            let (uw, uh) = if rotated { (fh, fw) } else { (fw, fh) };
            Some(SpriteFrame {
                uv: [frame.x / aw, frame.y / ah, uw / aw, uh / ah],
                rotated,
                time: frame.frame_time.max(0.0),
                image: frame.image_id,
            })
        })
        .collect()
}

impl Texture {
    pub fn payload_bytes(&self) -> usize {
        self.pixels.bytes() + self.video.as_ref().map_or(0, |payload| payload.len())
    }

    #[must_use]
    pub fn we_format(&self) -> i64 {
        match self.format {
            tex::TexFormat::Rgba8888 => 0,
            tex::TexFormat::Dxt5 => 4,
            tex::TexFormat::Dxt3 => 6,
            tex::TexFormat::Dxt1 => 7,
            tex::TexFormat::Rg88 => 8,
            tex::TexFormat::R8 => 9,
            tex::TexFormat::Other(raw) => i64::from(raw),
        }
    }

    #[must_use]
    pub fn atlas_frames(&self) -> Option<&[SpriteFrame]> {
        let playable = self.frames.len() > 1
            && self.frames.iter().all(|frame| frame.image == 0 && !frame.rotated)
            && self.frames.iter().map(|frame| frame.time).sum::<f32>() > 0.0;
        playable.then_some(self.frames.as_slice())
    }

    pub fn uv_scale(&self) -> (f32, f32) {
        (
            (self.img_width as f32 / self.width.max(1) as f32).clamp(0.0, 1.0),
            (self.img_height as f32 / self.height.max(1) as f32).clamp(0.0, 1.0),
        )
    }
}

fn vec3(value: Option<&Value>, props: &Properties) -> Option<(f32, f32, f32)> {
    let parts = crate::effects::bound_value(value?, props)?;
    if let [single] = parts[..] {
        return Some((single, single, single));
    }
    let mut parts = parts.into_iter();
    Some((parts.next()?, parts.next()?, parts.next().unwrap_or(0.0)))
}

fn vec2_or(value: Option<&Value>, props: &Properties, fallback: (f32, f32)) -> (f32, f32) {
    vec3(value, props).map_or(fallback, |(x, y, _)| (x, y))
}

pub(crate) fn number(value: Option<&Value>, props: &Properties, fallback: f32) -> f32 {
    let Some(value) = value else {
        return fallback;
    };
    crate::effects::bound_value(value, props)
        .and_then(|parts| parts.first().copied())
        .unwrap_or(fallback)
}

fn truthy(value: Option<&Value>, props: &Properties, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(num)) => num.as_f64().unwrap_or(1.0) != 0.0,
        Some(Value::String(text)) => text != "false" && text != "0",
        Some(value @ Value::Object(_)) => crate::effects::bound_value(value, props)
            .and_then(|parts| parts.first().copied())
            .is_none_or(|first| first != 0.0),
        _ => fallback,
    }
}

fn tex_candidates(raw: &str) -> Vec<String> {
    let trimmed = raw.trim_end_matches(".tex");
    let mut out = vec![format!("{trimmed}.tex")];
    if !trimmed.starts_with("materials/") {
        out.push(format!("materials/{trimmed}.tex"));
    }
    out
}

pub fn load_texture_bytes(bytes: &[u8]) -> Option<Texture> {
    let mut parsed = tex::parse(bytes).ok()?;
    let img_width = u32::try_from(parsed.meta.img_width).unwrap_or(0);
    let img_height = u32::try_from(parsed.meta.img_height).unwrap_or(0);
    let video = if parsed.meta.flags & tex::FLAG_IS_VIDEO != 0 {
        let mip = parsed.images.first_mut()?.first_mut()?;
        if mip.data.is_empty() {
            return None;
        }
        Some(std::sync::Arc::from(std::mem::take(&mut mip.data)))
    } else {
        None
    };
    let pixels =
        if video.is_some() { tex::Pixels::default() } else { tex::take_pixels(&mut parsed)? };
    let (width, height) = if video.is_some() {
        let width = u32::try_from(parsed.meta.tex_width).ok()?;
        let height = u32::try_from(parsed.meta.tex_height).ok()?;
        tex::PixelFormat::Rgba8.level_bytes(width, height)?;
        (width, height)
    } else {
        (pixels.width(), pixels.height())
    };
    let img_width = if img_width == 0 { width } else { img_width.min(width) };
    let img_height = if img_height == 0 { height } else { img_height.min(height) };
    let frames = sprite_frames(&parsed, width, height);
    Some(Texture {
        width,
        height,
        img_width,
        img_height,
        pixels,
        video,
        frames,
        clamp: parsed.meta.flags & tex::FLAG_CLAMP_UVS != 0,
        nearest: parsed.meta.flags & tex::FLAG_NO_INTERPOLATION != 0,
        format: if parsed.meta.free_image_format.is_some_and(|format| format >= 0) {
            tex::TexFormat::Rgba8888
        } else {
            parsed.meta.format
        },
    })
}

pub fn load_texture_named(pkg: &Package, raw: &str) -> Option<Texture> {
    load_texture(pkg, raw)
}

fn load_texture(pkg: &Package, raw: &str) -> Option<Texture> {
    for candidate in tex_candidates(raw) {
        let Some(bytes) = pkg.find(&candidate) else {
            continue;
        };
        return load_texture_bytes(bytes);
    }
    None
}

fn load_texture_or_asset(
    pkg: &Package,
    assets: &crate::effects::Assets,
    raw: &str,
) -> Option<Texture> {
    load_texture(pkg, raw).or_else(|| {
        tex_candidates(raw)
            .iter()
            .find_map(|candidate| assets.read_bytes(candidate))
            .and_then(|bytes| load_texture_bytes(&bytes))
    })
}

fn asset_json(
    pkg: &Package,
    assets: &crate::effects::Assets,
    path: &str,
) -> Option<serde_json::Value> {
    pkg.find_json(path)
        .ok()
        .flatten()
        .or_else(|| assets.read(path).and_then(|text| crate::json::parse(text.as_bytes()).ok()))
}

pub fn solid_texture() -> Texture {
    Texture {
        width: 1,
        height: 1,
        img_width: 1,
        img_height: 1,
        pixels: tex::Pixels::rgba(1, 1, vec![255, 255, 255, 255]),
        video: None,
        frames: Vec::new(),
        clamp: true,
        nearest: false,
        format: tex::TexFormat::Rgba8888,
    }
}

#[derive(Default)]
struct UtilModel {
    passthrough: bool,
    fullscreen: bool,
    flat: bool,
}

fn util_model(pkg: &Package, assets: &crate::effects::Assets, model_path: &str) -> UtilModel {
    let Some(model) = asset_json(pkg, assets, model_path) else {
        return UtilModel::default();
    };
    let flag = |key: &str| model.get(key) == Some(&Value::Bool(true));
    let flat = model
        .get("material")
        .and_then(Value::as_str)
        .and_then(|path| asset_json(pkg, assets, path))
        .is_some_and(|material| material_is_flat(&material));
    UtilModel { passthrough: flag("passthrough"), fullscreen: flag("fullscreen"), flat }
}

fn material_is_flat(material: &Value) -> bool {
    material
        .get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .and_then(|pass| pass.get("shader"))
        .and_then(Value::as_str)
        .is_some_and(|shader| shader == "flat")
}

fn material_texture(pkg: &Package, path: &str) -> Option<String> {
    let material = pkg.find_json(path).ok()??;
    let passes = material.get("passes")?.as_array()?;
    for pass in passes {
        if let Some(textures) = pass.get("textures").and_then(Value::as_array) {
            for slot in textures {
                if let Some(text) = slot.as_str()
                    && !text.is_empty()
                    && !text.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("_rt_"))
                {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

fn legacy_material_tint(pkg: &Package, model_path: &str) -> Option<([f32; 3], f32)> {
    let model = pkg.find_json(model_path).ok()??;
    let material = pkg.find_json(model.get("material")?.as_str()?).ok()??;
    let pass = material.get("passes")?.as_array()?.first()?;
    let shader = pass.get("shader")?.as_str()?;
    let versioned = pass.get("combos").and_then(|combos| combos.get("VERSION")).is_some();
    if !(shader == "genericimage" || (shader == "genericimage2" && !versioned)) {
        return None;
    }
    let constants = pass.get("constantshadervalues");
    let constant = |key: &str| {
        constants
            .and_then(|values| values.get(key))
            .and_then(Value::as_f64)
            .map(|value| value as f32)
    };
    let brightness = constant("Brightness").or_else(|| constant("Bright")).unwrap_or(1.0);
    let alpha = constant("Alpha").unwrap_or(1.0).clamp(0.0, 1.0);
    Some(([brightness; 3], alpha))
}

fn model_texture(pkg: &Package, path: &str) -> Option<String> {
    let model = pkg.find_json(path).ok()??;
    let material = model.get("material")?.as_str()?;
    material_texture(pkg, material)
}

fn model_puppet(pkg: &Package, path: &str) -> Result<Option<crate::puppet::Mesh>> {
    let Some(model) = pkg.find_json(path)? else {
        return Ok(None);
    };
    let Some(puppet) = model.get("puppet").and_then(Value::as_str) else {
        return Ok(None);
    };
    let bytes = pkg.find(puppet).ok_or_else(|| anyhow!("missing puppet {puppet}"))?;
    crate::puppet::parse(bytes).map(Some).map_err(|err| anyhow!("parse puppet {puppet}: {err}"))
}

fn puppet_animation_layers(
    object: &Value,
    props: &Properties,
) -> Vec<crate::puppet::AnimationLayer> {
    object
        .get("animationlayers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|layer| truthy(layer.get("visible"), props, true))
        .filter_map(|layer| {
            let id = u32::try_from(number(layer.get("animation"), props, -1.0) as i64).ok()?;
            Some(crate::puppet::AnimationLayer {
                id,
                rate: number(layer.get("rate"), props, 1.0).max(0.0),
                blend: number(layer.get("blend"), props, 1.0).clamp(0.0, 1.0),
                additive: truthy(layer.get("additive"), props, false),
            })
        })
        .collect()
}

pub fn load(pkg: &Package) -> Result<SceneModel> {
    let configured = std::env::var("SKWD_WE_ASSETS").ok();
    load_with(pkg, &crate::effects::Assets::discover(configured.as_deref()))
}

pub fn load_from_dir(pkg: &Package, dir: &std::path::Path) -> Result<SceneModel> {
    load_from_dir_with(pkg, dir, &Properties::new())
}

pub fn load_from_dir_with(
    pkg: &Package,
    dir: &std::path::Path,
    overrides: &Properties,
) -> Result<SceneModel> {
    let properties = std::fs::read(dir.join("project.json"))
        .ok()
        .and_then(|bytes| crate::json::parse(&bytes).ok())
        .map(|project| crate::effects::parse_properties(&project))
        .unwrap_or_default();
    let configured = std::env::var("SKWD_WE_ASSETS").ok();
    load_with(
        pkg,
        &crate::effects::Assets::discover(configured.as_deref())
            .with_properties(properties)
            .with_overrides(overrides.clone()),
    )
}

pub fn load_with(pkg: &Package, assets: &crate::effects::Assets) -> Result<SceneModel> {
    let props = &assets.properties;
    let scene = pkg
        .find_json("scene.json")
        .map_err(|err| anyhow!("{err:#}"))?
        .ok_or_else(|| anyhow!("no scene.json"))?;
    let general = scene.get("general");
    let ortho = general.and_then(|top| top.get("orthogonalprojection"));
    let canvas = (
        number(ortho.and_then(|proj| proj.get("width")), props, 1920.0).max(1.0),
        number(ortho.and_then(|proj| proj.get("height")), props, 1080.0).max(1.0),
    );
    let clear = vec3(general.and_then(|top| top.get("clearcolor")), props)
        .map_or([0.0, 0.0, 0.0], |(r, g, b)| [r, g, b]);
    let ambient = vec3(general.and_then(|top| top.get("ambientcolor")), props)
        .map_or([0.3, 0.3, 0.3], |(r, g, b)| [r, g, b]);
    let skylight = vec3(general.and_then(|top| top.get("skylightcolor")), props)
        .map_or([0.3, 0.3, 0.3], |(r, g, b)| [r, g, b]);

    let parallax = Parallax::of(&scene, canvas, props);
    let mouse = crate::mouse::Parallax {
        amount: parallax.as_ref().map_or(0.0, |p| p.amount),
        influence: if parallax.is_some() {
            number(general.and_then(|top| top.get("cameraparallaxmouseinfluence")), props, 0.5)
        } else {
            0.0
        },
        delay: number(general.and_then(|top| top.get("cameraparallaxdelay")), props, 0.1),
        camera_offset: parallax
            .as_ref()
            .map_or([0.0; 2], |p| [p.focus.0 / canvas.0 - 0.5, p.focus.1 / canvas.1 - 0.5]),
    };

    let mut layers = Vec::new();
    let mut particles = Vec::new();
    let mut skipped = Vec::new();
    let objects = scene.get("objects").and_then(Value::as_array).map_or(&[][..], |arr| arr);
    if objects.len() > MAX_SCENE_OBJECTS {
        return Err(anyhow!("scene has {} objects; limit is {MAX_SCENE_OBJECTS}", objects.len()));
    }
    let by_id: std::collections::HashMap<String, &Value> = objects
        .iter()
        .filter_map(|object| Some((object.get("id").and_then(id_of)?, object)))
        .collect();
    let mut render_target_layer_ids = std::collections::BTreeSet::new();
    for object in objects {
        collect_render_target_layer_ids(
            object,
            object.get("id").and_then(id_of).as_deref(),
            &mut render_target_layer_ids,
        );
    }
    let mut texture_bytes = 0_usize;
    for (object_index, object) in objects.iter().enumerate() {
        let id =
            object.get("id").and_then(id_of).unwrap_or_else(|| format!("@object-{object_index}"));
        let name = object.get("name").and_then(Value::as_str).unwrap_or("object").to_string();
        let visible = ancestor_chain(object, &by_id).iter().all(|node| {
            truthy(node.get("visible"), props, true)
                && !crate::dynamic_text::media_scripted(node.get("visible"))
        });
        if !visible && !render_target_layer_ids.contains(&id) {
            continue;
        }
        if object
            .get("instance")
            .and_then(|instance| instance.get("usertextures"))
            .and_then(Value::as_array)
            .is_some_and(|slots| {
                slots.iter().any(|slot| {
                    slot.get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| name.starts_with("$media"))
                })
            })
        {
            skipped.push(format!("{name}: media thumbnail layer hidden without playback"));
            continue;
        }
        let Some(model_path) = object.get("image").and_then(Value::as_str) else {
            if let Some(path) = object.get("particle").and_then(Value::as_str) {
                match crate::particles::load(pkg, assets, object, path) {
                    Some(system) => {
                        if let Some(texture) = &system.texture {
                            add_texture_bytes(&mut texture_bytes, texture.payload_bytes())?;
                        }
                        particles.push(ParticleLayer {
                            depth: system.origin.2,
                            scene_order: object_index,
                            system,
                        });
                    }
                    None => skipped.push(format!("{name}: particle {path} unsupported")),
                }
            } else if object.get("text").is_some() {
                match crate::text::render(pkg, assets, object, props) {
                    Some(rendered) => {
                        let transform = resolve_transform(object, &by_id, props, parallax.as_ref());
                        let (w, h) =
                            (rendered.texture.width as f32, rendered.texture.height as f32);
                        let color = vec3(object.get("color"), props)
                            .map_or([1.0, 1.0, 1.0], |(r, g, b)| [r, g, b]);
                        let alpha = layer_alpha(object, props);
                        let color_blend =
                            number(object.get("colorBlendMode"), props, 0.0).max(0.0) as u32;
                        let (effects, effect_skips) =
                            crate::effects::load_effects(pkg, assets, object);
                        for skip in effect_skips {
                            skipped.push(format!("{name}: {skip}"));
                        }
                        add_texture_bytes(&mut texture_bytes, rendered.texture.payload_bytes())?;
                        let (sx, sy) = transform.scale;
                        let angle = -transform.angle;
                        let (sin, cos) = angle.sin_cos();
                        let (ox, oy) = (
                            (rendered.offset.0 + w * 0.5 - 1.0) * sx,
                            (rendered.offset.1 + h * 0.5 - 1.0) * sy,
                        );
                        let center = (
                            transform.origin.0 + ox * cos - oy * sin,
                            canvas.1 - transform.origin.1 + ox * sin + oy * cos,
                        );
                        layers.push(Layer {
                            id,
                            name,
                            visible,
                            live_text: rendered.live,
                            is_text: true,
                            mouse: layer_mouse(object, &by_id, props, mouse, transform, true),
                            texture: rendered.texture,
                            puppet: None,
                            center,
                            size: (w * sx, h * sy),
                            scale: transform.scale,
                            depth: transform.origin.2,
                            scene_order: object_index,
                            alpha,
                            angle,
                            color,
                            color_blend,
                            passthrough: false,
                            solid: false,
                            effects,
                        });
                    }
                    None => skipped.push(format!("{name}: text unsupported")),
                }
            } else if object.get("light").is_some() {
                skipped.push(format!("{name}: unsupported object"));
            }
            continue;
        };
        let resolved = model_texture(pkg, model_path);
        let util = if resolved.is_none() {
            util_model(pkg, assets, model_path)
        } else {
            UtilModel::default()
        };
        let solid = resolved.is_none() && util.flat;
        let texture = if let Some(tex_path) = resolved {
            let Some(texture) = load_texture_or_asset(pkg, assets, &tex_path) else {
                skipped.push(format!("{name}: undecodable texture {tex_path}"));
                continue;
            };
            texture
        } else if util.passthrough || util.flat {
            solid_texture()
        } else {
            skipped.push(format!("{name}: no texture in {model_path}"));
            continue;
        };
        let puppet_mesh = match model_puppet(pkg, model_path) {
            Ok(puppet) => puppet,
            Err(err) => {
                skipped.push(format!("{name}: {err}"));
                continue;
            }
        };
        let transform = resolve_transform(object, &by_id, props, parallax.as_ref());
        let fallback = if util.passthrough {
            canvas
        } else {
            (texture.img_width as f32, texture.img_height as f32)
        };
        let base =
            if util.fullscreen { canvas } else { vec2_or(object.get("size"), props, fallback) };
        let puppet = puppet_mesh.map(|mesh| Puppet {
            mesh,
            size: base,
            layers: puppet_animation_layers(object, props),
        });
        let (color, alpha) = match legacy_material_tint(pkg, model_path) {
            Some(tint) => tint,
            None => (
                vec3(object.get("color"), props).map_or([1.0, 1.0, 1.0], |(r, g, b)| [r, g, b]),
                layer_alpha(object, props),
            ),
        };
        let color_blend = number(object.get("colorBlendMode"), props, 0.0).max(0.0) as u32;
        let (mut effects, effect_skips) = crate::effects::load_effects(pkg, assets, object);
        if util.passthrough && effects.is_empty() {
            continue;
        }
        if crate::effects::shader_blend(color_blend) {
            let (blend_color, blend_alpha) =
                if util.passthrough { (color, alpha) } else { ([1.0, 1.0, 1.0], 1.0) };
            match crate::effects::color_blend_effect(
                pkg,
                assets,
                color_blend,
                blend_color,
                blend_alpha,
                Some(&id),
            ) {
                Some(effect) => effects.push(effect),
                None => skipped.push(format!("{name}: colorBlendMode {color_blend} unavailable")),
            }
        }
        add_texture_bytes(&mut texture_bytes, texture.payload_bytes())?;
        for effect in &effects {
            for pass in &effect.passes {
                for slot in pass.textures.iter().flatten() {
                    add_texture_bytes(&mut texture_bytes, slot.payload_bytes())?;
                }
            }
        }
        for skip in effect_skips {
            skipped.push(format!("{name}: {skip}"));
        }
        let (center, size, angle) = if util.fullscreen {
            ((canvas.0 * 0.5, canvas.1 * 0.5), canvas, 0.0)
        } else {
            (
                (transform.origin.0, canvas.1 - transform.origin.1),
                (base.0 * transform.scale.0, base.1 * transform.scale.1),
                -transform.angle,
            )
        };
        layers.push(Layer {
            id,
            name,
            visible,
            live_text: None,
            is_text: false,
            mouse: layer_mouse(object, &by_id, props, mouse, transform, false),
            texture,
            puppet,
            center,
            size,
            scale: transform.scale,
            depth: transform.origin.2,
            scene_order: object_index,
            alpha,
            angle,
            color,
            color_blend,
            passthrough: util.passthrough,
            solid,
            effects,
        });
    }
    if truthy(general.and_then(|top| top.get("bloom")), props, false) {
        let strength = number(general.and_then(|top| top.get("bloomstrength")), props, 2.0);
        let threshold = number(general.and_then(|top| top.get("bloomthreshold")), props, 0.65);
        let tint =
            vec3(general.and_then(|top| top.get("bloomtint")), props).unwrap_or((1.0, 1.0, 1.0));
        let object = serde_json::json!({
            "id": "@bloom",
            "name": "bloom",
            "effects": [{
                "file": crate::effects::BLOOM_LDR_EFFECT_FILE,
                "passes": [{"constantshadervalues": {
                    "bloomstrength": strength,
                    "bloomthreshold": threshold,
                    "bloomtint": format!("{} {} {}", tint.0, tint.1, tint.2)
                }}]
            }]
        });
        let (effects, effect_skips) = crate::effects::load_effects(pkg, assets, &object);
        for skip in effect_skips {
            skipped.push(format!("bloom: {skip}"));
        }
        if effects.iter().any(|effect| !effect.passes.is_empty()) {
            layers.push(Layer {
                id: "@bloom".to_string(),
                name: "bloom".to_string(),
                visible: true,
                live_text: None,
                is_text: false,
                mouse: crate::mouse::LayerMouse::default(),
                texture: solid_texture(),
                puppet: None,
                center: (canvas.0 * 0.5, canvas.1 * 0.5),
                size: canvas,
                scale: (1.0, 1.0),
                depth: 0.0,
                scene_order: objects.len(),
                alpha: 1.0,
                angle: 0.0,
                color: [1.0, 1.0, 1.0],
                color_blend: 0,
                passthrough: true,
                solid: false,
                effects,
            });
        } else {
            skipped.push("bloom: engine bloom materials unavailable".to_string());
        }
    }
    layers.sort_by_key(|layer| layer.scene_order);
    let mut unified_order: Vec<(usize, bool, usize)> = layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.scene_order, false, index))
        .chain(
            particles
                .iter()
                .enumerate()
                .map(|(index, particle)| (particle.scene_order, true, index)),
        )
        .collect();
    unified_order.sort_by_key(|entry| entry.0);
    for (scene_order, (_, particle, index)) in unified_order.into_iter().enumerate() {
        if particle {
            particles[index].scene_order = scene_order;
        } else {
            layers[index].scene_order = scene_order;
        }
    }
    Ok(SceneModel {
        canvas,
        mouse,
        camera_fov: number(general.and_then(|top| top.get("fov")), props, 50.0).clamp(1.0, 179.0),
        clear,
        ambient,
        skylight,
        layers,
        particles,
        skipped,
    })
}

fn add_texture_bytes(total: &mut usize, bytes: usize) -> Result<()> {
    *total = total.checked_add(bytes).ok_or_else(|| anyhow!("scene texture size overflow"))?;
    if *total > MAX_SCENE_TEXTURE_BYTES {
        return Err(anyhow!(
            "scene textures use {total} bytes; limit is {MAX_SCENE_TEXTURE_BYTES}"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
