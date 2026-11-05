use super::*;
use serde_json::json;

fn view(objects: &Value, flat: bool) -> CameraView {
    let props = Properties::from([("style".into(), vec![1.0])]);
    active_camera_view(&json!({ "objects": objects }), &props, 2.0, flat)
}

#[test]
fn camera_object_view() {
    let camera = json!([{"camera":"default","origin":"100 50 0","zoom":2.0,"angles":"0 0 0.3"}]);
    assert_eq!(
        view(&camera, true),
        CameraView { zoom: 2.0, object_zoom: 2.0, origin: [100.0, 50.0], roll: 0.3 }
    );
    assert_eq!(view(&camera, false), CameraView::zoomed(2.0));
    assert_eq!(view(&json!([{"id":1,"image":"a.json"}]), true), CameraView::zoomed(2.0));
}

#[test]
fn visible_camera_selected() {
    let cameras = json!([
        {"camera":"default","zoom":3.0,"visible":{"user":{"condition":"0","name":"style"},"value":true}},
        {"camera":"default","zoom":5.0,"visible":{"user":{"condition":"1","name":"style"},"value":false}}
    ]);
    assert_eq!(view(&cameras, true).scale(), 10.0);
}

#[test]
fn unsupported_cameras_reported() {
    let props = Properties::new();
    for (camera, expected) in [
        (json!({"camera":"default"}), vec![]),
        (json!({"camera":"default","origin":{"value":"0 0 0","animation":{}}}), vec![]),
        (json!({"camera":"default","visible":{"value":true,"script":"x"}}), vec![]),
        (json!({"camera":"default","zoom":2.0}), vec![ZOOM_GAP]),
        (json!({"camera":"default","parent":null}), vec![]),
        (json!({"camera":"default","zoom":{"value":1.0,"script":"x"}}), vec![ZOOM_GAP]),
        (json!({"id":2,"camera":"default","parent":99}), vec![INVALID_GAP]),
        (json!({"camera":"default","origin":"nan 0 0"}), vec![INVALID_GAP]),
    ] {
        assert_eq!(perspective_gaps(&json!({"objects":[camera]}), &props), expected);
    }
}

#[test]
fn camera_object_placement() {
    let props = Properties::new();
    for (angles, center) in [
        ("0 0 0", [0.0, 0.0, 9.0]),
        ("0 1.5707964 0", [-1.0, 0.0, 10.0]),
        ("1.5707964 0 0", [0.0, 1.0, 10.0]),
    ] {
        let scene =
            json!({"objects":[{"camera":"default","origin":"0 0 10","angles":angles,"fov":30.0}]});
        let placement = placement(&scene, &props).unwrap();
        assert_eq!(placement.eye, [0.0, 0.0, 10.0]);
        assert_eq!(placement.fov, Some(30.0));
        for (actual, expected) in placement.center.iter().zip(center) {
            assert!((actual - expected).abs() < 1e-5, "{angles} {:?}", placement.center);
        }
    }
}

#[test]
fn flat_script_view() {
    let props = Properties::new();
    let view = CameraView::zoomed(1.0);
    let free = json!({"objects":[]});
    let mut source = CameraSource::of(&free, &props, true, |_| None);
    source.set_script([0.0; 3], [0.0; 3], [0.0, 1.0, 0.0], false);
    assert_eq!(source.flat_view(view), view);
    source.set_script([300.0, 100.0, 5.0], [0.0; 3], [-0.29552, 0.95534, 0.0], true);
    let scripted = source.flat_view(view);
    assert_eq!(scripted.origin, [300.0, 100.0]);
    assert!((scripted.roll - 0.3).abs() < 1e-4);
    let lens = json!({"objects":[{"camera":"default","origin":"0 0 0"}]});
    let mut covered = CameraSource::of(&lens, &props, true, |_| None);
    covered.set_script([300.0, 100.0, 5.0], [0.0; 3], [0.0, 1.0, 0.0], true);
    assert!(covered.next(&lens, &props, false).is_none());
    assert_eq!(covered.flat_view(view), view);
}

#[test]
fn last_camera_wins() {
    let props = Properties::new();
    let scene = json!({"objects":[{"camera":"default","origin":"1 0 0"},{"camera":"default","origin":"2 0 0"},
        {"camera":"default","origin":"3 0 0","visible":false}]});
    assert_eq!(placement(&scene, &props).unwrap().eye, [2.0, 0.0, 0.0]);
}

#[test]
fn source_follows_object() {
    let props = Properties::new();
    let mut scene = json!({"camera":{"eye":"0 0 5","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50.0},
        "objects":[{"id":1,"origin":"4 0 0"},{"camera":"default","parent":1,"origin":"1 2 3","fov":30.0}]});
    let mut source = CameraSource::of(&scene, &props, true, |_| None);
    source.set_script([9.0; 3], [8.0; 3], [0.0, 1.0, 0.0], true);
    assert!(source.next(&scene, &props, false).is_none());
    let moved = source.next(&scene, &props, true).unwrap();
    assert_eq!(moved.eye, [5.0, 2.0, 3.0]);
    assert_eq!(moved.fov, Some(30.0));
    scene["objects"][0]["visible"] = json!(false);
    let script = source.next(&scene, &props, false).unwrap();
    assert_eq!(script.eye, [9.0; 3]);
    assert_eq!(script.fov, Some(50.0));
    assert!(source.next(&scene, &props, false).is_none());
    scene["objects"][0]["visible"] = json!(true);
    assert_eq!(source.next(&scene, &props, false).unwrap().eye, [5.0, 2.0, 3.0]);
}

#[test]
fn source_restores_authored() {
    let props = Properties::new();
    let mut scene = json!({"camera":{"eye":"0 0 5","center":"0 0 0","up":"0 1 0"},
        "objects":[{"camera":"default","origin":"1 2 3"}]});
    let mut source = CameraSource::of(&scene, &props, true, |_| None);
    scene["objects"][0]["visible"] = json!(false);
    assert_eq!(source.next(&scene, &props, false).unwrap().eye, [0.0, 0.0, 5.0]);
}

#[test]
fn source_without_object() {
    let props = Properties::new();
    let scene = json!({"objects":[]});
    let mut source = CameraSource::of(&scene, &props, true, |_| None);
    assert!(source.next(&scene, &props, true).is_none());
    source.set_script([1.0; 3], [0.0; 3], [0.0, 1.0, 0.0], true);
    assert_eq!(source.next(&scene, &props, true).unwrap().center, [0.0; 3]);
    assert!(source.next(&scene, &props, true).is_none());
}

#[test]
fn camera_object_adopted() {
    let mut scene = json!({"camera":{"eye":"0 0 5","center":"0 0 0","up":"0 1 0"},
        "objects":[{"camera":"default","origin":"1 2 3"}]});
    scene["camera"]["zoom"] = json!(1.0);
    scene["general"] = json!({"fov": 50.0});
    scene["objects"][0]["fov"] = json!(30.0);
    adopt(&mut scene, &Properties::new());
    assert_eq!(scene["camera"]["zoom"], 1.0);
    assert_eq!(scene["general"]["fov"], 30.0);
    assert_eq!(scene["camera"]["eye"], "1 2 3");
    assert_eq!(scene["camera"]["center"], "1 2 2");
    assert_eq!(scene["camera"]["up"], "0 1 0");
}

#[test]
fn invalid_camera_ignored() {
    let props = Properties::new();
    let broken = json!({"objects":[{"camera":"default","origin":"nan 0 0"}]});
    assert!(placement(&broken, &props).is_none());
    let wide = json!({"objects":[{"camera":"default","fov":400.0}]});
    assert_eq!(placement(&wide, &props).unwrap().fov, None);
}

fn near(left: [f32; 3], right: [f32; 3]) -> bool {
    left.iter().zip(right).all(|(left, right)| (left - right).abs() < 0.001)
}

#[test]
fn parent_chain_places_camera() {
    let props = Properties::new();
    let scene = json!({"objects":[
        {"id":1,"origin":"10 0 0","angles":"0 1.5707964 0"},
        {"id":2,"parent":1,"origin":"0 2 0","scale":"3 3 3"},
        {"id":3,"parent":2,"camera":"default","origin":"0 0 1"}
    ]});
    let placement = placement(&scene, &props).unwrap();
    assert!(near(placement.eye, [13.0, 2.0, 0.0]), "{:?}", placement.eye);
    assert!(near(placement.center, [12.0, 2.0, 0.0]), "{:?}", placement.center);
    assert!(near(placement.up, [0.0, 1.0, 0.0]), "{:?}", placement.up);
}

#[test]
fn hidden_parent_skips_camera() {
    let props = Properties::new();
    let scene = json!({"objects":[
        {"id":1,"visible":false},
        {"id":2,"parent":1,"camera":"default","origin":"1 0 0"},
        {"id":3,"camera":"default","origin":"2 0 0"}
    ]});
    assert!(near(placement(&scene, &props).unwrap().eye, [2.0, 0.0, 0.0]));
}

#[test]
fn moving_chain_detected() {
    let still = json!({"objects":[{"id":1},{"id":2,"parent":1,"camera":"default"}]});
    assert!(!cameras_move(&still));
    let scripted = json!({"objects":[
        {"id":1,"angles":{"script":"x","value":"0 0 0"}},
        {"id":2,"parent":1,"camera":"default"}
    ]});
    assert!(cameras_move(&scripted));
    let switched =
        json!({"objects":[{"id":2,"camera":"default","visible":{"script":"x","value":true}}]});
    assert!(cameras_move(&switched));
}

#[test]
fn attached_camera_reported() {
    let scene = json!({"objects":[
        {"id":1},
        {"id":2,"parent":1,"camera":"default","attachment":"Head"}
    ]});
    assert!(perspective_gaps(&scene, &Properties::new()).contains(&PARENT_GAP));
}

#[test]
fn parent_cycle_invalid() {
    let scene = json!({"objects":[
        {"id":1,"parent":2},
        {"id":2,"parent":1,"camera":"default"}
    ]});
    assert!(perspective_gaps(&scene, &Properties::new()).contains(&INVALID_GAP));
}
