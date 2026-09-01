use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::model::Properties;
use crate::pkg::Package;

pub const MAX_SCENE_SOUNDS: usize = 16;
pub const MAX_CLIPS_PER_SOUND: usize = 32;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SoundInventory {
    pub objects: usize,
    pub autostart: usize,
    pub event_driven: usize,
    pub hidden: usize,
    pub multiple_clips: usize,
    pub looped: usize,
    pub one_shot: usize,
    pub random: usize,
    pub visibility_bound: usize,
    pub volume_bound: usize,
    pub start_bound: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackMode {
    Loop,
    Once,
    Random,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneSound {
    pub name: String,
    pub clips: Vec<String>,
    pub volume: f32,
    pub mode: PlaybackMode,
    pub min_gap: f32,
    pub max_gap: f32,
}

impl SceneSound {
    #[must_use]
    pub fn gap_range(&self) -> (f32, f32) {
        let min = self.min_gap.max(0.0);
        let max = self.max_gap.max(min);
        (min, max)
    }
}

pub fn scene_sounds(pkg: &Package, properties: &Properties) -> Result<Vec<SceneSound>> {
    let scene = pkg.find_json("scene.json")?.ok_or_else(|| anyhow!("no scene.json"))?;
    let objects = scene.get("objects").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let mut out = Vec::new();
    for object in objects {
        if out.len() >= MAX_SCENE_SOUNDS {
            break;
        }
        if object.get("sound").is_none() {
            continue;
        }
        if !truthy(object.get("visible"), properties, true)
            || truthy(object.get("startsilent"), properties, false)
        {
            continue;
        }
        let clips: Vec<String> = clip_paths(object.get("sound"))
            .filter(|path| pkg.find(path).is_some())
            .take(MAX_CLIPS_PER_SOUND)
            .map(str::to_string)
            .collect();
        if clips.is_empty() {
            continue;
        }
        out.push(SceneSound {
            name: object.get("name").and_then(Value::as_str).unwrap_or("sound").trim().to_string(),
            clips,
            volume: number(object.get("volume"), properties, 1.0).max(0.0),
            mode: mode_of(object.get("playbackmode")),
            min_gap: number(object.get("mintime"), properties, 0.0).max(0.0),
            max_gap: number(object.get("maxtime"), properties, 0.0).max(0.0),
        });
    }
    Ok(out)
}

pub fn has_event_driven_sounds(pkg: &Package) -> bool {
    let Ok(Some(scene)) = pkg.find_json("scene.json") else {
        return false;
    };
    let objects = scene.get("objects").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    objects
        .iter()
        .any(|object| object.get("sound").is_some() && event_driven(object.get("startsilent")))
}

pub fn sound_inventory(pkg: &Package) -> Result<SoundInventory> {
    let scene = pkg.find_json("scene.json")?.ok_or_else(|| anyhow!("no scene.json"))?;
    let objects = scene.get("objects").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let mut out = SoundInventory::default();
    for object in objects.iter().filter(|object| object.get("sound").is_some()) {
        out.objects += 1;
        let starts_from_event = event_driven(object.get("startsilent"));
        out.event_driven += usize::from(starts_from_event);
        out.autostart += usize::from(!starts_from_event);
        out.hidden += usize::from(!truthy(object.get("visible"), &Properties::new(), true));
        out.multiple_clips += usize::from(clip_paths(object.get("sound")).take(2).count() > 1);
        match mode_of(object.get("playbackmode")) {
            PlaybackMode::Loop => out.looped += 1,
            PlaybackMode::Once => out.one_shot += 1,
            PlaybackMode::Random => out.random += 1,
        }
        out.visibility_bound += usize::from(is_user_binding(object.get("visible")));
        out.volume_bound += usize::from(is_user_binding(object.get("volume")));
        out.start_bound += usize::from(is_user_binding(object.get("startsilent")));
    }
    Ok(out)
}

pub(crate) fn event_driven(value: Option<&Value>) -> bool {
    // A user binding is dynamic even when its authored fallback is false. The
    // native renderer cannot receive Wallpaper Engine sound-start events, so
    // accepting it from a fallback alone would be a false capability claim.
    is_user_binding(value) || truthy(value, &Properties::new(), false)
}

fn is_user_binding(value: Option<&Value>) -> bool {
    value.is_some_and(|value| value.as_object().is_some_and(|object| object.contains_key("user")))
}

fn mode_of(value: Option<&Value>) -> PlaybackMode {
    match value.and_then(Value::as_str) {
        Some(mode) if mode.eq_ignore_ascii_case("single") => PlaybackMode::Once,
        Some(mode) if mode.eq_ignore_ascii_case("random") => PlaybackMode::Random,
        _ => PlaybackMode::Loop,
    }
}

fn clip_paths(value: Option<&Value>) -> impl Iterator<Item = &str> {
    let single = match value {
        Some(Value::String(path)) if !path.is_empty() => Some(path.as_str()),
        _ => None,
    };
    let many = match value {
        Some(Value::Array(paths)) => Some(paths.as_slice()),
        _ => None,
    };
    single
        .into_iter()
        .chain(many.into_iter().flatten().filter_map(Value::as_str).filter(|path| !path.is_empty()))
}

fn truthy(value: Option<&Value>, properties: &Properties, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(1.0) != 0.0,
        Some(Value::String(text)) => text != "false" && text != "0",
        Some(value @ Value::Object(_)) => crate::effects::bound_value(value, properties)
            .and_then(|parts| parts.first().copied())
            .is_none_or(|first| first != 0.0),
        _ => fallback,
    }
}

fn number(value: Option<&Value>, properties: &Properties, fallback: f32) -> f32 {
    let Some(value) = value else {
        return fallback;
    };
    crate::effects::bound_value(value, properties)
        .and_then(|parts| parts.first().copied())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests;
