use super::{Properties, puppet_playback};
use crate::script::{Load, SceneScripts};
use serde_json::{Value, json};

#[test]
fn puppet_native_clock_rows() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", include_bytes!("../fixtures/a02/clock-step-scene.json")),
        ("models/Plana 01.json", include_bytes!("../fixtures/a02/clock-step-model.json")),
        ("models/Plana 01_puppet.mdl", include_bytes!("../fixtures/a02/clock-step.mdl")),
    ]))
    .unwrap();
    let mut scene: Value = serde_json::from_slice(package.find("scene.json").unwrap()).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("../fixtures/a02/clock-step.json")).unwrap();
    let puppets = puppet_playback(&scene, &package, &Properties::new());
    let source = scene["objects"][0]["visible"]["script"].as_str().unwrap();
    let sink = "if(row.length<900)console.log('SKWD_A02:'+row);";
    assert_eq!(source.matches(sink).count(), 1);
    scene["objects"][0]["visible"]["script"] = source.replace(sink,
        "(shared.rows ??= []).push(JSON.parse(row));thisLayer.name=JSON.stringify(shared.rows);").into();
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    for record in expected
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["label"].as_str().unwrap().starts_with("update-"))
    {
        let state = &record["fields"][0];
        host.tick(
            state["runtime"].as_f64().unwrap() as f32,
            state["frametime"].as_f64().unwrap() as f32,
            [0.5; 2],
        )
        .unwrap();
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let actual: Value =
        serde_json::from_str(host.scene["objects"][0]["name"].as_str().unwrap()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn puppet_native_visibility_rows() {
    for (source, records) in [
        (
            include_bytes!("../fixtures/a02/visible-control-scene.json").as_slice(),
            include_str!("../fixtures/a02/visible-control.json"),
        ),
        (
            include_bytes!("../fixtures/a02/visible-false-scene.json").as_slice(),
            include_str!("../fixtures/a02/visible-false.json"),
        ),
        (
            include_bytes!("../fixtures/a02/visible-restore-scene.json").as_slice(),
            include_str!("../fixtures/a02/visible-restore.json"),
        ),
    ] {
        let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
            ("scene.json", source),
            ("models/Plana 01.json", include_bytes!("../fixtures/a02/clock-step-model.json")),
            ("models/Plana 01_puppet.mdl", include_bytes!("../fixtures/a02/clock-step.mdl")),
        ]))
        .unwrap();
        let mut scene: Value = serde_json::from_slice(source).unwrap();
        let expected: Value = serde_json::from_str(records).unwrap();
        let puppets = puppet_playback(&scene, &package, &Properties::new());
        let script = scene["objects"][0]["visible"]["script"].as_str().unwrap();
        assert_eq!(script.matches("console.log(").count(), 1);
        scene["objects"][0]["visible"]["script"] = format!("function capture(line){{(shared.rows??=[]).push(JSON.parse(line.slice('SKWD_A02:'.length)));thisLayer.name=JSON.stringify(shared.rows);}}\n{}", script.replace("console.log(", "capture(")).into();
        let mut host = SceneScripts::load_with_storage(
            &mut scene,
            &Properties::new(),
            &json!({}),
            Load { puppets: Some(puppets), ..Load::default() },
        )
        .unwrap()
        .unwrap();
        for row in expected
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| matches!(row["label"].as_str(), Some("first-update" | "held")))
        {
            let time = row["fields"][0]["runtime"].as_f64().unwrap() as f32;
            host.tick(time, 0.0, [0.5; 2]).unwrap();
        }
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let actual: Value =
            serde_json::from_str(host.scene["objects"][0]["name"].as_str().unwrap()).unwrap();
        assert_eq!(actual, expected);
    }
}
