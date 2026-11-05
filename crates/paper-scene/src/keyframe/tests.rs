use super::*;
use serde_json::json;

fn host(scene: &mut Value) -> (Bindings, crate::script::SceneScripts) {
    let bindings = Bindings::parse(scene).unwrap();
    let host = crate::script::SceneScripts::load_with_storage(
        scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
        crate::script::Load { keyframes: Some(&bindings), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    (bindings, host)
}

#[test]
fn first_update_reads_keyframe() {
    let mut scene = json!({"objects":[{"id":1,"name":"probe","origin":{"value":"900 540 0",
        "animation":{"options":{"fps":10,"length":20,"mode":"single","startpaused":true,"name":"move"},
            "c0":[{"frame":0,"value":100},{"frame":20,"value":300}]}},
        "alpha":{"value":1,"script":r"
export function init() {
    thisLayer.name='init:'+thisLayer.origin.x;
    thisLayer.getAnimation('move').setFrame(20);
}
export function update() {
    thisLayer.name='update:'+thisLayer.origin.x;
    return 1;
}
"}}]});
    let (mut bindings, mut host) = host(&mut scene);
    assert_eq!(scene["objects"][0]["name"], "init:900");
    bindings.apply(&mut host.scene, 1.0 / 60.0);
    host.sync_keyframes(&bindings);
    host.tick(1.0 / 60.0, 1.0 / 60.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["name"], "update:300");
}

#[test]
fn timeline_metadata_and_commands() {
    let mut scale = property("single");
    scale["value"] = json!("1 1 1");
    scale["animation"]["options"] =
        json!({"fps":10,"length":20,"mode":"single","startpaused":true,"name":"move"});
    let script = r"
export function init() {
    const anim = thisLayer.getAnimation('move');
    const values = [anim.fps,anim.frameCount,anim.duration,anim.name,anim.rate,
        anim===thisLayer.getAnimation('move'),thisLayer.getAnimation('missing')===undefined,
        thisObject.getAnimation()===undefined];
    anim.setFrame(-5); values.push(anim.getFrame());
    anim.setFrame(100); values.push(anim.getFrame());
    anim.stop(); values.push(anim.getFrame(),anim.isPlaying());
    anim.setFrame(10); anim.play(); anim.pause(); values.push(anim.getFrame(),anim.isPlaying());
    anim.rate=0; anim.play(); values.push(anim.rate,anim.isPlaying());
    thisLayer.text=JSON.stringify(values);
}
";
    let mut scene =
        json!({"objects":[{"id":1,"text":"","scale":scale,"alpha":{"value":1,"script":script}}]});
    host(&mut scene);
    let values: Value =
        serde_json::from_str(scene["objects"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        values,
        json!([10, 20, 2, "move", 1, true, true, true, -5, 100, 0, false, 10, false, 0, true])
    );
}

#[test]
fn timeline_pause_keeps_render() {
    let mut scale = property("single");
    scale["animation"]["options"]["name"] = json!("move");
    scale["animation"]["options"]["startpaused"] = json!(true);
    let script = r"
let anim;
export function init() { anim=thisLayer.getAnimation('move'); anim.setFrame(60); anim.rate=2; anim.play(); }
export function update() { if(engine.runtime>=0.5)anim.pause(); return anim.getFrame(); }
";
    let mut scene = json!({"objects":[{"id":1,"scale":scale,"alpha":{"value":1,"script":script}}]});
    let (mut bindings, mut host) = host(&mut scene);
    let mut held = Value::Null;
    for time in [0.5, 1.0, 2.0] {
        bindings.apply(&mut host.scene, time);
        host.sync_keyframes(&bindings);
        host.tick(time, 0.5, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["alpha"], json!(89.999_992_370_605_47));
        if held.is_null() {
            held = host.scene["objects"][0]["scale"].clone();
        }
        assert_eq!(host.scene["objects"][0]["scale"], held);
    }
    bindings.apply(&mut host.scene, 2.0);
    assert!(!bindings.animated());
}

#[test]
fn nested_timeline_handle() {
    let mut alpha = property("single");
    alpha["script"] = json!(
        "export function init(){const anim=thisObject.getAnimation();thisLayer.text=JSON.stringify([!!anim,anim===thisObject.getAnimation()]);}"
    );
    let mut scene = json!({"objects":[{"id":1,"text":"","effects":[{"passes":[{"constantshadervalues":{"alpha":alpha}}]}]}]});
    host(&mut scene);
    assert_eq!(scene["objects"][0]["text"], "[true,true]");
}

#[test]
fn named_handles_keep_owners() {
    let mut alpha = property("single");
    alpha["animation"]["options"]["name"] = json!("fade");
    alpha["animation"]["options"]["startpaused"] = json!(true);
    let script = r"
export function init() {
    const first = thisScene.getLayer('first').getAnimation('fade');
    const second = thisScene.getLayer('second').getAnimation('fade');
    first.setFrame(60);
    thisLayer.text=JSON.stringify([first!==second,first.getFrame(),second.getFrame()]);
}
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"first","alpha":alpha,"text":{"value":"","script":script}},
        {"id":2,"name":"second","alpha":alpha}
    ]});
    host(&mut scene);
    assert_eq!(scene["objects"][0]["text"], "[true,59.999996185302734,0]");
}

#[test]
fn timeline_handle_in_init() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,
        "animation":{"options":{"fps":10,"length":20,"mode":"single","startpaused":true,"name":"fade"},
            "c0":[{"frame":0,"value":1},{"frame":20,"value":0}]},
        "script":"export function init(){return thisLayer.getAnimation('fade') ? 0.5 : 1;}"}}]});
    let bindings = Bindings::parse(&mut scene).unwrap();
    let host = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
        crate::script::Load { keyframes: Some(&bindings), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["alpha"], json!(0.5));
}

fn property(mode: &str) -> Value {
    json!({"value":0,"animation":{"options":{"fps":30,"length":150,"mode":mode},
        "c0":[{"frame":0,"value":0},{"frame":63,"value":0},{"frame":150,"value":1}]}})
}

#[test]
fn single_sampling_matches_we() {
    for (fps, frame, expected) in [
        (10, 4.0, 1.099_804_401_397_705),
        (10, 5.0, 1.149_764_537_811_279_3),
        (10, 6.0, 1.149_764_537_811_279_3),
        (10, 10.0, 1.275_196_194_648_742_7),
        (8, 10.0, 1.249_990_820_884_704_6),
        (10, 10.5, 1.262_593_507_766_723_6),
    ] {
        let mut scene = json!({"value":1,"animation":{"options":{
            "fps":fps,"length":20,"mode":"single","startpaused":true},
            "c0":[{"frame":0,"value":1},{"frame":20,"value":1.5}]}});
        let mut bindings = Bindings::parse(&mut scene).unwrap();
        bindings.clocks.borrow_mut().control(0, Control::SetFrame, frame);
        bindings.apply(&mut scene, 0.0);
        assert!((scene.as_f64().unwrap() - expected).abs() < 0.000_000_2);
    }
}

#[test]
fn fractional_lengths_render() {
    let mut scene = property("single");
    scene["animation"]["options"]["length"] = json!(0.5);
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 1.0);
    assert!(scene.as_f64().unwrap().is_finite());
}

#[test]
fn invalid_periods_rejected() {
    for (fps, length) in [(1.0e-45, 20.0), (1.0e-30, 1.0e30)] {
        let mut scene = property("single");
        scene["animation"]["options"]["fps"] = json!(fps);
        scene["animation"]["options"]["length"] = json!(length);
        assert!(Bindings::parse(&mut scene).unwrap().is_empty());
    }
}

#[test]
fn overflow_seek_preserves_clock() {
    let mut scene = property("single");
    scene["animation"]["options"]["fps"] = json!(1.0e-20);
    let bindings = Bindings::parse(&mut scene).unwrap();
    let mut clocks = bindings.clocks.borrow_mut();
    clocks.control(0, Control::SetFrame, 10.0).unwrap();
    let before = clocks.control(0, Control::GetFrame, 0.0);
    assert!(clocks.control(0, Control::SetFrame, f32::MAX).is_none());
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), before);
}

#[test]
fn effect_opacity_endpoint() {
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
    assert!((scene.pointer(path).unwrap().as_f64().unwrap() - 1.0).abs() < 0.0001);
    let endpoint = scene.pointer(path).cloned();
    assert!(!bindings.animated());
    assert!(!bindings.apply(&mut scene, 500.0));
    assert_eq!(scene.pointer(path).cloned(), endpoint);
}

#[test]
fn loops_and_mirrors() {
    for (mode, value) in [("loop", 0.0), ("mirror", 0.999_999_880_790_710_4)] {
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
fn zero_rate_loop_settles() {
    let mut scale = property("loop");
    scale["animation"]["options"] =
        json!({"fps":8,"length":16,"mode":"loop","startpaused":true,"name":"move"});
    let script = r"
export function init() {
    const anim=thisLayer.getAnimation('move');
    anim.rate=0; anim.setFrame(-32.5); anim.play();
}
";
    let mut scene = json!({"objects":[{"id":1,"scale":scale,"alpha":{"value":1,"script":script}}]});
    let (mut bindings, mut host) = host(&mut scene);
    assert!(!host.animated());
    assert!(bindings.animated());
    for (frame, animated) in [(-0.5, true), (15.5, false)] {
        bindings.apply(&mut host.scene, 0.0);
        assert_eq!(bindings.clocks.borrow_mut().control(0, Control::GetFrame, 0.0), Some(frame));
        assert_eq!(bindings.animated(), animated);
        assert!(!host.animated());
    }
}

#[test]
fn zero_duration_stays_idle() {
    let mut scene = property("loop");
    scene["animation"]["options"]["fps"] = json!(1.0e30);
    scene["animation"]["options"]["length"] = json!(1.0e-30);
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.clocks.borrow_mut().control(0, Control::SetRate, 0.0);
    bindings.clocks.borrow_mut().control(0, Control::SetFrame, -1.0);
    bindings.clocks.borrow_mut().control(0, Control::Play, 0.0);
    bindings.apply(&mut scene, 1.0);
    assert!(!bindings.animated());
}

#[test]
fn relative_components() {
    let mut scene = json!({"origin":{"value":"100 200 30","animation":{"relative":true,
        "options":{"fps":10,"length":10,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":10,"value":40}]}}});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut first = Value::Null;
    for time in [0.5, 0.5, 0.5] {
        bindings.apply(&mut scene, time);
        assert!((scene["origin"][0].as_f64().unwrap() - 124.0).abs() < 0.02);
        assert_eq!(scene["origin"][1], json!(200.0));
        assert_eq!(scene["origin"][2], json!(30.0));
        if first.is_null() {
            first = scene["origin"].clone();
        }
        assert_eq!(scene["origin"], first);
    }
}

#[test]
fn parent_clock_pause() {
    let mut scene = json!({"objects":[{"scale":property("single"),"alpha":property("loop")}]});
    scene["objects"][0]["alpha"]["animation"]["options"]["parent"] = json!({"key":"scale"});
    scene["objects"][0]["scale"]["animation"]["options"]["startpaused"] = json!(true);
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 4.0);
    assert_eq!(scene["objects"][0]["alpha"], json!(0.0));
    assert!(!bindings.animated());
}

#[test]
fn wrap_loop_endpoint() {
    let mut scene = property("single");
    scene["animation"]["options"]["wraploop"] = json!(true);
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 5.0);
    assert_eq!(scene, json!(0.0));
}

#[test]
fn bezier_handle_shape() {
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
fn step_frame_blend() {
    let mut scene = json!({"value":0,"animation":{"options":{"fps":1,"length":10,"mode":"single"},
        "c0":[{"frame":0,"value":0},{"frame":10,"value":1,"step":true}]}});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 9.0);
    assert_eq!(scene, json!(0.0));
    bindings.apply(&mut scene, 9.5);
    assert_eq!(scene, json!(0.5));
}

#[test]
fn invalid_update() {
    let mut scene = property("loop");
    scene["animation"]["options"]["fps"] = json!(0);
    assert!(Bindings::parse(&mut scene).unwrap().is_empty());
    let mut scene = property("single");
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    assert!(!bindings.apply(&mut scene, f32::NAN));
    assert_eq!(scene, json!(0));
}

#[test]
fn parent_cycles() {
    let mut scene = json!({"a":property("loop"),"b":property("loop")});
    scene["a"]["animation"]["options"]["parent"] = json!({"key":"b"});
    scene["b"]["animation"]["options"]["parent"] = json!({"key":"a"});
    assert!(Bindings::parse(&mut scene).unwrap_err().to_string().contains("parent cycle"));
}

#[test]
fn script_initial_values() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"moving","origin":{"value":"100 200 0","animation":{"relative":true,
            "options":{"fps":30,"length":30,"mode":"single"},
            "c0":[{"frame":0,"value":0},{"frame":30,"value":40}],"c1":[],"c2":[]}}},
        {"id":2,"alpha":{"value":0,"script":"export function update(v){return thisScene.getLayer('moving').origin.x;}"}},
        {"id":3,"alpha":{"value":0,"script":"export function update(v){return thisScene.getInitialLayerConfig(thisScene.getLayer('moving')).origin.split(' ')[0]*1;}"}}
    ]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
        crate::script::Load { keyframes: Some(&bindings), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    bindings.apply(&mut host.scene, 0.5);
    host.sync_keyframes(&bindings);
    host.tick(0.5, 0.5, [0.5; 2]).unwrap();
    assert!((host.scene["objects"][1]["alpha"].as_f64().unwrap() - 120.0).abs() < 0.001);
    assert_eq!(host.scene["objects"][2]["alpha"], json!(100));
}

#[test]
fn script_timeline_override() {
    let mut alpha = property("single");
    alpha["script"] = json!("export function update(v){return v*0.5;}");
    let mut scene = json!({"objects":[{"id":1,"alpha":alpha}]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
        crate::script::Load { keyframes: Some(&bindings), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    for _ in 0..3 {
        bindings.apply(&mut host.scene, 5.0);
        host.sync_keyframes(&bindings);
        host.tick(5.0, 0.1, [0.5; 2]).unwrap();
        assert!((host.scene["objects"][0]["alpha"].as_f64().unwrap() - 0.5).abs() < 0.0001);
    }
}

#[test]
fn script_owner_replacement() {
    let mut scene = json!({"objects":[{"id":1,
        "effects":[{"passes":[{"constantshadervalues":{"alpha":property("single")}}]}],
        "alpha":{"value":0,"script":"export function update(v){if(engine.runtime<1)thisLayer.effects[0].passes[0].constantshadervalues={alpha:0.2};return thisLayer.effects[0].passes[0].constantshadervalues.alpha;}"}
    }]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
        crate::script::Load { keyframes: Some(&bindings), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    for time in [0.5, 5.0] {
        bindings.apply(&mut host.scene, time);
        host.sync_keyframes(&bindings);
        host.tick(time, 0.5, [0.5; 2]).unwrap();
    }
    assert!((host.scene["objects"][0]["alpha"].as_f64().unwrap() - 1.0).abs() < 0.0001);
}

#[test]
fn angle_units() {
    let mut scene = json!({"objects":[{"id":1,
        "angles":{"value":"0 0 0","animation":{"options":{"fps":30,"length":30,"mode":"single"},
            "c0":[],"c1":[],"c2":[{"frame":0,"value":0},{"frame":30,"value":std::f32::consts::FRAC_PI_2}]}},
        "alpha":{"value":0,"script":"export function update(v){return thisLayer.angles.z;}"}
    }]});
    let mut bindings = Bindings::parse(&mut scene).unwrap();
    let mut host = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        &std::collections::BTreeMap::default(),
        &Value::Null,
        crate::script::Load { keyframes: Some(&bindings), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    bindings.apply(&mut host.scene, 1.0);
    host.sync_keyframes(&bindings);
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!((host.scene["objects"][0]["alpha"].as_f64().unwrap() - 90.0).abs() < 0.001);
    let angles = crate::effects::json_numbers(&host.scene["objects"][0]["angles"]).unwrap();
    assert!((angles[2] - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
}

#[test]
fn ancestor_effects() {
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

#[test]
fn scene_animation_receiver() {
    let mut scale = property("single");
    scale["animation"]["options"] =
        json!({"fps":10,"length":20,"mode":"single","startpaused":true,"name":"move"});
    let script = r"
export function init() {
    const scene = thisScene.getAnimation('move');
    const first = thisScene.getLayer('first').getAnimation('move');
    const second = thisLayer.getAnimation('move');
    thisLayer.text = JSON.stringify([
        Object.hasOwn(thisScene, 'getAnimation'),
        Object.prototype.propertyIsEnumerable.call(thisScene, 'getAnimation'),
        thisScene.getAnimation.length,
        thisScene.getAnimation() === null,
        thisScene.getAnimation('absent') === null,
        thisLayer.getAnimation('absent') === undefined,
        scene === thisScene.getAnimation('move'), scene === first, scene !== second
    ]);
}
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"first","scale":scale},
        {"id":2,"name":"second","scale":scale,"text":{"value":"","script":script}}
    ]});
    host(&mut scene);
    assert_eq!(scene["objects"][1]["text"], "[true,true,0,true,true,true,true,true,true]");
}

#[test]
fn scene_animation_controls() {
    let mut scale = property("single");
    scale["animation"]["options"] =
        json!({"fps":10,"length":20,"mode":"single","startpaused":true,"name":"move"});
    let script = r"
export function init() {
    const scene = thisScene.getAnimation('move');
    const first = thisScene.getLayer('first').getAnimation('move');
    const second = thisLayer.getAnimation('move');
    scene.setFrame(10);
    const values = [scene.getFrame(), first.getFrame(), second.getFrame()];
    first.setFrame(5);
    values.push(scene.getFrame(), first.getFrame(), second.getFrame());
    scene.play();
    values.push(scene.isPlaying(), first.isPlaying(), second.isPlaying());
    scene.pause();
    values.push(scene.isPlaying(), first.isPlaying(), second.isPlaying());
    thisLayer.text = JSON.stringify(values);
}
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"first","scale":scale},
        {"id":2,"name":"second","scale":scale,"text":{"value":"","script":script}}
    ]});
    host(&mut scene);
    assert_eq!(scene["objects"][1]["text"], "[10,10,0,5,5,0,true,true,false,false,false,false]");
}

#[test]
fn scene_animation_sample() {
    let scale = json!({"value":"1 1 1","animation":{
        "options":{"fps":10,"length":20,"mode":"single","startpaused":true,"name":"move"},
        "c0":[{"frame":0,"value":1},{"frame":20,"value":1.5}],
        "c1":[{"frame":0,"value":1},{"frame":20,"value":1}],
        "c2":[{"frame":0,"value":1},{"frame":20,"value":1}]
    }});
    let script = r"
export function init() {
    thisScene.getAnimation('move').setFrame(10);
    thisLayer.getAnimation('move').setFrame(5);
    thisScene.getAnimation('move').play();
}
export function update() {
    const first = thisScene.getAnimation('move');
    const second = thisScene.getLayer('second').getAnimation('move');
    thisLayer.text = JSON.stringify([
        first.getFrame(), first.isPlaying(), thisLayer.scale.x,
        second.getFrame(), second.isPlaying(), thisScene.getLayer('second').scale.x
    ]);
    return 1;
}
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"first","text":"","scale":scale,"alpha":{"value":1,"script":script}},
        {"id":2,"name":"second","scale":scale}
    ]});
    let (mut bindings, mut host) = host(&mut scene);
    let time = 0.002_527_f32;
    bindings.apply(&mut host.scene, time);
    host.sync_keyframes(&bindings);
    host.tick(time, time, [0.5; 2]).unwrap();
    assert_eq!(
        host.scene["objects"][0]["text"],
        "[5.025269985198975,true,1.125614047050476,0,false,1]"
    );
}

#[test]
fn scene_animation_scale_assignment() {
    let scale = json!({"value":"1 1 1","animation":{
        "options":{"fps":10,"length":20,"mode":"single","startpaused":true,"name":"move"},
        "c0":[{"frame":0,"value":1},{"frame":20,"value":1.5}],
        "c1":[{"frame":0,"value":1},{"frame":20,"value":1}],
        "c2":[{"frame":0,"value":1},{"frame":20,"value":1}]
    }});
    let mut scene = json!({"objects":[{"id":5,"text":"","scale":scale,"alpha":{"value":1,"script":r"
export function init() {
    const animation = thisLayer.getAnimation('move');
    animation.setFrame(10);
    thisLayer.scale = new Vec3(1.4, 1, 1);
    thisLayer.text = JSON.stringify([animation.getFrame(), animation.isPlaying(), thisLayer.scale.x]);
}
export function update() { thisLayer.text = String(thisLayer.scale.x); return 1; }
"}}]});
    let (mut bindings, mut host) = host(&mut scene);
    assert_eq!(host.scene["objects"][0]["text"], "[10,false,1.399999976158142]");
    let time = 0.001_885_4_f32;
    bindings.apply(&mut host.scene, time);
    host.sync_keyframes(&bindings);
    host.tick(time, time, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "1.2751961946487427");
}

#[test]
fn failed_sync_stops_scripts() {
    for failure in ["null", "() => { throw new Error('full'); }"] {
        let script = format!(
            "export function init() {{ globalThis.__keyframe = {failure}; }}\nexport function update() {{ return 1; }}"
        );
        let mut scene = json!({"objects":[{"id":1,"name":"probe","origin":{"value":"900 540 0",
            "animation":{"options":{"fps":10,"length":20,"mode":"loop","name":"move"},
                "c0":[{"frame":0,"value":100},{"frame":20,"value":300}]}},
            "alpha":{"value":1,"script":script}}]});
        let (mut bindings, mut host) = host(&mut scene);
        bindings.apply(&mut host.scene, 1.0 / 60.0);
        host.sync_keyframes(&bindings);
        assert!(host.diagnostics.iter().any(|line| line.starts_with("keyframe values stopped")));
        let reports = host.diagnostics.len();
        host.sync_keyframes(&bindings);
        host.tick(1.0 / 60.0, 1.0 / 60.0, [0.5; 2]).unwrap();
        assert_eq!(host.diagnostics.len(), reports);
    }
}
