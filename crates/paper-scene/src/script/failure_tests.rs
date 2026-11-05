use super::{Limits, Load, MAX_SCRIPT_BINDINGS, MAX_SCRIPT_SOURCE_BYTES, SceneScripts};
use crate::model::Properties;
use serde_json::{Value, json};
use std::time::Duration;

const COMPOSE: Duration = Duration::from_millis(100);

fn load(scene: &mut Value) -> Option<SceneScripts> {
    SceneScripts::load(scene, &Properties::new(), &json!({})).unwrap()
}

fn limited(scene: &mut Value) -> SceneScripts {
    let limits = Limits { load: Duration::from_millis(500), compose: COMPOSE };
    let options = Load { limits, ..Load::default() };
    SceneScripts::load_with_storage(scene, &Properties::new(), &json!({}), options)
        .unwrap()
        .unwrap()
}

fn scripted(script: &str) -> SceneScripts {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    load(&mut scene).unwrap()
}

fn eval(host: &SceneScripts, source: &str) {
    host.context.with(|ctx| ctx.eval::<(), _>(source)).unwrap();
}

fn text(host: &SceneScripts) -> &Value {
    &host.scene["objects"][0]["text"]
}

#[test]
fn update_throw_keeps_hooks() {
    let mut host = scripted(
        r"
let updates = 0;
export function update() { updates++; throw Error('update'); }
export function resizeScreen() { thisLayer.text = 'updates ' + updates; }
",
    );
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(!host.disabled);
    assert_eq!(host.diagnostics.len(), 1, "{:?}", host.diagnostics);
    assert!(host.resize_screen([800, 600]));
    assert_eq!(text(&host), "updates 1");
    assert!(!host.animated());
}

#[test]
fn init_throw_keeps_update() {
    let mut host = scripted(
        r"
export function init() { throw Error('init'); }
export function update() { return 'updated'; }
",
    );
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.diagnostics.len(), 1, "{:?}", host.diagnostics);
    assert_eq!(text(&host), "updated");
}

#[test]
fn hook_throw_keeps_update() {
    let mut host = scripted(
        r"
let resizes = 0;
export function resizeScreen() { resizes++; throw Error('resize'); }
export function update() { return 'resizes ' + resizes; }
",
    );
    host.resize_screen([800, 600]);
    host.resize_screen([640, 480]);
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert_eq!(text(&host), "resizes 1");
}

#[test]
fn neighbour_survives_throw() {
    let mut scene = json!({"objects":[
        {"id":1,"text":{"value":"","script":"export function update() { throw Error('first'); }"}},
        {"id":2,"text":{"value":"","script":"export function update() { return 'second'; }"}}
    ]});
    let mut host = load(&mut scene).unwrap();
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "second");
}

#[test]
fn interval_survives_throw() {
    let mut host = scripted(
        r"
let beats = 0;
export function init() { engine.setInterval(() => { beats++; throw Error('beat'); }, 1); }
export function update() { return 'beats ' + beats; }
",
    );
    for step in 1..4 {
        host.tick(step as f32 / 10.0, 0.1, [0.5; 2]).unwrap();
    }
    assert!(!host.disabled);
    assert_eq!(text(&host), "beats 2");
}

#[test]
fn ended_throw_keeps_frame() {
    let mut host = scripted("export function update() { return 'updated'; }");
    eval(
        &host,
        "__puppetCallbacks.set(7, [{callback() { throw Error('ended'); }}]); __puppetEnded(7);",
    );
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert!(!host.disabled);
    assert_eq!(text(&host), "updated");
}

#[test]
fn camera_throw_stops_scripts() {
    let mut host = scripted("export function update() { return 'updated'; }");
    eval(&host, "globalThis.__drainCamera = () => { throw Error('camera'); };");
    assert!(host.take_camera().is_none());
    assert!(host.disabled);
    assert!(host.take_camera().is_none());
    assert_eq!(host.diagnostics.len(), 1, "{:?}", host.diagnostics);
}

#[test]
fn camera_nan_keeps_zoom() {
    let mut host = scripted(
        r"
export function update() {
    if (engine.runtime < 1) thisScene.setCameraTransforms({eye:new Vec3(NaN, 0, 0), zoom:2});
    else if (engine.runtime < 2) thisScene.setCameraTransforms({eye:new Vec3(1, 2, 3), zoom:NaN});
    else thisScene.setCameraTransforms({zoom:3});
}
",
    );
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    let camera = host.take_camera().unwrap();
    assert!(camera.pose.is_none());
    assert_eq!(camera.zoom, Some(2.0));
    assert!(!host.disabled);
    host.tick(1.5, 0.016, [0.5; 2]).unwrap();
    let camera = host.take_camera().unwrap();
    assert_eq!(camera.pose.unwrap().eye, [1.0, 2.0, 3.0]);
    assert_eq!(camera.zoom, None);
    host.tick(2.5, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.take_camera().unwrap().zoom, Some(3.0));
}

#[test]
fn playing_throw_stops_scripts() {
    let states = [("1".to_owned(), true)];
    let cases: [(&str, fn(&mut SceneScripts, &[(String, bool)])); 2] = [
        ("__setSounds", SceneScripts::set_sound_states),
        ("__setParticles", SceneScripts::set_particle_states),
    ];
    for (receiver, send) in cases {
        let mut host = scripted("export function update() { return 'updated'; }");
        eval(&host, &format!("globalThis.{receiver} = () => {{ throw Error('playing'); }};"));
        send(&mut host, &states);
        assert!(host.disabled, "{receiver}");
    }
}

#[test]
fn videos_throw_stops_scripts() {
    let mut host = scripted("export function update() { return 'updated'; }");
    eval(&host, "globalThis.__setVideos = () => { throw Error('video'); };");
    host.set_videos(&[], false);
    assert!(host.disabled);
}

#[test]
fn order_throw_stops_scripts() {
    let mut host = scripted("export function update() { return 'updated'; }");
    eval(&host, "globalThis.__restoreLayerOrder = () => { throw Error('order'); };");
    host.reject_layer_order();
    assert!(host.disabled);
}

#[test]
fn init_listener_stall_loads() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"authored","script":"scene.on('init', () => { while (true) {} });"}}]});
    let host = limited(&mut scene);
    assert!(host.disabled);
    assert_eq!(scene["objects"][0]["text"], "authored");
}

#[test]
fn oversized_source_loads_static() {
    let script = "x".repeat(MAX_SCRIPT_SOURCE_BYTES + 1);
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"authored","script":script}}]});
    assert!(load(&mut scene).is_none());
    assert_eq!(scene["objects"][0]["text"], "authored");
}

#[test]
fn excess_bindings_load_static() {
    let binding = json!({"value":"authored","script":"export function init() {}"});
    let objects: Vec<Value> =
        (0..=MAX_SCRIPT_BINDINGS).map(|id| json!({"id":id,"text":binding})).collect();
    let mut scene = json!({"objects":objects});
    assert!(load(&mut scene).is_none());
    assert_eq!(scene["objects"][0]["text"], "authored");
}

#[test]
fn setup_failure_loads_static() {
    let mut nested = json!(0);
    for _ in 0..5000 {
        nested = json!([nested]);
    }
    let mut scene = json!({"extra":nested,"objects":[{"id":1,"text":{"value":"authored","script":"export function update() { return 'x'; }"}}]});
    assert!(load(&mut scene).is_none());
    assert_eq!(scene["objects"][0]["text"], "authored");
}

#[test]
fn bone_world_stays_lazy() {
    let mut host = scripted("export function update() { return 'updated'; }");
    eval(
        &host,
        r"
globalThis.poses = 0;
const pose = __worldPose;
globalThis.__worldPose = layer => { poses++; return pose(layer); };
__boneWorlds.set(__layers[0], {world:null, edits:new Map(), slot:0, springs:false});
",
    );
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert!(!host.diagnostics.iter().any(|message| message.contains("poses")));
    let poses = host.context.with(|ctx| ctx.eval::<i32, _>("poses")).unwrap();
    assert_eq!(poses, 0);
}

#[test]
fn compose_shares_deadline() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
const burn = () => { const start = Date.now(); while (Date.now() - start < 70) {} };
export function resizeScreen() { burn(); }
export function update() { burn(); return 'updated'; }
"}}]});
    let mut host = limited(&mut scene);
    assert!(!host.disabled);
    host.resize_screen([800, 600]);
    assert!(!host.disabled, "{:?}", host.diagnostics);
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert!(host.disabled);
}

#[test]
fn fresh_compose_renews_deadline() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
export function update() { const start = Date.now(); while (Date.now() - start < 70) {} return 'updated'; }
"}}]});
    let mut host = limited(&mut scene);
    for step in 1..4 {
        host.tick(step as f32, 0.016, [0.5; 2]).unwrap();
        host.finish_compose();
    }
    assert!(!host.disabled, "{:?}", host.diagnostics);
}

#[test]
fn spent_loop_stops_host() {
    let objects: Vec<Value> = (0..4)
        .map(|id| json!({"id":id,"text":{"value":"","script":r"
export function update() { const start = Date.now(); while (Date.now() - start < 40) {} return 'updated'; }
"}}))
        .collect();
    let mut host = limited(&mut json!({"objects":objects}));
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert!(host.disabled);
    let started = std::time::Instant::now();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(started.elapsed() < COMPOSE);
}

#[test]
fn stop_settles_retirement() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"victim","alpha":{"value":1,"script":"export function destroy() { while (true) {} }"}},
        {"id":2,"name":"child","parent":1},
        {"id":3,"text":{"value":"","script":"let done = false; export function update() { if (!done) { done = true; thisScene.destroyLayer(thisScene.getLayer('victim')); } }"}}
    ]});
    let mut host = limited(&mut scene);
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert!(host.disabled);
    assert_eq!(host.scene["objects"][0]["visible"], false);
    assert_eq!(host.scene["objects"][1]["parent"], Value::Null);
    host.deadline.cleanup();
    let settled = host
        .context
        .with(|ctx| {
            ctx.eval::<bool, _>("__current === null && __retiringLayers === null && !__nativeWrite")
        })
        .unwrap();
    assert!(settled);
}

#[test]
fn initial_config_full() {
    let mut scene = json!({"objects":[{"id":1,"name":"n","instanceoverride":{"alpha":0.5},"text":{"value":"","script":r"
export function init() {
    const config = thisScene.getInitialLayerConfig(thisLayer);
    return config.instanceoverride.alpha + ' ' + config.name + ' ' + ('id' in config);
}"}}]});
    let host = load(&mut scene).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(text(&host), "0.5 n false");
}

fn heap_growth(host: &SceneScripts, source: &str) -> usize {
    let before = host.heap_bytes();
    eval(host, source);
    host.heap_bytes() - before
}

#[test]
fn method_guards_shared() {
    let host = scripted("export function init() {}");
    let installs = heap_growth(
        &host,
        "{ const layer = __layers[0]; const method = () => 0; for (let i = 0; i < 400; i++) __installMethod(layer, 'probe' + i, method); }",
    );
    let guarded = heap_growth(
        &host,
        "{ const method = () => 0; globalThis.guards = []; for (let i = 0; i < 400; i++) guards.push(new Proxy(method, {apply(target) { return target(); }})); }",
    );
    assert!(installs < guarded, "{installs} {guarded}");
}

#[test]
fn authored_layers_stay_text() {
    let nested = json!([[(0..4000).map(|value| value % 10).collect::<Vec<_>>()]]);
    let bytes = |content: &Value| {
        let mut objects: Vec<Value> =
            (2..42).map(|id| json!({"id":id,"name":"layer","probe":content})).collect();
        objects.push(json!({"id":1,"text":{"value":"","script":"export function init() {}"}}));
        load(&mut json!({"objects":objects})).unwrap().heap_bytes()
    };
    let per_layer = (bytes(&nested) - bytes(&json!(0))) / 40;
    let host = scripted("export function init() {}");
    let parsed = heap_growth(&host, &format!("globalThis.copies = [JSON.parse('{nested}')];"));
    assert!(per_layer * 2 < parsed * 3, "{per_layer} {parsed}");
}

#[test]
fn hostile_listener_keeps_scripts() {
    let mut host = scripted(
        r"
scene.on('update', () => { throw {toString() { throw 1; }}; });
export function update() { return 'updated'; }
",
    );
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert!(!host.disabled, "{:?}", host.diagnostics);
    assert_eq!(text(&host), "updated");
}

#[test]
fn animation_event_applies_result() {
    let mut host =
        scripted("export function animationEvent(event, value) { return value + event.name; }");
    eval(&host, "__puppetEvent(0, 'step', 1);");
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert_eq!(text(&host), "step");
}

#[test]
fn animation_event_masks_throw() {
    let mut host = scripted(
        r"
let calls = 0;
export function animationEvent() { calls++; throw Error('event'); }
export function update() { return 'calls ' + calls; }
",
    );
    eval(&host, "__puppetEvent(0, 'step', 1); __puppetEvent(0, 'step', 2);");
    host.tick(0.5, 0.016, [0.5; 2]).unwrap();
    assert_eq!(text(&host), "calls 1");
}

#[test]
fn bone_setters_snapshot() {
    let host =
        scripted("export function update() { thisLayer.setLocalBoneTransform(0, new Mat4()); }");
    let readers = host.context.with(|ctx| ctx.eval::<bool, _>("__boneWorldReaders")).unwrap();
    assert!(readers);
}
