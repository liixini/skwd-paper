use super::*;

#[test]
fn preset_hot_updates_rebuild_when_parent_declarations_change() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("1");
    let preset = root.path().join("2");
    std::fs::create_dir(&base).unwrap();
    std::fs::create_dir(&preset).unwrap();
    std::fs::write(base.join("scene.pkg"), b"fixture").unwrap();
    std::fs::write(base.join("project.json"), b"{}").unwrap();
    std::fs::write(preset.join("project.json"), br#"{"dependency":"1","preset":{}}"#).unwrap();
    let directory = preset.to_str().unwrap();
    let changed = Properties::from([("opacity".into(), vec![0.5])]);
    let source = PropertySource::new(directory, &Properties::new());
    assert!(source.accepts(directory, &changed));
    std::fs::write(base.join("project.json"), br#"{"general":{}}"#).unwrap();
    assert!(!source.accepts(directory, &changed));
}

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
fn static_sound_callbacks() {
    let root = tempfile::tempdir().unwrap();
    let scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[
            {"id":7,"name":"BGM","sound":["missing.ogg"],"startsilent":true},
            {"id":2,"alpha":{"user":"opacity","value":1.0},"visible":{
                "value":true,"script":"export function init(value) { thisScene.getLayer('BGM').play(); return value; } export function applyUserProperties(props) { const sound=thisScene.getLayer('BGM'); thisLayer.name=String(sound.isPlaying()); if(props.opacity>0.5) sound.play(); else sound.stop(); }"
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
    let shared = crate::shared::test_device();
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
    let directory = root.path().to_string_lossy();
    let mut source = PropertySource::new(&directory, &Properties::new());
    let mut ctl = crate::ctl::Ctl::start_opts("", true, 100, false, false);
    ctl.set_scene_voices(
        vec![paper_audio::Voice {
            id: "7".into(),
            name: "BGM".into(),
            clips: vec!["missing.ogg".into()],
            gain: 1.0,
            mode: paper_audio::VoiceMode::Once,
            min_gap: 0.0,
            max_gap: 0.0,
            autostart: false,
        }],
        None,
    );
    group.apply_script_sounds(&mut ctl);
    assert_eq!(ctl.scene_voice_states(), [("7".into(), true)]);
    group.sync_sound_states(&ctl);
    ctl.scene_sound("7", paper_audio::VoiceOp::Stop);
    for (opacity, before, after) in [(0.25, false, false), (1.0, false, true), (0.25, true, false)]
    {
        assert!(group.update_properties(
            &mut source,
            &directory,
            &Properties::from([("opacity".into(), vec![opacity])]),
            &mut ctl,
        ));
        assert!(!group.animated());
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert_eq!(host.scene["objects"][1]["name"], before.to_string());
        assert_eq!(ctl.scene_voice_states(), vec![("7".into(), after)]);
        assert!(group.take_script_sounds().is_empty());
    }
    group.destroy();
}
