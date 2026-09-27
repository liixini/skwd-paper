use anyhow::{Result, anyhow};
use serde_json::Value;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Key(String),
    Index(usize),
}

#[derive(Clone, Copy, Debug)]
struct Key {
    frame: f32,
    value: f32,
    back: [f32; 2],
    front: [f32; 2],
    step: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Single,
    Loop,
    Mirror,
}

#[derive(Debug)]
struct Track {
    path: Vec<Part>,
    channels: Vec<Vec<Key>>,
    components: usize,
    parent_path: Option<Vec<Part>>,
    parent: Option<usize>,
    base: [f32; 4],
    fps: f32,
    length: f32,
    mode: Mode,
    paused: bool,
    previous: Option<[f32; 4]>,
}

#[derive(Default, Debug)]
pub struct Bindings {
    tracks: Vec<Track>,
    time: f32,
}

fn number(value: &Value) -> Option<f32> {
    let value = value.as_f64()? as f32;
    value.is_finite().then_some(value)
}

fn handle(value: &Value) -> [f32; 2] {
    if !value.is_object() || value["enabled"] == false {
        return [0.0; 2];
    }
    [number(&value["x"]).unwrap_or(0.0), number(&value["y"]).unwrap_or(0.0)]
}

fn channel(value: &Value, offset: f32, wrap: Option<f32>) -> Vec<Key> {
    let mut keys = Vec::<Key>::new();
    for raw in value.as_array().into_iter().flatten() {
        let (Some(frame), Some(value)) = (number(&raw["frame"]), number(&raw["value"])) else {
            continue;
        };
        if frame < 0.0 || keys.last().is_some_and(|key| key.frame >= frame) {
            continue;
        }
        let step = raw["step"] == true;
        keys.push(Key {
            frame,
            value: value + offset,
            back: if step { [0.0; 2] } else { handle(&raw["back"]) },
            front: if step { [0.0; 2] } else { handle(&raw["front"]) },
            step,
        });
    }
    if let Some(length) = wrap.filter(|_| keys.len() >= 2) {
        while keys.len() > 1 && keys.last().is_some_and(|key| key.frame > length) {
            keys.pop();
        }
        if keys.len() >= 2 {
            let first = keys[0];
            if keys.last().is_some_and(|key| key.frame != length) {
                keys.push(Key { frame: length, ..first });
            }
            let last = keys.last_mut().unwrap();
            last.value = first.value;
            last.back = [-first.front[0], -first.front[1]];
        }
    }
    keys
}

fn cubic(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
}

fn sample(keys: &[Key], frame: f32) -> Option<f32> {
    let first = keys.first()?;
    if frame <= first.frame {
        return Some(first.value);
    }
    let index = keys.partition_point(|key| key.frame <= frame);
    let a = keys[index - 1];
    let Some(b) = keys.get(index) else { return Some(a.value) };
    if b.step || frame == a.frame {
        return Some(a.value);
    }
    let span = (b.frame - a.frame) * 0.5;
    let (mut lo, mut hi) = (0.0, 1.0);
    let mut t = 0.5;
    for _ in 0..24 {
        let x = cubic(a.frame, a.frame + span * a.front[0], b.frame + span * b.back[0], b.frame, t);
        if (x - frame).abs() < 0.001 {
            break;
        }
        if x < frame {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) * 0.5;
    }
    Some(cubic(a.value, a.value + a.front[1], b.value + b.back[1], b.value, t))
}

impl Track {
    fn parse(raw: &Value, path: &[Part]) -> Option<Self> {
        let animation = raw.get("animation")?;
        let options = animation.get("options")?;
        let fps = number(&options["fps"])?;
        let length = number(&options["length"])?;
        if fps <= 0.0 || length <= 0.0 {
            return None;
        }
        let initial = crate::effects::json_numbers(&raw["value"])?;
        let components = initial.len().min(4);
        let mut base = [0.0; 4];
        for (out, value) in base.iter_mut().zip(initial) {
            *out = value;
        }
        let relative = animation["relative"] == true;
        let wrap = (options["wraploop"] == true).then_some(length);
        let channels: Vec<_> = (0..4)
            .map_while(|i| {
                animation
                    .get(format!("c{i}"))
                    .map(|v| channel(v, if relative { base[i] } else { 0.0 }, wrap))
            })
            .collect();
        if channels.is_empty() || channels.iter().all(Vec::is_empty) {
            return None;
        }
        Some(Self {
            components: components.max(channels.len()),
            parent_path: options["parent"]["key"].as_str().map(|key| {
                let mut parent = path.to_vec();
                parent.pop();
                parent.push(Part::Key(key.to_owned()));
                parent
            }),
            parent: None,
            path: path.to_vec(),
            channels,
            base,
            fps,
            length,
            mode: match options["mode"].as_str() {
                Some("single") => Mode::Single,
                Some("mirror") => Mode::Mirror,
                _ => Mode::Loop,
            },
            paused: options["startpaused"] == true,
            previous: None,
        })
    }

    fn frame(&self, time: f32) -> f32 {
        let frame = if self.paused { 0.0 } else { time.max(0.0) * self.fps };
        match self.mode {
            Mode::Single => frame.min(self.length),
            Mode::Loop => frame.rem_euclid(self.length),
            Mode::Mirror => self.length - (frame.rem_euclid(self.length * 2.0) - self.length).abs(),
        }
    }

    fn apply(&mut self, scene: &mut Value, frame: f32) -> bool {
        let mut values = self.base;
        for (value, keys) in values.iter_mut().zip(&self.channels) {
            if let Some(a) = sample(keys, frame.floor()) {
                let b = sample(keys, frame.ceil()).unwrap_or(a);
                *value = a + (b - a) * frame.fract();
            }
        }
        if values.iter().any(|v| !v.is_finite()) {
            return false;
        }
        let mut target = scene;
        for part in &self.path {
            let next = match part {
                Part::Key(key) => target.get_mut(key),
                Part::Index(index) => target.get_mut(*index),
            };
            let Some(next) = next else { return false };
            target = next;
        }
        if target.is_object() && target.get("value").is_some() {
            target = target.get_mut("value").unwrap();
        }
        let changed = self.previous != Some(values);
        self.previous = Some(values);
        if self.components == 1 {
            *target = Value::from(values[0]);
        } else {
            if !target.is_array() {
                *target = Value::Array(vec![Value::Null; self.components]);
            }
            if let Some(array) = target.as_array_mut() {
                array.resize(self.components, Value::Null);
                for (target, value) in array.iter_mut().zip(values) {
                    *target = Value::from(value);
                }
            }
        }
        changed
    }
}

impl Bindings {
    pub fn parse(scene: &mut Value) -> Result<Self> {
        let mut bindings = Self::default();
        bindings.collect(scene, &mut Vec::new(), &mut 0)?;
        for index in 0..bindings.tracks.len() {
            bindings.tracks[index].parent = bindings.tracks[index]
                .parent_path
                .as_ref()
                .and_then(|path| bindings.tracks.iter().position(|t| &t.path == path));
        }
        for index in 0..bindings.tracks.len() {
            let mut root = index;
            let mut remaining = bindings.tracks.len();
            while let Some(parent) = bindings.tracks[root].parent {
                if remaining == 0 {
                    return Err(anyhow!("property keyframe clocks contain a parent cycle"));
                }
                remaining -= 1;
                root = parent;
            }
            bindings.tracks[index].parent = (root != index).then_some(root);
        }
        Ok(bindings)
    }

    fn collect(&mut self, value: &mut Value, path: &mut Vec<Part>, keys: &mut usize) -> Result<()> {
        if let Some(track) = Track::parse(value, path) {
            *keys += track.channels.iter().map(Vec::len).sum::<usize>();
            if self.tracks.len() >= 4096 || *keys > 262_144 {
                return Err(anyhow!("property keyframes exceed the scene budget"));
            }
            self.tracks.push(track);
            value.as_object_mut().unwrap().remove("animation");
            if value.get("script").is_none() && value.get("user").is_none() {
                *value = value["value"].take();
            }
            return Ok(());
        }
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    path.push(Part::Key(key.clone()));
                    self.collect(child, path, keys)?;
                    path.pop();
                }
            }
            Value::Array(array) => {
                for (index, child) in array.iter_mut().enumerate() {
                    path.push(Part::Index(index));
                    self.collect(child, path, keys)?;
                    path.pop();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn clock(&self, index: usize) -> usize {
        self.tracks[index].parent.unwrap_or(index)
    }

    pub fn paths(&self) -> Vec<String> {
        self.tracks
            .iter()
            .map(|track| {
                track
                    .path
                    .iter()
                    .map(|part| match part {
                        Part::Key(key) => format!("/{}", key.replace('~', "~0").replace('/', "~1")),
                        Part::Index(index) => format!("/{index}"),
                    })
                    .collect()
            })
            .collect()
    }

    pub fn values(&self) -> impl Iterator<Item = (usize, &[f32; 4], usize)> {
        self.tracks
            .iter()
            .enumerate()
            .filter_map(|(index, track)| Some((index, track.previous.as_ref()?, track.components)))
    }

    pub fn general_keys(&self) -> impl Iterator<Item = &str> {
        self.tracks.iter().filter_map(|track| match track.path.as_slice() {
            [Part::Key(root), Part::Key(key)] if root == "general" => Some(key.as_str()),
            _ => None,
        })
    }

    pub fn affects_layer(&self, scene: &Value, id: &str) -> bool {
        let Some(objects) = scene["objects"].as_array() else { return false };
        let mut id = id.to_owned();
        for _ in 0..objects.len() {
            let Some((index, object)) = objects.iter().enumerate().find(|(index, object)| {
                object
                    .get("id")
                    .and_then(|id| match id {
                        Value::String(id) => Some(id.clone()),
                        Value::Number(id) => Some(id.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("@object-{index}"))
                    == id
            }) else {
                return false;
            };
            if self.tracks.iter().any(|track| {
                matches!(track.path.as_slice(),
                [Part::Key(root), Part::Index(i), ..] if root == "objects" && *i == index)
            }) {
                return true;
            }
            let Some(parent) = object.get("parent") else { return false };
            id = parent.as_str().map_or_else(|| parent.to_string(), str::to_owned);
        }
        false
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn animated(&self) -> bool {
        self.tracks.iter().enumerate().any(|(i, _)| {
            let t = &self.tracks[self.clock(i)];
            !t.paused && (t.mode != Mode::Single || self.time * t.fps < t.length)
        })
    }

    pub fn apply(&mut self, scene: &mut Value, time: f32) -> bool {
        if !time.is_finite() {
            return false;
        }
        self.time = time;
        let mut changed = false;
        for index in 0..self.tracks.len() {
            let frame = self.tracks[self.clock(index)].frame(time);
            changed |= self.tracks[index].apply(scene, frame);
        }
        changed
    }
}
