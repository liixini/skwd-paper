use super::*;

#[test]
fn plain_scene_is_native() {
    let mut features = SceneFeatures {
        objects_image: 4,
        objects_particle: 2,
        parallax: true,
        ..SceneFeatures::default()
    };
    features.effects.insert("blur".into());
    assert!(assess_native(&features).full_fidelity());
}

#[test]
fn gap_codes_stable() {
    let mut features = SceneFeatures {
        objects_sound: 2,
        objects_sound_event: 1,
        objects_light: 1,
        objects_text: 1,
        objects_other: 3,
        puppet: true,
        puppet_unsupported: true,
        animated_image_textures: ["materials/animated".into()].into_iter().collect(),
        audio: true,
        tex_video: 1,
        tex_failures: 4,
        json_failures: 5,
        refs_truncated: true,
        ..SceneFeatures::default()
    };
    features.tex_other_format.insert("FORMAT_99".into());

    assert_eq!(
        assess_native(&features).reason_codes(),
        vec![
            "event-driven-sounds",
            "light-objects",
            "text-objects",
            "other-objects",
            "puppet",
            "animated-image-textures",
            "audio-reactive",
            "unknown-texture-formats",
            "texture-parse-failures",
            "json-parse-failures",
            "reference-scan-truncated",
        ]
    );
}

#[test]
fn event_sounds_only_gap() {
    let mixed = SceneFeatures { objects_sound: 6, ..SceneFeatures::default() };
    assert!(assess_native(&mixed).full_fidelity());

    let event =
        SceneFeatures { objects_sound: 6, objects_sound_event: 2, ..SceneFeatures::default() };
    assert_eq!(assess_native(&event).reason_codes(), vec!["event-driven-sounds"]);
    assert_eq!(assess_native(&event).gaps[0].instances(), 2);
}
