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

#[test]
fn particles_use_unit_parallax_depth_and_keep_sprite_scale_unchanged() {
    let mut scene = serde_json::json!({
        "general":{"cameraparallax":true,"cameraparallaxamount":0.5},
        "objects":[
            {"id":1,"origin":"1500 600 0","particle":"particles/test.json"},
            {"id":2,"origin":"1500 900 0","particle":"particles/test.json","parallaxDepth":"0 0"},
            {"id":3,"origin":"1500 1200 0","particle":"particles/test.json","parallaxDepth":"1 1"},
            {"id":4,"origin":"1500 1500 0","particle":"particles/test.json","parallaxDepth":"2 2"}
        ]
    });
    let props = Properties::new();
    let states =
        particle_frames(&scene, ["1", "2", "3", "4"].into_iter(), (3840.0, 2160.0), &props);
    for (state, expected) in states.iter().zip([
        (1290.0, 360.0, 0.0),
        (1500.0, 900.0, 0.0),
        (1290.0, 1260.0, 0.0),
        (1080.0, 1920.0, 0.0),
    ]) {
        let state = state.as_ref().unwrap();
        assert_eq!(state.origin, expected);
        assert_eq!(state.scale, [1.0; 3]);
    }
    scene["objects"][0]["origin"] = serde_json::json!("1800 900 0");
    let states = particle_frames(&scene, ["1"].into_iter(), (3840.0, 2160.0), &props);
    assert_eq!(states[0].as_ref().unwrap().origin, (1740.0, 810.0, 0.0));
}
