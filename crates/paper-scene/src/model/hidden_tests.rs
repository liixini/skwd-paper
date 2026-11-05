use super::*;
use serde_json::json;

#[test]
fn visibility_writes_detected() {
    for (source, writes) in [
        ("thisLayer.visible = false;", true),
        ("layer.visible=true", true),
        ("layer['visible'] = true;", true),
        ("layer.visible ||= shown;", true),
        ("Object.assign(layer, {visible: true});", true),
        ("return thisLayer.visible;", true),
        ("child.parent = shown;", true),
        ("if (layer.visible == true) {}", false),
        ("if (layer.visible === shown) {}", false),
        ("layer.origin = point;", false),
    ] {
        assert_eq!(assigns_visibility(source), writes, "{source}");
    }
}

#[test]
fn deferrable_hidden_layers() {
    let scene = json!({"objects":[
        {"id":1,"visible":false},
        {"id":2,"visible":{"user":"show","value":false}},
        {"id":3,"visible":{"value":false,"script":"export function update(v){return v;}"}},
        {"id":4,"visible":{"value":false,"animation":{}}},
        {"id":5,"parent":3,"visible":false},
        {"id":6,"parent":1,"visible":false}
    ]});
    let hidden = HiddenLayers::of(&scene);
    assert!(hidden.deferrable(0) && hidden.deferrable(1) && hidden.deferrable(5));
    assert!(!hidden.deferrable(2) && !hidden.deferrable(3) && !hidden.deferrable(4));
    let written = json!({"objects":[{"id":1,"visible":false},
        {"id":2,"origin":{"value":"0 0 0","script":"thisScene.getLayer('a').visible = true;"}}]});
    assert!(!HiddenLayers::of(&written).deferrable(0));
}
