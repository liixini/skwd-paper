use super::*;

fn wait_file(path: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.exists() {
        assert!(std::time::Instant::now() < deadline, "capture did not complete");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn capture_writes_a_bounded_frame_and_releases_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frame.png");
    capture_scene(
        paper_control::SceneCapture { source: "/scene".into(), path: path.display().to_string() },
        Ok((1600, 900, [30, 80, 120, 255].repeat(1600 * 900))),
    );
    wait_file(&path);
    let image = image::open(&path).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (1280, 720));
    assert_eq!(image.get_pixel(0, 0).0, [30, 80, 120, 255]);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn failed_capture_leaves_no_image() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frame.png");
    capture_scene(
        paper_control::SceneCapture { source: "/scene".into(), path: path.display().to_string() },
        Err(anyhow!("scene changed")),
    );
    wait_file(&path.with_extension("png.error"));
    assert!(!path.exists());
    assert_eq!(std::fs::read_to_string(path.with_extension("png.error")).unwrap(), "scene changed");
}
