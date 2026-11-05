use super::super::{Attachments, Properties, ShadowLayout, lights};
use super::*;
use serde_json::json;

const NATIVE_UNIT: f32 = 0.998_308_1;
const FRONT: SunView = SunView { eye: [1.03, 2.07, 3.01], forward: [0.0, 0.0, -1.0], flat: false };

fn sun_scene(sun: &Value, budget: u64) -> Value {
    json!({"general":{"lightconfig":{"directional":2,"directionalshadow":budget}},"objects":[sun]})
}

fn one_sun(sun: &Value) -> super::super::Light {
    lights(&sun_scene(sun, 1), &Properties::new(), &Attachments::new()).remove(0)
}

fn receiver(light: &super::super::Light, view: &SunView) -> [Mat4; CASCADES] {
    let mut views = light.shadow_views(ShadowView::Receiver, view);
    std::array::from_fn(|_| views.next().unwrap())
}

#[test]
fn cascade_ranges_native() {
    let sun =
        |distances: [f32; 3]| Sun { slot: None, up: [0.0; AXES], right: [0.0; AXES], distances };
    assert_eq!(sun([3.0, 10.0, 100.0]).ranges(), [[3.0, 40.0], [10.0, 40.0], [100.0, 150.0]]);
    assert_eq!(sun([3.0, 50.0, 100.0]).ranges(), [[3.0, 200.0], [50.0, 200.0], [100.0, 200.0]]);
}

#[test]
fn default_distances_native() {
    let light = one_sun(&json!({"id":1,"light":"ldirectional","castshadow":true}));
    assert_eq!(light.sun.unwrap().distances, [3.0, 10.0, 100.0]);
}

#[test]
fn origin_follows_view() {
    let light = one_sun(&json!({"id":1,"light":"ldirectional","castshadow":true}));
    let sun = light.sun.as_ref().unwrap();
    let origin = sun.origin(light.forward, &FRONT, 3.0);
    let texel = 3.0 / 256.0;
    assert!((origin[0] - 1.03).abs() < 1e-6);
    assert!((origin[1] - 2.07).abs() < texel);
    assert!((origin[2] - 1.51).abs() < texel);
    let tilted = SunView { forward: [0.8, 0.0, -0.6], ..FRONT };
    let origin = sun.origin(light.forward, &tilted, 3.0);
    assert!((origin[0] - (1.03 + 0.4 * 1.5)).abs() < 1e-6);
}

#[test]
fn origin_snaps_texels() {
    let light = one_sun(&json!({"id":1,"light":"ldirectional","castshadow":true}));
    let sun = light.sun.as_ref().unwrap();
    let first = sun.origin(light.forward, &FRONT, 3.0);
    let near = SunView { eye: [1.03, 2.071, 3.0105], ..FRONT };
    assert_eq!(first, sun.origin(light.forward, &near, 3.0));
    let moved = SunView { eye: [1.03, 2.09, 3.01], ..FRONT };
    assert_ne!(first, sun.origin(light.forward, &moved, 3.0));
}

#[test]
fn flat_cascade_plane() {
    let light = one_sun(&json!({"id":1,"light":"ldirectional","castshadow":true}));
    let flat = SunView { flat: true, ..FRONT };
    assert_eq!(light.sun.as_ref().unwrap().origin(light.forward, &flat, 3.0)[2], 0.0);
}

#[test]
fn orthographic_reverse_depth() {
    let light = one_sun(&json!({"id":1,"light":"ldirectional","castshadow":true,
        "cascadedistance0":8.0,"cascadedistance1":10.0}));
    let [first, ..] = receiver(&light, &SunView { eye: [0.0; 3], ..FRONT });
    assert_eq!(first[0], 0.0);
    assert_eq!(first[1], 0.0);
    assert_eq!(first[2], -NATIVE_UNIT / 40.0);
    assert_eq!(first[8], NATIVE_UNIT * 2.0 / 8.0);
    assert_eq!(first[5], NATIVE_UNIT * 2.0 / 8.0);
    assert_eq!(first[15], 1.0);
    let depth = |x: f32| first[2] * x + first[14];
    assert!(depth(20.0) < depth(0.0) && depth(0.0) < depth(-20.0));
    assert!((depth(0.0) - 0.5).abs() < 1e-5);
    assert!((depth(20.0) - 0.0).abs() < 1e-3);
    assert!((depth(-20.0) - 1.0).abs() < 1e-3);
}

#[test]
fn caster_depth_bias() {
    let light =
        one_sun(&json!({"id":1,"light":"ldirectional","castshadow":true,"angles":"0 0.5 0"}));
    let casters = light.shadow_views(ShadowView::Caster, &FRONT);
    for (mut receiver, caster) in receiver(&light, &FRONT).into_iter().zip(casters) {
        receiver[14] -= 0.0005;
        assert_eq!(receiver, caster);
    }
}

#[test]
fn budget_caps_suns() {
    let scene = json!({"general":{"lightconfig":{"directional":2,"directionalshadow":1}},"objects":[
        {"id":1,"light":"ldirectional","castshadow":true},
        {"id":2,"light":"ldirectional","castshadow":true}
    ]});
    let lights = lights(&scene, &Properties::new(), &Attachments::new());
    assert_eq!(lights[0].sun.as_ref().unwrap().slot, Some(0));
    assert_eq!(lights[1].sun.as_ref().unwrap().slot, None);
    assert!(lights[0].casting() && !lights[1].casting());
    for (config, expected) in [(0, 0), (3, 2), (4, 0)] {
        assert_eq!(budget(&sun_scene(&json!({}), config)), expected);
    }
}

#[test]
fn hidden_sun_yields_slot() {
    let scene = json!({"general":{"lightconfig":{"directional":2,"directionalshadow":1}},"objects":[
        {"id":1,"light":"ldirectional","castshadow":true,"visible":false},
        {"id":2,"light":"ldirectional","castshadow":true}
    ]});
    let lights = lights(&scene, &Properties::new(), &Attachments::new());
    assert_eq!(lights[0].sun.as_ref().unwrap().slot, None);
    assert_eq!(lights[1].sun.as_ref().unwrap().slot, Some(0));
}

#[test]
fn unusable_sun_skipped() {
    for sun in [
        json!({"id":1,"light":"ldirectional","castshadow":true,"scale":"0 0 0"}),
        json!({"id":1,"light":"ldirectional","castshadow":true,"cascadedistance2":0.0}),
    ] {
        assert!(!one_sun(&sun).casting());
    }
}

#[test]
fn sun_tile_cells() {
    let scene = json!({"general":{"lightconfig":{"directional":1,"directionalshadow":1}},"objects":[
        {"id":1,"light":"lpoint","castshadow":true},
        {"id":2,"light":"ldirectional","castshadow":true}
    ]});
    let lights = lights(&scene, &Properties::new(), &Attachments::new());
    let layout = ShadowLayout::new(&lights).unwrap();
    assert_eq!(layout.extent(), [512, 512]);
    assert_eq!(layout.tile(&lights[1], 0), Some([256, 0, 256, 256]));
    assert_eq!(layout.tile(&lights[1], 1), Some([0, 256, 256, 256]));
    assert_eq!(layout.tile(&lights[1], 2), Some([256, 256, 256, 256]));
    assert_eq!(layout.tile(&lights[1], 3), None);
    assert!(layout.matches(&lights));
    assert!(!layout.matches(&lights[..1]));
}

#[test]
fn projections_follow_spots() {
    let scene = json!({"general":{"lightconfig":{"directional":2,"directionalshadow":1,"spot":1}},"objects":[
        {"id":1,"light":"lspot","castshadow":true},
        {"id":2,"light":"ldirectional"},
        {"id":3,"light":"ldirectional","castshadow":true}
    ]});
    let lights = lights(&scene, &Properties::new(), &Attachments::new());
    let values = ShadowLayout::new(&lights).unwrap().uniforms(&lights, &FRONT);
    let matrices = &values["g_LFeature_ShadowProjection"];
    let transforms = &values["g_LFeature_ShadowProjectionTransform"];
    assert_eq!(values["g_LDirectional_Shadow"][..8], [0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0]);
    assert_eq!(matrices[16..64], receiver(&lights[2], &FRONT).concat());
    assert_eq!(transforms[4..8], [0.5, 0.0, 0.5, 0.5]);
    assert_eq!(transforms[8..12], [0.0, 0.5, 0.5, 0.5]);
    assert_eq!(transforms[12..16], [0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn authored_view_parsed() {
    let scene = json!({"camera":{"eye":"1 2 3","center":"1 2 0","up":"0 1 0"},"general":{}});
    let view = SunView::of(&scene, &Properties::new());
    assert_eq!(view, SunView { eye: [1.0, 2.0, 3.0], forward: [0.0, 0.0, -1.0], flat: false });
    let flat = json!({"general":{"orthogonalprojection":{"width":640,"height":360}}});
    let view = SunView::of(&flat, &Properties::new());
    assert_eq!(view, SunView { eye: [320.0, 180.0, 0.0], forward: [0.0, 0.0, -1.0], flat: true });
}
