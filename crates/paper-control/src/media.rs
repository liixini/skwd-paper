use std::path::Path;

pub const VIDEO_EXTS: &[&str] =
    &["mp4", "mkv", "webm", "mov", "avi", "m4v", "flv", "wmv", "h264", "ivf"];

pub fn is_video_path(path: &str) -> bool {
    Path::new(path).extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        VIDEO_EXTS.iter().any(|video_extension| extension.eq_ignore_ascii_case(video_extension))
            || (extension.eq_ignore_ascii_case("gif") && animated_gif(path).unwrap_or(false))
    })
}

fn animated_gif(path: &str) -> Result<bool, gif::DecodingError> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut options = gif::DecodeOptions::new();
    options.skip_frame_decoding(true);
    let mut decoder = options.read_info(file)?;
    Ok(decoder.read_next_frame()?.is_some() && decoder.read_next_frame()?.is_some())
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
