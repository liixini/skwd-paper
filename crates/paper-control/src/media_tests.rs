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
