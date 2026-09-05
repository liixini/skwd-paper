use crate::cli::{Command, EngineArg, FillModeArg, KindArg, LayerArg};
use anyhow::{Context, Result, anyhow};
use paper_control::{
    ApplyRequest, Assignment, AudioSetRequest, CapabilitiesRequest, FillMode, Layer, PauseRequest,
    RendererPolicy, Request, RequestParams, Source, SourceKind, StatusRequest, StopRequest,
    TransitionPolicy, VideoEngine, is_video_path,
};
use std::io::Read;
use std::path::Path;

const MAX_MANIFEST: u64 = 1024 * 1024;

pub(crate) fn run() -> Result<()> {
    match crate::cli::Cli::read().command {
        Command::Serve => crate::server::run(),
        Command::Apply(args) => send(RequestParams::Apply(apply_request(args)?)),
        Command::Stop(args) => {
            send(RequestParams::Stop(StopRequest { outputs: normalize_outputs(args.outputs) }))
        }
        Command::Pause => send(RequestParams::Pause(PauseRequest { paused: true })),
        Command::Resume => send(RequestParams::Pause(PauseRequest { paused: false })),
        Command::Audio(args) => send(RequestParams::AudioSet(AudioSetRequest {
            outputs: normalize_outputs(args.outputs),
            mute: args.mute,
            volume: args.volume,
        })),
        Command::Status => send(RequestParams::Status(StatusRequest {})),
        Command::Outputs => {
            println!("{}", serde_json::to_string_pretty(&crate::outputs::query()?)?);
            Ok(())
        }
        Command::Capabilities(args) => send(RequestParams::Capabilities(CapabilitiesRequest {
            reset_decode_cache: args.reset_decode_cache,
        })),
        Command::PresentPlasma(args) => {
            let assignment: Assignment =
                serde_json::from_str(&args.assignment).context("decode Plasma assignment")?;
            let mut validated = assignment.clone();
            let transition = validated.transition.take();
            ApplyRequest { assignments: vec![validated], replace_all: false, policy: None }
                .validate()
                .map_err(|error| anyhow!(error.to_string()))?;
            if let Some(transition) = transition {
                transition.validate().map_err(|error| anyhow!(error.to_string()))?;
            }
            crate::backend::present_plasma(
                &assignment,
                &args.stream_size,
                args.stream_fps,
                args.stream_fd,
                args.paused,
            )
        }
    }
}

fn apply_request(args: crate::cli::ApplyArgs) -> Result<ApplyRequest> {
    if let Some(manifest) = args.manifest {
        let manifest = read_manifest(&manifest)?;
        let mut request: ApplyRequest =
            serde_json::from_str(&manifest).context("decode Paper apply manifest")?;
        if args.replace_all {
            request.replace_all = true;
        }
        resolve_paths(&mut request)?;
        request.validate().map_err(|error| anyhow!(error.to_string()))?;
        return Ok(request);
    }

    let output = args.output.ok_or_else(|| anyhow!("apply requires OUTPUT"))?;
    let path = args.path.ok_or_else(|| anyhow!("apply requires PATH"))?;
    let kind = args.kind.map_or_else(|| infer_kind(&path), source_kind);
    let engine = args.engine.map(video_engine);
    let properties = args.properties.as_deref().map(parse_properties).transpose()?;
    let source = Source { kind, path, engine, frame_rate: args.frame_rate, properties };
    let outputs = normalize_outputs(output.split(',').map(str::to_string).collect());
    let mut assignment = Assignment::new(outputs, source);
    if let Some(fill_mode) = args.fill_mode {
        assignment.fill_mode = protocol_fill(fill_mode);
    }
    if let Some(mute) = args.mute {
        assignment.mute = mute;
    }
    if let Some(volume) = args.volume {
        assignment.volume = volume;
    }
    if let Some(layer) = args.layer {
        assignment.layer = protocol_layer(layer);
    }
    if args.transition
        || args.transition_from.is_some()
        || args.effect.is_some()
        || args.duration_ms.is_some()
    {
        assignment.transition = Some(TransitionPolicy {
            from: args.transition_from,
            effect: args.effect,
            duration_ms: args.duration_ms,
        });
    }
    let policy = args
        .idle_seconds
        .map(|seconds| RendererPolicy { idle_seconds: Some(seconds), ..Default::default() });
    let mut request =
        ApplyRequest { assignments: vec![assignment], replace_all: args.replace_all, policy };
    resolve_paths(&mut request)?;
    request.validate().map_err(|error| anyhow!(error.to_string()))?;
    Ok(request)
}

fn normalize_outputs(outputs: Vec<String>) -> Vec<String> {
    outputs.into_iter().map(|output| if output == "ALL" { "*".into() } else { output }).collect()
}

fn resolve_paths(request: &mut ApplyRequest) -> Result<()> {
    for assignment in &mut request.assignments {
        let paths = std::iter::once(&mut assignment.source.path)
            .chain(assignment.transition.as_mut().and_then(|transition| transition.from.as_mut()));
        for path in paths {
            if !path.trim().is_empty() && !path.starts_with('-') {
                *path = std::path::absolute(&*path)?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| anyhow!("media path is not valid UTF-8"))?;
            }
        }
    }
    Ok(())
}

fn parse_properties(raw: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    let value: serde_json::Value =
        serde_json::from_str(raw).context("decode scene properties JSON")?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(anyhow!("scene properties must be a JSON object")),
    }
}

fn read_manifest(value: &str) -> Result<String> {
    if value == "-" {
        return read_bounded(std::io::stdin().lock(), "stdin");
    }
    if let Some(path) = value.strip_prefix('@') {
        if path.is_empty() {
            return Err(anyhow!("Paper manifest file path is empty"));
        }
        let file = std::fs::File::open(path)
            .with_context(|| format!("open Paper apply manifest {path}"))?;
        return read_bounded(file, path);
    }
    if value.len() as u64 > MAX_MANIFEST {
        return Err(anyhow!("Paper apply manifest exceeds {MAX_MANIFEST} bytes"));
    }
    Ok(value.to_string())
}

fn read_bounded(reader: impl Read, label: &str) -> Result<String> {
    let mut bytes = Vec::new();
    reader.take(MAX_MANIFEST + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err(anyhow!("Paper apply manifest from {label} exceeds {MAX_MANIFEST} bytes"));
    }
    String::from_utf8(bytes)
        .with_context(|| format!("Paper apply manifest from {label} is not UTF-8"))
}

fn infer_kind(path: &str) -> SourceKind {
    if Path::new(path).is_dir() {
        SourceKind::WallpaperEngine
    } else if is_video_path(path) {
        SourceKind::Video
    } else {
        SourceKind::Static
    }
}

const fn source_kind(kind: KindArg) -> SourceKind {
    match kind {
        KindArg::Static => SourceKind::Static,
        KindArg::Video => SourceKind::Video,
        KindArg::We => SourceKind::WallpaperEngine,
    }
}

const fn video_engine(engine: EngineArg) -> VideoEngine {
    match engine {
        EngineArg::Default => VideoEngine::Default,
        EngineArg::Tinier => VideoEngine::Tinier,
    }
}

const fn protocol_fill(fill_mode: FillModeArg) -> FillMode {
    match fill_mode {
        FillModeArg::Fill => FillMode::Fill,
        FillModeArg::Fit => FillMode::Fit,
        FillModeArg::Stretch => FillMode::Stretch,
        FillModeArg::Center => FillMode::Center,
        FillModeArg::Tile => FillMode::Tile,
        FillModeArg::Span => FillMode::Span,
    }
}

const fn protocol_layer(layer: LayerArg) -> Layer {
    match layer {
        LayerArg::Background => Layer::Background,
        LayerArg::Bottom => Layer::Bottom,
        LayerArg::Top => Layer::Top,
    }
}

fn send(params: RequestParams) -> Result<()> {
    let request = Request::new(u64::from(std::process::id()), params);
    let response = crate::client::request(&request)?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    if let Some(error) = response.get("error") {
        let message = error.get("message").and_then(serde_json::Value::as_str).unwrap_or("failed");
        return Err(anyhow!(message.to_string()));
    }
    Ok(())
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
