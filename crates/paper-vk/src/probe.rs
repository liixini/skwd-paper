mod concurrent;

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use ffmpeg_the_third as ff;

const REPEATS: usize = 5;

struct Attempt {
    backend: &'static str,
    accepted: bool,
    detail: String,
    samples: Vec<Duration>,
}

impl Attempt {
    fn median_ms(&self) -> f64 {
        let mut sorted: Vec<f64> =
            self.samples.iter().map(|sample| sample.as_secs_f64() * 1000.0).collect();
        sorted.sort_by(f64::total_cmp);
        sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
    }

    fn spread_ms(&self) -> (f64, f64) {
        let mut sorted: Vec<f64> =
            self.samples.iter().map(|sample| sample.as_secs_f64() * 1000.0).collect();
        sorted.sort_by(f64::total_cmp);
        (sorted.first().copied().unwrap_or(0.0), sorted.last().copied().unwrap_or(0.0))
    }
}

fn time_it<T>(mut run: impl FnMut() -> Result<T>) -> (bool, String, Vec<Duration>) {
    let mut samples = Vec::with_capacity(REPEATS);
    let mut accepted = false;
    let mut detail = String::new();
    for _ in 0..REPEATS {
        let started = Instant::now();
        let outcome = run();
        samples.push(started.elapsed());
        match outcome {
            Ok(_) => accepted = true,
            Err(error) => detail = format!("{error:#}"),
        }
    }
    (accepted, detail, samples)
}

fn bare_vaapi_device(render_node: Option<&std::path::Path>) -> Result<()> {
    let mut dev: *mut ff::ffi::AVBufferRef = std::ptr::null_mut();
    let node = render_node.and_then(|path| path.to_str()).unwrap_or_default();
    let c_node = std::ffi::CString::new(node).unwrap_or_default();
    let node_ptr = if node.is_empty() { std::ptr::null() } else { c_node.as_ptr() };
    let rc = unsafe {
        ff::ffi::av_hwdevice_ctx_create(
            &raw mut dev,
            ff::ffi::AVHWDeviceType::VAAPI,
            node_ptr,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc < 0 || dev.is_null() {
        return Err(anyhow!("av_hwdevice_ctx_create returned {rc}"));
    }
    unsafe { ff::ffi::av_buffer_unref(&raw mut dev) };
    Ok(())
}

fn vaapi_drm_vulkan(shared: &crate::shared::SharedDevice, path: &str) -> Result<()> {
    let mut decoder = crate::decode::VaapiDecoder::open(path, shared.render_node.as_deref())?;
    let (frame, _) = decoder.next_hw_frame()?;
    let mapped = crate::decode::map_to_drm(&frame)?;
    mapped.wait_ready()?;
    let renderer = crate::vk::Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        decoder.width,
        decoder.height,
    )?;
    let imported =
        renderer.import_nv12(decoder.width, decoder.height, &mapped.luma, &mapped.chroma)?;
    renderer.destroy_frame(imported);
    Ok(())
}

fn probe_video(shared: &crate::shared::SharedDevice, path: &str) -> Vec<Attempt> {
    let mut attempts = Vec::new();

    let (accepted, detail, samples) = time_it(|| {
        crate::decode::open_decoder(
            path,
            Some(shared.hwdev),
            shared.video_decode,
            shared.render_node.as_deref(),
            false,
        )
        .map(|_| ())
    });
    attempts.push(Attempt { backend: "cascade", accepted, detail, samples });

    let (accepted, detail, samples) = time_it(|| bare_vaapi_device(shared.render_node.as_deref()));
    attempts.push(Attempt { backend: "vaapi-device-only", accepted, detail, samples });

    if shared.video_decode {
        let (accepted, detail, samples) =
            time_it(|| crate::decode::VulkanDecoder::open_with(path, shared.hwdev).map(|_| ()));
        attempts.push(Attempt { backend: "vulkan", accepted, detail, samples });
    } else {
        attempts.push(Attempt {
            backend: "vulkan",
            accepted: false,
            detail: String::from("no vulkan video decode queue on this device"),
            samples: vec![Duration::ZERO],
        });
    }

    let (accepted, detail, samples) =
        time_it(|| crate::decode::VulkanDecoder::open(path).map(|_| ()));
    attempts.push(Attempt { backend: "vulkan-standalone", accepted, detail, samples });

    let (accepted, detail, samples) = time_it(|| {
        crate::decode::VaapiDecoder::open(path, shared.render_node.as_deref()).map(|_| ())
    });
    attempts.push(Attempt { backend: "vaapi", accepted, detail, samples });

    let (accepted, detail, samples) = time_it(|| vaapi_drm_vulkan(shared, path));
    attempts.push(Attempt { backend: "vaapi-drm-vulkan", accepted, detail, samples });

    let (accepted, detail, samples) = time_it(|| crate::decode::SwDecoder::open(path).map(|_| ()));
    attempts.push(Attempt { backend: "software", accepted, detail, samples });

    attempts
}

fn report(path: &str, attempts: &[Attempt]) {
    println!("\nvideo: {path}");
    println!("  {:<18} {:>9} {:>9} {:>9}  outcome", "backend", "median", "min", "max");
    for attempt in attempts {
        let (low, high) = attempt.spread_ms();
        let outcome = if attempt.accepted {
            String::from("accepted")
        } else {
            format!("rejected: {}", attempt.detail)
        };
        println!(
            "  {:<18} {:>8.1}ms {:>8.1}ms {:>8.1}ms  {}",
            attempt.backend,
            attempt.median_ms(),
            low,
            high,
            outcome
        );
    }
}

fn json_line(path: &str, fingerprint: &str, attempts: &[Attempt]) -> String {
    let backends: Vec<String> = attempts
        .iter()
        .map(|attempt| {
            format!(
                "{{\"backend\":\"{}\",\"accepted\":{},\"median_ms\":{:.3},\"detail\":{}}}",
                attempt.backend,
                attempt.accepted,
                attempt.median_ms(),
                serde_json::Value::String(attempt.detail.clone())
            )
        })
        .collect();
    format!(
        "{{\"video\":{},\"fingerprint\":{},\"repeats\":{},\"backends\":[{}]}}",
        serde_json::Value::String(path.to_string()),
        serde_json::Value::String(fingerprint.to_string()),
        REPEATS,
        backends.join(",")
    )
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    if std::env::var_os("SKWD_FFMPEG_LOG").is_some() {
        ff::util::log::set_level(ff::util::log::Level::Verbose);
    }
    let workers = args
        .iter()
        .position(|arg| arg == "--concurrent")
        .and_then(|index| args.get(index + 1))
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|error| anyhow!("--concurrent needs a worker count: {error}"))?;
    let videos: Vec<String> = {
        let mut kept = Vec::new();
        let mut skip = false;
        for arg in args {
            if skip {
                skip = false;
                continue;
            }
            if arg == "--concurrent" {
                skip = true;
                continue;
            }
            kept.push(arg.clone());
        }
        kept
    };
    let videos = &videos[..];
    if videos.is_empty() {
        return Err(anyhow!(
            "usage: skwd-wall-vk --decode-probe [--concurrent N] <video> [<video>...]"
        ));
    }
    let shared = crate::shared::create(std::ptr::null_mut())
        .map_err(|error| anyhow!("vulkan device unavailable: {error:#}"))?;

    println!("skwd-wall-vk decode probe");
    println!("  fingerprint        : {}", shared.decode_fingerprint);
    println!("  vulkan decode queue: {}", shared.video_decode);
    println!(
        "  render node        : {}",
        shared
            .render_node
            .as_deref()
            .map_or_else(|| String::from("none"), |path| path.display().to_string())
    );
    println!("  repeats per backend: {REPEATS}");

    let mut lines = Vec::new();
    for path in videos {
        let attempts = probe_video(&shared, path);
        report(path, &attempts);
        lines.push(json_line(path, &shared.decode_fingerprint, &attempts));
    }

    if let Some(workers) = workers {
        concurrent::run(&videos[0], shared.render_node.as_deref(), workers.max(2))?;
    }

    println!("\njson");
    for line in lines {
        println!("{line}");
    }
    Ok(())
}
