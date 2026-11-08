use super::{SceneScripts, ScriptCommand};
use crate::model::Properties;
use serde_json::json;

const INIT: &str = "export function init(){BODY;return true;}";
const UPDATE: &str = "export function update(v){BODY;return v;}";

const PROBE: &[(&str, &str, Option<f32>)] = &[
    (INIT, "thisLayer.getEffect(0).setMaterialProperty('Tint',0.5)", Some(0.5)),
    (INIT, "thisLayer.getEffect(0).setMaterialProperty('g_Tint',0.5)", None),
    (INIT, "thisLayer.getEffect(0).setMaterialProperty('tint',0.5)", None),
    (UPDATE, "thisLayer.getEffect(0).setMaterialProperty('Tint',0.5)", Some(0.5)),
    (INIT, "thisLayer.getEffect(0).getMaterial(0).Tint=0.5", Some(0.5)),
    (INIT, "thisLayer.getEffect(0).getMaterial(0).g_Tint=0.5", None),
    (INIT, "thisLayer.getEffect(0).getMaterial(0).tint=0.5", None),
    (UPDATE, "thisLayer.getEffect(0).getMaterial(0).Tint=0.5", Some(0.5)),
    (
        INIT,
        "var m=thisLayer.getEffect(0).getMaterial(0);m.Tint=(typeof m.setValue==='function')?0.5:0.25",
        Some(0.25),
    ),
    (
        INIT,
        "var m=thisLayer.getEffect(0).getMaterial(0);try{m.setValue('Tint',0.5)}catch(e){m.Tint=0.25}",
        Some(0.25),
    ),
    (
        INIT,
        "var m=thisLayer.getEffect(0).getMaterial(0);var t=m.Tint;m.Tint=(t===1)?0.5:(t===undefined?0.25:0.75)",
        Some(0.5),
    ),
    (
        INIT,
        "var m=thisLayer.getEffect(0).getMaterial(0);m.Tint=('g_Tint' in m)?0.5:0.25",
        Some(0.25),
    ),
    (INIT, "var m=thisLayer.getEffect(0).getMaterial(0);m.Tint=('Tint' in m)?0.5:0.25", Some(0.5)),
    (INIT, "thisLayer.getMaterial().setValue('Tint',0.5)", None),
    (UPDATE, "thisLayer.getMaterial().setValue('g_Tint',0.5)", None),
];

fn probe_host(scene: &mut serde_json::Value) -> SceneScripts {
    let source = scene.to_string();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/white.json", br#"{"material":"materials/white.json"}"#),
        ("materials/white.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/probe/effect.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
        (
            "shaders/probe.frag",
            br#"uniform float g_Tint; // {"material":"Tint","default":1,"range":[0,1]}"#,
        ),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    SceneScripts::load_with_storage(
        scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap()
}

fn probe_layer(id: usize, csv: &serde_json::Value, script: &str) -> serde_json::Value {
    json!({"id":id,"image":"models/white.json",
        "effects":[{"file":"effects/probe/effect.json","passes":[{"constantshadervalues":csv}]}],
        "visible":{"value":true,"script":script}})
}

fn tint(id: usize, value: f32) -> ScriptCommand {
    ScriptCommand::Material {
        id: id.to_string(),
        effect: (0, 0),
        name: "g_Tint".into(),
        values: vec![value],
    }
}

#[test]
fn we_probe_matrix() {
    let objects: Vec<_> = PROBE
        .iter()
        .enumerate()
        .map(|(idx, (shape, body, _))| {
            probe_layer(idx + 1, &json!({}), &shape.replace("BODY", body))
        })
        .collect();
    let mut scene = json!({ "objects": objects });
    let mut host = probe_host(&mut scene);
    let mut commands = host.take_commands();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    commands.extend(host.take_commands());
    let expected: Vec<_> = PROBE
        .iter()
        .enumerate()
        .filter_map(|(idx, (_, _, value))| value.map(|value| tint(idx + 1, value)))
        .collect();
    commands.sort_by_key(|command| match command {
        ScriptCommand::Material { id, .. } => id.parse::<usize>().unwrap(),
        _ => 0,
    });
    assert_eq!(commands, expected);
}

#[test]
fn layer_material_absent() {
    let script = INIT.replace("BODY", "thisLayer.getMaterial().setValue('Alpha',0.75)");
    let mut scene = json!({"objects":[probe_layer(1, &json!({}), &script)]});
    let mut host = probe_host(&mut scene);
    assert!(host.take_commands().is_empty());
    assert_eq!(host.diagnostics.len(), 1);
    assert!(host.diagnostics[0].contains("not a function"), "{:?}", host.diagnostics);
}

#[test]
fn property_miss_silent() {
    let script = UPDATE.replace("BODY", "thisLayer.getEffect(0).setMaterialProperty('g_Tint',0.5)");
    let mut scene = json!({"objects":[probe_layer(1, &json!({}), &script)]});
    let mut host = probe_host(&mut scene);
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.take_commands().is_empty());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn constant_read_back() {
    let read = "var m=thisLayer.getEffect(0).getMaterial(0);var t=m.Tint;m.Tint=(Math.abs(t-0.6)<0.01)?0.5:0.25";
    let script = INIT.replace("BODY", read);
    for (csv, value) in [(json!({"Tint":0.6}), 0.5), (json!({"g_Tint":0.6}), 0.25)] {
        let mut scene = json!({"objects":[probe_layer(1, &csv, &script)]});
        let mut host = probe_host(&mut scene);
        assert_eq!(host.take_commands(), [tint(1, value)]);
    }
}
