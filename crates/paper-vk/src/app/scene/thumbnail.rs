use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use paper_control::{SceneThumbnailRequest, SceneThumbnailResponse};

const SIZE: (u32, u32) = (1280, 720);

pub(in crate::app) fn run() -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut shared = None;
    loop {
        let mut line = Vec::new();
        if (&mut input).take(1024 * 1024 + 1).read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        ensure!(line.len() <= 1024 * 1024, "scene thumbnail request is too large");
        let request: SceneThumbnailRequest = serde_json::from_slice(&line)?;
        let error = capture(&mut shared, &request).err().map(|error| format!("{error:#}"));
        let failed = error.is_some();
        serde_json::to_writer(
            &mut output,
            &SceneThumbnailResponse { source: request.source, error },
        )?;
        output.write_all(b"\n")?;
        output.flush()?;
        if failed {
            return Ok(());
        }
    }
}

fn capture(
    shared: &mut Option<crate::shared::SharedDevice>,
    request: &SceneThumbnailRequest,
) -> Result<()> {
    ensure!(Path::new(&request.source).is_absolute(), "thumbnail source must be absolute");
    let destination = Path::new(&request.destination);
    ensure!(destination.is_absolute(), "thumbnail destination must be absolute");
    if let Some(video) = video_source(Path::new(&request.source))? {
        let image = crate::decode::thumbnail::rgba_near(&video.to_string_lossy(), 1.0, SIZE)?;
        return write_png(destination, &image);
    }
    if shared.is_none() {
        *shared = Some(crate::shared::create(std::ptr::null_mut())?);
    }
    capture_scene(shared.as_ref().expect("device initialized"), request)
}

fn video_source(source: &Path) -> Result<Option<PathBuf>> {
    if source.is_file() {
        ensure!(
            paper_control::is_video_path(&source.to_string_lossy()),
            "thumbnail source is neither a video nor a Wallpaper Engine item"
        );
        return Ok(Some(source.to_path_buf()));
    }
    if !source.join("project.json").is_file() {
        return Ok(None);
    }
    let project = paper_control::we_project::Project::resolve(source)?;
    if project.kind().as_deref() != Some("video") {
        return Ok(None);
    }
    Ok(Some(project.video_file()?))
}

fn capture_scene(
    shared: &crate::shared::SharedDevice,
    request: &SceneThumbnailRequest,
) -> Result<()> {
    let package = paper_scene::pkg::Package::open(&super::locate_pkg(&request.source)?)?;
    let properties = super::parse_scene_properties(&serde_json::to_string(&request.properties)?);
    let mut model =
        paper_scene::model::load_from_dir_with(&package, Path::new(&request.source), &properties)?;
    drop(package);
    let strict = super::strict_scene_startup();
    super::validate_scene_skips(strict, &model.skipped)?;
    ensure!(
        !model.layers.is_empty() || !model.particles.is_empty(),
        "scene has no renderable layers"
    );
    let mut group =
        super::build_group(shared, &mut model, strict, &[SIZE], paper_geom::FillMode::Fit)?;
    drop(model);
    let result = (|| {
        for frame in 0..60 {
            group.compose(frame as f32 / 30.0, 1.0 / 30.0)?;
        }
        let (width, height, rgba) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow!("thumbnail frame size mismatch"))?;
        write_png(Path::new(&request.destination), &image)
    })();
    group.destroy();
    result
}

fn write_png(destination: &Path, image: &image::RgbaImage) -> Result<()> {
    let output = std::fs::File::create(destination)?;
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        output,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::NoFilter,
    );
    image.write_with_encoder(encoder).context("write captured thumbnail")
}

#[cfg(test)]
mod tests;
