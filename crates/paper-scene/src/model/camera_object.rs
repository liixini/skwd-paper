use super::{
    ANGLES_FIELD, ATTACHMENT_FIELD, CAMERA_CENTER, CAMERA_EYE, CAMERA_FIELD, CAMERA_FOV, CAMERA_UP,
    FOV_RANGE, GENERAL_FIELD, OBJECTS_FIELD, ORIGIN_FIELD, PARENT_FIELD, Properties, SCALE_FIELD,
    UNIT_ZOOM, VISIBLE_FIELD, ZOOM_FIELD, ancestor_chain, id_of, number, truthy, vec3,
};
use crate::matrix::{mat4_mul, model_matrix};
use serde_json::Value;
use std::collections::HashMap;

const ZOOM_GAP: &str = "perspective camera object zoom unsupported";
const PARENT_GAP: &str = "attached perspective camera object unsupported";
const INVALID_GAP: &str = "invalid perspective camera object";
const LIVE_FIELDS: [&str; 5] = [ORIGIN_FIELD, ANGLES_FIELD, SCALE_FIELD, CAMERA_FOV, VISIBLE_FIELD];
const ID_FIELD: &str = "id";
const ANIMATION_KEYS: [&str; 2] = ["animation", "script"];
const UP_COLUMN: usize = 4;
const BACK_COLUMN: usize = 8;
const ORIGIN_COLUMN: usize = 12;
const AXES: usize = 3;

#[path = "camera_path.rs"]
mod path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub zoom: f32,
    pub object_zoom: f32,
    pub origin: [f32; 2],
    pub roll: f32,
}

impl CameraView {
    pub const UNIT: Self =
        Self { zoom: UNIT_ZOOM, object_zoom: UNIT_ZOOM, origin: [0.0; 2], roll: 0.0 };

    pub fn zoomed(zoom: f32) -> Self {
        Self { zoom, ..Self::UNIT }
    }

    pub fn scale(self) -> f32 {
        self.zoom * self.object_zoom
    }

    pub fn identity(self) -> bool {
        self.scale() == UNIT_ZOOM && self.origin == [0.0; 2] && self.roll == 0.0
    }
}

pub struct Placement {
    pub eye: [f32; 3],
    pub center: [f32; 3],
    pub up: [f32; 3],
    pub fov: Option<f32>,
}

#[derive(Clone, Copy)]
struct StoredCamera {
    eye: [f32; AXES],
    center: [f32; AXES],
    up: [f32; AXES],
    fov: Option<f32>,
}

#[derive(Default)]
pub struct CameraSource {
    chains: Vec<Vec<usize>>,
    stored: Option<StoredCamera>,
    script_moved: bool,
    scripted: bool,
    on_object: bool,
    paths: path::CameraPaths,
    path_moved: bool,
}

impl CameraSource {
    pub(super) fn of(
        scene: &Value,
        props: &Properties,
        perspective: bool,
        docs: impl Fn(&str) -> Option<Value>,
    ) -> Self {
        let objects = scene.get(OBJECTS_FIELD).and_then(Value::as_array);
        let by_id = by_id(scene);
        let chains: Vec<_> = objects
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, object)| object.get(CAMERA_FIELD).is_some_and(Value::is_string))
            .filter_map(|(index, object)| {
                let mut chain = vec![index];
                for node in ancestor_chain(object, &by_id).into_iter().skip(1) {
                    chain.push(objects?.iter().position(|other| std::ptr::eq(other, node))?);
                }
                Some(chain)
            })
            .collect();
        let triple = |field: &str| {
            let (x, y, z) = vec3(scene.get(CAMERA_FIELD)?.get(field), props)?;
            Some([x, y, z])
        };
        let stored = (|| {
            Some(StoredCamera {
                eye: triple(CAMERA_EYE)?,
                center: triple(CAMERA_CENTER)?,
                up: triple(CAMERA_UP)?,
                fov: scene
                    .get(GENERAL_FIELD)
                    .and_then(|general| general.get(CAMERA_FOV))
                    .map(|value| number(Some(value), props, f32::NAN))
                    .filter(|fov| (FOV_RANGE.0..=FOV_RANGE.1).contains(fov)),
            })
        })();
        let paths = path::CameraPaths::load(
            objects.map_or(&[], Vec::as_slice),
            &chains,
            props,
            perspective,
            docs,
        );
        Self {
            chains,
            stored,
            script_moved: false,
            scripted: false,
            on_object: active(scene, props).is_some(),
            paths,
            path_moved: false,
        }
    }

    pub fn pathed(&self) -> bool {
        !self.paths.is_empty()
    }

    pub fn paths_live(&self) -> bool {
        self.paths.live()
    }

    pub fn play_paths(&mut self, scene: &mut Value, props: &Properties, dt: f32) -> bool {
        let moved = self.paths.play(scene, props, dt);
        self.path_moved |= moved;
        moved
    }

    pub fn set_script(
        &mut self,
        eye: [f32; AXES],
        center: [f32; AXES],
        up: [f32; AXES],
        scripted: bool,
    ) {
        let fov = self.stored.and_then(|stored| stored.fov);
        self.stored = Some(StoredCamera { eye, center, up, fov });
        self.script_moved = true;
        self.scripted |= scripted;
    }

    pub fn flat_view(&self, view: CameraView) -> CameraView {
        match self.stored.filter(|_| self.scripted && !self.on_object) {
            Some(stored) => CameraView {
                origin: [stored.eye[0], stored.eye[1]],
                roll: (-stored.up[0]).atan2(stored.up[1]),
                ..view
            },
            None => view,
        }
    }

    pub fn next(&mut self, scene: &Value, props: &Properties, moving: bool) -> Option<Placement> {
        let script_moved = std::mem::take(&mut self.script_moved);
        let path_moved = std::mem::take(&mut self.path_moved);
        let objects = scene.get(OBJECTS_FIELD).and_then(Value::as_array)?;
        let active = self.chains.iter().rfind(|chain| chain_visible(objects, chain, props));
        let was_on_object = std::mem::replace(&mut self.on_object, active.is_some());
        if let Some(chain) = active {
            return (moving || path_moved || !was_on_object)
                .then(|| chain_placement(objects, chain, props))
                .flatten();
        }
        (script_moved || was_on_object).then_some(self.stored).flatten().map(|stored| Placement {
            eye: stored.eye,
            center: stored.center,
            up: stored.up,
            fov: stored.fov,
        })
    }
}

fn chain_visible(objects: &[Value], chain: &[usize], props: &Properties) -> bool {
    chain.iter().all(|index| {
        objects.get(*index).is_some_and(|node| truthy(node.get(VISIBLE_FIELD), props, true))
    })
}

fn chain_placement(objects: &[Value], chain: &[usize], props: &Properties) -> Option<Placement> {
    let nodes = chain.iter().filter_map(|index| objects.get(*index));
    world_placement(&objects[chain[0]], nodes, props)
}

fn cameras(scene: &Value) -> impl DoubleEndedIterator<Item = &Value> {
    scene
        .get(OBJECTS_FIELD)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|object| object.get(CAMERA_FIELD).is_some_and(Value::is_string))
}

fn by_id(scene: &Value) -> HashMap<String, &Value> {
    if !cameras(scene).any(|object| object.get(PARENT_FIELD).is_some_and(|value| !value.is_null()))
    {
        return HashMap::new();
    }
    scene
        .get(OBJECTS_FIELD)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|object| Some((id_of(object.get(ID_FIELD)?)?, object)))
        .collect()
}

fn active_in<'a>(
    scene: &'a Value,
    by_id: &HashMap<String, &'a Value>,
    props: &Properties,
) -> Option<&'a Value> {
    cameras(scene).rfind(|object| {
        ancestor_chain(object, by_id)
            .iter()
            .all(|node| truthy(node.get(VISIBLE_FIELD), props, true))
    })
}

fn active<'a>(scene: &'a Value, props: &Properties) -> Option<&'a Value> {
    active_in(scene, &by_id(scene), props)
}

fn node_matrix(node: &Value, props: &Properties) -> crate::matrix::Mat4 {
    let triple = |field: &str, fallback: f32| {
        let (x, y, z) = vec3(node.get(field), props).unwrap_or((fallback, fallback, fallback));
        [x, y, z]
    };
    model_matrix(triple(ORIGIN_FIELD, 0.0), triple(SCALE_FIELD, 1.0), triple(ANGLES_FIELD, 0.0))
}

fn unit(axis: &[f32]) -> Option<[f32; AXES]> {
    let length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    (length.is_finite() && length > f32::EPSILON)
        .then(|| std::array::from_fn(|index| axis[index] / length))
}

pub fn placement(scene: &Value, props: &Properties) -> Option<Placement> {
    let by_id = by_id(scene);
    let object = active_in(scene, &by_id, props)?;
    world_placement(object, ancestor_chain(object, &by_id).into_iter(), props)
}

fn world_placement<'a>(
    object: &Value,
    chain: impl Iterator<Item = &'a Value>,
    props: &Properties,
) -> Option<Placement> {
    let world = chain
        .map(|node| node_matrix(node, props))
        .reduce(|child, parent| mat4_mul(&parent, &child))?;
    if !world.iter().all(|value| value.is_finite()) {
        return None;
    }
    let eye = [world[ORIGIN_COLUMN], world[ORIGIN_COLUMN + 1], world[ORIGIN_COLUMN + 2]];
    let back = unit(&world[BACK_COLUMN..BACK_COLUMN + AXES])?;
    let up = unit(&world[UP_COLUMN..UP_COLUMN + AXES])?;
    let center = [eye[0] - back[0], eye[1] - back[1], eye[2] - back[2]];
    let fov = object
        .get(CAMERA_FOV)
        .map(|value| number(Some(value), props, f32::NAN))
        .filter(|fov| (FOV_RANGE.0..=FOV_RANGE.1).contains(fov));
    Some(Placement { eye, center, up, fov })
}

pub(super) fn adopt(scene: &mut Value, props: &Properties) {
    let Some(placement) = placement(scene, props) else { return };
    let text = |[x, y, z]: [f32; AXES]| Value::from(format!("{x} {y} {z}"));
    if !scene[CAMERA_FIELD].is_object() {
        scene[CAMERA_FIELD] = Value::Object(serde_json::Map::new());
    }
    for (field, value) in
        [(CAMERA_EYE, placement.eye), (CAMERA_CENTER, placement.center), (CAMERA_UP, placement.up)]
    {
        scene[CAMERA_FIELD][field] = text(value);
    }
    if let Some(fov) = placement.fov
        && let Some(general) = scene.get_mut(GENERAL_FIELD).and_then(Value::as_object_mut)
    {
        general.insert(CAMERA_FOV.into(), Value::from(fov));
    }
}

fn moving(value: &Value) -> bool {
    ANIMATION_KEYS.iter().any(|key| value.get(*key).is_some())
}

pub fn cameras_move(scene: &Value) -> bool {
    let by_id = by_id(scene);
    cameras(scene).any(|object| {
        ancestor_chain(object, &by_id)
            .iter()
            .any(|node| LIVE_FIELDS.iter().filter_map(|field| node.get(*field)).any(moving))
    })
}

pub(super) fn perspective_gaps(scene: &Value, props: &Properties) -> Vec<&'static str> {
    let Some(object) = active(scene, props) else {
        return Vec::new();
    };
    let mut gaps = Vec::new();
    if number(object.get(ZOOM_FIELD), props, UNIT_ZOOM) != UNIT_ZOOM
        || object.get(ZOOM_FIELD).is_some_and(moving)
    {
        gaps.push(ZOOM_GAP);
    }
    let by_id = by_id(scene);
    let chain = ancestor_chain(object, &by_id);
    if chain.iter().any(|node| node.get(ATTACHMENT_FIELD).is_some_and(|value| !value.is_null())) {
        gaps.push(PARENT_GAP);
    }
    if chain.last().is_some_and(|root| root.get(PARENT_FIELD).is_some_and(|value| !value.is_null()))
    {
        gaps.push(INVALID_GAP);
    }
    if placement(scene, props).is_none() {
        gaps.push(INVALID_GAP);
    }
    gaps
}

pub fn active_camera_view(scene: &Value, props: &Properties, zoom: f32, flat: bool) -> CameraView {
    let Some(object) = active(scene, props) else {
        return CameraView::zoomed(zoom);
    };
    if !flat {
        return CameraView::zoomed(zoom);
    }
    let object_zoom = Some(number(object.get(ZOOM_FIELD), props, UNIT_ZOOM))
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(UNIT_ZOOM);
    let (x, y, _) = vec3(object.get(ORIGIN_FIELD), props).unwrap_or_default();
    let (.., roll) = vec3(object.get(ANGLES_FIELD), props).unwrap_or_default();
    let finite = [x, y, roll].iter().all(|value| value.is_finite());
    if !finite {
        return CameraView::zoomed(zoom);
    }
    CameraView { zoom, object_zoom, origin: [x, y], roll }
}

#[cfg(test)]
#[path = "camera_object_tests.rs"]
mod tests;
