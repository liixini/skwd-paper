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

#[cfg(feature = "shared-device")]
const REAL_TIME_FLAG: &str = "--real-time";

fn usage() -> ! {
    tracing::info!(
        "usage: skwd-wall-vk <output|*> <video> [--standalone] [--layer background|bottom|top|overlay] [-o mute=yes;volume=80]"
    );
    std::process::exit(2);
}

fn parse_flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|arg| arg == name).and_then(|idx| args.get(idx + 1)).map(String::as_str)
}

fn duration_flag(args: &[String], shader: &str) -> u64 {
    parse_flag(args, "--duration-ms")
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| paper_control::transitions::transition_duration(shader))
}

#[cfg(feature = "shared-device")]
fn parse_flags<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2).filter(|pair| pair[0] == name).map(|pair| pair[1].as_str()).collect()
}

#[cfg(feature = "shared-device")]
pub(super) fn stream_targets(args: &[String], paused: bool) -> Vec<crate::preview::StreamTarget> {
    let sizes = parse_flags(args, "--stream-size");
    let rates = parse_flags(args, "--stream-fps");
    let outputs = parse_flags(args, "--stream-output");
    let paused_outputs = parse_flags(args, "--stream-paused");
    parse_flags(args, "--stream-fd")
        .iter()
        .enumerate()
        .filter_map(|(index, fd)| {
            let socket = fd.parse().ok()?;
            let size = sizes.get(index).or(sizes.first()).copied();
            let (width, height) = crate::preview::parse_size(size);
            let fps =
                rates.get(index).or(rates.first()).and_then(|text| text.parse().ok()).unwrap_or(30);
            let output = outputs.get(index).copied().unwrap_or_default().to_string();
            Some(crate::preview::StreamTarget {
                socket,
                width,
                height,
                fps,
                paused: paused || (!output.is_empty() && paused_outputs.contains(&output.as_str())),
                output,
            })
        })
        .collect()
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
    #[cfg(feature = "shared-device")]
    if args.get(1).map(String::as_str) == Some("--scene-thumbnail-worker") {
        return super::scene::thumbnail::run();
    }
    ctl::set_stdin_enabled(control_stdin_enabled(&args));
    #[cfg(feature = "shared-device")]
    if let Some(index) = args.iter().position(|arg| arg == "--decode-probe") {
        return crate::probe::run(&args[index + 1..]);
    }
    #[cfg(feature = "shared-device")]
    if args.get(1).map(String::as_str) == Some("scene-dump") {
        if args.len() < 3 {
            usage();
        }
        let opts = &args[3..];
        let number = |flag: &str, fallback: f32| {
            parse_flag(opts, flag).and_then(|text| text.parse::<f32>().ok()).unwrap_or(fallback)
        };
        let (width, height) = crate::preview::parse_size(parse_flag(opts, "--size"));
        let frames =
            parse_flag(opts, "--frames").and_then(|text| text.parse::<u32>().ok()).unwrap_or(1);
        let warmup =
            parse_flag(opts, "--warmup").and_then(|text| text.parse::<u32>().ok()).unwrap_or(0);
        let out = parse_flag(opts, "--out")
            .map(std::path::PathBuf::from)
            .context("scene-dump requires --out")?;
        let properties = parse_flag(opts, "--scene-properties")
            .map(super::scene::parse_scene_properties)
            .unwrap_or_default();
        let fill = parse_flag(opts, "--fill-mode")
            .and_then(|text| text.parse::<FillMode>().ok())
            .unwrap_or_default();
        set_fill_mode(fill);
        return super::scene::dump_scene(&super::scene::DumpSpec {
            dir: &args[2],
            properties: &properties,
            size: (width, height),
            frames,
            warmup,
            real_time: opts.iter().any(|opt| opt == REAL_TIME_FLAG),
            dt: number("--dt", 1.0 / 30.0),
            time0: number("--time0", 0.0),
            daytime: number("--daytime", 0.5),
            out_dir: &out,
        });
    }
    #[cfg(feature = "shared-device")]
    if args.iter().any(|arg| arg == "--video-stream") {
        if args.len() < 3 {
            usage();
        }
        let fill = parse_flag(&args[3..], "--fill-mode")
            .and_then(|text| text.parse::<FillMode>().ok())
            .unwrap_or_default();
        let fps =
            parse_flag(&args[3..], "--stream-fps").and_then(|text| text.parse().ok()).unwrap_or(30);
        let transition_from = parse_flag(&args[3..], "--transition-from");
        let shader = parse_flag(&args[3..], "--shader").unwrap_or("fade");
        let duration_ms = duration_flag(&args[3..], shader);
        let mute = parse_flag(&args[3..], "--mute").is_none_or(|value| value == "true");
        let volume = parse_flag(&args[3..], "--volume")
            .and_then(|text| text.parse::<u32>().ok())
            .unwrap_or(80)
            .min(100);
        let paused = args[3..].iter().any(|arg| arg == "--paused");
        let write_header = !args[3..].iter().any(|arg| arg == "--stream-no-header");
        set_fill_mode(fill);
        let (width, height) = crate::preview::parse_size(parse_flag(&args[3..], "--stream-size"));
        let targets = stream_targets(&args[3..], paused);
        if args[3..].iter().any(|arg| arg == "--scene") {
            anyhow::ensure!(!targets.is_empty(), "scene stream requires --stream-fd");
            let properties = parse_flag(&args[3..], "--scene-properties")
                .map(super::scene::parse_scene_properties)
                .unwrap_or_default();
            return super::scene::stream_scene(&args[2], &properties, targets, mute, volume);
        }
        return if targets.is_empty() {
            let stream = crate::preview::StreamTarget::single(
                crate::preview::NO_SOCKET,
                width,
                height,
                fps,
                paused,
            );
            crate::preview::video_stream(&args[2], &stream, write_header)
        } else {
            crate::preview::dmabuf_video_stream(
                &args[2],
                targets,
                transition_from,
                shader,
                duration_ms,
                mute,
                volume,
                write_header,
            )
        };
    }
    #[cfg(feature = "shared-device")]
    if args.iter().any(|arg| arg == "--preview-stream") {
        if args.len() < 3 {
            usage();
        }
        let from = parse_flag(&args[3..], "--transition-from").unwrap_or(&args[2]);
        let shader = parse_flag(&args[3..], "--shader").unwrap_or("fade");
        let duration_ms = duration_flag(&args[3..], shader);
        let frame_ms: u64 = parse_flag(&args[3..], "--preview-frame-ms")
            .and_then(|text| text.parse().ok())
            .unwrap_or(16);
        let frame_interval = parse_flag(&args[3..], "--preview-fps")
            .and_then(|text| text.parse::<u32>().ok())
            .map_or_else(
                || std::time::Duration::from_millis(frame_ms.clamp(4, 200)),
                |fps| {
                    std::time::Duration::from_nanos(1_000_000_000 / u64::from(fps.clamp(1, 1000)))
                },
            );
        let write_header = !args[3..].iter().any(|arg| arg == "--stream-no-header");
        let once = args[3..].iter().any(|arg| arg == "--preview-once");
        let fill = parse_flag(&args[3..], "--fill-mode")
            .and_then(|text| text.parse::<FillMode>().ok())
            .unwrap_or_default();
        set_fill_mode(fill);
        let preview = crate::preview::Preview {
            from,
            to: &args[2],
            shader,
            size: crate::preview::parse_size(parse_flag(&args[3..], "--preview-size")),
            duration_ms,
            frame_interval,
            drawing: preview_drawing(shader, std::env::var(paper_control::DRAWING_ENV).ok())?,
        };
        if let Some(socket) =
            parse_flag(&args[3..], "--stream-fd").and_then(|value| value.parse().ok())
        {
            anyhow::ensure!(once, "GPU transition streams must be one-shot");
            return crate::preview::gpu::stream(&preview, socket);
        }
        return crate::preview::stream(&preview, write_header, once);
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
        let duration_ms =
            duration_flag(&args[3..], shader.unwrap_or(paper_control::transitions::DEFAULT_EFFECT));
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
    let shader = parse_flag(&args[3..], "--shader");
    let start_fade = parse_flag(&args[3..], "--transition-from").map(|from| StartFade {
        from: from.to_string(),
        shader: shader.map(String::from),
        duration_ms: duration_flag(
            &args[3..],
            shader.unwrap_or(paper_control::transitions::DEFAULT_EFFECT),
        ),
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
    target.set_content_type(
        wayland_protocols::wp::content_type::v1::client::wp_content_type_v1::Type::Video,
    );
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

#[cfg(feature = "shared-device")]
fn preview_drawing(
    shader: &str,
    value: Option<String>,
) -> Result<Option<paper_control::DrawingTransition>> {
    if shader != paper_shaders::DRAWING_SHADER {
        return Ok(None);
    }
    Ok(value.map(|value| serde_json::from_str(&value)).transpose()?)
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
        if crate::surface::needs_shader() {
            return run_shared_dmabuf(target, video, mute, volume, start_fade);
        }
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
