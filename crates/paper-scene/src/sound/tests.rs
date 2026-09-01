use super::*;

fn push_string(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn package(files: &[(&str, &[u8])]) -> Package {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "PKGV0007");
    bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    for (path, data) in files {
        push_string(&mut bytes, path);
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in files {
        bytes.extend_from_slice(data);
    }
    Package::parse(bytes).unwrap()
}

fn sounds(scene: &[u8], files: &[(&str, &[u8])]) -> Vec<SceneSound> {
    let mut all: Vec<(&str, &[u8])> = vec![("scene.json", scene)];
    all.extend_from_slice(files);
    scene_sounds(&package(&all), &Properties::new()).unwrap()
}

#[test]
fn autostart_sounds_in_order() {
    let scene = br#"{"objects":[
        {"name":"Ambience","sound":["sounds/wind.mp3"],"volume":0.5,"playbackmode":"loop"},
        {"name":"Silent","sound":["sounds/theme.mp3"],"startsilent":true},
        {"name":"Music","sound":"sounds/theme.mp3","volume":0.8,"playbackmode":"single"}
    ]}"#;
    let found = sounds(scene, &[("sounds/wind.mp3", b"a"), ("sounds/theme.mp3", b"b")]);
    assert_eq!(
        found.iter().map(|sound| sound.name.as_str()).collect::<Vec<_>>(),
        ["Ambience", "Music"]
    );
    assert_eq!(found[0].mode, PlaybackMode::Loop);
    assert_eq!(found[1].mode, PlaybackMode::Once);
    assert!((found[1].volume - 0.8).abs() < f32::EPSILON);
}

#[test]
fn clip_pool_kept_whole() {
    let scene = br#"{"objects":[{"name":"Birds","sound":["s/a.ogg","s/missing.ogg","s/b.ogg"],
        "playbackmode":"random","mintime":10.0,"maxtime":30.0}]}"#;
    let found = sounds(scene, &[("s/a.ogg", b"a"), ("s/b.ogg", b"b")]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].clips, ["s/a.ogg", "s/b.ogg"]);
    assert_eq!(found[0].mode, PlaybackMode::Random);
    assert_eq!(found[0].gap_range(), (10.0, 30.0));
}

#[test]
fn missing_hidden_skipped() {
    let scene = br#"{"objects":[
        {"sound":"sounds/hidden.ogg","visible":false},
        {"sound":["sounds/missing.ogg"]},
        {"sound":[]},
        {"name":"NotASound"}
    ]}"#;
    assert!(sounds(scene, &[("sounds/hidden.ogg", b"a")]).is_empty());
}

#[test]
fn gain_above_unity_kept() {
    let scene = br#"{"objects":[{"sound":"s/loud.mp3","volume":1.3},
                                {"sound":"s/loud.mp3","volume":-2.0}]}"#;
    let found = sounds(scene, &[("s/loud.mp3", b"a")]);
    assert!((found[0].volume - 1.3).abs() < f32::EPSILON);
    assert!((found[1].volume - 0.0).abs() < f32::EPSILON);
}

#[test]
fn user_bound_volume() {
    let scene = br#"{"objects":[{"name":"Music","sound":"s/theme.mp3",
        "volume":{"user":"music_volume","value":0.5}}]}"#;
    let files: &[(&str, &[u8])] = &[("s/theme.mp3", b"a")];
    let baked = sounds(scene, files);
    assert!((baked[0].volume - 0.5).abs() < f32::EPSILON);

    let mut properties = Properties::new();
    properties.insert("music_volume".into(), vec![0.1]);
    let mut all: Vec<(&str, &[u8])> = vec![("scene.json", scene)];
    all.extend_from_slice(files);
    let tuned = scene_sounds(&package(&all), &properties).unwrap();
    assert!((tuned[0].volume - 0.1).abs() < f32::EPSILON);
}

#[test]
fn sound_count_bounded() {
    let mut objects = String::from("{\"objects\":[");
    for index in 0..(MAX_SCENE_SOUNDS + 8) {
        if index > 0 {
            objects.push(',');
        }
        objects.push_str(r#"{"sound":"s/a.mp3"}"#);
    }
    objects.push_str("]}");
    let found = sounds(objects.as_bytes(), &[("s/a.mp3", b"a")]);
    assert_eq!(found.len(), MAX_SCENE_SOUNDS);
}

#[test]
fn event_driven_gap() {
    let quiet =
        package(&[("scene.json", br#"{"objects":[{"sound":"s/a.mp3","startsilent":true}]}"#)]);
    assert!(has_event_driven_sounds(&quiet));
    let plain = package(&[("scene.json", br#"{"objects":[{"sound":"s/a.mp3"}]}"#)]);
    assert!(!has_event_driven_sounds(&plain));
    let none = package(&[("scene.json", br#"{"objects":[{"image":"m.json"}]}"#)]);
    assert!(!has_event_driven_sounds(&none));
}

#[test]
fn property_bound_start_is_a_precise_dynamic_gap() {
    let bound = package(&[
        (
            "scene.json",
            br#"{"objects":[{"sound":"s/a.mp3","startsilent":{"user":"sound_enabled","value":0}}]}"#,
        ),
        ("s/a.mp3", b"a"),
    ]);
    assert!(has_event_driven_sounds(&bound));
    let features = crate::scene::extract(&bound).unwrap();
    assert_eq!(features.objects_sound, 1);
    assert_eq!(features.objects_sound_event, 1);
    assert_eq!(
        crate::capability::assess_native(&features).reason_codes(),
        vec!["event-driven-sounds"]
    );
}

#[test]
fn inventory_covers_authored_sound_semantics() {
    let pkg = package(&[
        (
            "scene.json",
            br#"{"objects":[
                {"sound":["s/a.mp3","s/b.mp3"],"playbackmode":"random","visible":{"user":"birds","value":1},"volume":{"user":"loudness","value":0.5}},
                {"sound":"s/a.mp3","playbackmode":"single","startsilent":true},
                {"sound":"s/b.mp3","startsilent":{"user":"music","value":0},"visible":false}
            ]}"#,
        ),
        ("s/a.mp3", b"a"),
        ("s/b.mp3", b"b"),
    ]);
    assert_eq!(
        sound_inventory(&pkg).unwrap(),
        SoundInventory {
            objects: 3,
            autostart: 1,
            event_driven: 2,
            hidden: 1,
            multiple_clips: 1,
            looped: 1,
            one_shot: 1,
            random: 1,
            visibility_bound: 1,
            volume_bound: 1,
            start_bound: 1,
        }
    );
}
