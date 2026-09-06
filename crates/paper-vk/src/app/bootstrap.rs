use super::model::StartFade;
use super::upload::run_upload;
#[cfg(feature = "shared-device")]
use super::{
    dmabuf_present::{run_nv12, run_shared_dmabuf},
    shared_upload::run_shared,
};
#[cfg(feature = "shared-device")]
use crate::fill::fill_mode;
use crate::fill::set_fill_mode;
use crate::{ctl, wayland};
use anyhow::{Context, Result};
use paper_geom::FillMode;

fn usage() -> ! {
    tracing::info!("usage: skwd-wall-vk <output|*> <video> [--standalone] [-o mute=yes;volume=80]");
    std::process::exit(2);
}

fn parse_flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|arg| arg == name).and_then(|idx| args.get(idx + 1)).map(String::as_str)
}

fn parse_idle_secs(value: Option<&str>) -> u32 {
    value.and_then(|text| text.parse::<u32>().ok()).unwrap_or(0)
}

fn renderer_path(explicit: Option<&str>, kwin: bool, transition: bool) -> &str {
    if transition {
        return "dmabuf-present";
    }
    match explicit {
        Some("cpu") | None => {
            if kwin {
                "shared"
            } else {
                "dmabuf-present"
            }
        }
        Some(path) => path,
    }
}

fn control_stdin_enabled(args: &[String]) -> bool {
    !args.iter().any(|arg| arg == "--standalone")
}

fn transition_hold_enabled(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--transition-hold")
}

pub(crate) fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ctl::set_stdin_enabled(control_stdin_enabled(&args));
    #[cfg(feature = "shared-device")]
    if let Some(index) = args.iter().position(|arg| arg == "--decode-probe") {
        return crate::probe::run(&args[index + 1..]);
    }
    if args.iter().any(|arg| arg == "--video-stream") {
        if args.len() < 3 {
            usage();
        }
        let fill = parse_flag(&args[3..], "--fill-mode")
            .and_then(|text| text.parse::<FillMode>().ok())
            .unwrap_or_default();
        let fps =
            parse_flag(&args[3..], "--stream-fps").and_then(|text| text.parse().ok()).unwrap_or(30);
        let stream_fd = parse_flag(&args[3..], "--stream-fd").and_then(|text| text.parse().ok());
        let transition_from = parse_flag(&args[3..], "--transition-from");
        let shader = parse_flag(&args[3..], "--shader").unwrap_or("fade");
        let duration_ms = parse_flag(&args[3..], "--duration-ms")
            .and_then(|text| text.parse().ok())
            .unwrap_or(600);
        let mute = parse_flag(&args[3..], "--mute").is_none_or(|value| value == "true");
        let volume = parse_flag(&args[3..], "--volume")
            .and_then(|text| text.parse::<u32>().ok())
            .unwrap_or(80)
            .min(100);
        let paused = args[3..].iter().any(|arg| arg == "--paused");
        let write_header = !args[3..].iter().any(|arg| arg == "--stream-no-header");
        set_fill_mode(fill);
        let (width, height) = crate::preview::parse_size(parse_flag(&args[3..], "--stream-size"));
        if args[3..].iter().any(|arg| arg == "--scene") {
            let stream_fd = stream_fd.context("scene stream requires --stream-fd")?;
            let properties = parse_flag(&args[3..], "--scene-properties")
                .map(super::scene::parse_scene_properties)
                .unwrap_or_default();
            return super::scene::stream_scene(
                &args[2],
                &properties,
                width,
                height,
                fps,
                stream_fd,
                mute,
                volume,
                paused,
            );
        }
        return if let Some(stream_fd) = stream_fd {
            crate::preview::dmabuf_video_stream(
                &args[2],
                width,
                height,
                fps,
                stream_fd,
                transition_from,
                shader,
                duration_ms,
                mute,
                volume,
                paused,
                write_header,
            )
        } else {
            crate::preview::video_stream(&args[2], width, height, fps, write_header)
        };
    }
    #[cfg(feature = "shared-device")]
    if args.iter().any(|arg| arg == "--preview-stream") {
        if args.len() < 3 {
            usage();
        }
        let from = parse_flag(&args[3..], "--transition-from").unwrap_or(&args[2]);
        let shader = parse_flag(&args[3..], "--shader").unwrap_or("fade");
        let duration_ms = parse_flag(&args[3..], "--duration-ms")
            .and_then(|text| text.parse().ok())
            .unwrap_or(600);
        let frame_ms = parse_flag(&args[3..], "--preview-frame-ms")
            .and_then(|text| text.parse().ok())
            .unwrap_or(16);
        let write_header = !args[3..].iter().any(|arg| arg == "--stream-no-header");
        let once = args[3..].iter().any(|arg| arg == "--preview-once");
        let fill = parse_flag(&args[3..], "--fill-mode")
            .and_then(|text| text.parse::<FillMode>().ok())
            .unwrap_or_default();
        set_fill_mode(fill);
        let (width, height) = crate::preview::parse_size(parse_flag(&args[3..], "--preview-size"));
        if let Some(socket) =
            parse_flag(&args[3..], "--stream-fd").and_then(|value| value.parse().ok())
        {
            anyhow::ensure!(once, "GPU transition streams must be one-shot");
            return crate::preview::gpu::stream(
                from,
                &args[2],
                shader,
                width,
                height,
                duration_ms,
                frame_ms,
                socket,
            );
        }
        return crate::preview::stream(
            from,
            &args[2],
            shader,
            width,
            height,
            duration_ms,
            frame_ms,
            write_header,
            once,
        );
    }
    if args.len() < 3 {
        usage();
    }
    let idle_env = std::env::var("SKWD_PAPER_IDLE_SEC").ok();
    let idle_secs = parse_idle_secs(idle_env.as_deref());
    #[cfg(feature = "shared-device")]
    if args[1] == "--multi-json" {
        let entries = super::multi::parse_manifest(&args[2]);
        if entries.is_empty() {
            usage();
        }
        let layer = parse_flag(&args[3..], "--layer");
        let shader = parse_flag(&args[3..], "--shader");
        let duration_ms = parse_flag(&args[3..], "--duration-ms")
            .and_then(|text| text.parse().ok())
            .unwrap_or(600);
        let fill = parse_flag(&args[3..], "--fill-mode")
            .and_then(|text| text.parse::<FillMode>().ok())
            .unwrap_or_default();
        set_fill_mode(fill);
        tracing::info!(outputs = entries.len(), fill = ?fill, "starting skwd-wall-vk (multi)");
        return super::multi::run_multi(&entries, layer, idle_secs, shader, duration_ms);
    }
    #[cfg(feature = "shared-device")]
    let scene_dir = parse_flag(&args[1..], "--scene").map(String::from);
    #[cfg(feature = "shared-device")]
    let scene_properties = parse_flag(&args[1..], "--scene-properties")
        .map(super::scene::parse_scene_properties)
        .unwrap_or_default();
    let (output, video) = (&args[1], &args[2]);
    let (mut mute, mut volume) = ctl::parse_audio_opts(&args[3..]);
    if let Some(flag) = parse_flag(&args[3..], "--mute") {
        mute = flag == "true";
    }
    if let Some(vol) = parse_flag(&args[3..], "--volume").and_then(|text| text.parse::<u32>().ok())
    {
        volume = vol.min(100);
    }
    let persist = args[3..].iter().any(|arg| arg == "--persist");
    let start_fade = parse_flag(&args[3..], "--transition-from").map(|from| StartFade {
        from: from.to_string(),
        shader: parse_flag(&args[3..], "--shader").map(String::from),
        duration_ms: parse_flag(&args[3..], "--duration-ms")
            .and_then(|text| text.parse().ok())
            .unwrap_or(600),
        overlay: !persist,
        held: transition_hold_enabled(&args[3..]),
    });
    let layer = parse_flag(&args[3..], "--layer");
    let fill = parse_flag(&args[3..], "--fill-mode")
        .and_then(|text| text.parse::<FillMode>().ok())
        .unwrap_or_default();
    set_fill_mode(fill);
    tracing::info!(output = %output, video = %video, fill = ?fill, "starting skwd-wall-vk");

    let mut target = wayland::setup(output, layer).context("wayland setup")?;
    if scene_dir.is_none() {
        target.set_content_type(
            wayland_protocols::wp::content_type::v1::client::wp_content_type_v1::Type::Video,
        );
    }
    for surf in &target.app.surfaces {
        tracing::info!(w = surf.width, h = surf.height, output = %surf.name, "vk surface");
    }
    if idle_secs > 0 {
        target.arm_idle(idle_secs);
    }

    #[cfg(feature = "shared-device")]
    if let Some(dir) = scene_dir {
        let result =
            super::scene::run_scene(&mut target, &dir, &scene_properties, mute, volume, start_fade);
        if let Err(err) = &result {
            tracing::error!("skwd-wall-vk: scene render failed: {err:#}");
        }
        return result;
    }
    let result = dispatch_path(&mut target, video, mute, volume, start_fade);
    if result.is_err()
        && let Some(perr) = target.conn.protocol_error()
    {
        tracing::error!(
            "wayland protocol error on {} (object {}), code {}: {}",
            perr.object_interface,
            perr.object_id,
            perr.code,
            perr.message
        );
    }
    result
}

fn dispatch_path(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
    start_fade: Option<StartFade>,
) -> Result<()> {
    #[cfg(feature = "shared-device")]
    {
        let cover = matches!(fill_mode(), FillMode::Fill | FillMode::Span);
        let cuts_only = std::env::var("SKWD_PAPER_TRANSITIONS").as_deref() == Ok("0");
        let explicit_path = std::env::var("SKWD_VK_PATH").ok();
        let kwin = wayland::is_kwin_session();
        let path = renderer_path(explicit_path.as_deref(), kwin, start_fade.is_some());
        tracing::info!(kwin, path, "skwd-wall-vk renderer policy");
        match path {
            "shared" => return run_shared(target, video, mute, volume),
            "dmabuf-present" => {
                if cover && cuts_only && start_fade.is_none() && !crate::decode::sw_decode_forced()
                {
                    match run_nv12(target, video, mute, volume) {
                        Ok(()) => return Ok(()),
                        Err(err) => {
                            tracing::info!(
                                "skwd-wall-vk: nv12 direct-present unavailable ({err:#}), using dmabuf-present"
                            );
                        }
                    }
                }
                return run_shared_dmabuf(target, video, mute, volume, start_fade);
            }
            "nv12" => {
                if cover {
                    return run_nv12(target, video, mute, volume);
                }
                tracing::info!(
                    "skwd-wall-vk: nv12 path is cover-only, using dmabuf-present for {:?}",
                    fill_mode()
                );
                return run_shared_dmabuf(target, video, mute, volume, start_fade);
            }
            _ => {}
        }
    }

    let _ = start_fade;
    run_upload(target, video, mute, volume)
}

#[cfg(test)]
mod tests;
