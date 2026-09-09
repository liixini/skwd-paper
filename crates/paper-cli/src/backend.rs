use anyhow::{Context, Result, anyhow};
use paper_control::{
    Assignment, Layer, PaperCommand, RendererCapability, RendererDiscovery, RendererPolicy,
    RuntimeDependencyStatus, SandQuality, SandScope, Source, SourceKind, VideoEngine,
};
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

const EXIT_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(crate) struct RendererUnavailable {
    message: String,
}

impl std::fmt::Display for RendererUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RendererUnavailable {}

pub(crate) struct Worker {
    pub(crate) assignment: Assignment,
    pub(crate) output: String,
    pub(crate) generation: u64,
    pub(crate) child: Child,
    pid: u32,
    physical_dynamic: bool,
    freeze_capable: bool,
    retain_capable: bool,
    stdin: Option<ChildStdin>,
    _freeze_directory: Option<Arc<tempfile::TempDir>>,
}

impl Worker {
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn exited(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().context("query Paper worker")
    }

    pub(crate) fn dynamic(&self) -> bool {
        self.physical_dynamic
    }

    pub(crate) fn freeze_capable(&self) -> bool {
        self.freeze_capable
    }

    pub(crate) fn retain_capable(&self) -> bool {
        self.retain_capable
    }

    pub(crate) async fn send(&mut self, command: &PaperCommand) -> Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("Paper worker stdin unavailable"))?;
        stdin.write_all(command.line().as_bytes()).await.context("write Paper worker command")?;
        stdin.flush().await.context("flush Paper worker command")
    }

    pub(crate) async fn stop(mut self) {
        self.stdin.take();
        if matches!(tokio::time::timeout(EXIT_GRACE, self.child.wait()).await, Ok(Ok(_))) {
            return;
        }
        unsafe {
            libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
        }
        if matches!(tokio::time::timeout(EXIT_GRACE, self.child.wait()).await, Ok(Ok(_))) {
            return;
        }
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }

    pub(crate) fn stop_blocking(mut self) {
        self.stdin.take();
        if wait_for_exit(&mut self.child, EXIT_GRACE) {
            return;
        }
        unsafe {
            libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
        }
        if wait_for_exit(&mut self.child, EXIT_GRACE) {
            return;
        }
        let _ = self.child.start_kill();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

#[derive(Clone)]
struct BackendExecutable {
    path: Option<PathBuf>,
    capability: RendererCapability,
}

#[derive(Clone)]
pub(crate) struct BackendPaths {
    vk: BackendExecutable,
    still: BackendExecutable,
    tinier: BackendExecutable,
}

impl BackendPaths {
    pub(crate) fn discover() -> Self {
        let wayland = wayland_dependencies();
        Self {
            vk: discover_backend(
                "SKWD_PAPER_VK_BIN",
                "skwd-wall-vk",
                &[SourceKind::Video, SourceKind::WallpaperEngine],
                &[VideoEngine::Default],
                false,
                vk_dependencies(wayland.clone()),
            ),
            still: discover_backend(
                "SKWD_PAPER_STILL_BIN",
                "skwd-wall-still",
                &[SourceKind::Static],
                &[],
                false,
                wayland.clone(),
            ),
            tinier: discover_backend(
                "SKWD_PAPER_TINIER_BIN",
                "skwd-paper-tinier",
                &[SourceKind::Video],
                &[VideoEngine::Tinier],
                true,
                wayland,
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_executables(vk: PathBuf, still: PathBuf) -> Self {
        Self {
            vk: injected_backend(
                "skwd-wall-vk",
                vk,
                &[SourceKind::Video, SourceKind::WallpaperEngine],
                &[VideoEngine::Default],
            ),
            still: injected_backend("skwd-wall-still", still, &[SourceKind::Static], &[]),
            tinier: unresolved_backend(
                "skwd-paper-tinier",
                &[SourceKind::Video],
                &[VideoEngine::Tinier],
                "skwd-paper-tinier was not injected for this Paper instance".into(),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_executables_with_tinier(
        vk: PathBuf,
        still: PathBuf,
        tinier: PathBuf,
    ) -> Self {
        let mut backends = Self::from_executables(vk, still);
        backends.tinier = injected_backend(
            "skwd-paper-tinier",
            tinier,
            &[SourceKind::Video],
            &[VideoEngine::Tinier],
        );
        backends
    }

    pub(crate) fn capabilities(&self) -> Vec<RendererCapability> {
        [&self.vk, &self.still, &self.tinier]
            .into_iter()
            .map(|backend| backend.capability.clone())
            .collect()
    }

    pub(crate) fn spawn(
        &self,
        assignment: Assignment,
        output: String,
        socket: &Path,
        generation: u64,
        policy: Option<&RendererPolicy>,
    ) -> Result<Worker> {
        let physical = assignment.clone();
        self.spawn_as(&physical, assignment, output, socket, generation, policy, None, false)
    }

    pub(crate) fn spawn_overlay(
        &self,
        assignment: Assignment,
        output: String,
        socket: &Path,
        generation: u64,
        policy: Option<&RendererPolicy>,
    ) -> Result<Worker> {
        let mut physical = assignment.clone();
        physical.source = Source::video(transition_source(&assignment.source)?, None);
        physical.layer = Layer::Bottom;
        physical.mute = true;
        self.spawn_as(&physical, assignment, output, socket, generation, policy, None, true)
    }

    pub(crate) fn spawn_frozen(
        &self,
        logical: Assignment,
        snapshot: &Path,
        output: String,
        socket: &Path,
        generation: u64,
        policy: Option<&RendererPolicy>,
        freeze_directory: Arc<tempfile::TempDir>,
    ) -> Result<Worker> {
        let mut physical = logical.clone();
        physical.source = Source::static_file(snapshot.display().to_string());
        physical.transition = None;
        self.spawn_as(
            &physical,
            logical,
            output,
            socket,
            generation,
            policy,
            Some(freeze_directory),
            false,
        )
    }

    fn spawn_as(
        &self,
        assignment: &Assignment,
        logical: Assignment,
        output: String,
        socket: &Path,
        generation: u64,
        policy: Option<&RendererPolicy>,
        freeze_directory: Option<Arc<tempfile::TempDir>>,
        overlay: bool,
    ) -> Result<Worker> {
        let source = &assignment.source;
        let physical_dynamic = source.kind != SourceKind::Static;
        let retain_capable = source.kind == SourceKind::Video
            && source.effective_video_engine() == Some(VideoEngine::Tinier);
        let (mut command, freeze_capable) = match source.kind {
            SourceKind::Static => {
                let executable = self.still.require("static image")?;
                let mut command = Command::new(executable);
                command
                    .arg(&output)
                    .arg(&source.path)
                    .arg("--persist")
                    .arg("--fill-mode")
                    .arg(assignment.fill_mode.as_str())
                    .arg("--layer")
                    .arg(layer(assignment.layer));
                (command, false)
            }
            SourceKind::Video => {
                if source.effective_video_engine() == Some(VideoEngine::Tinier) {
                    let executable = self.tinier.require("tinier video")?;
                    let frame_rate = source
                        .frame_rate
                        .as_deref()
                        .ok_or_else(|| anyhow!("tinier video source has no frame rate"))?;
                    let mut command = Command::new(executable);
                    if output != "*" {
                        command.arg("--output").arg(&output);
                    }
                    command.arg(&source.path).arg(frame_rate).arg("bt709");
                    (command, true)
                } else {
                    let executable = self.vk.require("video")?;
                    let mut command = Command::new(executable);
                    command.arg(&output).arg(&source.path);
                    command.env_remove("SKWD_VK_PATH");
                    (command, true)
                }
            }
            SourceKind::WallpaperEngine => match crate::we_source::resolve(&source.path)? {
                crate::we_source::WeTarget::Scene(path) => {
                    let executable = self.vk.require("Wallpaper Engine scene")?;
                    let mut command = Command::new(executable);
                    command.arg(&output).arg(&path).arg("--scene").arg(path);
                    if let Some(properties) = scene_properties_arg(source) {
                        command.arg("--scene-properties").arg(properties);
                    }
                    command.env_remove("SKWD_VK_PATH");
                    (command, true)
                }
                crate::we_source::WeTarget::Video(path) => {
                    let executable = self.vk.require("Wallpaper Engine video")?;
                    let mut command = Command::new(executable);
                    command.arg(&output).arg(path);
                    command.env_remove("SKWD_VK_PATH");
                    (command, true)
                }
            },
        };
        if source.kind != SourceKind::Static
            && source.effective_video_engine() != Some(VideoEngine::Tinier)
        {
            command
                .arg(if overlay { "--transition-hold" } else { "--persist" })
                .arg("--fill-mode")
                .arg(assignment.fill_mode.as_str())
                .arg("--mute")
                .arg(assignment.mute.to_string())
                .arg("--volume")
                .arg(assignment.volume.to_string())
                .arg("--layer")
                .arg(layer(assignment.layer));
            if let Some(transition) = &assignment.transition
                && let Some(from) = &transition.from
            {
                command
                    .arg("--transition-from")
                    .arg(from)
                    .arg("--shader")
                    .arg(transition.effect())
                    .arg("--duration-ms")
                    .arg(transition.duration_ms().to_string());
            }
        }
        clear_policy_env(&mut command);
        if let Some(policy) = policy {
            apply_policy(&mut command, policy);
        }
        command
            .env("SKWD_PAPER_READY_SOCKET", socket)
            .env("SKWD_PAPER_GENERATION", generation.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().with_context(|| {
            format!(
                "start {:?} media capability worker {} for {output}",
                assignment.source.kind,
                command.as_std().get_program().to_string_lossy()
            )
        })?;
        let pid = child.id().ok_or_else(|| anyhow!("Paper worker PID unavailable"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Paper worker stdin unavailable"))?;
        Ok(Worker {
            assignment: logical,
            output,
            generation,
            child,
            pid,
            physical_dynamic,
            freeze_capable: freeze_capable
                && !overlay
                && policy.is_none_or(|policy| policy.surface.is_none()),
            retain_capable,
            stdin: Some(stdin),
            _freeze_directory: freeze_directory,
        })
    }
}

pub(crate) fn present_plasma(
    assignment: &Assignment,
    stream_size: &str,
    stream_fps: u32,
    stream_fd: i32,
    paused: bool,
) -> Result<()> {
    let backends = BackendPaths::discover();
    let gpu_stream = std::env::var("SKWD_PAPER_PLASMA_GPU_STREAM").as_deref() == Ok("1");
    let transition = plasma_transition_command(&backends, assignment, stream_size, stream_fps)
        .inspect_err(|error| tracing::warn!(%error, "Plasma transition prelude unavailable"))
        .ok()
        .flatten();
    let prefaced = if let Some(mut transition) = transition {
        if gpu_stream {
            paper_runtime::plasma::begin_stream(stream_fd, 1)?;
            transition.arg("--stream-fd").arg(stream_fd.to_string());
            transition.env("SKWD_PAPER_STREAM_EPOCH", "1");
        } else {
            write_stream_header(stream_size)?;
        }
        match transition.status() {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::warn!(%status, "Plasma transition prelude exited early"),
            Err(error) => tracing::warn!(%error, "Plasma transition prelude failed to start"),
        }
        true
    } else {
        false
    };
    let mut command = plasma_command(
        &backends,
        assignment,
        stream_size,
        stream_fps,
        stream_fd,
        paused,
        !prefaced || gpu_stream,
    )?;
    if gpu_stream {
        paper_runtime::plasma::begin_stream(stream_fd, 2)?;
        command.env("SKWD_PAPER_STREAM_EPOCH", "2");
    }
    command.env("SKWD_PAPER_PLASMA_FD", stream_fd.to_string());
    let executable = command.get_program().to_string_lossy().into_owned();
    let error = command.exec();
    Err(error).with_context(|| format!("start Plasma presenter {executable}"))
}

fn plasma_command(
    backends: &BackendPaths,
    assignment: &Assignment,
    stream_size: &str,
    stream_fps: u32,
    stream_fd: i32,
    paused: bool,
    write_header: bool,
) -> Result<StdCommand> {
    let source = &assignment.source;
    let (executable, uses_vk) = match source.kind {
        SourceKind::Static => {
            (backends.still.require_headless("Plasma static image presentation")?, false)
        }
        SourceKind::Video if source.effective_video_engine() == Some(VideoEngine::Tinier) => {
            (backends.tinier.require_headless("Plasma tinier video presentation")?, false)
        }
        SourceKind::Video => (backends.vk.require_headless("Plasma video presentation")?, true),
        SourceKind::WallpaperEngine => {
            (backends.vk.require_headless("Plasma Wallpaper Engine presentation")?, true)
        }
    };
    let mut command = StdCommand::new(executable);
    match source.kind {
        SourceKind::Static => {
            command
                .arg("*")
                .arg(&source.path)
                .arg("--frame-stream")
                .arg(stream_size)
                .arg("--fill-mode")
                .arg(assignment.fill_mode.as_str());
            if !write_header {
                command.arg("--stream-no-header");
            }
        }
        SourceKind::Video if source.effective_video_engine() == Some(VideoEngine::Tinier) => {
            let frame_rate = source
                .frame_rate
                .as_deref()
                .ok_or_else(|| anyhow!("tinier video source has no frame rate"))?;
            command
                .arg("--frame-stream")
                .arg(stream_size)
                .arg("--fill-mode")
                .arg(assignment.fill_mode.as_str());
            if paused {
                command.arg("--paused");
            }
            if !write_header {
                command.arg("--stream-no-header");
            }
            command.arg(&source.path).arg(frame_rate).arg("bt709");
        }
        SourceKind::Video => {
            command.arg("--video-stream").arg(&source.path);
        }
        SourceKind::WallpaperEngine => match crate::we_source::resolve(&source.path)? {
            crate::we_source::WeTarget::Scene(path) => {
                command.arg("--video-stream").arg(&path).arg("--scene");
                if let Some(properties) = scene_properties_arg(source) {
                    command.arg("--scene-properties").arg(properties);
                }
            }
            crate::we_source::WeTarget::Video(path) => {
                command.arg("--video-stream").arg(path);
            }
        },
    }
    if uses_vk {
        command
            .arg("--stream-size")
            .arg(stream_size)
            .arg("--stream-fps")
            .arg(stream_fps.clamp(1, 240).to_string())
            .arg("--fill-mode")
            .arg(assignment.fill_mode.as_str())
            .arg("--mute")
            .arg(assignment.mute.to_string())
            .arg("--volume")
            .arg(assignment.volume.min(100).to_string());
        if std::env::var("SKWD_PAPER_PLASMA_GPU_STREAM").as_deref() != Ok("0") {
            command.arg("--stream-fd").arg(stream_fd.to_string());
        } else if command.get_args().any(|arg| arg == "--scene") {
            anyhow::bail!("Plasma graphics backend cannot import GPU scene frames");
        }
        if paused {
            command.arg("--paused");
        }
        if !write_header {
            command.arg("--stream-no-header");
        }
    }
    Ok(command)
}

fn plasma_transition_command(
    backends: &BackendPaths,
    assignment: &Assignment,
    stream_size: &str,
    stream_fps: u32,
) -> Result<Option<StdCommand>> {
    let Some(transition) = assignment.transition.as_ref() else { return Ok(None) };
    let Some(from) = transition.from.as_deref() else { return Ok(None) };
    let from = transition_media(from)?;
    let to = transition_source(&assignment.source)?;
    if from == to {
        return Ok(None);
    }
    let executable = backends.vk.require_headless("Plasma transition presentation")?;
    let mut command = StdCommand::new(executable);
    command
        .arg("--preview-stream")
        .arg(to)
        .arg("--transition-from")
        .arg(from)
        .arg("--shader")
        .arg(transition.effect())
        .arg("--duration-ms")
        .arg(transition.duration_ms().to_string())
        .arg("--preview-size")
        .arg(stream_size)
        .arg("--preview-frame-ms")
        .arg((1000 / stream_fps.clamp(1, 240)).max(4).to_string())
        .arg("--fill-mode")
        .arg(assignment.fill_mode.as_str())
        .arg("--preview-once")
        .arg("--stream-no-header")
        .stdin(Stdio::null());
    Ok(Some(command))
}

fn transition_media(path: &str) -> Result<String> {
    if Path::new(path).is_dir() {
        return Ok(crate::we_source::transition_media(path)?.display().to_string());
    }
    Ok(path.to_string())
}

fn write_stream_header(size: &str) -> Result<()> {
    let (width, height) = size
        .split_once('x')
        .and_then(|(width, height)| Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?)))
        .ok_or_else(|| anyhow!("Plasma stream size must be WIDTHxHEIGHT"))?;
    let mut output = std::io::stdout().lock();
    output.write_all(b"SKWP")?;
    output.write_all(&width.to_le_bytes())?;
    output.write_all(&height.to_le_bytes())?;
    output.flush()?;
    Ok(())
}

fn scene_properties_arg(source: &Source) -> Option<String> {
    let properties = source.properties.as_ref().filter(|map| !map.is_empty())?;
    serde_json::to_string(properties).ok()
}

fn clear_policy_env(command: &mut Command) {
    for key in [
        "SKWD_PAPER_NAMESPACE",
        "SKWD_PAPER_BLUR",
        "SKWD_PAPER_DIM",
        "SKWD_PAPER_IDLE_SEC",
        "SKWD_PAPER_TRANSITIONS",
        "SKWD_PAPER_SAND_QUALITY",
        "SKWD_PAPER_SAND_SCOPE",
        "SKWD_PAPER_SAND_PRIMARY",
        "SKWD_PAPER_SAND_SHARP",
        "SKWD_PAPER_SAND_FPS",
        "SKWD_PAPER_WE_FPS",
        "SKWD_PAPER_WE_DISABLE_PARTICLES",
        "SKWD_WE_ASSETS",
        "SKWD_VK_SCENE_MAX",
        "SKWD_VK_SCENE_FX",
        "SKWD_VK_FX_PASSES",
        "SKWD_VK_SCENE_STRICT",
        "SKWD_PAPER_OUTPUT_FPS",
    ] {
        command.env_remove(key);
    }
}

pub(crate) fn transition_source(source: &Source) -> Result<String> {
    match source.kind {
        SourceKind::Static | SourceKind::Video => Ok(source.path.clone()),
        SourceKind::WallpaperEngine => {
            Ok(crate::we_source::transition_media(&source.path)?.display().to_string())
        }
    }
}

fn apply_policy(command: &mut Command, policy: &RendererPolicy) {
    if let Some(surface) = &policy.surface {
        command.env("SKWD_VK_INPUT", "passthrough");
        command.env_remove("SKWD_VK_LAYER");
        command.env("SKWD_PAPER_NAMESPACE", &surface.namespace);
        command.env("SKWD_PAPER_BLUR", surface.blur.to_string());
        command.env("SKWD_PAPER_DIM", surface.dim.to_string());
        command.arg("--namespace").arg(&surface.namespace);
        command.arg("--blur").arg(surface.blur.to_string());
        command.arg("--dim").arg(surface.dim.to_string());
    }
    if let Some(seconds) = policy.idle_seconds {
        command.env("SKWD_PAPER_IDLE_SEC", seconds.to_string());
    }
    if let Some(enabled) = policy.transitions_enabled {
        command.env("SKWD_PAPER_TRANSITIONS", if enabled { "1" } else { "0" });
    }
    if let Some(sand) = &policy.sand {
        if let Some(quality) = sand.quality {
            command.env("SKWD_PAPER_SAND_QUALITY", sand_quality(quality));
        }
        if let Some(scope) = sand.scope {
            command.env("SKWD_PAPER_SAND_SCOPE", sand_scope(scope));
        }
        if let Some(primary) = &sand.primary {
            command.env("SKWD_PAPER_SAND_PRIMARY", primary);
        }
        if let Some(sharp) = sand.sharp {
            command.env("SKWD_PAPER_SAND_SHARP", if sharp { "1" } else { "0" });
        }
        if let Some(fps) = sand.fps {
            command.env("SKWD_PAPER_SAND_FPS", fps.to_string());
        }
    }
    if let Some(scene) = &policy.scene {
        if let Some(fps) = scene.fps {
            command.env("SKWD_PAPER_WE_FPS", fps.to_string());
        }
        if let Some(disabled) = scene.disable_particles {
            command.env("SKWD_PAPER_WE_DISABLE_PARTICLES", if disabled { "1" } else { "0" });
        }
        if let Some(assets) = &scene.assets_dir {
            command.env("SKWD_WE_ASSETS", assets);
        }
        if let Some(dimension) = scene.max_dimension {
            command.env("SKWD_VK_SCENE_MAX", dimension.to_string());
        }
        if let Some(chains) = scene.max_effect_chains {
            command.env("SKWD_VK_SCENE_FX", chains.to_string());
        }
        if let Some(passes) = scene.max_effect_passes {
            command.env("SKWD_VK_FX_PASSES", passes.to_string());
        }
        if let Some(strict) = scene.strict {
            command.env("SKWD_VK_SCENE_STRICT", if strict { "1" } else { "0" });
        }
    }
    if !policy.output_fps.is_empty() {
        let output_fps = policy
            .output_fps
            .iter()
            .map(|(output, fps)| format!("{output}={fps}"))
            .collect::<Vec<_>>()
            .join(";");
        command.env("SKWD_PAPER_OUTPUT_FPS", output_fps);
    }
}

impl BackendExecutable {
    fn require(&self, media: &str) -> Result<&Path> {
        self.require_with(media, false)
    }

    fn require_headless(&self, media: &str) -> Result<&Path> {
        self.require_with(media, true)
    }

    fn require_with(&self, media: &str, headless: bool) -> Result<&Path> {
        if !self.capability.executable_file {
            return Err(RendererUnavailable {
                message: format!(
                    "{media} media capability requires {}: {}",
                    self.capability.executable,
                    self.capability
                        .diagnostic
                        .as_deref()
                        .unwrap_or("renderer executable is unavailable")
                ),
            }
            .into());
        }
        if let Some(dependency) = self
            .capability
            .dependencies
            .iter()
            .find(|item| !item.available && (!headless || item.name != "wayland_connection"))
        {
            return Err(RendererUnavailable {
                message: format!(
                    "{media} media capability requires {} runtime dependency {}: {}",
                    self.capability.executable, dependency.name, dependency.detail
                ),
            }
            .into());
        }
        self.path.as_deref().ok_or_else(|| {
            RendererUnavailable {
                message: format!(
                    "{media} media capability requires {}, but its resolved path is unavailable",
                    self.capability.executable
                ),
            }
            .into()
        })
    }
}

fn discover_backend(
    env_key: &str,
    name: &'static str,
    source_kinds: &[SourceKind],
    video_engines: &[VideoEngine],
    private_sibling: bool,
    dependencies: Vec<RuntimeDependencyStatus>,
) -> BackendExecutable {
    let configured = std::env::var_os(env_key).map(|path| (env_key.to_string(), path.into()));
    select_backend(
        name,
        source_kinds,
        video_engines,
        configured,
        discovery_candidates(name, private_sibling),
        dependencies,
    )
}

fn select_backend(
    name: &'static str,
    source_kinds: &[SourceKind],
    video_engines: &[VideoEngine],
    configured: Option<(String, PathBuf)>,
    candidates: Vec<(PathBuf, RendererDiscovery)>,
    dependencies: Vec<RuntimeDependencyStatus>,
) -> BackendExecutable {
    if let Some((key, path)) = configured {
        return inspect_backend(
            name,
            path,
            RendererDiscovery::Configured,
            source_kinds,
            video_engines,
            dependencies,
            Some(&key),
        );
    }
    let mut invalid = None;
    for (path, discovery) in candidates {
        let backend = inspect_backend(
            name,
            path,
            discovery,
            source_kinds,
            video_engines,
            dependencies.clone(),
            None,
        );
        if backend.capability.executable_file {
            return backend;
        }
        if backend.capability.present && invalid.is_none() {
            invalid = Some(backend);
        }
    }
    invalid.unwrap_or_else(|| {
        unresolved_backend(
            name,
            source_kinds,
            video_engines,
            format!("{name} was not found next to skwd-paper or on PATH"),
        )
        .with_dependencies(dependencies)
    })
}

fn discovery_candidates(name: &str, private_sibling: bool) -> Vec<(PathBuf, RendererDiscovery)> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push((parent.join(name), RendererDiscovery::Sibling));
        if private_sibling && let Some(prefix) = parent.parent() {
            candidates.push((
                prefix.join("lib/skwd-paper").join(name),
                RendererDiscovery::PrivateSibling,
            ));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .map(|directory| (directory.join(name), RendererDiscovery::Path)),
        );
    }
    candidates
}

#[cfg(test)]
fn injected_backend(
    name: &'static str,
    path: PathBuf,
    source_kinds: &[SourceKind],
    video_engines: &[VideoEngine],
) -> BackendExecutable {
    inspect_backend(
        name,
        path,
        RendererDiscovery::Injected,
        source_kinds,
        video_engines,
        Vec::new(),
        None,
    )
}

fn inspect_backend(
    name: &'static str,
    path: PathBuf,
    discovery: RendererDiscovery,
    source_kinds: &[SourceKind],
    video_engines: &[VideoEngine],
    dependencies: Vec<RuntimeDependencyStatus>,
    configured_by: Option<&str>,
) -> BackendExecutable {
    let metadata = path.metadata().ok();
    let present = metadata.is_some();
    let executable = metadata
        .as_ref()
        .is_some_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
    let diagnostic =
        (!executable).then(|| backend_diagnostic(name, &path, metadata.as_ref(), configured_by));
    let resolved = present.then(|| std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()));
    let selected = resolved.clone().unwrap_or(path);
    BackendExecutable {
        path: Some(selected.clone()),
        capability: RendererCapability {
            executable: name.into(),
            source_kinds: source_kinds.to_vec(),
            video_engines: video_engines.to_vec(),
            path: Some(selected.display().to_string()),
            discovery,
            present,
            executable_file: executable,
            dependencies,
            diagnostic,
        },
    }
}

fn backend_diagnostic(
    name: &str,
    path: &Path,
    metadata: Option<&std::fs::Metadata>,
    configured_by: Option<&str>,
) -> String {
    let source = configured_by.map_or_else(String::new, |key| format!(" configured by {key}"));
    match metadata {
        None => format!("{name}{source} does not exist at {}", path.display()),
        Some(metadata) if !metadata.is_file() => {
            format!("{name}{source} is not a regular file at {}", path.display())
        }
        Some(_) => format!("{name}{source} is not executable at {}", path.display()),
    }
}

fn unresolved_backend(
    name: &'static str,
    source_kinds: &[SourceKind],
    video_engines: &[VideoEngine],
    diagnostic: String,
) -> BackendExecutable {
    BackendExecutable {
        path: None,
        capability: RendererCapability {
            executable: name.into(),
            source_kinds: source_kinds.to_vec(),
            video_engines: video_engines.to_vec(),
            path: None,
            discovery: RendererDiscovery::Unresolved,
            present: false,
            executable_file: false,
            dependencies: Vec::new(),
            diagnostic: Some(diagnostic),
        },
    }
}

impl BackendExecutable {
    fn with_dependencies(mut self, dependencies: Vec<RuntimeDependencyStatus>) -> Self {
        self.capability.dependencies = dependencies;
        self
    }
}

fn wayland_dependencies() -> Vec<RuntimeDependencyStatus> {
    vec![wayland_dependency(
        std::env::var_os("WAYLAND_SOCKET"),
        std::env::var_os("WAYLAND_DISPLAY"),
        std::env::var_os("XDG_RUNTIME_DIR"),
    )]
}

fn wayland_dependency(
    socket: Option<OsString>,
    display: Option<OsString>,
    runtime: Option<OsString>,
) -> RuntimeDependencyStatus {
    if let Some(socket) = socket {
        let value = socket.to_string_lossy();
        let available = value.parse::<i32>().is_ok_and(connected_wayland_fd);
        return RuntimeDependencyStatus {
            name: "wayland_connection".into(),
            available,
            detail: if available {
                format!("WAYLAND_SOCKET={value} is a connected Unix socket")
            } else {
                format!("WAYLAND_SOCKET={value} is not a connected Unix socket descriptor")
            },
        };
    }
    let Some(display) = display.filter(|value| !value.is_empty()) else {
        return RuntimeDependencyStatus {
            name: "wayland_connection".into(),
            available: false,
            detail: "WAYLAND_DISPLAY and WAYLAND_SOCKET are unset".into(),
        };
    };
    let display_path = PathBuf::from(&display);
    let resolved = if display_path.is_absolute() {
        Some(display_path)
    } else {
        runtime
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|dir| dir.join(display_path))
    };
    let available = resolved
        .as_deref()
        .is_some_and(|path| std::os::unix::net::UnixStream::connect(path).is_ok());
    RuntimeDependencyStatus {
        name: "wayland_connection".into(),
        available,
        detail: match resolved {
            Some(path) if available => {
                format!("Wayland compositor is reachable at {}", path.display())
            }
            Some(path) => format!("Wayland compositor is not reachable at {}", path.display()),
            None => format!(
                "WAYLAND_DISPLAY={} is relative but XDG_RUNTIME_DIR is unset",
                display.to_string_lossy()
            ),
        },
    }
}

fn connected_wayland_fd(fd: i32) -> bool {
    if fd < 0 || unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1 {
        return false;
    }
    let mut address: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let connected = unsafe {
        libc::getpeername(fd, std::ptr::addr_of_mut!(address).cast::<libc::sockaddr>(), &mut length)
    };
    connected == 0 && i32::from(address.ss_family) == libc::AF_UNIX
}

fn vk_dependencies(mut dependencies: Vec<RuntimeDependencyStatus>) -> Vec<RuntimeDependencyStatus> {
    let handle = unsafe {
        libc::dlopen(c"libvulkan.so.1".as_ptr().cast(), libc::RTLD_LAZY | libc::RTLD_LOCAL)
    };
    let available = !handle.is_null();
    if available {
        unsafe {
            libc::dlclose(handle);
        }
    }
    dependencies.push(RuntimeDependencyStatus {
        name: "vulkan_loader".into(),
        available,
        detail: if available {
            "libvulkan.so.1 is loadable".into()
        } else {
            "libvulkan.so.1 is not loadable".into()
        },
    });
    dependencies
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

const fn layer(layer: Layer) -> &'static str {
    match layer {
        Layer::Background => "background",
        Layer::Bottom => "bottom",
        Layer::Top => "top",
        Layer::Overlay => "overlay",
    }
}

const fn sand_quality(quality: SandQuality) -> &'static str {
    match quality {
        SandQuality::Auto => "auto",
        SandQuality::Full => "full",
        SandQuality::Low => "low",
    }
}

const fn sand_scope(scope: SandScope) -> &'static str {
    match scope {
        SandScope::All => "all",
        SandScope::Primary => "primary",
    }
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
