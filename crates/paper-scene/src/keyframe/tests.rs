use super::*;
use serde_json::json;

fn property(mode: &str) -> Value {
    json!({"value":0,"animation":{"options":{"fps":30,"length":150,"mode":mode},
        "c0":[{"frame":0,"value":0},{"frame":63,"value":0},{"frame":150,"value":1}]}})
}

#[test]
fn effect_opacity_reaches_and_retains_the_cloud_endpoint() {
    let mut scene = json!({"objects":[{"effects":[{"passes":[{"constantshadervalues":{"alpha":property("single")}}]}]}]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let path = "/objects/0/effects/0/passes/0/constantshadervalues/alpha";
    bindings.apply(&mut scene, 0.0);
    assert_eq!(scene.pointer(path), Some(&json!(0.0)));
    bindings.apply(&mut scene, 2.0);
    assert_eq!(scene.pointer(path), Some(&json!(0.0)));
    bindings.apply(&mut scene, 3.55);
    assert!((scene.pointer(path).unwrap().as_f64().unwrap() - 0.5).abs() < 0.0001);
    bindings.apply(&mut scene, 5.0);
    assert_eq!(scene.pointer(path), Some(&json!(1.0)));
    assert!(!bindings.animated());
    assert!(!bindings.apply(&mut scene, 500.0));
    assert_eq!(scene.pointer(path), Some(&json!(1.0)));
}

#[test]
fn loops_restart_and_mirrors_reverse() {
    for (mode, value) in [("loop", 0.0), ("mirror", 1.0)] {
        let mut scene = property(mode);
        let mut bindings = Bindings::parse(&mut scene).unwrap();
        bindings.apply(&mut scene, 5.0);
        assert_eq!(scene, json!(value));
        assert!(bindings.animated());
        bindings.apply(&mut scene, 10.0);
        assert_eq!(scene, json!(0.0));
    }
}

#[test]
fn relative_tracks_preserve_unanimated_components_without_accumulation() {
    let mut scene = json!({"origin":{"value":"100 200 30","animation":{"relative":true,
        "options":{"fps":10,"length":10,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":10,"value":40}]}}});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    for time in [0.5, 0.5, 0.5] {
        bindings.apply(&mut scene, time);
        assert_eq!(scene["origin"], json!([120.0, 200.0, 30.0]));
    }
}

#[test]
fn parent_timelines_supply_the_clock_and_paused_state() {
    let mut scene = json!({"objects":[{"scale":property("single"),"alpha":property("loop")}]});
    scene["objects"][0]["alpha"]["animation"]["options"]["parent"] = json!({"key":"scale"});
    scene["objects"][0]["scale"]["animation"]["options"]["startpaused"] = json!(true);
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 4.0);
    assert_eq!(scene["objects"][0]["alpha"], json!(0.0));
    assert!(!bindings.animated());
}

#[test]
fn wrap_loop_replaces_the_final_key_with_the_first_value() {
    let mut scene = property("single");
    scene["animation"]["options"]["wraploop"] = json!(true);
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 5.0);
    assert_eq!(scene, json!(0.0));
}

#[test]
fn bezier_handles_use_half_the_segment_width_and_absolute_value_offsets() {
    let keys = channel(
        &json!([
            {"frame":0,"value":0,"front":{"enabled":true,"x":1,"y":0}},
            {"frame":100,"value":1,"back":{"enabled":true,"x":-1,"y":0}}
        ]),
        0.0,
        None,
    );
    assert!((sample(&keys, 50.0).unwrap() - 0.5).abs() < 0.0001);
    assert!((sample(&keys, 25.0).unwrap() - 0.105893).abs() < 0.0001);
}

#[test]
fn step_uses_the_destination_key_and_fractional_frames_blend_samples() {
    let mut scene = json!({"value":0,"animation":{"options":{"fps":1,"length":10,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":10,"value":1,"step":true}]}});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 9.0);
    assert_eq!(scene, json!(0.0));
    bindings.apply(&mut scene, 9.5);
    assert_eq!(scene, json!(0.5));
}

#[test]
fn bad_clocks_and_nonfinite_updates_preserve_the_property() {
    let mut scene = property("loop");
    scene["animation"]["options"]["fps"] = json!(0);
    assert!(Bindings::parse(&mut scene).unwrap().is_empty());
    let mut scene = property("single");
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    assert!(!bindings.apply(&mut scene, f32::NAN));
    assert_eq!(scene, json!(0));
}

#[test]
fn cyclic_parent_clocks_are_rejected_before_playback() {
    let mut scene = json!({"a":property("loop"),"b":property("loop")});
    scene["a"]["animation"]["options"]["parent"] = json!({"key":"b"});
    scene["b"]["animation"]["options"]["parent"] = json!({"key":"a"});
    assert!(Bindings::parse(&mut scene).unwrap_err().to_string().contains("parent cycle"));
}

#[test]
fn scripts_observe_animated_values_and_preserve_authored_initial_values() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"moving","origin":{"value":"100 200 0","animation":{"relative":true,
            "options":{"fps":30,"length":30,"mode":"single"},
            "c0":[{"frame":0,"value":0},{"frame":30,"value":40}],"c1":[],"c2":[]}}},
        {"id":2,"alpha":{"value":0,"script":"export function update(v){return thisScene.getLayer('moving').origin.x;}"}},
        {"id":3,"alpha":{"value":0,"script":"export function update(v){return thisScene.getInitialLayerConfig(thisScene.getLayer('moving')).origin.split(' ')[0]*1;}"}}
    ]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
    )
    .unwrap()
    .unwrap();
    host.set_keyframes(&bindings).unwrap();
    bindings.apply(&mut host.scene, 0.5);
    host.sync_keyframes(&bindings).unwrap();
    host.tick(0.5, 0.5, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["alpha"], json!(120));
    assert_eq!(host.scene["objects"][2]["alpha"], json!(100));
}

#[test]
fn script_result_overrides_a_timeline_on_the_same_property() {
    let mut alpha = property("single");
    alpha["script"] = json!("export function update(v){return v*0.5;}");
    let mut scene = json!({"objects":[{"id":1,"alpha":alpha}]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
    )
    .unwrap()
    .unwrap();
    host.set_keyframes(&bindings).unwrap();
    for _ in 0..3 {
        bindings.apply(&mut host.scene, 5.0);
        host.sync_keyframes(&bindings).unwrap();
        host.tick(5.0, 0.1, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["alpha"], json!(0.5));
    }
}

#[test]
fn script_replacement_of_a_timeline_owner_keeps_updates_attached() {
    let mut scene = json!({"objects":[{"id":1,
        "effects":[{"passes":[{"constantshadervalues":{"alpha":property("single")}}]}],
        "alpha":{"value":0,"script":"export function update(v){if(engine.runtime<1)thisLayer.effects[0].passes[0].constantshadervalues={alpha:0.2};return thisLayer.effects[0].passes[0].constantshadervalues.alpha;}"}
    }]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
    )
    .unwrap()
    .unwrap();
    host.set_keyframes(&bindings).unwrap();
    for time in [0.5, 5.0] {
        bindings.apply(&mut host.scene, time);
        host.sync_keyframes(&bindings).unwrap();
        host.tick(time, 0.5, [0.5; 2]).unwrap();
    }
    assert_eq!(host.scene["objects"][0]["alpha"], json!(1));
}

#[test]
fn angles_remain_radians_in_native_state_and_degrees_in_scripts() {
    let mut scene = json!({"objects":[{"id":1,
        "angles":{"value":"0 0 0","animation":{"options":{"fps":30,"length":30,"mode":"single"},
            "c0":[],"c1":[],"c2":[{"frame":0,"value":0},{"frame":30,"value":std::f32::consts::FRAC_PI_2}]}},
        "alpha":{"value":0,"script":"export function update(v){return thisLayer.angles.z;}"}
    }]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
    )
    .unwrap()
    .unwrap();
    host.set_keyframes(&bindings).unwrap();
    bindings.apply(&mut host.scene, 1.0);
    host.sync_keyframes(&bindings).unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!((host.scene["objects"][0]["alpha"].as_f64().unwrap() - 90.0).abs() < 0.001);
    let angles = crate::effects::json_numbers(&host.scene["objects"][0]["angles"]).unwrap();
    assert!((angles[2] - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
}

#[test]
fn animated_effects_follow_anonymous_layers_and_ancestors() {
    let mut scene = json!({"objects":[
        {"alpha":property("single")},
        {"id":2,"parent":"@object-0"},
        {"id":3}
    ]});
    let bindings = Bindings::parse(&mut scene).unwrap();
    assert!(bindings.affects_layer(&scene, "@object-0"));
    assert!(bindings.affects_layer(&scene, "2"));
    assert!(!bindings.affects_layer(&scene, "3"));
}
