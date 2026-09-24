use super::*;
use std::fs;

#[test]
fn preset_resolves_parent_media_and_preserves_scene_identity() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("1");
    let preset = root.path().join("2");
    fs::create_dir(&base).unwrap();
    fs::create_dir(&preset).unwrap();
    fs::write(base.join("project.json"), r#"{"type":"scene"}"#).unwrap();
    fs::write(base.join("scene.pkg"), b"fixture").unwrap();
    fs::write(
        preset.join("project.json"),
        r#"{"dependency":"1","preset":{},"preview":"preview.png"}"#,
    )
    .unwrap();
    fs::write(preset.join("preview.png"), b"preview").unwrap();
    let WeTarget::Scene(path) = resolve(preset.to_str().unwrap()).unwrap() else {
        panic!("expected scene")
    };
    assert_eq!(path, preset);
    assert_eq!(transition_media(preset.to_str().unwrap()).unwrap(), preset.join("preview.png"));
    let linked = root.path().join("linked-preset");
    std::os::unix::fs::symlink(&preset, &linked).unwrap();
    assert_eq!(transition_media(linked.to_str().unwrap()).unwrap(), preset.join("preview.png"));
    fs::write(base.join("project.json"), r#"{"type":"video","file":"clip.mp4"}"#).unwrap();
    fs::write(base.join("clip.mp4"), b"fixture").unwrap();
    let WeTarget::Video(path) = resolve(preset.to_str().unwrap()).unwrap() else {
        panic!("expected video")
    };
    assert_eq!(path, base.join("clip.mp4"));
    fs::remove_file(base.join("clip.mp4")).unwrap();
    assert!(resolve(preset.to_str().unwrap()).is_err());
}

#[test]
fn resolves_projects() {
    let temp = tempfile::tempdir().unwrap();
    let scene = temp.path().join("scene");
    fs::create_dir(&scene).unwrap();
    fs::write(scene.join("project.json"), r#"{"type":"scene"}"#).unwrap();
    fs::write(scene.join("scene.pkg"), []).unwrap();
    assert!(matches!(resolve(scene.to_str().unwrap()).unwrap(), WeTarget::Scene(_)));

    let video = temp.path().join("video");
    fs::create_dir(&video).unwrap();
    fs::write(video.join("project.json"), r#"{"type":"video","file":"media/a.mp4"}"#).unwrap();
    fs::create_dir(video.join("media")).unwrap();
    fs::write(video.join("media/a.mp4"), []).unwrap();
    let WeTarget::Video(resolved) = resolve(video.to_str().unwrap()).unwrap() else {
        panic!("expected video project");
    };
    assert_eq!(resolved, video.join("media/a.mp4").canonicalize().unwrap());
}

#[test]
fn rejects_bad_projects() {
    let temp = tempfile::tempdir().unwrap();
    let item = temp.path().join("item");
    fs::create_dir(&item).unwrap();
    fs::write(item.join("project.json"), r#"{"type":"web"}"#).unwrap();
    assert!(resolve(item.to_str().unwrap()).is_err());

    fs::write(item.join("project.json"), r#"{"type":"video","file":"../outside.mp4"}"#).unwrap();
    fs::write(temp.path().join("outside.mp4"), []).unwrap();
    assert!(resolve(item.to_str().unwrap()).is_err());

    fs::write(item.join("project.json"), r#"{"type":"video","file":"missing.mp4"}"#).unwrap();
    assert!(resolve(item.to_str().unwrap()).is_err());
}

#[test]
fn scene_preview_media() {
    let temp = tempfile::tempdir().unwrap();
    let scene = temp.path().join("scene");
    fs::create_dir(&scene).unwrap();
    fs::write(scene.join("project.json"), r#"{"type":"scene"}"#).unwrap();
    fs::write(scene.join("scene.pkg"), []).unwrap();
    fs::write(scene.join("preview.txt"), []).unwrap();
    assert!(transition_media(scene.to_str().unwrap()).is_err());

    fs::write(scene.join("preview.png"), []).unwrap();
    assert_eq!(
        transition_media(scene.to_str().unwrap()).unwrap(),
        scene.join("preview.png").canonicalize().unwrap()
    );

    let outside = temp.path().join("preview.jpg");
    fs::write(&outside, []).unwrap();
    std::os::unix::fs::symlink(&outside, scene.join("preview.jpg")).unwrap();
    assert_eq!(
        transition_media(scene.to_str().unwrap()).unwrap(),
        scene.join("preview.png").canonicalize().unwrap()
    );
}

#[test]
fn scene_transition_respects_safe_preview_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let scene = temp.path().join("scene");
    fs::create_dir(&scene).unwrap();
    fs::write(scene.join("scene.pkg"), []).unwrap();
    fs::write(scene.join("thumbnail.png"), []).unwrap();
    fs::write(scene.join("preview.png"), []).unwrap();
    fs::write(scene.join("project.json"), r#"{"type":"scene","preview":"thumbnail.png"}"#).unwrap();
    assert_eq!(transition_media(scene.to_str().unwrap()).unwrap(), scene.join("thumbnail.png"));
    fs::write(temp.path().join("outside.png"), []).unwrap();
    fs::write(scene.join("project.json"), r#"{"type":"scene","preview":"../outside.png"}"#)
        .unwrap();
    assert_eq!(transition_media(scene.to_str().unwrap()).unwrap(), scene.join("preview.png"));
}
