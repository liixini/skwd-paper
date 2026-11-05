use super::*;
use crate::matrix::model_matrix;
use serde_json::json;
use std::f32::consts::FRAC_PI_2;

fn keys(from: f32, to: f32) -> Value {
    json!([{"frame":0,"value":from},{"frame":10,"value":to}])
}

fn path(eye_x: f32, mode: &str) -> Value {
    json!({
        "options":{"fps":10,"length":10,"mode":mode},
        "eye":{"c0":keys(eye_x, eye_x + 1.0),"c1":keys(0.0, 0.0),"c2":keys(5.0, 5.0)},
        "center":{"c0":keys(eye_x - 5.0, eye_x - 4.0),"c1":keys(0.0, 0.0),"c2":keys(5.0, 5.0)},
        "up":{"c0":keys(0.0, 0.0),"c1":keys(1.0, 1.0),"c2":keys(0.0, 0.0)},
        "fov":keys(40.0, 60.0),
        "zoom":keys(1.0, 3.0),
        "visible":true
    })
}

fn single(eye_x: f32) -> Value {
    path(eye_x, "single")
}

fn load(
    camera: &Value,
    doc: &Value,
    props: &Properties,
    perspective: bool,
) -> (Value, CameraPaths) {
    let scene = json!({ "objects": [camera] });
    let objects = scene[OBJECTS_FIELD].as_array().unwrap().clone();
    let loaded = CameraPaths::load(&objects, &[vec![0]], props, perspective, |name| {
        (name == "p.json").then(|| doc.clone())
    });
    (scene, loaded)
}

fn paths(queue: &str, doc: &Value) -> (Value, CameraPaths) {
    let camera = json!({"camera":"default","path":"p.json","queuemode":queue,
        "origin":"0 0 0","angles":"0 0 0"});
    load(&camera, doc, &Properties::new(), true)
}

fn camera(scene: &Value) -> &Value {
    &scene[OBJECTS_FIELD][0]
}

fn eye_x(scene: &Value) -> f32 {
    camera(scene)[ORIGIN_FIELD][0].as_f64().unwrap() as f32
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 2e-3
}

#[test]
fn look_angles_match() {
    let eye = [1.0, 2.0, 3.0];
    let center = [-2.0, 0.5, -1.0];
    let angles = look_angles(eye, center, [0.0, 1.0, 0.0]).unwrap();
    let matrix = model_matrix(eye, [1.0; AXES], angles);
    let back = unit(&std::array::from_fn::<f32, AXES, _>(|axis| eye[axis] - center[axis])).unwrap();
    for axis in 0..AXES {
        assert!((matrix[BACK_COLUMN + axis] - back[axis]).abs() < 1e-5, "{matrix:?}");
    }
    assert!(matrix[UP_COLUMN + 1] > 0.0);
    assert!(matrix[1].abs() < 1e-5, "level right axis");
}

#[test]
fn single_paths_advance() {
    let (mut scene, mut loaded) = paths(SEQUENTIAL, &json!({"paths":[single(0.0), single(10.0)]}));
    let props = Properties::new();
    assert!(loaded.play(&mut scene, &props, 0.55));
    assert!(near(eye_x(&scene), 0.55));
    assert!((camera(&scene)[CAMERA_FOV].as_f64().unwrap() - 51.0).abs() < 0.05);
    let angles: Vec<f32> = serde_json::from_value(camera(&scene)[ANGLES_FIELD].clone()).unwrap();
    assert!(near(angles[0], 0.0) && near(angles[1], FRAC_PI_2) && near(angles[2], 0.0));
    loaded.play(&mut scene, &props, 0.3);
    assert!(near(eye_x(&scene), 0.85));
    loaded.play(&mut scene, &props, 0.3);
    assert!(near(eye_x(&scene), 1.0));
    loaded.play(&mut scene, &props, 0.25);
    assert!(near(eye_x(&scene), 10.25));
    loaded.play(&mut scene, &props, 1.0);
    loaded.play(&mut scene, &props, 0.25);
    assert!(near(eye_x(&scene), 0.25), "wraps to first");
}

#[test]
fn looping_modes_wrap() {
    for (mode, expected) in [("loop", [0.55, 0.15, 0.45]), ("mirror", [0.55, 0.85, 0.55])] {
        let (mut scene, mut loaded) = paths(SEQUENTIAL, &json!({ "paths": [path(0.0, mode)] }));
        for (dt, eye) in [0.55, 0.6, 0.3].into_iter().zip(expected) {
            loaded.play(&mut scene, &Properties::new(), dt);
            assert!(near(eye_x(&scene), eye), "{mode} {dt} {}", eye_x(&scene));
        }
    }
}

#[test]
fn lens_follows_projection() {
    let doc = json!({ "paths": [single(0.0)] });
    for perspective in [true, false] {
        let object = json!({"camera":"default","path":"p.json","origin":"0 0 0"});
        let (mut scene, mut loaded) = load(&object, &doc, &Properties::new(), perspective);
        loaded.play(&mut scene, &Properties::new(), 0.55);
        assert_eq!(camera(&scene).get(CAMERA_FOV).is_some(), perspective);
        assert_eq!(camera(&scene).get(ZOOM_FIELD).is_some(), !perspective);
        if !perspective {
            assert!((camera(&scene)[ZOOM_FIELD].as_f64().unwrap() - 2.1).abs() < 0.01);
        }
    }
}

#[test]
fn paused_path_idles() {
    let mut paused = single(0.0);
    paused["options"]["startpaused"] = json!(true);
    let (mut scene, mut loaded) = paths(SEQUENTIAL, &json!({ "paths": [paused] }));
    let props = Properties::new();
    assert!(loaded.live());
    assert!(loaded.play(&mut scene, &props, 0.5));
    assert!(!loaded.play(&mut scene, &props, 0.5));
    assert!(!loaded.live());
}

#[test]
fn hidden_camera_holds() {
    let (mut scene, mut loaded) = paths(SEQUENTIAL, &json!({"paths":[single(0.0)]}));
    scene[OBJECTS_FIELD][0][VISIBLE_FIELD] = json!(false);
    assert!(!loaded.play(&mut scene, &Properties::new(), 0.5));
    assert_eq!(camera(&scene)[ORIGIN_FIELD], "0 0 0");
    assert!(!loaded.live());
}

#[test]
fn degenerate_camera_moves() {
    let object = json!({"camera":"default","path":"p.json","scale":"0 0 0",
        "origin":{"user":"spot","value":"0 0 0"}});
    let props = Properties::from([("spot".into(), vec![9.0, 9.0, 9.0])]);
    let (mut scene, mut loaded) = load(&object, &json!({"paths":[single(0.0)]}), &props, true);
    assert!(loaded.play(&mut scene, &props, 0.55));
    assert!(camera(&scene)[ORIGIN_FIELD].get(USER_FIELD).is_none());
    let (x, ..) = crate::model::vec3(camera(&scene).get(ORIGIN_FIELD), &props).unwrap();
    assert!(near(x, 0.55));
}

#[test]
fn sequential_rechecks_visibility() {
    let mut toggled = single(10.0);
    toggled[VISIBLE_FIELD] = json!({"user":"show","value":true});
    let doc = json!({"paths":[single(0.0), toggled]});
    let shown = Properties::from([("show".into(), vec![1.0])]);
    let hidden = Properties::from([("show".into(), vec![0.0])]);
    let object = json!({"camera":"default","path":"p.json","queuemode":"sequential"});
    let (mut scene, mut loaded) = load(&object, &doc, &shown, true);
    loaded.play(&mut scene, &hidden, 1.0);
    loaded.play(&mut scene, &hidden, 0.25);
    assert!(near(eye_x(&scene), 0.25));
}

#[test]
fn random_never_repeats() {
    let mut hidden = single(20.0);
    hidden[VISIBLE_FIELD] = json!(false);
    let (mut scene, mut loaded) =
        paths("random", &json!({"paths":[single(0.0), single(10.0), hidden]}));
    let props = Properties::new();
    let mut picks = Vec::new();
    for _ in 0..8 {
        loaded.play(&mut scene, &props, 1.0);
        picks.push(loaded.queues[0].current.unwrap());
    }
    assert!(picks.windows(2).all(|pair| pair[0] != pair[1]), "{picks:?}");
    assert!(!picks.contains(&2));
}

#[test]
fn empty_paths_skipped() {
    let (_, loaded) = paths("random", &json!({"paths":[]}));
    assert!(loaded.is_empty());
    assert!(!loaded.live());
}
