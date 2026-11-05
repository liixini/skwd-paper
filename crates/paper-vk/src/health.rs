mod input;
mod policy;

use anyhow::{Context, Result, anyhow, ensure};
use input::Input;
pub(crate) use policy::Outcome;
use policy::{Budget, Cache, Content, Key};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::ffi::OsString;
use std::io::Write;
use std::num::NonZeroUsize;
use std::os::unix::process::ExitStatusExt;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(crate) use policy::Device;
const MODE: &str = "--decode-health-child";
const EXPERIMENT: &str = "SKWD_PAPER_EXPERIMENTAL_VIDEO_HEALTH";
const ENABLED: &str = "1";
const INPUT: &str = "/proc/self/fd/0";
const CONTRACT: &str = "scene-vulkan-frame-v1";
const REJECTED: i32 = 2;
const SCENE_LIMIT: Duration = Duration::from_secs(5);
const CHILD_LIMIT: Duration = Duration::from_secs(2);
const CACHE_ENTRIES: usize = 16;
pub(crate) const REPORT_BYTES: usize = 4096;
const CHILD_ARGUMENTS: usize = 3;
const REPORT_END: u8 = b'\n';
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
thread_local! { static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) }; }

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    device: Device,
    clip: Option<crate::decode::Clip>,
}

pub(crate) enum Job {
    Vulkan(Request),
    #[cfg(test)]
    Fault(String),
}

impl Job {
    pub(crate) fn arguments(&self) -> Vec<OsString> {
        match self {
            Self::Vulkan(request) => {
                vec![MODE.into(), serde_json::to_string(request).expect("probe request").into()]
            }
            #[cfg(test)]
            Self::Fault(_) => vec![
                "--exact".into(),
                "sandbox::broker::tests::fault_child".into(),
                "--nocapture".into(),
            ],
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    contract: String,
    device: Device,
    frames: u32,
}

#[derive(Debug)]
enum ProbeStage {
    Started,
    Device,
    Decoder,
    Frame,
    Renderer,
    Target,
    Submitted,
    Waited,
    TargetDropped,
    FrameDropped,
    DecoderDropped,
    RendererDropped,
}

fn stage(value: &ProbeStage) {
    eprintln!("health probe stage {value:?}");
}

pub(crate) struct SceneBudget(Option<Instant>);

impl SceneBudget {
    pub(crate) fn new() -> Self {
        Self(DEADLINE.with(|slot| {
            let previous = slot.get();
            let proposed = Instant::now() + SCENE_LIMIT;
            slot.set(Some(previous.map_or(proposed, |deadline| deadline.min(proposed))));
            previous
        }))
    }
}

impl Drop for SceneBudget {
    fn drop(&mut self) {
        DEADLINE.with(|slot| slot.set(self.0));
    }
}

fn enabled() -> bool {
    std::env::var(EXPERIMENT).as_deref() == Ok(ENABLED)
}

pub(crate) fn child_requested(arguments: &[String]) -> bool {
    arguments.get(1).is_some_and(|argument| argument == MODE)
}

pub(crate) fn child(arguments: &[String]) -> i32 {
    let result = child_request(arguments).and_then(|request| {
        crate::sandbox::restrict_renderer().map_err(|error| anyhow!(error))?;
        let identity = complete_frame(&request)?;
        let report = Report { contract: CONTRACT.into(), device: identity, frames: 1 };
        let mut encoded = serde_json::to_vec(&report)?;
        ensure!(encoded.len() < REPORT_BYTES, "probe report exceeds limit");
        encoded.push(REPORT_END);
        std::io::stdout().lock().write_all(&encoded)?;
        Ok(())
    });
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error:#}");
            REJECTED
        }
    }
}

fn child_request(arguments: &[String]) -> Result<Request> {
    ensure!(arguments.len() == CHILD_ARGUMENTS, "invalid probe arguments");
    let encoded = &arguments[2];
    ensure!(encoded.len() <= REPORT_BYTES, "probe request exceeds limit");
    Ok(serde_json::from_str(encoded)?)
}

fn complete_frame(request: &Request) -> Result<Device> {
    stage(&ProbeStage::Started);
    let expected = &request.device;
    let shared = crate::shared::create_with_policy(std::ptr::null_mut(), true, Some(expected))?;
    stage(&ProbeStage::Device);
    ensure!(&shared.decode_device == expected, "probe device identity mismatch");
    let source = request.clip.map_or_else(|| INPUT.to_owned(), |clip| clip.url(INPUT));
    let mut decoder = crate::decode::VulkanDecoder::open_with(&source, shared.hwdev)?;
    stage(&ProbeStage::Decoder);
    let (frame, _) = decoder.next_hw_frame()?;
    stage(&ProbeStage::Frame);
    ensure!(
        frame.format() == ffmpeg_the_third::format::Pixel::VULKAN,
        "probe returned software frame"
    );
    let mut renderer = crate::shared::renderer(&shared, decoder.width, decoder.height)?;
    stage(&ProbeStage::Renderer);
    let target =
        renderer.create_video_texture_target(decoder.width, decoder.height, true, false)?;
    stage(&ProbeStage::Target);
    let rendered = renderer.render_video_texture(&target, &crate::vk::Src::Avvk(&frame));
    if rendered.is_ok() {
        stage(&ProbeStage::Submitted);
    }
    let waited = finish_submission(rendered, || renderer.wait_render());
    if waited.is_ok() {
        stage(&ProbeStage::Waited);
    }
    drop(target);
    stage(&ProbeStage::TargetDropped);
    waited?;
    drop(frame);
    stage(&ProbeStage::FrameDropped);
    drop(decoder);
    stage(&ProbeStage::DecoderDropped);
    drop(renderer);
    stage(&ProbeStage::RendererDropped);
    Ok(shared.decode_device.clone())
}

pub(crate) enum Media<'a> {
    Scratch { path: &'a str, payload: &'a [u8] },
    Package { file: &'a std::fs::File, logical: &'a str, clip: crate::decode::Clip },
}

impl Media<'_> {
    fn logical(&self) -> &str {
        match *self {
            Self::Scratch { path, .. } => path,
            Self::Package { logical, .. } => logical,
        }
    }

    fn path(&self) -> String {
        match *self {
            Self::Scratch { path, .. } => path.to_owned(),
            Self::Package { file, clip, .. } => clip.url(&input::descriptor(file)),
        }
    }

    fn admission(&self) -> Result<(Input, Content)> {
        match *self {
            Self::Scratch { path, payload } => {
                let content = Content::Digest {
                    payload: Sha256::digest(payload).into(),
                    bytes: payload.len() as u64,
                };
                Ok((Input::open(path.into())?, content))
            }
            Self::Package { file, logical, clip } => {
                let input = Input::new(file.try_clone()?, logical.into(), Some(clip))?;
                let content = Content::Package { identity: input.identity().clone(), clip };
                Ok((input, content))
            }
        }
    }
}

pub(crate) fn open(
    shared: &crate::shared::SharedDevice,
    media: &Media<'_>,
    force_software: bool,
) -> Result<crate::decode::AnyDecoder> {
    let logical = media.logical();
    if !enabled()
        || crate::decode::decoder_requires_software(logical, force_software)
        || crate::decode::vaapi_decode_forced()
    {
        let path = media.path();
        return crate::decode::open_decoder(
            &crate::decode::HardwareDecode {
                path: &path,
                ..crate::decode::HardwareDecode::shared(shared, logical)
            },
            force_software,
        );
    }
    let (input, content) = media.admission().context("open admission source")?;
    let key = Key { device: shared.decode_device.clone(), content, configuration: CONTRACT.into() };
    let admitted = shared.video_decode && admit(&input, key);
    ensure!(input.unchanged()?, "admission source changed: {}", input.logical().display());
    crate::decode::open_hardware(&crate::decode::HardwareDecode {
        path: &input.decode_path(),
        logical,
        hwdev: Some(shared.hwdev),
        vulkan_decode: admitted,
        render_node: shared.render_node.as_deref(),
    })
}

fn admit(input: &Input, key: Key) -> bool {
    let cache = CACHE.get_or_init(|| {
        Mutex::new(Cache::new(NonZeroUsize::new(CACHE_ENTRIES).expect("cache capacity")))
    });
    if let Some(outcome) = cache.lock().expect("admission cache").get(&key) {
        return outcome == Outcome::Healthy;
    }
    let now = Instant::now();
    let scene = DEADLINE.with(Cell::get).unwrap_or(now + SCENE_LIMIT);
    let budget = Budget::new(now, scene.saturating_duration_since(now), CHILD_LIMIT)
        .expect("bounded deadline");
    let Some(deadline) = budget.submit(now) else { return false };
    let outcome = screen(input, &key.device, deadline).unwrap_or_else(|error| {
        tracing::info!("skwd-wall-vk: Vulkan startup screen unavailable ({error:#}), trying VAAPI");
        error_outcome(error.downcast_ref::<std::io::Error>().map(std::io::Error::kind))
    });
    if let Err(error) = retain(cache, input, key, outcome) {
        tracing::info!("skwd-wall-vk: Vulkan startup source invalidated ({error:#})");
        return false;
    }
    if outcome != Outcome::Healthy {
        tracing::info!(?outcome, "skwd-wall-vk: Vulkan startup screen rejected, trying VAAPI");
    }
    outcome == Outcome::Healthy
}

fn retain(cache: &Mutex<Cache>, input: &Input, key: Key, outcome: Outcome) -> Result<()> {
    ensure!(input.unchanged()?, "admission source changed: {}", input.logical().display());
    cache.lock().expect("admission cache").record(key, outcome);
    Ok(())
}

fn finish_submission(submitted: Result<()>, wait: impl FnOnce() -> Result<()>) -> Result<()> {
    submitted?;
    wait()
}

pub(crate) fn error_outcome(kind: Option<std::io::ErrorKind>) -> Outcome {
    match kind {
        Some(std::io::ErrorKind::TimedOut) => Outcome::TimedOut,
        Some(std::io::ErrorKind::Interrupted) => Outcome::Cancelled,
        Some(std::io::ErrorKind::WouldBlock) => Outcome::Saturated,
        _ => Outcome::Invalid,
    }
}

fn screen(input: &Input, device: &Device, deadline: Instant) -> Result<Outcome> {
    let probe = crate::sandbox::broker::Probe {
        source: input.duplicate()?,
        job: Job::Vulkan(Request { device: device.clone(), clip: input.clip() }),
        deadline,
    };
    let finished = crate::sandbox::broker::submit(probe)?.wait()?;
    if let Some((signal, diagnostics)) = crash_diagnostics(&finished) {
        tracing::info!(signal, diagnostics = %diagnostics, "skwd-wall-vk: startup screen child crashed");
    } else if !finished.diagnostics.is_empty() {
        tracing::debug!(diagnostics = %String::from_utf8_lossy(&finished.diagnostics), "skwd-wall-vk: startup screen diagnostics");
    }
    validate(&finished, device)
}

fn crash_diagnostics(finished: &crate::sandbox::broker::Finished) -> Option<(i32, String)> {
    let signal = finished.status.signal()?;
    matches!(signal, libc::SIGSEGV | libc::SIGBUS)
        .then(|| (signal, String::from_utf8_lossy(&finished.diagnostics).into_owned()))
}

fn validate(finished: &crate::sandbox::broker::Finished, device: &Device) -> Result<Outcome> {
    if let Some(signal) = finished.status.signal() {
        return Ok(if matches!(signal, libc::SIGSEGV | libc::SIGBUS) {
            Outcome::Crashed
        } else {
            Outcome::Invalid
        });
    }
    if finished.status.code() == Some(REJECTED) {
        return Ok(Outcome::Invalid);
    }
    ensure!(finished.status.success(), "probe exit rejected");
    let report: Report = serde_json::from_slice(&finished.report)?;
    ensure!(
        report.contract == CONTRACT && report.frames == 1 && &report.device == device,
        "probe response mismatch"
    );
    Ok(Outcome::Healthy)
}

#[cfg(test)]
mod tests;
