use super::{Parallax, Properties, ancestor_chain, resolve_transform};
use serde_json::{Value, json};

fn scene(amount: f64) -> Value {
    json!({"general": {"cameraparallax": true, "cameraparallaxamount": amount}})
}

fn by_id(objects: &[Value]) -> std::collections::HashMap<String, &Value> {
    objects.iter().filter_map(|object| Some((object.get("id")?.to_string(), object))).collect()
}

#[test]
fn parallax_pushes_a_layer_away_from_the_canvas_centre_by_amount_times_depth() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    let parallax = Parallax::of(&scene(0.5), canvas, &props).expect("enabled");
    let object = json!({"id": 1, "origin": "700 500 0", "parallaxDepth": "2 0.5"});
    let objects = [object.clone()];
    let placed = resolve_transform(&object, &by_id(&objects), &props, Some(&parallax));
    assert!((placed.origin.0 - (700.0 + (700.0 - 500.0) * 0.5 * 2.0)).abs() < 1e-3);
    assert!((placed.origin.1 - (500.0 + (500.0 - 400.0) * 0.5 * 0.5)).abs() < 1e-3);
}

#[test]
fn a_layer_at_the_focus_point_and_a_layer_without_depth_do_not_move() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    let parallax = Parallax::of(&scene(0.5), canvas, &props).expect("enabled");
    let centred = json!({"id": 1, "origin": "500 400 0", "parallaxDepth": "3 3"});
    let depthless = json!({"id": 2, "origin": "700 500 0"});
    for object in [&centred, &depthless] {
        let objects = [(*object).clone()];
        let base = resolve_transform(object, &by_id(&objects), &props, None);
        let placed = resolve_transform(object, &by_id(&objects), &props, Some(&parallax));
        assert!((placed.origin.0 - base.origin.0).abs() < 1e-3);
        assert!((placed.origin.1 - base.origin.1).abs() < 1e-3);
    }
}

#[test]
fn a_child_inherits_the_root_ancestors_offset_and_ignores_its_own_depth() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    let parallax = Parallax::of(&scene(0.5), canvas, &props).expect("enabled");
    let root = json!({"id": 1, "origin": "700 400 0", "parallaxDepth": "2 2"});
    let child = json!({"id": 2, "parent": 1, "origin": "10 0 0", "parallaxDepth": "50 50"});
    let objects = [root.clone(), child.clone()];
    let map = by_id(&objects);
    assert_eq!(ancestor_chain(&child, &map).len(), 2);
    let base = resolve_transform(&child, &map, &props, None);
    let placed = resolve_transform(&child, &map, &props, Some(&parallax));
    assert!((placed.origin.0 - base.origin.0 - (700.0 - 500.0) * 0.5 * 2.0).abs() < 1e-3);
    assert!((placed.origin.1 - base.origin.1).abs() < 1e-3);
}

#[test]
fn parallax_is_off_unless_the_scene_enables_it_and_defaults_to_half_amount() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    assert!(Parallax::of(&json!({"general": {}}), canvas, &props).is_none());
    assert!(Parallax::of(&json!({"general": {"cameraparallax": false}}), canvas, &props).is_none());
    let defaulted =
        Parallax::of(&json!({"general": {"cameraparallax": true}}), canvas, &props).expect("on");
    let object = json!({"id": 1, "origin": "600 400 0", "parallaxDepth": "1 1"});
    assert!((defaulted.offset(&object, &props).0 - 50.0).abs() < 1e-3);
}
