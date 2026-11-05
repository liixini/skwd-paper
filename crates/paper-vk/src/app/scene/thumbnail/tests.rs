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

fn decode(path: &Path) -> (image::ImageFormat, image::DynamicImage) {
    let reader = image::ImageReader::open(path).unwrap().with_guessed_format().unwrap();
    (reader.format().unwrap(), reader.decode().unwrap())
}

fn request(destination: &Path, reference: bool) -> SceneThumbnailRequest {
    SceneThumbnailRequest {
        source: String::new(),
        destination: destination.display().to_string(),
        properties: serde_json::Map::default(),
        reference,
    }
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
fn video_reference_first_frame() {
    let root = tempfile::tempdir().unwrap();
    let item = root.path().join("item");
    project(&item, r#"{"type":"video","file":"clip.gif"}"#);
    clip(&item.join("clip.gif"));
    let destination = root.path().join("frame");
    let mut shared = None;
    for (reference, channel, format) in
        [(false, 2, image::ImageFormat::Png), (true, 0, image::ImageFormat::WebP)]
    {
        capture(
            &mut shared,
            &SceneThumbnailRequest {
                source: item.to_string_lossy().into_owned(),
                destination: destination.to_string_lossy().into_owned(),
                properties: serde_json::Map::default(),
                reference,
            },
        )
        .unwrap();
        assert!(shared.is_none());
        let (written, image) = decode(&destination);
        assert_eq!(written, format);
        let image = image.into_rgba8();
        assert_eq!(image.dimensions(), (64, 36));
        assert!(image.get_pixel(8, 8).0[channel] > 180);
    }
}

#[test]
fn reference_capture_bounds() {
    let mut request: SceneThumbnailRequest =
        serde_json::from_str(r#"{"source":"/clip.mp4","destination":"/frame.png"}"#).unwrap();
    assert_eq!(capture_size(&request), (1280, 720));
    request.reference = true;
    assert_eq!(capture_size(&request), (2560, 2560));
}

#[test]
fn reference_preserves_large_frame() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), r#"{"type":"video","file":"large.gif"}"#);
    let source = root.path().join("large.gif");
    image::RgbaImage::from_pixel(3000, 1500, image::Rgba([20, 30, 220, 255]))
        .save(&source)
        .unwrap();
    let destination = root.path().join("frame");
    let mut request = SceneThumbnailRequest {
        source: root.path().display().to_string(),
        destination: destination.display().to_string(),
        reference: false,
        properties: serde_json::Map::default(),
    };
    let mut shared = None;
    for (reference, expected) in [(false, (1280, 640)), (true, (2560, 1280))] {
        request.reference = reference;
        capture(&mut shared, &request).unwrap();
        assert_eq!(image::GenericImageView::dimensions(&decode(&destination).1), expected);
    }
    assert!(shared.is_none());
}

#[test]
fn opaque_frames_drop_alpha() {
    let directory = tempfile::tempdir().unwrap();
    for (alpha, expected) in [
        (u8::MAX, image::ColorType::Rgb8),
        (250, image::ColorType::Rgb8),
        (249, image::ColorType::Rgba8),
        (7, image::ColorType::Rgba8),
    ] {
        let path = directory.path().join(format!("{alpha}.png"));
        let frame = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, alpha]));
        write_capture(&request(&path, false), &frame).unwrap();
        let (format, decoded) = decode(&path);
        assert_eq!(format, image::ImageFormat::Png);
        assert_eq!(decoded.color(), expected);
        assert_eq!(decoded.to_rgba8().get_pixel(0, 0).0[..3], [10, 20, 30]);
    }
}

#[test]
fn reference_writes_webp() {
    let directory = tempfile::tempdir().unwrap();
    for alpha in [u8::MAX, OPAQUE_ALPHA] {
        let path = directory.path().join(format!("{alpha}"));
        let frame = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 120, 40, alpha]));
        write_capture(&request(&path, true), &frame).unwrap();
        let (format, decoded) = decode(&path);
        assert_eq!(format, image::ImageFormat::WebP);
        assert_eq!(decoded.color(), image::ColorType::Rgb8);
        let pixel = decoded.to_rgb8().get_pixel(8, 8).0;
        assert!(
            pixel.iter().zip([200, 120, 40]).all(|(got, want)| got.abs_diff(want) <= 2),
            "{pixel:?}"
        );
    }
}

#[test]
fn translucent_reference_lossless() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reference");
    let frame = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 120, 40, 7]));
    write_capture(&request(&path, true), &frame).unwrap();
    let (format, decoded) = decode(&path);
    assert_eq!(format, image::ImageFormat::WebP);
    assert_eq!(decoded.color(), image::ColorType::Rgba8);
    assert_eq!(decoded.to_rgba8().get_pixel(8, 8).0, [200, 120, 40, 7]);
}
