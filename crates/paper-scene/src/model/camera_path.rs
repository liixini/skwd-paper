use super::{AXES, BACK_COLUMN, ORIGIN_COLUMN, UP_COLUMN, chain_visible, node_matrix, unit};
use crate::keyframe::{Curve, Mode, Timing, wrapped};
use crate::matrix::cross3;
use crate::model::{
    ANGLES_FIELD, ANIMATION_VALUE_FIELD, CAMERA_CENTER, CAMERA_EYE, CAMERA_FOV, CAMERA_UP,
    DEFAULT_CAMERA_FOV, OBJECTS_FIELD, ORIGIN_FIELD, Properties, UNIT_ZOOM, USER_FIELD,
    VISIBLE_FIELD, ZOOM_FIELD, truthy,
};
use crate::particles::Rng;
use serde_json::Value;

const PATH_FIELD: &str = "path";
const PATHS_FIELD: &str = "paths";
const OPTIONS_FIELD: &str = "options";
const QUEUE_FIELD: &str = "queuemode";
const SEQUENTIAL: &str = "sequential";
const CHANNELS: [&str; AXES] = ["c0", "c1", "c2"];
const LOOK_DISTANCE: f32 = 5.0;
const QUEUE_SEED: u32 = 1;

struct Path {
    shown: Option<Value>,
    timing: Timing,
    seconds: f32,
    reversed: bool,
    finished: bool,
    eye: [Curve; AXES],
    center: [Curve; AXES],
    up: [Curve; AXES],
    fov: Curve,
    zoom: Curve,
}

#[derive(Clone, Copy, PartialEq)]
struct Pose {
    eye: [f32; AXES],
    angles: [f32; AXES],
    lens: f32,
}

struct Queue {
    chain: Vec<usize>,
    paths: Vec<Path>,
    enabled: Vec<usize>,
    sequential: bool,
    current: Option<usize>,
    bag: Vec<usize>,
    last: Option<usize>,
    pose: Option<Pose>,
}

pub struct CameraPaths {
    queues: Vec<Queue>,
    rng: Rng,
    perspective: bool,
    live: bool,
}

impl Default for CameraPaths {
    fn default() -> Self {
        Self { queues: Vec::new(), rng: Rng::new(QUEUE_SEED), perspective: true, live: false }
    }
}

impl Path {
    fn parse(raw: &Value) -> Option<Self> {
        let timing = Timing::parse(raw.get(OPTIONS_FIELD).filter(|options| options.is_object())?)?;
        let triple =
            |field: &str| CHANNELS.map(|channel| Curve::parse(&raw[field][channel], timing));
        Some(Self {
            shown: raw.get(VISIBLE_FIELD).cloned(),
            timing,
            seconds: 0.0,
            reversed: false,
            finished: false,
            eye: triple(CAMERA_EYE),
            center: triple(CAMERA_CENTER),
            up: triple(CAMERA_UP),
            fov: Curve::parse(&raw[CAMERA_FOV], timing),
            zoom: Curve::parse(&raw[ZOOM_FIELD], timing),
        })
    }

    fn visible(&self, props: &Properties) -> bool {
        truthy(self.shown.as_ref(), props, true)
    }

    fn step(&mut self, dt: f32) {
        let duration = self.timing.duration();
        if self.finished || self.timing.paused || duration <= 0.0 {
            return;
        }
        if self.timing.mode == Mode::Single {
            self.seconds += dt;
            if self.seconds >= duration {
                self.seconds = duration;
                self.finished = true;
            }
            return;
        }
        self.seconds += if self.reversed { -dt } else { dt };
        (self.seconds, self.reversed) =
            wrapped(self.timing.mode, self.seconds, self.reversed, duration);
    }
}

impl Queue {
    fn pick(&mut self, rng: &mut Rng, props: &Properties) -> Option<usize> {
        if let Some(index) = self.current {
            let path = &mut self.paths[index];
            if !path.finished {
                return Some(index);
            }
            path.finished = false;
            path.seconds = 0.0;
        }
        self.current = if self.sequential { self.next_visible(props) } else { self.draw(rng) };
        self.current
    }

    fn next_visible(&self, props: &Properties) -> Option<usize> {
        let count = self.paths.len();
        let start = self.current.map_or(0, |index| index + 1);
        (0..count)
            .map(|step| (start + step) % count)
            .find(|&index| self.paths[index].visible(props))
    }

    fn draw(&mut self, rng: &mut Rng) -> Option<usize> {
        if self.bag.is_empty() {
            self.bag.clone_from(&self.enabled);
        }
        let count = self.bag.len();
        let start = rng.index(u32::try_from(count.checked_sub(1)?).ok()?);
        let slot = (0..count)
            .map(|step| (start + step) % count)
            .find(|&slot| self.last != Some(self.bag[slot]))
            .unwrap_or(start);
        let chosen = self.bag.remove(slot);
        self.last = Some(chosen);
        Some(chosen)
    }
}

impl CameraPaths {
    pub(super) fn load(
        objects: &[Value],
        chains: &[Vec<usize>],
        props: &Properties,
        perspective: bool,
        docs: impl Fn(&str) -> Option<Value>,
    ) -> Self {
        let queues: Vec<_> = chains
            .iter()
            .filter_map(|chain| {
                let camera = objects.get(*chain.first()?)?;
                let doc = docs(camera.get(PATH_FIELD)?.as_str()?)?;
                let paths: Vec<_> =
                    doc.get(PATHS_FIELD)?.as_array()?.iter().filter_map(Path::parse).collect();
                let enabled: Vec<_> =
                    (0..paths.len()).filter(|&index| paths[index].visible(props)).collect();
                (!enabled.is_empty()).then(|| Queue {
                    chain: chain.clone(),
                    paths,
                    enabled,
                    sequential: crate::text::text_value(camera.get(QUEUE_FIELD))
                        .is_some_and(|mode| mode == SEQUENTIAL),
                    current: None,
                    bag: Vec::new(),
                    last: None,
                    pose: None,
                })
            })
            .collect();
        Self { live: !queues.is_empty(), queues, perspective, ..Self::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.queues.is_empty()
    }

    pub fn live(&self) -> bool {
        self.live
    }

    pub(super) fn play(&mut self, scene: &mut Value, props: &Properties, dt: f32) -> bool {
        let Some(objects) = scene.get_mut(OBJECTS_FIELD).and_then(Value::as_array_mut) else {
            return false;
        };
        let mut moved = false;
        self.live = false;
        for queue in &mut self.queues {
            if !chain_visible(objects, &queue.chain, props) {
                continue;
            }
            let Some(index) = queue.pick(&mut self.rng, props) else { continue };
            let path = &mut queue.paths[index];
            path.step(dt);
            self.live |= !path.timing.paused;
            let Some(camera) = objects.get_mut(queue.chain[0]) else { continue };
            let Some(pose) = pose(camera, path, props, self.perspective) else { continue };
            moved |= queue.pose.replace(pose) != Some(pose);
        }
        moved
    }
}

fn sampled(
    curves: &[Curve; AXES],
    (frame, blend): (f32, f32),
    fallback: impl FnOnce() -> Option<[f32; AXES]>,
) -> Option<[f32; AXES]> {
    let values = curves.each_ref().map(|curve| curve.at(frame, blend));
    if let [Some(x), Some(y), Some(z)] = values {
        return Some([x, y, z]);
    }
    let fallback = fallback()?;
    Some(std::array::from_fn(|axis| values[axis].unwrap_or(fallback[axis])))
}

fn pose(camera: &mut Value, path: &Path, props: &Properties, perspective: bool) -> Option<Pose> {
    let at = path.timing.frame(path.seconds);
    let local = node_matrix(camera, props);
    let origin: [f32; AXES] = std::array::from_fn(|axis| local[ORIGIN_COLUMN + axis]);
    let eye = sampled(&path.eye, at, || Some(origin))?;
    let center = sampled(&path.center, at, || {
        let back = unit(&local[BACK_COLUMN..BACK_COLUMN + AXES])?;
        Some(std::array::from_fn(|axis| origin[axis] - back[axis] * LOOK_DISTANCE))
    })?;
    let up = sampled(&path.up, at, || unit(&local[UP_COLUMN..UP_COLUMN + AXES]))?;
    let angles = look_angles(eye, center, up)?;
    let (lens, field, fallback) = if perspective {
        (&path.fov, CAMERA_FOV, DEFAULT_CAMERA_FOV)
    } else {
        (&path.zoom, ZOOM_FIELD, UNIT_ZOOM)
    };
    let lens = lens.at(at.0, at.1).unwrap_or(fallback);
    store(camera, ORIGIN_FIELD, Value::from(eye.to_vec()));
    store(camera, ANGLES_FIELD, Value::from(angles.to_vec()));
    store(camera, field, Value::from(lens));
    Some(Pose { eye, angles, lens })
}

fn look_angles(eye: [f32; AXES], center: [f32; AXES], up: [f32; AXES]) -> Option<[f32; AXES]> {
    let forward = unit(&std::array::from_fn::<f32, AXES, _>(|axis| center[axis] - eye[axis]))?;
    let right = unit(&cross3(forward, up))?;
    let lifted = cross3(right, forward);
    let back = forward.map(|value| -value);
    let z = right[1].atan2(right[0]);
    let y = (-right[2]).atan2(lifted[2].hypot(back[2]));
    let (sz, cz) = z.sin_cos();
    let x = (back[0] * sz - back[1] * cz).atan2(lifted[1] * cz - lifted[0] * sz);
    Some([x, y, z])
}

fn store(object: &mut Value, field: &str, value: Value) {
    let slot = &mut object[field];
    match slot.as_object_mut() {
        Some(bound) if bound.contains_key(ANIMATION_VALUE_FIELD) => {
            bound.remove(USER_FIELD);
            bound.insert(ANIMATION_VALUE_FIELD.into(), value);
        }
        _ => *slot = value,
    }
}

#[cfg(test)]
#[path = "camera_path_tests.rs"]
mod tests;
