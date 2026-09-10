use std::io::{BufRead, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, ensure};
use paper_control::{SceneThumbnailRequest, SceneThumbnailResponse};

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
        if shared.is_none() {
            shared = Some(crate::shared::create(std::ptr::null_mut())?);
        }
        let error = capture(shared.as_ref().expect("device initialized"), &request)
            .err()
            .map(|error| format!("{error:#}"));
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

fn capture(shared: &crate::shared::SharedDevice, request: &SceneThumbnailRequest) -> Result<()> {
    ensure!(Path::new(&request.source).is_absolute(), "scene source must be absolute");
    ensure!(
        Path::new(&request.destination).is_absolute(),
        "thumbnail destination must be absolute"
    );
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
        super::build_group(shared, &mut model, strict, &[(1280, 720)], paper_geom::FillMode::Fit)?;
    drop(model);
    let result = (|| {
        for frame in 0..60 {
            group.compose(frame as f32 / 30.0, 1.0 / 30.0)?;
        }
        let (width, height, rgba) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow!("thumbnail frame size mismatch"))?;
        let output = std::fs::File::create(&request.destination)?;
        let encoder = image::codecs::png::PngEncoder::new_with_quality(
            output,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        );
        image.write_with_encoder(encoder).context("write captured thumbnail")
    })();
    group.destroy();
    result
}
