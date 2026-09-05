use super::*;

#[test]
fn video_ext_case_insensitive() {
    for extension in VIDEO_EXTS {
        assert!(is_video_path(&format!("/lib/clip.{extension}")), "{extension}");
        assert!(is_video_path(&format!("/lib/CLIP.{}", extension.to_uppercase())));
    }
    for path in ["/lib/a.png", "/lib/b.jpg", "/lib/c.webp", "/lib/noext", "/lib/mp4"] {
        assert!(!is_video_path(path), "{path}");
    }
}

#[test]
fn animated_gifs_use_video_but_single_frames_stay_static() {
    let directory = tempfile::tempdir().unwrap();
    for frames in [1, 2] {
        let path = directory.path().join(format!("{frames}.GIF"));
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = gif::Encoder::new(file, 2, 2, &[255, 0, 0, 0, 255, 0]).unwrap();
        for index in 0..frames {
            encoder
                .write_frame(&gif::Frame {
                    width: 2,
                    height: 2,
                    delay: 10,
                    buffer: std::borrow::Cow::Owned(vec![index; 4]),
                    ..Default::default()
                })
                .unwrap();
        }
        drop(encoder);
        assert_eq!(is_video_path(path.to_str().unwrap()), frames > 1);
    }
    let broken = directory.path().join("broken.gif");
    std::fs::write(&broken, b"GIF89a").unwrap();
    assert!(!is_video_path(broken.to_str().unwrap()));
}
