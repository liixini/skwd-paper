use crate::pkg::Package;
use crate::tex;
use anyhow::{Result, anyhow};
use serde_json::Value;

pub const MAX_SCENE_TEXTURE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_SCENE_OBJECTS: usize = 16_384;

pub struct SceneModel {
    pub canvas: (f32, f32),
    pub clear: [f32; 3],
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
    pub depth: f32,
    pub scene_order: usize,
    pub alpha: f32,
    pub angle: f32,
    pub color: [f32; 3],
    pub color_blend: u32,
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

fn object_transform(object: &Value, props: &Properties) -> Transform {
    Transform {
        origin: vec3(object.get("origin"), props).unwrap_or((0.0, 0.0, 0.0)),
        scale: vec3(object.get("scale"), props).map_or((1.0, 1.0), |(x, y, _)| (x, y)),
        angle: vec3(object.get("angles"), props).map_or(0.0, |(_, _, z)| z),
    }
}

fn compose(parent: Transform, child: Transform) -> Transform {
    let (sin, cos) = parent.angle.to_radians().sin_cos();
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

fn resolve_transform(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value>,
    props: &Properties,
) -> Transform {
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
    let mut out = Transform { scale: (1.0, 1.0), ..Transform::default() };
    for node in chain.iter().rev() {
        out = compose(out, object_transform(node, props));
    }
    out
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

#[derive(Clone, Copy)]
pub struct SpriteFrame {
    pub uv: [f32; 4],
    pub rotated: bool,
    pub time: f32,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub img_width: u32,
    pub img_height: u32,
    pub rgba: Vec<u8>,
    pub frames: Vec<SpriteFrame>,
    pub clamp: bool,
    pub nearest: bool,
    pub format: tex::TexFormat,
}

pub fn apply_particle_channels(texture: &mut Texture) {
    match texture.format {
        tex::TexFormat::R8 => {
            for px in texture.rgba.chunks_exact_mut(4) {
                px[3] = px[0];
                px[0] = 255;
                px[1] = 255;
                px[2] = 255;
            }
        }
        tex::TexFormat::Rg88 => {
            for px in texture.rgba.chunks_exact_mut(4) {
                px[3] = px[1];
                px[1] = px[0];
                px[2] = px[0];
            }
        }
        _ => {}
    }
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
            })
        })
        .collect()
}

impl Texture {
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

fn number(value: Option<&Value>, props: &Properties, fallback: f32) -> f32 {
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
    let parsed = tex::parse(bytes).ok()?;
    let img_width = u32::try_from(parsed.meta.img_width).unwrap_or(0);
    let img_height = u32::try_from(parsed.meta.img_height).unwrap_or(0);
    let (width, height, rgba) = tex::decode_rgba(&parsed)?;
    let img_width = if img_width == 0 { width } else { img_width.min(width) };
    let img_height = if img_height == 0 { height } else { img_height.min(height) };
    let frames = sprite_frames(&parsed, width, height);
    Some(Texture {
        width,
        height,
        img_width,
        img_height,
        rgba,
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
        rgba: vec![255, 255, 255, 255],
        frames: Vec::new(),
        clamp: true,
        nearest: false,
        format: tex::TexFormat::Rgba8888,
    }
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
                {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
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
        let visible = truthy(object.get("visible"), props, true);
        if !visible && !render_target_layer_ids.contains(&id) {
            continue;
        }
        let Some(model_path) = object.get("image").and_then(Value::as_str) else {
            if let Some(path) = object.get("particle").and_then(Value::as_str) {
                match crate::particles::load(pkg, assets, object, path) {
                    Some(system) => {
                        if let Some(texture) = &system.texture {
                            add_texture_bytes(&mut texture_bytes, texture.rgba.len())?;
                        }
                        particles.push(ParticleLayer {
                            depth: system.origin.2,
                            scene_order: object_index,
                            system,
                        });
                    }
                    None => skipped.push(format!("{name}: particle {path} unsupported")),
                }
            } else if object.get("light").is_some() || object.get("text").is_some() {
                skipped.push(format!("{name}: unsupported object"));
            }
            continue;
        };
        let resolved = model_texture(pkg, model_path);
        let texture = if let Some(tex_path) = resolved {
            let Some(texture) = load_texture(pkg, &tex_path) else {
                skipped.push(format!("{name}: undecodable texture {tex_path}"));
                continue;
            };
            texture
        } else {
            {
                let model = asset_json(pkg, assets, model_path);
                let passthrough = model
                    .as_ref()
                    .and_then(|model| model.get("passthrough"))
                    .is_some_and(|value| value == &Value::Bool(true));
                let flat = model
                    .as_ref()
                    .and_then(|model| model.get("material"))
                    .and_then(Value::as_str)
                    .and_then(|path| asset_json(pkg, assets, path))
                    .is_some_and(|material| material_is_flat(&material));
                if passthrough || !flat {
                    skipped.push(format!("{name}: no texture in {model_path}"));
                    continue;
                }
                solid_texture()
            }
        };
        let puppet_mesh = match model_puppet(pkg, model_path) {
            Ok(puppet) => puppet,
            Err(err) => {
                skipped.push(format!("{name}: {err}"));
                continue;
            }
        };
        let transform = resolve_transform(object, &by_id, props);
        let base = vec2_or(
            object.get("size"),
            props,
            (texture.img_width as f32, texture.img_height as f32),
        );
        let puppet = puppet_mesh.map(|mesh| Puppet {
            mesh,
            size: base,
            layers: puppet_animation_layers(object, props),
        });
        let color = vec3(object.get("color"), props).map_or([1.0, 1.0, 1.0], |(r, g, b)| [r, g, b]);
        let (effects, effect_skips) = crate::effects::load_effects(pkg, assets, object);
        add_texture_bytes(&mut texture_bytes, texture.rgba.len())?;
        for effect in &effects {
            for pass in &effect.passes {
                for slot in pass.textures.iter().flatten() {
                    add_texture_bytes(&mut texture_bytes, slot.rgba.len())?;
                }
            }
        }
        for skip in effect_skips {
            skipped.push(format!("{name}: {skip}"));
        }
        layers.push(Layer {
            id,
            name,
            visible,
            texture,
            puppet,
            center: (transform.origin.0, canvas.1 - transform.origin.1),
            size: (base.0 * transform.scale.0, base.1 * transform.scale.1),
            depth: transform.origin.2,
            scene_order: object_index,
            alpha: number(object.get("alpha"), props, 1.0).clamp(0.0, 1.0),
            angle: -transform.angle.to_radians(),
            color,
            color_blend: number(object.get("colorBlendMode"), props, 0.0).max(0.0) as u32,
            effects,
        });
    }
    layers.sort_by(|a, b| {
        a.depth.total_cmp(&b.depth).then_with(|| a.scene_order.cmp(&b.scene_order))
    });
    let mut unified_order: Vec<(f32, usize, bool, usize)> = layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.depth, layer.scene_order, false, index))
        .chain(
            particles
                .iter()
                .enumerate()
                .map(|(index, particle)| (particle.depth, particle.scene_order, true, index)),
        )
        .collect();
    unified_order
        .sort_by(|left, right| left.0.total_cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    for (scene_order, (_, _, particle, index)) in unified_order.into_iter().enumerate() {
        if particle {
            particles[index].scene_order = scene_order;
        } else {
            layers[index].scene_order = scene_order;
        }
    }
    Ok(SceneModel { canvas, clear, layers, particles, skipped })
}

fn add_texture_bytes(total: &mut usize, bytes: usize) -> Result<()> {
    *total = total.checked_add(bytes).ok_or_else(|| anyhow!("scene texture size overflow"))?;
    if *total > MAX_SCENE_TEXTURE_BYTES {
        return Err(anyhow!(
            "decoded scene textures use {total} bytes; limit is {MAX_SCENE_TEXTURE_BYTES}"
        ));
    }
    Ok(())
}
