use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod clock_tests;

mod keys {
    pub(super) const ENABLED: &str = "enabled";
    pub(super) const X: &str = "x";
    pub(super) const Y: &str = "y";
    pub(super) const FRAME: &str = "frame";
    pub(super) const VALUE: &str = "value";
    pub(super) const STEP: &str = "step";
    pub(super) const BACK: &str = "back";
    pub(super) const FRONT: &str = "front";
    pub(super) const ANIMATION: &str = "animation";
    pub(super) const OPTIONS: &str = "options";
    pub(super) const FPS: &str = "fps";
    pub(super) const LENGTH: &str = "length";
    pub(super) const NAME: &str = "name";
    pub(super) const RELATIVE: &str = "relative";
    pub(super) const WRAP_LOOP: &str = "wraploop";
    pub(super) const PARENT: &str = "parent";
    pub(super) const KEY: &str = "key";
    pub(super) const MODE: &str = "mode";
    pub(super) const SINGLE: &str = "single";
    pub(super) const MIRROR: &str = "mirror";
    pub(super) const START_PAUSED: &str = "startpaused";
    pub(super) const SCRIPT: &str = "script";
    pub(super) const USER: &str = "user";
    pub(super) const GENERAL: &str = "general";
    pub(super) const OBJECTS: &str = "objects";
    pub(super) const ID: &str = "id";
    pub(super) const POINTER_TILDE_ESCAPE: &str = "~0";
    pub(super) const POINTER_SLASH_ESCAPE: &str = "~1";
}

const COMPONENTS: usize = 4;
const FRAME_SAMPLE_OFFSET: f32 = 1.0;
const TRACK_LIMIT: usize = 4096;
const KEY_LIMIT: usize = 262_144;
const CURVE_STEPS: usize = 1000;
const CURVE_STEP: f32 = 0.999;
const CURVE_EPSILON: f64 = 0.01;
const CUBIC_DEGREE: f32 = 3.0;

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
pub(crate) enum Mode {
    Single,
    Loop,
    Mirror,
}

#[derive(Debug)]
struct Track {
    name: Option<String>,
    path: Vec<Part>,
    channels: Vec<Vec<Key>>,
    components: usize,
    parent_path: Option<Vec<Part>>,
    parent: Option<usize>,
    base: [f32; COMPONENTS],
    previous: Option<[f32; COMPONENTS]>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Timing {
    pub(crate) fps: f32,
    pub(crate) length: f32,
    pub(crate) mode: Mode,
    pub(crate) wrap: bool,
    pub(crate) paused: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Curve(Vec<Key>);

#[derive(Serialize)]
pub(crate) struct Timeline<'a> {
    pub(crate) path: String,
    pub(crate) name: Option<&'a str>,
    pub(crate) fps: f32,
    pub(crate) length: f32,
    pub(crate) clock: usize,
}

#[derive(Debug)]
pub(crate) struct Clock {
    pub(crate) fps: f32,
    pub(crate) length: f32,
    pub(crate) mode: Mode,
    pub(crate) paused: bool,
    finished: bool,
    reversed: bool,
    seconds: f32,
    since: f32,
    rate: f32,
}

#[derive(Clone, Copy)]
pub(crate) enum Control {
    Play,
    Pause,
    Stop,
    GetFrame,
    SetFrame,
    GetRate,
    SetRate,
    IsPlaying,
}

impl Control {
    pub(crate) const ALL: [Self; 8] = [
        Self::Play,
        Self::Pause,
        Self::Stop,
        Self::SetFrame,
        Self::SetRate,
        Self::GetFrame,
        Self::GetRate,
        Self::IsPlaying,
    ];
}

#[derive(Default, Debug)]
pub(crate) struct Clocks {
    pub(crate) entries: Vec<Clock>,
    pub(crate) time: f32,
    pending: bool,
}

#[derive(Default, Debug)]
pub struct Bindings {
    tracks: Vec<Track>,
    pub(crate) clocks: Rc<RefCell<Clocks>>,
}

fn number(value: &Value) -> Option<f32> {
    let value = value.as_f64()? as f32;
    value.is_finite().then_some(value)
}

fn handle(value: &Value) -> [f32; 2] {
    if !value.is_object() || value[keys::ENABLED] == false {
        return [0.0; 2];
    }
    [number(&value[keys::X]).unwrap_or(0.0), number(&value[keys::Y]).unwrap_or(0.0)]
}

fn channel(value: &Value, offset: f32, wrap: Option<f32>) -> Vec<Key> {
    let mut keys = Vec::<Key>::new();
    for raw in value.as_array().into_iter().flatten() {
        let (Some(frame), Some(value)) = (number(&raw[keys::FRAME]), number(&raw[keys::VALUE]))
        else {
            continue;
        };
        if frame < 0.0 || keys.last().is_some_and(|key| key.frame >= frame) {
            continue;
        }
        let step = raw[keys::STEP] == true;
        keys.push(Key {
            frame,
            value: value + offset,
            back: if step { [0.0; 2] } else { handle(&raw[keys::BACK]) },
            front: if step { [0.0; 2] } else { handle(&raw[keys::FRONT]) },
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

fn blended(keys: &[Key], frame: f32, blend: f32) -> Option<f32> {
    let a = sample(keys, frame)?;
    let b = sample(keys, frame + FRAME_SAMPLE_OFFSET).unwrap_or(a);
    Some(a * (1.0 - blend) + b * blend)
}

impl Curve {
    pub(crate) fn parse(value: &Value, timing: Timing) -> Self {
        Self(channel(value, 0.0, timing.wrap.then_some(timing.length)))
    }

    pub(crate) fn at(&self, frame: f32, blend: f32) -> Option<f32> {
        blended(&self.0, frame, blend)
    }
}

impl Timing {
    pub(crate) fn parse(options: &Value) -> Option<Self> {
        let fps = number(&options[keys::FPS])?;
        let length = number(&options[keys::LENGTH])?;
        if fps <= 0.0 || length <= 0.0 || !fps.recip().is_finite() || !(length / fps).is_finite() {
            return None;
        }
        Some(Self {
            fps,
            length,
            mode: match options[keys::MODE].as_str() {
                Some(keys::SINGLE) => Mode::Single,
                Some(keys::MIRROR) => Mode::Mirror,
                _ => Mode::Loop,
            },
            wrap: options[keys::WRAP_LOOP] == true,
            paused: options[keys::START_PAUSED] == true,
        })
    }

    pub(crate) fn duration(self) -> f32 {
        self.length / self.fps
    }

    pub(crate) fn frame(self, seconds: f32) -> (f32, f32) {
        frame_at(seconds, self.fps, self.length)
    }
}

pub(crate) fn wrapped(mode: Mode, seconds: f32, reversed: bool, duration: f32) -> (f32, bool) {
    match mode {
        Mode::Loop => {
            let seconds =
                if seconds < 0.0 { libm::fmodf(seconds + duration, duration) } else { seconds };
            let seconds =
                if seconds >= duration { libm::fmodf(seconds, duration) } else { seconds };
            (seconds, reversed)
        }
        Mode::Mirror if !reversed && seconds >= duration => {
            (duration - libm::fmodf(seconds, duration), true)
        }
        Mode::Mirror if reversed && seconds <= 0.0 => (-libm::fmodf(seconds, duration), false),
        Mode::Single | Mode::Mirror => (seconds, reversed),
    }
}

fn frame_at(seconds: f32, fps: f32, length: f32) -> (f32, f32) {
    let period = fps.recip();
    (
        (seconds / period).trunc().clamp(0.0, (length - 1.0).max(0.0)),
        libm::fmodf(seconds, period) / period,
    )
}

fn cubic(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    let front = u * CUBIC_DEGREE * u * t;
    let back = u * CUBIC_DEGREE * t * t;
    b * front + u * u * u * a + c * back + t * t * t * d
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
    let mut step = CURVE_STEP;
    let mut t = ((frame - a.frame) / (b.frame - a.frame)).trunc();
    for _ in 0..CURVE_STEPS {
        let x = cubic(a.frame, a.frame + span * a.front[0], b.frame + span * b.back[0], b.frame, t);
        if f64::from((x - frame).abs()) < CURVE_EPSILON {
            break;
        }
        step *= 0.5;
        t += if x <= frame { step } else { -step };
    }
    Some(cubic(a.value, a.value + a.front[1], b.value + b.back[1], b.value, t.clamp(0.0, 1.0)))
}

impl Track {
    fn parse(raw: &Value, path: &[Part]) -> Option<(Self, Clock)> {
        let animation = raw.get(keys::ANIMATION)?;
        let options = animation.get(keys::OPTIONS)?;
        let timing = Timing::parse(options)?;
        let initial = crate::effects::json_numbers(&raw[keys::VALUE])?;
        let components = initial.len().min(COMPONENTS);
        let mut base = [0.0; COMPONENTS];
        for (out, value) in base.iter_mut().zip(initial) {
            *out = value;
        }
        let relative = animation[keys::RELATIVE] == true;
        let wrap = timing.wrap.then_some(timing.length);
        let channels: Vec<_> = (0..COMPONENTS)
            .map_while(|i| {
                animation
                    .get(format!("c{i}"))
                    .map(|v| channel(v, if relative { base[i] } else { 0.0 }, wrap))
            })
            .collect();
        if channels.is_empty() || channels.iter().all(Vec::is_empty) {
            return None;
        }
        let track = Self {
            name: options[keys::NAME].as_str().map(str::to_owned),
            components: components.max(channels.len()),
            parent_path: options[keys::PARENT][keys::KEY].as_str().map(|key| {
                let mut parent = path.to_vec();
                parent.pop();
                parent.push(Part::Key(key.to_owned()));
                parent
            }),
            parent: None,
            path: path.to_vec(),
            channels,
            base,
            previous: None,
        };
        let clock = Clock {
            fps: timing.fps,
            length: timing.length,
            mode: timing.mode,
            paused: timing.paused,
            finished: false,
            reversed: false,
            seconds: 0.0,
            since: 0.0,
            rate: 1.0,
        };
        Some((track, clock))
    }

    fn apply(&mut self, scene: &mut Value, frame: f32, blend: f32) -> bool {
        if !frame.is_finite() || !blend.is_finite() {
            return false;
        }
        let mut values = self.base;
        for (value, keys) in values.iter_mut().zip(&self.channels) {
            if let Some(sampled) = blended(keys, frame, blend) {
                *value = sampled;
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
        if target.is_object() && target.get(keys::VALUE).is_some() {
            target = target.get_mut(keys::VALUE).unwrap();
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

impl Clock {
    fn seconds(&self, time: f32) -> f32 {
        self.seconds
            + if self.paused || self.finished {
                0.0
            } else {
                (time - self.since).max(0.0) * if self.reversed { -self.rate } else { self.rate }
            }
    }

    fn period(&self) -> f32 {
        1.0 / self.fps
    }

    fn position(&self, time: f32) -> f32 {
        self.seconds(time) / self.period()
    }

    fn advance(&mut self, time: f32) {
        let duration = self.length / self.fps;
        if duration <= 0.0 {
            self.since = time;
            return;
        }
        (self.seconds, self.reversed) =
            wrapped(self.mode, self.seconds(time), self.reversed, duration);
        self.since = time;
    }

    fn sample(&self, time: f32) -> (f32, f32) {
        frame_at(self.seconds(time), self.fps, self.length)
    }
}

impl Clocks {
    fn advance(&mut self, time: f32) {
        self.time = time;
        self.pending = false;
        for clock in &mut self.entries {
            if clock.paused || clock.finished {
                continue;
            }
            if clock.mode != Mode::Single {
                clock.advance(time);
                continue;
            }
            if clock.rate == 0.0 {
                continue;
            }
            let duration = clock.length / clock.fps;
            if clock.seconds >= duration {
                clock.since = time;
                continue;
            }
            let seconds = clock.seconds(time);
            if seconds >= duration {
                clock.seconds = duration;
                clock.since = time;
                clock.finished = true;
            }
        }
    }

    pub(crate) fn control(&mut self, index: usize, op: Control, value: f32) -> Option<f32> {
        let clock = self.entries.get_mut(index)?;
        match op {
            Control::GetFrame => return Some(clock.position(self.time)),
            Control::GetRate => return Some(clock.rate),
            Control::IsPlaying => return Some(f32::from(!clock.paused && !clock.finished)),
            Control::SetFrame | Control::SetRate if !value.is_finite() => return None,
            Control::SetFrame if !(value * clock.period()).is_finite() => return None,
            _ => {}
        }
        self.pending = true;
        clock.seconds = clock.seconds(self.time);
        clock.since = self.time;
        match op {
            Control::Play => {
                if clock.finished {
                    clock.seconds = 0.0;
                }
                clock.paused = false;
                clock.finished = false;
            }
            Control::Pause => clock.paused = true,
            Control::Stop => {
                clock.paused = true;
                clock.finished = false;
                clock.reversed = false;
                clock.seconds = 0.0;
            }
            Control::SetFrame => clock.seconds = value * clock.period(),
            Control::SetRate => clock.rate = value,
            Control::GetFrame | Control::GetRate | Control::IsPlaying => unreachable!(),
        }
        Some(clock.position(self.time))
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
        if let Some((track, clock)) = Track::parse(value, path) {
            *keys += track.channels.iter().map(Vec::len).sum::<usize>();
            if self.tracks.len() >= TRACK_LIMIT || *keys > KEY_LIMIT {
                return Err(anyhow!("property keyframes exceed the scene budget"));
            }
            self.tracks.push(track);
            self.clocks.borrow_mut().entries.push(clock);
            value.as_object_mut().unwrap().remove(keys::ANIMATION);
            if value.get(keys::SCRIPT).is_none() && value.get(keys::USER).is_none() {
                *value = value[keys::VALUE].take();
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

    pub(crate) fn descriptors(&self) -> Vec<Timeline<'_>> {
        let clocks = self.clocks.borrow();
        self.tracks
            .iter()
            .enumerate()
            .map(|(index, track)| Timeline {
                path: track
                    .path
                    .iter()
                    .map(|part| match part {
                        Part::Key(key) => format!(
                            "/{}",
                            key.replace('~', keys::POINTER_TILDE_ESCAPE)
                                .replace('/', keys::POINTER_SLASH_ESCAPE)
                        ),
                        Part::Index(index) => format!("/{index}"),
                    })
                    .collect(),
                name: track.name.as_deref(),
                fps: clocks.entries[index].fps,
                length: clocks.entries[index].length,
                clock: self.clock(index),
            })
            .collect()
    }

    pub fn values(&self) -> impl Iterator<Item = (usize, &[f32; COMPONENTS], usize)> {
        self.tracks
            .iter()
            .enumerate()
            .filter_map(|(index, track)| Some((index, track.previous.as_ref()?, track.components)))
    }

    pub fn general_keys(&self) -> impl Iterator<Item = &str> {
        self.tracks.iter().filter_map(|track| match track.path.as_slice() {
            [Part::Key(root), Part::Key(key)] if root == keys::GENERAL => Some(key.as_str()),
            _ => None,
        })
    }

    pub fn affects_layer(&self, scene: &Value, id: &str) -> bool {
        let Some(objects) = scene[keys::OBJECTS].as_array() else { return false };
        let mut id = id.to_owned();
        for _ in 0..objects.len() {
            let Some((index, object)) = objects.iter().enumerate().find(|(index, object)| {
                object
                    .get(keys::ID)
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
                [Part::Key(root), Part::Index(i), ..] if root == keys::OBJECTS && *i == index)
            }) {
                return true;
            }
            let Some(parent) = object.get(keys::PARENT) else { return false };
            id = parent.as_str().map_or_else(|| parent.to_string(), str::to_owned);
        }
        false
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn animated(&self) -> bool {
        let clocks = self.clocks.borrow();
        clocks.pending
            || self.tracks.iter().enumerate().any(|(i, _)| {
                let clock = &clocks.entries[self.clock(i)];
                let wraps = clock.mode == Mode::Loop
                    && clock.seconds < 0.0
                    && clock.length / clock.fps > 0.0;
                !clock.paused
                    && !clock.finished
                    && (clock.rate != 0.0 || wraps)
                    && (clock.mode != Mode::Single || clock.seconds < clock.length / clock.fps)
            })
    }

    pub fn apply(&mut self, scene: &mut Value, time: f32) -> bool {
        if !time.is_finite() {
            return false;
        }
        let mut clocks = self.clocks.borrow_mut();
        clocks.advance(time);
        let mut changed = false;
        for index in 0..self.tracks.len() {
            let (frame, blend) = clocks.entries[self.clock(index)].sample(time);
            changed |= self.tracks[index].apply(scene, frame, blend);
        }
        changed
    }
}
