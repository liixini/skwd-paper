use super::*;
use serde_json::json;

fn project() -> Value {
    json!({"general":{"properties":{"style":{"type":"customshader","value":0,
        "options":[{"value":0,"shader":"red"},{"value":1,"shader":"green"},{"value":2,"shader":"broken"}]}}}})
}

#[test]
fn choices_validate() {
    for options in [
        json!([]),
        json!(vec![json!({"value":0,"shader":"red"}); MAX_CHOICES + 1]),
        json!([{"value":0,"shader":"x".repeat(MAX_SHADER_NAME_BYTES + 1)}]),
        json!([{"value":0,"shader":"red"},{"value":0,"shader":"green"}]),
        json!([{"value":0,"shader":"../escape"}]),
        json!([{"value":0,"shader":"/escape"}]),
        json!([{"value":0,"shader":"..\\escape"}]),
        json!([{"value":0,"shader":""}]),
        json!([{"value":1e100,"shader":"red"}]),
    ] {
        let mut project = project();
        project["general"]["properties"]["style"]["options"] = options;
        assert!(selected(&project, &super::super::parse_properties(&project)).is_err());
    }
}

#[test]
fn choices_switch_reset() {
    let project = project();
    let defaults = super::super::parse_properties(&project);
    assert_eq!(selected(&project, &defaults).unwrap()["style"], "red");
    let green = Properties::from([("style".into(), vec![1.0])]);
    assert_eq!(selected(&project, &green).unwrap()["style"], "green");
    assert!(change(&project, &defaults, &green) == Change::Reload);
    assert!(change(&project, &green, &Properties::new()) == Change::Reload);
    assert!(change(&project, &defaults, &Properties::new()) == Change::Unchanged);
    assert!(selected(&project, &Properties::from([("style".into(), vec![9.0])])).is_err());
}

#[test]
fn shader_rejection_recovers() {
    let material =
        json!({"passes":[{"shader":{"value":"red","user":"style"},"textures":["white"]}]})
            .to_string();
    let texture = crate::tests::build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let vertex = b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/image.json","size":"32 32"}]}"#),
        ("models/image.json", br#"{"material":"materials/image.json"}"#),
        ("materials/image.json", material.as_bytes()),
        ("materials/white.tex", &texture),
        ("shaders/red.vert", vertex),
        ("shaders/green.vert", vertex),
        ("shaders/broken.vert", vertex),
        ("shaders/red.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ("shaders/green.frag", b"void main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}"),
    ]))
    .unwrap();
    let project = project();
    for (index, expected) in
        [(0.0, Some("red")), (2.0, None), (1.0, Some("green")), (0.0, Some("red"))]
    {
        let assets = Assets::discover(Some("/nonexistent"))
            .with_properties(Properties::from([("style".into(), vec![index])]))
            .with_custom_shaders(&project)
            .unwrap();
        let model = crate::model::load_with(&package, &assets);
        if let Some(expected) = expected {
            let model = model.unwrap();
            assert!(model.custom_shaders);
            assert!(model.skipped.is_empty());
            assert_eq!(model.layers[0].material.as_ref().unwrap().pass.name, expected);
        } else {
            assert!(model.is_err());
        }
    }
}

#[test]
fn hidden_shader_rejected() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/flat.json","size":"32 32","effects":[{"file":"effects/test.json","visible":false}]}]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
        ("materials/test.json", br#"{"passes":[{"shader":{"user":"style"}}]}"#),
    ])).unwrap();
    let project = project();
    let assets = Assets::discover(None)
        .with_properties(super::super::parse_properties(&project))
        .with_custom_shaders(&project)
        .unwrap();
    let scene = package.scene_json().unwrap();
    let (_, skipped) = super::super::load_effects(&package, &assets, &scene["objects"][0], true);
    assert!(!skipped.is_empty());
}
