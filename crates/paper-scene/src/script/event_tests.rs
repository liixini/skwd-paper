use super::SceneScripts;
use crate::model::Properties;
use serde_json::json;

fn host(script: &str) -> SceneScripts {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap()
}

#[test]
fn event_order_and_removal() {
    let mut host = host(
        r"
let calls = [];
const second = () => calls.push('second');
scene.on('init', () => calls.push('init'));
scene.on('applyUserProperties', () => calls.push('properties'));
scene.on('applyGeneralSettings', () => calls.push('settings'));
scene.on('update', () => { calls.push('first'); scene.off('update', second); });
scene.on('update', second);
scene.on('update', () => { thisLayer.text = calls.join(','); scene.off('update'); });
export function init() { calls.push('export'); }
",
    );
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "export,init,properties,settings,first");
    assert!(!host.animated());
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "export,init,properties,settings,first");
}

#[test]
fn event_cursor_and_media() {
    let mut host = host(
        r"
let calls = [];
scene.on('cursorDown', event => calls.push('down:' + event.cursorWorldPosition.x));
scene.on('cursorClick', () => calls.push('click'));
scene.on('resizeScreen', (width, height) => calls.push(width + 'x' + height));
scene.on('mediaPropertiesChanged', event => calls.push(event.title));
scene.on('update', () => { thisLayer.text = calls.join(','); });
",
    );
    assert!(host.needs_media());
    assert!(host.animated());
    host.pointer([0.5; 2], [true, false, false], vec![]).unwrap();
    host.pointer([0.5; 2], [false; 3], vec![]).unwrap();
    host.resize_screen([800, 600]);
    let media = json!({"mediaPropertiesChanged":{"title":"Song"}});
    host.media(&media).unwrap();
    host.media(&media).unwrap();
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "down:960,click,800x600,Song");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn event_owner_retirement() {
    let mut host = host(
        r"
scene.on('mediaPropertiesChanged', () => { thisLayer.text = 'stale'; });
scene.on('update', () => { thisScene.destroyLayer(thisLayer); });
",
    );
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(!host.needs_media());
    assert!(!host.animated());
    host.media(&json!({"mediaPropertiesChanged":{"title":"Song"}})).unwrap();
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    assert_ne!(host.scene["objects"][0]["text"], "stale");
}

#[test]
fn event_listener_failure() {
    let mut host = host(
        r"
scene.on('update', () => { throw Error('listener failed'); });
scene.on('update', () => { thisLayer.text = 'continued'; });
",
    );
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "continued");
}

#[test]
fn shortcut_requires_click() {
    let mut host = host(
        r"
scene.on('userShortcut', event => { thisLayer.text += event.name + ';'; });
export function init() { if (engine.openUserShortcut('next')) throw Error('no click'); }
export function cursorDown() {
    engine.openUserShortcut('next');
    try { engine.openUserShortcut('previous'); } catch (_) { thisLayer.text += 'blocked;'; }
}
export function cursorClick() {
    try { engine.openUserShortcut('play'); } catch (_) { thisLayer.text += 'blocked;'; }
}
",
    );
    assert!(host.take_commands().is_empty());
    for _ in 0..2 {
        host.pointer([0.5; 2], [true, false, false], vec!["1".into()]).unwrap();
        host.pointer([0.5; 2], [false; 3], vec!["1".into()]).unwrap();
    }
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "next;blocked;blocked;next;blocked;blocked;");
    assert_eq!(
        host.take_commands(),
        [
            super::ScriptCommand::Shortcut { name: "next".into() },
            super::ScriptCommand::Shortcut { name: "next".into() },
        ]
    );
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn shortcut_from_listener() {
    let mut host = host(
        r"
scene.on('userShortcut', event => { thisLayer.text = event.name; });
scene.on('cursorClick', () => engine.openUserShortcut('playpause'));
",
    );
    host.pointer([0.5; 2], [true, false, false], vec![]).unwrap();
    host.pointer([0.5; 2], [false; 3], vec![]).unwrap();
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "playpause");
    assert_eq!(host.take_commands(), [super::ScriptCommand::Shortcut { name: "playpause".into() }]);
}

#[test]
fn shortcut_rejects_outside() {
    for (press, release) in [([-1.0; 2], [0.5; 2]), ([0.5; 2], [-1.0; 2]), ([-1.0; 2], [-1.0; 2])] {
        let mut host = host("scene.on('cursorClick', () => engine.openUserShortcut('playpause'));");
        host.pointer(press, [true, false, false], vec![]).unwrap();
        host.pointer(release, [false; 3], vec![]).unwrap();
        host.tick(1.0, 0.1, release).unwrap();
        assert!(host.take_commands().is_empty());
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
}

#[test]
fn shortcut_up_requires_hit() {
    let mut host = host("export function cursorUp() { engine.openUserShortcut('playpause'); }");
    host.pointer([0.5; 2], [true, false, false], vec!["1".into()]).unwrap();
    host.pointer([-1.0; 2], [false; 3], vec![]).unwrap();
    host.tick(1.0, 0.1, [-1.0; 2]).unwrap();
    assert!(host.take_commands().is_empty());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn outside_release_clears_drag() {
    let mut host = host(
        r"
scene.on('cursorDown', () => { thisLayer.text='down'; });
scene.on('cursorUp', () => { thisLayer.text+='up'; engine.openUserShortcut('playpause'); });
scene.on('cursorClick', () => { thisLayer.text+='click'; });
",
    );
    host.pointer([0.5; 2], [true, false, false], vec![]).unwrap();
    host.pointer([-1.0; 2], [false; 3], vec![]).unwrap();
    host.tick(1.0, 0.1, [-1.0; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "downup");
    assert!(host.take_commands().is_empty());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}
