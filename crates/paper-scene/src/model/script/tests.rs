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
    let states = particle_frames(
        &scene,
        ["2", "4"].into_iter(),
        (3840.0, 2160.0),
        &props,
        &crate::model::Attachments::new(),
    );
    let first = states[0].as_ref().unwrap();
    assert_eq!(first.origin, (1940.0, 1020.0, 24.0));
    assert_eq!(first.scale, [10.0, 18.0, 28.0]);
    assert!(first.visible);
    assert!(!states[1].as_ref().unwrap().visible);
    let props = Properties::from([("language".into(), vec![2.0])]);
    let states = particle_frames(
        &scene,
        ["2"].into_iter(),
        (3840.0, 2160.0),
        &props,
        &crate::model::Attachments::new(),
    );
    assert!(!states[0].as_ref().unwrap().visible);
}

#[test]
fn particle_parent_rotation_and_script_changes_update_the_same_system() {
    let mut scene = json!({"objects":[
        {"id":1,"origin":"100 200 0","angles":format!("0 0 {}",std::f32::consts::FRAC_PI_2)},
        {"id":2,"parent":1,"origin":"10 0 0","particle":"particles/stars.json"}
    ]});
    let props = Properties::new();
    let states = particle_frames(
        &scene,
        ["2"].into_iter(),
        (800.0, 600.0),
        &props,
        &crate::model::Attachments::new(),
    );
    let first = states[0].as_ref().unwrap();
    assert!((first.origin.0 - 100.0).abs() < 0.001);
    assert!((first.origin.1 - 210.0).abs() < 0.001);
    assert!((first.angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    scene["objects"][0]["visible"] = json!(false);
    scene["objects"][0]["origin"] = json!("300 400 0");
    let states = particle_frames(
        &scene,
        ["2"].into_iter(),
        (800.0, 600.0),
        &props,
        &crate::model::Attachments::new(),
    );
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
    let states = particle_frames(
        &scene,
        ["1", "2", "3", "4"].into_iter(),
        (3840.0, 2160.0),
        &props,
        &crate::model::Attachments::new(),
    );
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
    let states = particle_frames(
        &scene,
        ["1"].into_iter(),
        (3840.0, 2160.0),
        &props,
        &crate::model::Attachments::new(),
    );
    assert_eq!(states[0].as_ref().unwrap().origin, (1740.0, 810.0, 0.0));
}

#[test]
fn attached_frames_inherit() {
    let scene = json!({"objects":[
        {"id":1,"origin":"100 200 0","scale":"2 2 1"},
        {"id":2,"parent":1,"attachment":"sword","origin":"3 0 0"},
        {"id":3,"parent":2,"origin":"0 5 0"},
        {"id":4,"parent":1,"attachment":"missing","origin":"3 0 0"}
    ]});
    let props = Properties::new();
    let mut attachments = crate::model::Attachments::from([(
        ("1", "sword"),
        crate::puppet::Transform {
            translation: [10.0, 20.0, 0.0],
            rotation: [0.0, 0.0, std::f32::consts::FRAC_PI_2],
            scale: [1.0; 3],
        }
        .into(),
    )]);
    let layout = super::Layout {
        id: "3".into(),
        size: [8.0, 6.0],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let states = super::frames(&scene, &[layout], (800.0, 600.0), &props, &attachments);
    let layer = states[0].as_ref().unwrap();
    assert!((layer.rect[0] - 110.0).abs() < 0.001);
    assert!((layer.rect[1] - 354.0).abs() < 0.001);
    assert_eq!(&layer.rect[2..], &[16.0, 12.0]);
    let particles =
        particle_frames(&scene, ["3", "4"].into_iter(), (800.0, 600.0), &props, &attachments);
    let particle = particles[0].as_ref().unwrap();
    assert!((particle.origin.0 - layer.rect[0]).abs() < 0.001);
    assert!((particle.origin.1 - (600.0 - layer.rect[1])).abs() < 0.001);
    assert_eq!(particles[1].as_ref().unwrap().origin, (106.0, 200.0, 0.0));
    attachments.get_mut(&("1", "sword")).unwrap().affine.x += 5.0;
    let moved = particle_frames(&scene, ["3"].into_iter(), (800.0, 600.0), &props, &attachments);
    assert!((moved[0].as_ref().unwrap().origin.0 - 120.0).abs() < 0.001);
}

#[test]
fn affine_frames_preserve_size() {
    let mut scene = json!({"objects":[
        {"id":1,"origin":"100 200 0","scale":"0 2 1"},
        {"id":2,"parent":1,"angles":format!("0 0 {}",std::f32::consts::FRAC_PI_4)}
    ]});
    let layout = super::Layout {
        id: "2".into(),
        size: [80.0, 40.0],
        offset: [40.0, 20.0],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    for scale in [0.0, -3.0, 2.0] {
        scene["objects"][0]["scale"] = json!(format!("{scale} 2 1"));
        let states = super::frames(
            &scene,
            std::slice::from_ref(&layout),
            (800.0, 600.0),
            &Properties::new(),
            &crate::model::Attachments::new(),
        );
        let geometry = states[0].as_ref().unwrap().geometry.unwrap();
        assert_eq!(geometry.size, layout.size);
        let origin = geometry.transform.point([-40.0, -20.0, 0.0]);
        assert!((origin[0] - 100.0).abs() < 0.0001);
        assert!((origin[1] - 400.0).abs() < 0.0001);
        assert_eq!(geometry.transform.inverse().is_some(), scale != 0.0);
    }
}

#[test]
fn keyframed_parent_scope() {
    let mut scene = json!({"objects":[
        {"id":1,"angles":{"value":"0 0 0","animation":{
            "options":{"fps":10,"length":10,"mode":"single"},
            "c0":[{"frame":0,"value":0},{"frame":10,"value":0.5}]
        }}},
        {"id":2,"parent":1,"origin":"10 20 0"}
    ]});
    let layout = super::Layout {
        id: "2".into(),
        size: [80.0, 40.0],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let mut bindings = crate::keyframe::Bindings::parse(&mut scene).unwrap();
    for (time, supported) in [(0.0, true), (1.0, false)] {
        bindings.apply(&mut scene, time);
        let frames = super::frames(
            &scene,
            std::slice::from_ref(&layout),
            (800.0, 600.0),
            &Properties::new(),
            &crate::model::Attachments::new(),
        );
        let frame = frames[0].as_ref().unwrap();
        assert_eq!(frame.planar_owner, supported);
        assert!(frame.geometry.unwrap().spatial.is_none());
    }
}

#[test]
fn particles_keep_tilt() {
    let scene = json!({"objects":[
        {"id":1,"angles":"0.5 0 0","origin":"1 0 0"},
        {"id":2,"parent":1,"particle":"particles/stars.json"},
        {"id":3,"angles":"0.5 0 0","particle":"particles/stars.json"},
        {"id":4,"particle":"particles/stars.json"}
    ]});
    let states = particle_frames(
        &scene,
        ["2", "3", "4"].into_iter(),
        (1920.0, 1080.0),
        &Properties::new(),
        &crate::model::Attachments::new(),
    );
    let tilted: Vec<_> =
        states.iter().map(|state| state.as_ref().unwrap().spatial.is_some()).collect();
    assert_eq!(tilted, [true, true, false]);
}

#[test]
fn fullscreen_ignores_transform() {
    let scene = serde_json::json!({"objects":[
        {"id":3,"origin":"100 900 0","angles":"0 0 1","scale":"2 2 1"}
    ]});
    let layout = super::Layout {
        id: "3".into(),
        size: [8.0, 6.0],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: true,
    };
    let props = Properties::new();
    let states =
        super::frames(&scene, &[layout], (800.0, 600.0), &props, &super::Attachments::new());
    let frame = states[0].as_ref().unwrap();
    assert_eq!(frame.rect, [400.0, 300.0, 800.0, 600.0]);
    assert_eq!(frame.angle, 0.0);
    assert!(frame.geometry.is_none());
}
