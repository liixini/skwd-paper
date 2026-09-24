use super::{Parallax, Properties, ancestor_chain, resolve_transform};
use serde_json::{Value, json};

#[test]
fn preset_scene_load_uses_parent_then_preset_then_explicit_properties() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("1");
    let preset = root.path().join("2");
    std::fs::create_dir(&base).unwrap();
    std::fs::create_dir(&preset).unwrap();
    std::fs::write(
        base.join("project.json"),
        r#"{"type":"scene","general":{"properties":{"tint":{"type":"color","value":"1 0 0"}}}}"#,
    )
    .unwrap();
    std::fs::write(preset.join("project.json"), r#"{"dependency":"1","preset":{"tint":"0 1 0"}}"#)
        .unwrap();
    let scene =
        json!({"general":{"clearcolor":{"user":"tint","value":"1 1 1"}},"objects":[]}).to_string();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(b"PKGV0007");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&10u32.to_le_bytes());
    bytes.extend_from_slice(b"scene.json");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(scene.len() as u32).to_le_bytes());
    bytes.extend_from_slice(scene.as_bytes());
    let package = crate::pkg::Package::parse(bytes).unwrap();
    assert_eq!(super::load_from_dir(&package, &base).unwrap().clear, [1.0, 0.0, 0.0]);
    assert_eq!(super::load_from_dir(&package, &preset).unwrap().clear, [0.0, 1.0, 0.0]);
    let properties = Properties::from([("tint".into(), vec![0.0, 0.0, 1.0])]);
    assert_eq!(
        super::load_from_dir_with(&package, &preset, &properties).unwrap().clear,
        [0.0, 0.0, 1.0]
    );
}

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

#[test]
fn animated_texture_retains_pages_and_uses_each_pages_dimensions() {
    let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [0i32, 4, 4, 2, 4, 2, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"TEXB0001\0");
    bytes.extend(2i32.to_le_bytes());
    for (width, height, color) in [(4i32, 2i32, [255u8, 0, 0, 255]), (2, 2, [0, 255, 0, 255])] {
        for value in [1, width, height, width * height * 4] {
            bytes.extend(value.to_le_bytes());
        }
        for _ in 0..width * height {
            bytes.extend(color);
        }
    }
    bytes.extend(b"TEXS0003\0");
    for value in [2i32, 2, 2] {
        bytes.extend(value.to_le_bytes());
    }
    for image in [0i32, 1] {
        bytes.extend(image.to_le_bytes());
        for value in [0.1f32, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0] {
            bytes.extend(value.to_le_bytes());
        }
    }
    let texture = super::load_texture_bytes(&bytes).unwrap();
    assert_eq!(texture.pages.len(), 1);
    assert_eq!(texture.payload_bytes(), 48);
    let frames = texture.atlas_frames().unwrap();
    assert_eq!(frames[0].uv, [0.0, 0.0, 0.5, 1.0]);
    assert_eq!(frames[1].uv, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(frames[1].image, 1);
    assert_eq!(texture.pages[0].base_rgba().unwrap(), [0, 255, 0, 255].repeat(4));
}

#[test]
fn hidden_script_layer_preserves_effect_source_alpha() {
    let mut scene = json!({"objects":[{"id":1,"visible":false},{"id":2,"parent":1,"color":"0.2 0.4 0.6","alpha":0.5}]});
    let layout = super::script::Layout {
        id: "2".into(),
        size: [64.0; 2],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
    };
    let frame = super::script::frames(
        &scene,
        std::slice::from_ref(&layout),
        (64.0, 64.0),
        &Properties::new(),
    )
    .remove(0)
    .unwrap();
    assert_eq!(frame.tint, [0.2, 0.4, 0.6, 0.0]);
    assert_eq!(frame.source_tint, [0.2, 0.4, 0.6, 0.5]);
    scene["objects"][0]["visible"] = json!(true);
    let shown = super::script::frames(&scene, &[layout], (64.0, 64.0), &Properties::new())
        .remove(0)
        .unwrap();
    assert_eq!(shown.tint, frame.source_tint);
    assert_eq!(shown.source_tint, frame.source_tint);
}

#[test]
fn playback_parallax_ignores_saved_editor_camera_offset() {
    let mut scene = scene(1.27);
    scene["camera"] = json!({"eye":"20.05524 289.45834 1", "center":"20.05524 289.45834 0"});
    let props = Properties::new();
    let parallax = Parallax::of(&scene, (3840.0, 2160.0), &props).unwrap();
    let background = json!({"origin":"1920 1080 0", "parallaxDepth":"-1.71 -1.71"});
    assert_eq!(parallax.offset(&background, &props), (0.0, 0.0));
}

#[test]
fn text_defaults_to_unit_depth_for_placement_and_pointer_motion() {
    let props = Properties::new();
    let parallax = Parallax::of(&scene(0.5), (1000.0, 800.0), &props).unwrap();
    let root = json!({"id":1,"text":"Clock", "origin":"700 500 0"});
    let child =
        json!({"id":2,"parent":1,"image":"child.json","origin":"10 0 0","parallaxDepth":"0 0"});
    let objects = [root.clone(), child.clone()];
    let map = by_id(&objects);
    let transform = resolve_transform(&child, &map, &props, Some(&parallax));
    assert_eq!(transform.origin, (810.0, 550.0, 0.0));
    let mouse = crate::mouse::Parallax { amount: 0.5, influence: 1.0, ..Default::default() };
    assert_eq!(
        super::layer_mouse(&child, &map, &props, mouse, transform, false).parallax,
        [1.0; 2]
    );
    let flat = json!({"text":"Clock", "origin":"700 500 0","parallaxDepth":"0 0"});
    assert_eq!(parallax.offset(&flat, &props), (0.0, 0.0));
    assert_eq!(super::parallax_depth(&json!({"image":"background.json"}), &props), (0.0, 0.0));
}
