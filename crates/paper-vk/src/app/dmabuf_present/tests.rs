use super::{
    consume_output_releases, control_enabled_for_overlay, require_presentation_confirmation,
    should_park_still, should_release_source_frame,
};

#[test]
fn still_pending_swap() {
    assert!(!should_park_still(true, false, true, 1));
}

#[test]
fn still_parks_when_idle() {
    assert!(should_park_still(true, false, false, 1));
    assert!(!should_park_still(true, true, false, 1));
    assert!(!should_park_still(true, false, false, 0));
    assert!(!should_park_still(false, false, false, 1));
}

#[test]
fn transition_pacer_retains_outgoing_video_frame() {
    assert!(!should_release_source_frame(false, true));
    assert!(!should_release_source_frame(true, true));
    assert!(!should_release_source_frame(true, false));
    assert!(should_release_source_frame(false, false));
}

#[test]
fn held_overlay_keeps_control_reader() {
    assert!(control_enabled_for_overlay(true, true));
    assert!(!control_enabled_for_overlay(true, false));
    assert!(control_enabled_for_overlay(false, false));
}

#[test]
fn available_presentation_feedback_is_fail_closed() {
    assert!(require_presentation_confirmation(Some(true), "frame").is_ok());
    assert!(require_presentation_confirmation(Some(false), "frame").is_err());
    assert!(require_presentation_confirmation(None, "frame").is_ok());
}

#[test]
fn closed_output_no_release() {
    let mut pending = [true, true];
    assert!(!consume_output_releases(&mut pending, |si| si == 1));
    assert_eq!(pending, [true, false]);
}

#[test]
fn release_after_all_outputs() {
    let mut pending = [true, false, true];
    assert!(!consume_output_releases(&mut pending, |si| si == 0));
    assert!(consume_output_releases(&mut pending, |si| si == 2));
    assert_eq!(pending, [false, false, false]);
}

#[test]
fn groups_distinct_origins() {
    let dims = [(32, 32); 3];
    for (paths, expected) in [
        ([None, None, None], vec![0, 0, 0]),
        ([Some("red".into()), Some("green".into()), Some("red".into())], vec![0, 1, 0]),
        ([None, Some("green".into()), None], vec![0, 1, 0]),
    ] {
        assert_eq!(super::shared_layout(&dims, &paths).indices, expected);
    }
}

fn transition_video(directory: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let video = directory.join("changing.mp4");
    let encoded = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=black:s=32x32:r=10:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=white:s=32x32:r=10:d=1",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "mpeg4",
            "-y",
        ])
        .arg(&video)
        .status()?;
    assert!(encoded.success());
    Ok(video)
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn shared_video_origins() -> anyhow::Result<()> {
    use super::*;
    let directory = tempfile::tempdir()?;
    let video = transition_video(directory.path())?;
    let shared = crate::shared::test_device();
    let dims = [(32, 32); 3];
    let mut paths = Vec::new();
    for (name, color) in [("red", [255, 0, 0]), ("green", [0, 255, 0]), ("blue", [0, 0, 255])] {
        let path = directory.path().join(format!("{name}.png"));
        image::RgbImage::from_pixel(32, 32, image::Rgb(color)).save(&path)?;
        paths.push(Some(path.display().to_string()));
    }
    let layout = shared_layout(&dims, &paths);
    let mut output = build_shared_outputs(&shared, layout, None, 1, true, &[], false)?;
    let mut origins =
        load_origins(&mut output.renderers[0], &paths, &output.ridx, &output.distinct);
    assert_eq!(origins.len(), 3);
    let request = ctl::SwapReq {
        to: video.display().to_string(),
        duration_ms: 1000,
        shader: None,
        drawing: None,
        properties: None,
    };
    let mut fade = start_swap(&request, &shared, true, &output.distinct, &dims).unwrap();
    assert!(!fade.still_b);
    let worker = fade.worker.as_ref().unwrap().thread().id();
    let old = decode::RenderFrame::plain(ffmpeg_the_third::frame::Video::empty());
    let chosen = vec![Some(0); 3];
    render_surfaces(
        &mut output.renderers,
        &output.rts,
        &mut output.exports,
        &chosen,
        &output.distinct,
        &None,
        &old,
        &output.uvs,
        Some(&fade),
        Some(0.0),
        0,
        None,
        None,
        &RgbaOrigins { shared: None, outputs: &origins },
        None,
    )?;
    for channel in 0..3 {
        let pixels = read_rgba_export(
            &mut output.renderers[channel],
            &output.exports[channel][0],
            (32, 32),
        )?;
        let pixel = pixels.get_pixel(16, 16);
        assert!(pixel[channel] > 200, "{pixel:?}");
        assert!(pixel[(channel + 1) % 3] < 30, "{pixel:?}");
    }
    let mut upload = None;
    let mut frames = Vec::new();
    for after in [0.0, 1.2] {
        while fade.cur.1 < after {
            let SendFrame(frame, pts) = fade.rx.recv_timeout(std::time::Duration::from_secs(2))?;
            fade.cur = (frame, pts);
        }
        ensure_frame_slot(&mut upload, &output.renderers[0], &fade.cur.0, fade.cur.1)?;
        render_surfaces(
            &mut output.renderers,
            &output.rts,
            &mut output.exports,
            &chosen,
            &output.distinct,
            &None,
            &old,
            &output.uvs,
            Some(&fade),
            Some(0.5),
            0,
            None,
            upload.as_ref(),
            &RgbaOrigins { shared: None, outputs: &origins },
            None,
        )?;
        frames.push(read_rgba_export(&mut output.renderers[0], &output.exports[0][0], (32, 32))?);
        assert_eq!(fade.worker.as_ref().unwrap().thread().id(), worker);
    }
    assert!(frames[1].get_pixel(16, 16)[1] > frames[0].get_pixel(16, 16)[1] + 80);
    wait_render_all(&mut output.renderers, &None)?;
    clear_origins(&output.renderers[0], &mut origins);
    assert!(origins.is_empty());
    let steady = shared_layout(&dims, &[None, None, None]);
    let replacement = build_shared_outputs(&shared, steady, None, 1, true, &[], false)?;
    drop(std::mem::replace(&mut output, replacement));
    assert_eq!(output.renderers.len(), 1);
    assert_eq!(output.ridx, vec![0, 0, 0]);
    let SendFrame(_, next_pts) = fade.rx.recv_timeout(std::time::Duration::from_secs(2))?;
    assert!(next_pts > fade.cur.1);
    assert_eq!(fade.worker.as_ref().unwrap().thread().id(), worker);
    upload.take().unwrap().destroy(&output.renderers[0]);
    fade.cancel();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn video_origin_fallback() -> anyhow::Result<()> {
    use super::*;
    let directory = tempfile::tempdir()?;
    let dims = [(32, 32); 4];
    let shared = crate::shared::test_device();
    let mut paths = Vec::new();
    for (name, color) in [("red", [255, 0, 0]), ("green", [0, 255, 0])] {
        let image = directory.path().join(format!("{name}.png"));
        image::RgbImage::from_pixel(32, 32, image::Rgb(color)).save(&image)?;
        let video = image.with_extension("mp4");
        let encoded = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-loop", "1", "-i"])
            .arg(&image)
            .args(["-t", "0.2", "-c:v", "mpeg4", "-pix_fmt", "yuv420p", "-y"])
            .arg(&video)
            .status()?;
        assert!(encoded.success());
        paths.push(Some(video.display().to_string()));
    }
    paths.push(Some(directory.path().join("missing.mp4").display().to_string()));
    paths.push(None);
    let layout = shared_layout(&dims, &paths);
    let mut output = build_shared_outputs(&shared, layout, None, 1, true, &[], false)?;
    let mut origins =
        load_origins(&mut output.renderers[0], &paths, &output.ridx, &output.distinct);
    assert_eq!(origins.len(), 2);
    let fallback = directory.path().join("fallback.png");
    image::RgbImage::from_pixel(32, 32, image::Rgb([0, 0, 255])).save(&fallback)?;
    let fallback =
        load_origin(&mut output.renderers[0], fallback.to_str().unwrap(), &output.distinct)?
            .unwrap();
    let empty = decode::RenderFrame::plain(ffmpeg_the_third::frame::Video::empty());
    render_surfaces(
        &mut output.renderers,
        &output.rts,
        &mut output.exports,
        &[Some(0); 4],
        &output.distinct,
        &None,
        &empty,
        &output.uvs,
        None,
        None,
        0,
        None,
        None,
        &RgbaOrigins { shared: Some(&fallback), outputs: &origins },
        None,
    )?;
    for (index, channel) in [0, 1, 2, 2].into_iter().enumerate() {
        let pixels =
            read_rgba_export(&mut output.renderers[index], &output.exports[index][0], (32, 32))?;
        let pixel = pixels.get_pixel(16, 16);
        assert!(pixel[channel] > 200, "{pixel:?}");
        assert!(pixel[(channel + 1) % 3] < 30, "{pixel:?}");
    }
    wait_render_all(&mut output.renderers, &None)?;
    clear_origins(&output.renderers[0], &mut origins);
    output.renderers[0].destroy_scene_texture(fallback.texture);
    assert!(origins.is_empty());
    Ok(())
}
