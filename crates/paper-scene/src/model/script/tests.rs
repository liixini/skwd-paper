use super::{Properties, particle_frames};
use serde_json::json;

#[test]
fn particles_inherit_ancestor_placement_scale_and_visibility() {
    let scene = json!({"objects":[
        {"id":1,"origin":"1920 1080 4","scale":"2 3 4","visible":{"user":{"name":"language","condition":"1"},"value":true}},
        {"id":2,"parent":1,"origin":"10 -20 5","scale":"5 6 7","particle":"particles/stars.json"},
        {"id":3,"origin":"80 90 0","visible":false},
        {"id":4,"parent":3,"particle":"particles/stars.json"}
    ]});
    let props = Properties::from([("language".into(), vec![1.0])]);
    let states = particle_frames(&scene, ["2", "4"].into_iter(), (3840.0, 2160.0), &props);
    let first = states[0].as_ref().unwrap();
    assert_eq!(first.origin, (1940.0, 1020.0, 9.0));
    assert_eq!(first.scale, [10.0, 18.0, 28.0]);
    assert!(first.visible);
    assert!(!states[1].as_ref().unwrap().visible);
    let props = Properties::from([("language".into(), vec![2.0])]);
    let states = particle_frames(&scene, ["2"].into_iter(), (3840.0, 2160.0), &props);
    assert!(!states[0].as_ref().unwrap().visible);
}

#[test]
fn particle_parent_rotation_and_script_changes_update_the_same_system() {
    let mut scene = json!({"objects":[
        {"id":1,"origin":"100 200 0","angles":format!("0 0 {}",std::f32::consts::FRAC_PI_2)},
        {"id":2,"parent":1,"origin":"10 0 0","particle":"particles/stars.json"}
    ]});
    let props = Properties::new();
    let states = particle_frames(&scene, ["2"].into_iter(), (800.0, 600.0), &props);
    let first = states[0].as_ref().unwrap();
    assert!((first.origin.0 - 100.0).abs() < 0.001);
    assert!((first.origin.1 - 210.0).abs() < 0.001);
    assert!((first.angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    scene["objects"][0]["visible"] = json!(false);
    scene["objects"][0]["origin"] = json!("300 400 0");
    let states = particle_frames(&scene, ["2"].into_iter(), (800.0, 600.0), &props);
    let changed = states[0].as_ref().unwrap();
    assert!(!changed.visible);
    assert!((changed.origin.0 - 300.0).abs() < 0.001);
    assert!((changed.origin.1 - 410.0).abs() < 0.001);
}
