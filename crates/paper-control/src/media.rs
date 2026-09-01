use std::path::Path;

pub const VIDEO_EXTS: &[&str] =
    &["mp4", "mkv", "webm", "mov", "avi", "m4v", "flv", "wmv", "h264", "ivf"];

pub fn is_video_path(path: &str) -> bool {
    Path::new(path).extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        VIDEO_EXTS.iter().any(|video_extension| extension.eq_ignore_ascii_case(video_extension))
    })
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
