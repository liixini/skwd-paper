use super::*;
use std::fs;

fn clip(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut encoder = image::codecs::gif::GifEncoder::new(file);
    for color in [[220, 25, 25, 255], [20, 30, 220, 255]] {
        encoder
            .encode_frame(image::Frame::from_parts(
                image::RgbaImage::from_pixel(64, 36, image::Rgba(color)),
                0,
                0,
                image::Delay::from_numer_denom_ms(500, 1),
            ))
            .unwrap();
    }
}

fn project(directory: &Path, document: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join("project.json"), document).unwrap();
}

#[test]
fn routes_video_projects_and_files_away_from_the_scene_renderer() {
    let root = tempfile::tempdir().unwrap();
    let video = root.path().join("video");
    project(&video, r#"{"type":"Video","file":"media/clip.mp4"}"#);
    fs::create_dir(video.join("media")).unwrap();
    fs::write(video.join("media/clip.mp4"), b"fixture").unwrap();
    assert_eq!(
        video_source(&video).unwrap(),
        Some(video.join("media/clip.mp4").canonicalize().unwrap())
    );
    let file = video.join("media/clip.mp4");
    assert_eq!(video_source(&file).unwrap(), Some(file));

    let scene = root.path().join("scene");
    project(&scene, r#"{"type":"scene"}"#);
    assert_eq!(video_source(&scene).unwrap(), None);
    let unpacked = root.path().join("unpacked");
    fs::create_dir(&unpacked).unwrap();
    assert_eq!(video_source(&unpacked).unwrap(), None);
}

#[test]
fn rejects_sources_that_are_not_videos_or_escape_their_item() {
    let root = tempfile::tempdir().unwrap();
    let image = root.path().join("still.png");
    fs::write(&image, b"fixture").unwrap();
    assert!(video_source(&image).is_err());

    let item = root.path().join("item");
    project(&item, r#"{"type":"video","file":"../outside.mp4"}"#);
    fs::write(root.path().join("outside.mp4"), b"fixture").unwrap();
    assert!(video_source(&item).is_err());
    project(&item, r#"{"type":"video","file":"missing.mp4"}"#);
    assert!(video_source(&item).is_err());
}

#[test]
fn video_capture_writes_a_png_without_starting_the_gpu() {
    let root = tempfile::tempdir().unwrap();
    let item = root.path().join("item");
    project(&item, r#"{"type":"video","file":"clip.gif"}"#);
    clip(&item.join("clip.gif"));
    let destination = root.path().join("frame.png");
    let mut shared = None;
    capture(
        &mut shared,
        &SceneThumbnailRequest {
            source: item.to_string_lossy().into_owned(),
            destination: destination.to_string_lossy().into_owned(),
            properties: Default::default(),
        },
    )
    .unwrap();
    assert!(shared.is_none());
    let image = image::open(&destination).unwrap().into_rgba8();
    assert_eq!(image.dimensions(), (64, 36));
    assert!(image.get_pixel(8, 8).0[2] > 180, "{:?}", image.get_pixel(8, 8));
}
