use super::{Load, SceneScripts};
use crate::model::Properties;
use serde_json::{Value, json};

fn native_rows(package: &[u8], tag: &str, field: &str) -> Value {
    let package = crate::pkg::Package::parse(package.to_vec()).unwrap();
    let mut scene: Value = serde_json::from_slice(package.find("scene.json").unwrap()).unwrap();
    let layers = scene["objects"].as_array_mut().unwrap();
    assert_eq!(layers.len(), 6);
    for (slot, layer) in layers.iter_mut().enumerate() {
        let source = layer["alpha"]["script"].as_str().unwrap();
        let sink = format!("console.log('{tag}:'+JSON.stringify(");
        assert_eq!(source.matches(&sink).count(), 1);
        let close = format!("fields:[{field}]}}));");
        assert_eq!(source.matches(&close).count(), 1);
        let mut script = source.replace(&sink, "(shared.__a03 ??= []).push(");
        script = script.replace(&close, &format!("fields:[{field}]}});"));
        if slot == 5 {
            let update = "export function update(){return alpha;}";
            assert_eq!(script.matches(update).count(), 1);
            script = script.replace(
                update,
                "export function update(){thisLayer.name=JSON.stringify(shared.__a03);return alpha;}",
            );
        }
        layer["alpha"]["script"] = script.into();
    }
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { images: Some(images), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    host.tick(0.2, 0.1, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let records: Value =
        serde_json::from_str(host.scene["objects"][5]["name"].as_str().unwrap()).unwrap();
    assert_eq!(records.as_array().unwrap().len(), 6);
    records
}

#[test]
fn matrix_native_rows() {
    let actual = native_rows(include_bytes!("../fixtures/a03/matrix.pkg"), "SKWD_A03", "fields");
    let expected: Value =
        serde_json::from_str(include_str!("../fixtures/a03/matrix.json")).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn matrix_detail_rows() {
    let actual =
        native_rows(include_bytes!("../fixtures/a03/matrix-detail.pkg"), "SKWD_A03R2", "result");
    let expected: Value =
        serde_json::from_str(include_str!("../fixtures/a03/matrix-detail.json")).unwrap();
    assert_eq!(actual, expected);
}
