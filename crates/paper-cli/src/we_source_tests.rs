use super::*;
use std::fs;

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
