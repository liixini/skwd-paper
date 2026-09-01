use crate::pkg::Package;
use crate::tex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const MAX_WALKED_REFS: usize = 4096;

#[derive(Debug, Default)]
pub struct SceneFeatures {
    pub objects_image: usize,
    pub objects_particle: usize,
    pub objects_sound: usize,
    pub objects_sound_event: usize,
    pub objects_light: usize,
    pub objects_text: usize,
    pub objects_other: usize,
    pub other_objects: Vec<OtherObject>,
    pub effects: BTreeSet<String>,
    pub shared_effects: BTreeSet<String>,
    pub shaders: BTreeSet<String>,
    pub combos: BTreeSet<String>,
    pub particles: BTreeSet<String>,
    pub shared_particles: BTreeSet<String>,
    pub puppet: bool,
    pub puppet_unsupported: bool,
    pub parallax: bool,
    pub audio: bool,
    pub tex_formats: BTreeMap<String, usize>,
    pub tex_gif: usize,
    pub animated_image_textures: BTreeSet<String>,
    pub tex_video: usize,
    pub tex_other_format: BTreeSet<String>,
    pub tex_failures: usize,
    pub json_failures: usize,
    pub refs_truncated: bool,
    pub sample_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherObject {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub properties: Vec<String>,
}

impl SceneFeatures {
    pub fn tier(&self) -> u8 {
        if self.puppet {
            return 3;
        }
        if self.objects_particle > 0 || !self.particles.is_empty() {
            return 2;
        }
        u8::from(!self.effects.is_empty() || !self.shared_effects.is_empty())
    }
}

fn stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    let dir = path.rsplit('/').nth(1).unwrap_or(file);
    if file == "effect.json" || file == "particle.json" || file == "material.json" {
        dir.to_string()
    } else {
        file.trim_end_matches(".json").to_string()
    }
}

fn as_path(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text.as_str()),
        Value::Object(obj) => obj.get("file").and_then(Value::as_str),
        _ => None,
    }
}

fn other_object(value: &Value) -> OtherObject {
    let properties =
        value.as_object().map(|object| object.keys().cloned().collect()).unwrap_or_default();
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| !kind.is_empty())
        .map(|kind| format!("type:{kind}"))
        .or_else(|| value.get("camera").is_some().then(|| "camera".into()))
        .or_else(|| value.get("shape").is_some().then(|| "shape".into()))
        .or_else(|| value.get("attachment").is_some().then(|| "attachment".into()))
        .unwrap_or_else(|| "transform".into());
    OtherObject {
        kind,
        id: value.get("id").map_or_else(String::new, |id| {
            id.as_str().map_or_else(|| id.to_string(), String::from)
        }),
        name: value.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        properties,
    }
}

struct Walk<'a> {
    pkg: &'a Package,
    cache: HashMap<String, Option<std::rc::Rc<Value>>>,
    budget: usize,
}

impl Walk<'_> {
    fn json(&mut self, path: &str, out: &mut SceneFeatures) -> Option<std::rc::Rc<Value>> {
        if self.budget == 0 {
            out.refs_truncated = true;
            return None;
        }
        if let Some(hit) = self.cache.get(path) {
            return hit.clone();
        }
        self.budget -= 1;
        let parsed = match self.pkg.find_json(path) {
            Ok(value) => value.map(std::rc::Rc::new),
            Err(err) => {
                out.json_failures += 1;
                if out.sample_error.is_none() {
                    out.sample_error = Some(format!("{err:#}"));
                }
                None
            }
        };
        self.cache.insert(path.to_string(), parsed.clone());
        parsed
    }

    fn material(&mut self, path: &str, out: &mut SceneFeatures, image_layer: bool) {
        let Some(material) = self.json(path, out) else {
            return;
        };
        let Some(passes) = material.get("passes").and_then(Value::as_array) else {
            return;
        };
        for pass in passes {
            if let Some(shader) = pass.get("shader").and_then(Value::as_str) {
                out.shaders.insert(shader.to_string());
            }
            if let Some(combos) = pass.get("combos").and_then(Value::as_object) {
                for key in combos.keys() {
                    out.combos.insert(key.clone());
                }
            }
            if image_layer && let Some(textures) = pass.get("textures").and_then(Value::as_array) {
                for texture in textures.iter().filter_map(Value::as_str) {
                    if texture_is_animated(self.pkg, texture) {
                        out.animated_image_textures.insert(texture.to_string());
                    }
                }
            }
        }
    }

    fn model(&mut self, path: &str, out: &mut SceneFeatures) {
        let Some(model) = self.json(path, out) else {
            return;
        };
        if let Some(puppet) = model.get("puppet").and_then(Value::as_str) {
            out.puppet = true;
            out.puppet_unsupported =
                self.pkg.find(puppet).is_none_or(|bytes| crate::puppet::parse(bytes).is_err());
        }
        if let Some(material) = model.get("material").and_then(Value::as_str) {
            let material = material.to_string();
            self.material(&material, out, true);
        }
    }

    fn effect(&mut self, value: &Value, out: &mut SceneFeatures) {
        let Some(file) = value.get("file").and_then(Value::as_str) else {
            return;
        };
        let file = file.to_string();
        let name = stem(&file);
        let Some(effect) = self.json(&file, out) else {
            out.shared_effects.insert(name);
            return;
        };
        out.effects.insert(name);
        if let Some(passes) = effect.get("passes").and_then(Value::as_array) {
            let materials: Vec<String> = passes
                .iter()
                .filter_map(|pass| pass.get("material").and_then(Value::as_str))
                .map(String::from)
                .collect();
            for material in materials {
                self.material(&material, out, false);
            }
        }
        if let Some(deps) = effect.get("dependencies").and_then(Value::as_array) {
            for dep in deps.iter().filter_map(Value::as_str) {
                out.combos.insert(format!("dep:{dep}"));
            }
        }
    }
}

fn texture_is_animated(pkg: &Package, raw: &str) -> bool {
    let trimmed = raw.trim_end_matches(".tex");
    let direct = format!("{trimmed}.tex");
    let material = format!("materials/{trimmed}.tex");
    [direct, material].into_iter().any(|path| {
        pkg.find(&path)
            .and_then(|bytes| tex::parse_meta(bytes).ok())
            .is_some_and(|meta| meta.flags & tex::FLAG_IS_GIF != 0)
    })
}

fn key_present(value: &Value, needle: &str) -> bool {
    match value {
        Value::Object(obj) => {
            obj.keys().any(|key| key.to_ascii_lowercase().contains(needle))
                || obj.values().any(|inner| key_present(inner, needle))
        }
        Value::Array(items) => items.iter().any(|inner| key_present(inner, needle)),
        _ => false,
    }
}

pub fn extract(pkg: &Package) -> Result<SceneFeatures, String> {
    let mut out = SceneFeatures::default();
    let scene = match pkg.find_json("scene.json") {
        Ok(Some(value)) => value,
        Ok(None) => match pkg.find_json("gifscene.json") {
            Ok(Some(value)) => value,
            Ok(None) => return Err("no scene.json in pkg".into()),
            Err(err) => return Err(format!("{err:#}")),
        },
        Err(err) => return Err(format!("{err:#}")),
    };

    let mut walk = Walk { pkg, cache: HashMap::new(), budget: MAX_WALKED_REFS };
    if let Some(objects) = scene.get("objects").and_then(Value::as_array) {
        for object in objects {
            if let Some(image) = object.get("image").and_then(as_path) {
                out.objects_image += 1;
                let image = image.to_string();
                walk.model(&image, &mut out);
            } else if let Some(particle) = object.get("particle").and_then(as_path) {
                out.objects_particle += 1;
                let name = stem(particle);
                if pkg.find(particle).is_some() {
                    out.particles.insert(name);
                } else {
                    out.shared_particles.insert(name);
                }
            } else if object.get("sound").is_some() {
                out.objects_sound += 1;
                if crate::sound::event_driven(object.get("startsilent")) {
                    out.objects_sound_event += 1;
                }
            } else if object.get("light").is_some() {
                out.objects_light += 1;
            } else if object.get("text").is_some() {
                out.objects_text += 1;
            } else {
                let other = other_object(object);
                out.objects_other += usize::from(other.kind != "transform");
                out.other_objects.push(other);
            }
            if let Some(effects) = object.get("effects").and_then(Value::as_array) {
                for effect in effects {
                    walk.effect(effect, &mut out);
                }
            }
        }
    }

    out.parallax = key_present(&scene, "parallax");
    out.audio = key_present(&scene, "audioprocessing")
        || out.combos.iter().any(|combo| combo.to_ascii_lowercase().contains("audio"));

    for entry in pkg.entries() {
        if !entry.path.ends_with(".tex") {
            continue;
        }
        match tex::parse_meta(pkg.read(entry)) {
            Ok(meta) => {
                *out.tex_formats.entry(meta.format.name()).or_insert(0) += 1;
                if let tex::TexFormat::Other(_) = meta.format {
                    out.tex_other_format.insert(meta.format.name());
                }
                if meta.flags & tex::FLAG_IS_GIF != 0 {
                    out.tex_gif += 1;
                }
                if meta.flags & tex::FLAG_IS_VIDEO != 0 {
                    out.tex_video += 1;
                }
            }
            Err(err) => {
                out.tex_failures += 1;
                if out.sample_error.is_none() {
                    out.sample_error = Some(format!("{}: {err:#}", entry.path));
                }
            }
        }
    }

    Ok(out)
}
