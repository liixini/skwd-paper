use super::SceneScripts;
use crate::model::Properties;
use serde_json::json;

#[test]
fn no_scripts_creates_no_runtime_and_preserves_scene() {
    let mut scene = json!({"objects":[{"id":1,"alpha":0.5}]});
    let original = scene.clone();
    assert!(
        SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
            .unwrap()
            .is_none()
    );
    assert_eq!(scene, original);
}

#[test]
fn modules_keep_independent_state_and_init_return_values() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0.0,"script":"let n=0; export function init(value){return 0.25;} export function update(value){n++; return value+0.1;}"}},{"id":2,"alpha":{"value":0.0,"script":"let n=0; export function update(value){n++; return n;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!((host.scene["objects"][0]["alpha"].as_f64().unwrap() - 0.45).abs() < 1e-6);
    assert_eq!(host.scene["objects"][1]["alpha"], 2);
}

#[test]
fn vector_returns_and_cross_layer_writes_reach_scene() {
    let mut scene = json!({"objects":[{"id":1,"name":"first","origin":{"value":"10 20 0","script":"import * as WEMath from 'WEMath'; export function update(value){thisScene.getLayer('other').visible=false; return value.add(new Vec3(1,2,0));}"}},{"id":2,"name":"other","visible":true}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["origin"], "11 22 0");
    assert_eq!(scene["objects"][1]["visible"], false);
}

#[test]
fn infinite_loop_is_stopped_by_frame_budget() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1.0,"script":"export function update(){while(true){}}"}}]});
    let start = std::time::Instant::now();
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(!host.animated());
    assert!(!host.diagnostics.is_empty());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn unchanged_text_does_not_request_a_render_update() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"hi","script":"export function update(value){return value;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(!host.tick(1.0, 0.016, [0.5; 2]).unwrap());
}

#[test]
fn saved_script_properties_override_declared_defaults() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "scriptproperties":{"label":"saved","choice":"b"},"script":"export const scriptProperties=createScriptProperties().addText({name:'label',value:'default'}).addCombo({name:'choice',options:[{value:'a'},{value:'b'}]}).finish(); export function update(){return scriptProperties.label+scriptProperties.choice;}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "savedb");
}

#[test]
fn failed_update_does_not_stop_other_modules() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function update(){throw Error('bad script');}"}},{"id":2,"alpha":{"value":0,"script":"export function update(v){return v+1;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.animated());
    assert_eq!(host.diagnostics.len(), 1);
    assert_eq!(host.scene["objects"][1]["alpha"], 2);
}

#[test]
fn timer_only_script_remains_scheduled_until_deadline() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"export function init(){engine.setTimeout(()=>{thisLayer.alpha=1;},500);}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.animated());
    host.tick(0.2, 0.2, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 0);
    host.tick(0.6, 0.4, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    assert!(!host.animated());
}

#[test]
fn click_requires_press_and_release_over_the_same_layer() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"export function cursorClick(){thisLayer.alpha+=1;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [true, false, false], vec!["1".into()]).unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    host.pointer([0.2; 2], [true, false, false], vec!["1".into()]).unwrap();
    host.pointer([0.8; 2], [false; 3], vec![]).unwrap();
    host.tick(2.0, 0.016, [0.8; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
}

#[test]
fn heap_exhaustion_stops_script_without_crashing_host() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function update(){const a=new ArrayBuffer(64*1024*1024);return a.byteLength;}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(!host.animated());
    assert_eq!(host.diagnostics.len(), 1);
    assert!(host.heap_bytes() < 32 * 1024 * 1024);
}

#[test]
fn runaway_pointer_is_disabled_after_one_failure() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function cursorEnter(){while(true){}}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    let count = host.diagnostics.len();
    assert_eq!(count, 1);
    host.pointer([0.3; 2], [false; 3], vec!["1".into()]).unwrap();
    assert_eq!(host.diagnostics.len(), count);
    assert!(!host.animated());
}

#[test]
fn oversized_frame_output_stops_scripts_and_preserves_scene() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"safe","script":"export function update(v){return engine.runtime > 0 ? 'x'.repeat(1100000) : v;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(!host.animated());
    assert_eq!(host.scene["objects"][0]["text"], "safe");
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn failed_init_cannot_receive_later_cursor_events() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"export function init(){throw Error('failed');} export function cursorEnter(){thisLayer.alpha=1;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 0);
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn scripts_receive_resolved_user_property_values() {
    let mut scene = json!({"objects":[{"id":1,"visible":{"value":true,"user":"enabled"},"alpha":{"value":0.2,"user":"opacity","script":"export function init(v){return thisLayer.visible ? 1 : v;}"}}]});
    let props = Properties::from([("enabled".into(), vec![0.0]), ("opacity".into(), vec![0.75])]);
    let host = SceneScripts::load(&mut scene, &props, &serde_json::Value::Null).unwrap().unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(scene["objects"][0]["alpha"], 0.75);
}

#[test]
fn project_properties_preserve_text_booleans_and_colors() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":"export function update(){return engine.userProperties.name + ':' + (engine.userProperties.Enabled === false) + ':' + engine.userProperties.tint.x;}"}}]});
    let project = json!({"general":{"properties":{"name":{"type":"textinput","value":"YOURNAME"},"Enabled":{"type":"bool","value":true},"tint":{"type":"color","value":"0.5 0.25 1"}}}});
    let mut props = crate::effects::parse_properties(&project);
    props.insert("enabled".into(), vec![0.0]);
    let host = SceneScripts::load(&mut scene, &props, &project).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "YOURNAME:true:0.5");
}

#[test]
fn storage_preserves_vectors_across_reloads_and_isolates_screens() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let make = || json!({"objects":[{"id":1,"origin":{"value":"0 0 0","script":"export function init(v){const old=localStorage.get('position');localStorage.set('position',new Vec3(12,34,0));localStorage.set('global',7,localStorage.LOCATION_GLOBAL);return old ? old.add(new Vec3(1,0,0)) : v;}"}}]});
    let mut scene = make();
    let storage = super::Storage::at(root.path(), project.path(), "DP-1");
    SceneScripts::load_with_storage(&mut scene, &Properties::new(), &json!({}), storage).unwrap();
    let mut scene = make();
    let storage = super::Storage::at(root.path(), project.path(), "DP-1");
    let host = SceneScripts::load_with_storage(&mut scene, &Properties::new(), &json!({}), storage)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["origin"], "13 34 0");
    let mut scene = make();
    let storage = super::Storage::at(root.path(), project.path(), "DP-2");
    let host = SceneScripts::load_with_storage(&mut scene, &Properties::new(), &json!({}), storage)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(scene["objects"][0]["origin"], "0 0 0");
    assert_eq!(
        super::Storage::at(root.path(), project.path(), "DP-3").unwrap().data["global"]["global"],
        7
    );
}

#[test]
fn storage_limit_stops_only_the_offending_script() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function init(){localStorage.set('oversized','x'.repeat(102401));}"}},{"id":2,"alpha":{"value":0,"script":"export function update(){return 0.75;}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(host.diagnostics.len(), 1);
    assert_eq!(scene["objects"][1]["alpha"], 0.75);
}

#[test]
fn parent_access_and_documented_cursor_world_position_allow_dragging() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"name":"parent","origin":"10 20 0"},{"id":2,"parent":1,"origin":{"value":"0 0 0","script":"let down=false;export function init(v){return thisLayer.getParent().origin.copy();} export function cursorDown(e){down=true;} export function cursorMove(e){if(down)thisLayer.origin=e.worldPosition.copy();} export function cursorUp(e){down=false;localStorage.set('position',thisLayer.origin);}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(scene["objects"][1]["origin"], "10 20 0");
    host.pointer([0.1, 0.2], [true, false, false], vec!["2".into()]).unwrap();
    host.pointer([0.7, 0.6], [true, false, false], vec![]).unwrap();
    host.pointer([0.7, 0.6], [false; 3], vec![]).unwrap();
    host.tick(1.0, 0.1, [0.7, 0.6]).unwrap();
    let origin = host.scene["objects"][1]["origin"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .map(|n| n.parse::<f32>().unwrap())
        .collect::<Vec<_>>();
    assert!((origin[0] - 70.0).abs() < 0.001 && (origin[1] - 40.0).abs() < 0.001);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn media_callbacks_keep_state_and_do_not_repeat_unchanged_events() {
    let mut scene = json!({"objects":[{"id":1,"visible":false,"text":{"value":"","script":"let n=0;let title='';export function mediaPropertiesChanged(e){n++;title=e.title;}export function mediaPlaybackChanged(e){thisLayer.visible=e.state===MediaPlaybackEvent.PLAYBACK_PLAYING;}export function update(){return title+':'+n;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.needs_media());
    let events =
        json!({"mediaPropertiesChanged":{"title":"Track"},"mediaPlaybackChanged":{"state":1}});
    host.media(&events).unwrap();
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    host.media(&events).unwrap();
    host.tick(2.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "Track:1");
    assert_eq!(host.scene["objects"][0]["visible"], true);
    host.media(&json!({"mediaPlaybackChanged":{"state":0}})).unwrap();
    host.tick(3.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["visible"], false);
}

#[test]
fn color_module_converts_hsv_and_preserves_hdr_values() {
    let mut scene = serde_json::json!({"objects":[{"id":1,"color":{"value":"0 0 0","script":"import * as C from 'WEColor'; export function update() { const rgb=C.hsv2rgb(new Vec3(0.5,1,1.6)); const hsv=C.rgb2hsv(rgb); if(Math.abs(hsv.x-0.5)>0.0001 || Math.abs(hsv.z-1.6)>0.0001) throw Error('roundtrip'); if(!C.normalizeColor(C.expandColor(new Vec3(1,0,0))).equals(new Vec3(1,0,0))) throw Error('range'); return rgb; }"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::default(), &serde_json::json!({}))
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["color"], "0 1.6 1.6");
}

#[test]
fn cursor_local_position_accounts_for_parent_rotation_and_scale() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"origin":"10 20 0","scale":"2 2 1","angles":"0 0 1.5707963267948966"},{"id":2,"parent":1,"size":"20 10","origin":{"value":"5 0 0","script":"export function cursorDown(e){shared.local=e.localPosition;} export function update(v){return shared.local || v;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.pointer([0.1, 0.7], [true, false, false], vec!["2".into()]).unwrap();
    host.tick(1.0, 0.1, [0.1, 0.7]).unwrap();
    let p = host.scene["objects"][1]["origin"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .map(|v| v.parse::<f32>().unwrap())
        .collect::<Vec<_>>();
    assert!((p[0] - 10.0).abs() < 0.001 && (p[1] - 5.0).abs() < 0.001, "{p:?}");
}
