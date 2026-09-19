use super::*;

#[test]
fn hot_updates_require_changed_properties_and_unchanged_source_files() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().to_string_lossy();
    std::fs::write(root.path().join("scene.pkg"), b"fixture").unwrap();
    std::fs::write(root.path().join("project.json"), b"{}").unwrap();
    let original = Properties::new();
    let updated = Properties::from([("opacity".into(), vec![0.5])]);
    let source = PropertySource::new(&directory, &original);
    assert!(!source.accepts(&directory, &original));
    assert!(source.accepts(&directory, &updated));
    assert!(!source.accepts("/another-scene", &updated));
    std::fs::write(root.path().join("project.json"), b"{\"changed\":true}").unwrap();
    assert!(!source.accepts(&directory, &updated));
    let source = PropertySource::new(&directory, &original);
    assert!(source.accepts(&directory, &updated));
    std::fs::write(root.path().join("scene.pkg"), b"changed package").unwrap();
    assert!(!source.accepts(&directory, &updated));
}

#[test]
#[ignore = "requires Vulkan"]
fn static_property_callbacks_deliver_sound_commands_without_another_frame() {
    let root = tempfile::tempdir().unwrap();
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":7,"name":"BGM","sound":["missing.ogg"],"startsilent":true},
            {"id":2,"alpha":{"user":"opacity","value":1.0},"visible":{
                "value":true,"script":"export function applyUserProperties(props) { const sound=thisScene.getLayer('BGM'); if(props.opacity>0.5) sound.play(); else sound.stop(); }"
            }}
        ]
    }).to_string();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(b"PKGV0007");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&10u32.to_le_bytes());
    bytes.extend_from_slice(b"scene.json");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(scene.len() as u32).to_le_bytes());
    bytes.extend_from_slice(scene.as_bytes());
    std::fs::write(root.path().join("scene.pkg"), &bytes).unwrap();
    std::fs::write(
        root.path().join("project.json"),
        r#"{"general":{"properties":{"opacity":{"type":"slider","value":1.0}}}}"#,
    )
    .unwrap();
    let package = paper_scene::pkg::Package::parse(bytes).unwrap();
    let mut model = paper_scene::model::load_from_dir(&package, root.path()).unwrap();
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut group = super::super::build_group(
        &shared,
        &mut model,
        true,
        &[(64, 64)],
        paper_geom::FillMode::Fit,
    )
    .unwrap();
    assert!(!group.animated());
    assert!(!group.frozen);
    group.take_script_sounds();
    let directory = root.path().to_string_lossy();
    let mut source = PropertySource::new(&directory, &Properties::new());
    let mut delivered = Vec::new();
    for (opacity, expected) in
        [(0.25, paper_audio::VoiceOp::Stop), (1.0, paper_audio::VoiceOp::Play)]
    {
        assert!(group.update_properties(
            &mut source,
            &directory,
            &Properties::from([("opacity".into(), vec![opacity])]),
            |id, op| delivered.push((id.to_owned(), op)),
        ));
        assert!(!group.animated());
        assert_eq!(delivered.pop(), Some(("7".into(), expected)));
        assert!(group.take_script_sounds().is_empty());
    }
    group.destroy();
}
