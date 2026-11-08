use anyhow::{Context, Result, anyhow};
use paper_control::{
    Assignment, Layer, PaperCommand, RendererCapability, RendererDiscovery, RendererPolicy,
    RuntimeDependencyStatus, SandQuality, SandScope, Source, SourceKind, StillCommand,
};
use std::collections::BTreeMap;
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

const SPECTRUM_MODE_ENV: &str = "SKWD_VK_AUDIO_MODE";
const SYSTEM_SPECTRUM: &str = "system";
const WALLPAPER_SPECTRUM: &str = "wallpaper";

fn spectrum_mode(assignment: &Assignment) -> &'static str {
    if assignment.system_audio.unwrap_or(true) { SYSTEM_SPECTRUM } else { WALLPAPER_SPECTRUM }
}

const EXIT_GRACE: Duration = Duration::from_millis(500);

pub(crate) fn capture_scenes() -> Result<()> {
    let backends = BackendPaths::discover();
    let executable = backends.vk.require_headless("Wallpaper Engine thumbnail capture")?;
    let error = StdCommand::new(executable).arg("--scene-thumbnail-worker").exec();
    Err(error).context("start scene thumbnail worker")
}

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

pub(crate) struct OverlaySpec {
    pub(crate) assignment: Assignment,
    pub(crate) snapshot: Option<PathBuf>,
}

pub(crate) struct Worker {
    pub(crate) members: Vec<Assignment>,
    pub(crate) assignment: Assignment,
    pub(crate) output: String,
    pub(crate) generation: u64,
    pub(crate) child: Child,
    pid: u32,
    physical_dynamic: bool,
    pub(crate) drawing_hold: Option<Arc<tempfile::TempDir>>,
    pub(crate) pending_photos: Vec<Arc<tempfile::TempDir>>,
    freeze_capable: bool,
    surface_policy: bool,
    stdin: Option<ChildStdin>,
    freeze_directories: Vec<Arc<tempfile::TempDir>>,
}

impl Worker {
    pub(crate) fn assignments(&self) -> &[Assignment] {
        if self.members.is_empty() { std::slice::from_ref(&self.assignment) } else { &self.members }
    }

    pub(crate) fn control_file(&self) -> Result<std::fs::File> {
        use std::os::fd::AsFd;
        let stdin = self.stdin.as_ref().ok_or_else(|| anyhow!("Paper worker stdin unavailable"))?;
        Ok(std::fs::File::from(stdin.as_fd().try_clone_to_owned()?))
    }

    pub(crate) fn retain_snapshot(&mut self, directory: Arc<tempfile::TempDir>) {
        self.freeze_directories.push(directory);
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn exited(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().context("query Paper worker")
    }

    pub(crate) fn dynamic(&self) -> bool {
        self.physical_dynamic
    }

    pub(crate) async fn snapshot(&mut self, path: &Path, output: &str, hold: bool) -> Result<()> {
        let path = path.display().to_string();
        let mut command = PaperCommand::snapshot(&path, output);
        if hold && self.dynamic() {
            command.pause = Some(true);
        }
        self.send(&command).await
    }

    pub(crate) fn drawing_overlay(&self) -> bool {
        self.drawing_hold.is_none()
            && self
                .assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.drawing.is_some())
    }

    pub(crate) fn freeze_capable(&self) -> bool {
        self.freeze_capable
    }

    pub(crate) fn pause_freeze_capable(&self) -> bool {
        self.freeze_capable && !self.surface_policy && self.members.len() < 2
    }

    pub(crate) async fn send_audio(
        &mut self,
        request: &paper_control::AudioSetRequest,
    ) -> Result<()> {
        if request.duck == Some(true) {
            self.send(&PaperCommand::duck(true)).await?;
        }
        if request.mute.is_some() || request.volume.is_some() || request.system_audio.is_some() {
            let mut command = PaperCommand::audio(request.mute, request.volume);
            command.system_audio = request.system_audio;
            self.send(&command).await?;
        }
        if request.duck == Some(false) {
            self.send(&PaperCommand::duck(false)).await?;
        }
        Ok(())
    }

    pub(crate) async fn send(&mut self, command: &PaperCommand) -> Result<()> {
        self.send_line(&command.line()).await
    }

    pub(crate) async fn mirror(&mut self, target: &paper_control::MirrorTarget) -> Result<()> {
        if self.dynamic() {
            self.send(&PaperCommand::mirror(&target.output, &target.socket)).await
        } else {
            self.send_line(&StillCommand::mirror(&target.output, &target.socket).line()).await
        }
    }

    pub(crate) async fn set_surface(
        &mut self,
        surface: &paper_control::SurfacePolicy,
    ) -> Result<()> {
        if self.dynamic() {
            let mut command = PaperCommand::audio(None, None);
            command.surface = Some(Box::new(surface.clone()));
            self.send(&command).await
        } else {
            let mut command = StillCommand::new("");
            command.surface = Some(surface.clone());
            self.send_line(&command.line()).await
        }
    }

    pub(crate) async fn set_widgets(&mut self, scene: &paper_control::WidgetScene) -> Result<()> {
        if self.dynamic() {
            let mut command = PaperCommand::audio(None, None);
            command.widgets = Some(Box::new(scene.clone()));
            return self.send(&command).await;
        }
        let mut command = StillCommand::new("");
        command.widgets = Some(scene.clone());
        self.send_line(&command.line()).await
    }

    pub(crate) async fn reveal_still(&mut self) -> Result<bool> {
        if !static_transition(&self.assignment) {
            return Ok(false);
        }
        if self.dynamic() {
            let mut command = PaperCommand::audio(None, None);
            command.reveal = Some(true);
            self.send(&command).await?;
        } else {
            self.send_line(&StillCommand::reveal().line()).await?;
        }
        Ok(true)
    }

    async fn send_line(&mut self, line: &str) -> Result<()> {
        if self.exited()?.is_some() {
            return Err(anyhow!("Paper worker exited"));
        }
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("Paper worker stdin unavailable"))?;
        stdin.write_all(line.as_bytes()).await.context("write Paper worker command")?;
        stdin.flush().await.context("flush Paper worker command")
    }

    pub(crate) async fn stop(self) {
        let pid = self.pid;
        self.halt().await;
        paper_runtime::scratch::release(pid);
    }

    async fn halt(mut self) {
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

    pub(crate) fn stop_blocking(self) {
        let pid = self.pid;
        self.halt_blocking();
        paper_runtime::scratch::release(pid);
    }

    fn halt_blocking(mut self) {
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
    transition_sources: BTreeMap<String, String>,
    pub(crate) pause: crate::manager::playback::Control,
    pub(crate) duck: crate::manager::playback::Control,
    pub(crate) surface_overrides: paper_control::SurfaceOverridesRequest,
    vk: BackendExecutable,
    still: BackendExecutable,
}

impl BackendPaths {
    pub(crate) fn discover() -> Self {
        let wayland = wayland_dependencies();
        Self {
            transition_sources: BTreeMap::new(),
            pause: crate::manager::playback::Control::default(),
            duck: crate::manager::playback::Control::default(),
            surface_overrides: paper_control::SurfaceOverridesRequest::default(),
            vk: discover_backend(
                "SKWD_PAPER_VK_BIN",
                "skwd-wall-vk",
                &[SourceKind::Video, SourceKind::WallpaperEngine],
                vk_dependencies(wayland.clone()),
            ),
            still: discover_backend(
                "SKWD_PAPER_STILL_BIN",
                "skwd-wall-still",
                &[SourceKind::Static],
                wayland,
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_executables(vk: PathBuf, still: PathBuf) -> Self {
        Self {
            transition_sources: BTreeMap::new(),
            pause: crate::manager::playback::Control::default(),
            duck: crate::manager::playback::Control::default(),
            surface_overrides: paper_control::SurfaceOverridesRequest::default(),
            vk: injected_backend(
                "skwd-wall-vk",
                vk,
                &[SourceKind::Video, SourceKind::WallpaperEngine],
            ),
            still: injected_backend("skwd-wall-still", still, &[SourceKind::Static]),
        }
    }

    pub(crate) fn capabilities(&self) -> Vec<RendererCapability> {
        [&self.vk, &self.still].into_iter().map(|backend| backend.capability.clone()).collect()
    }

    pub(crate) fn for_members(&self, members: &[Assignment]) -> Self {
        let mut backends = self.clone();
        backends.transition_sources = members
            .iter()
            .filter(|member| !static_transition(member))
            .flat_map(|member| {
                member
                    .transition
                    .as_ref()
                    .and_then(|transition| transition.from.as_ref())
                    .into_iter()
                    .flat_map(|from| {
                        member.outputs.iter().map(move |output| (output.clone(), from.clone()))
                    })
            })
            .collect();
        backends
    }

    pub(crate) fn spawn(
        &self,
        assignment: Assignment,
        output: String,
        socket: &Path,
        generation: u64,
        policy: Option<&RendererPolicy>,
    ) -> Result<Worker> {
        let mut physical = assignment.clone();
        if assignment.source.kind != SourceKind::Static && static_transition(&assignment) {
            physical.transition = None;
            physical.mute = true;
        }
        self.spawn_as(&physical, assignment, output, socket, generation, policy, None, false)
    }

    pub(crate) fn spawn_overlay(
        &self,
        spec: OverlaySpec,
        output: String,
        socket: &Path,
        generation: u64,
        policy: Option<&RendererPolicy>,
    ) -> Result<Worker> {
        let assignment = spec.assignment;
        let mut physical = assignment.clone();
        let source = spec.snapshot.map_or_else(
            || transition_source(&assignment.source),
            |path| Ok(path.display().to_string()),
        )?;
        physical.source = Source::video(source);
        physical.mute = true;
        let drawing_policy =
            assignment.transition.as_ref().filter(|transition| transition.drawing.is_some()).map(
                |_| RendererPolicy {
                    transitions_enabled: Some(true),
                    ..policy.cloned().unwrap_or_default()
                },
            );
        self.spawn_as(
            &physical,
            assignment,
            output,
            socket,
            generation,
            drawing_policy.as_ref().or(policy),
            None,
            true,
        )
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
                let executable = self.vk.require("video")?;
                let mut command = Command::new(executable);
                command.arg(&output).arg(&source.path);
                command.env_remove("SKWD_VK_PATH");
                (command, true)
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
        if source.kind != SourceKind::Static {
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
        let playback = paper_control::PlaybackState {
            paused: self.pause.outputs(&logical.outputs),
            ducked: self.duck.outputs(&logical.outputs),
        };
        command.env(paper_control::PLAYBACK_ENV, serde_json::to_string(&playback)?);
        command.env_remove(paper_control::DRAWING_SOURCE_ENV);
        if let Some(transition) =
            assignment.transition.as_ref().filter(|transition| transition.drawing.is_some())
        {
            command.env(paper_control::DRAWING_SOURCE_ENV, drawing_source(transition, &logical));
        }
        command.env_remove(paper_control::DRAWING_ENV);
        if let Some(drawing) =
            assignment.transition.as_ref().and_then(|transition| transition.drawing.as_ref())
        {
            command.env(paper_control::DRAWING_ENV, serde_json::to_string(drawing)?);
        }
        command.env("SKWD_PAPER_BACKGROUND", serde_json::to_string(&assignment.background)?);
        command.env_remove(paper_control::TRANSITION_SOURCES_ENV);
        if self.transition_sources.len() > 1 {
            command.env(
                paper_control::TRANSITION_SOURCES_ENV,
                serde_json::to_string(&self.transition_sources)?,
            );
        }
        command.env_remove(paper_control::PREPARE_HIDDEN_ENV);
        if !overlay && static_transition(&logical) {
            command.env(paper_control::PREPARE_HIDDEN_ENV, "1");
        }
        command.env(SPECTRUM_MODE_ENV, spectrum_mode(assignment));
        clear_policy_env(&mut command);
        let mut launch_policy = policy.cloned();
        if let Some(surface) = self.surface_overrides.surfaces.get(&logical.source.kind) {
            launch_policy.get_or_insert_with(RendererPolicy::default).surface =
                Some(Box::new(surface.clone()));
        }
        if overlay
            && logical.transition.as_ref().is_some_and(|transition| transition.drawing.is_some())
            && let Some(surface) = launch_policy.as_mut().and_then(|policy| policy.surface.as_mut())
        {
            surface.blur = 0;
            surface.dim = 0;
        }
        if let Some(policy) = &launch_policy {
            apply_policy(&mut command, policy);
        }
        if let Some(fps) = assignment.transition.as_ref().and_then(|transition| transition.fps) {
            command.env("SKWD_PAPER_TRANSITION_FPS", fps.to_string());
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
            members: Vec::new(),
            drawing_hold: None,
            pending_photos: Vec::new(),
            assignment: logical,
            output,
            generation,
            child,
            pid,
            physical_dynamic,
            freeze_capable: freeze_capable && !overlay,
            surface_policy: policy.is_some_and(|policy| policy.surface.is_some()),
            stdin: Some(stdin),
            freeze_directories: freeze_directory.into_iter().collect(),
        })
    }
}

const STREAM_CONNECTOR: &str = "connector";
const STREAM_LOGICAL: &str = "logical";
const STILL_STREAM_OUTPUT: &str = "--stream-output";
const ALL_STREAM_OUTPUTS: &str = "*";
const EMPTY_CONNECTOR: &str = "empty stream connector";
const EMPTY_LOGICAL_SIZE: &str = "empty stream logical size";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlasmaStream {
    pub(crate) fd: i32,
    pub(crate) frame_fd: i32,
    pub(crate) size: String,
    pub(crate) fps: u32,
    pub(crate) output: String,
    pub(crate) connector: Option<String>,
    pub(crate) logical: Option<String>,
    pub(crate) paused: bool,
}

pub(crate) fn parse_plasma_stream(spec: &str, paused: bool) -> Result<PlasmaStream> {
    let mut stream = PlasmaStream {
        fd: -1,
        frame_fd: -1,
        size: String::new(),
        fps: 30,
        output: String::new(),
        connector: None,
        logical: None,
        paused,
    };
    for item in spec.split(',').filter(|item| !item.is_empty()) {
        let (key, value) =
            item.split_once('=').ok_or_else(|| anyhow!("stream field {item} needs key=value"))?;
        match key {
            "fd" => stream.fd = value.parse().with_context(|| format!("stream fd {value}"))?,
            "frame_fd" => {
                stream.frame_fd =
                    value.parse().with_context(|| format!("stream frame fd {value}"))?;
            }
            "size" => stream.size = value.to_string(),
            "fps" => stream.fps = value.parse().with_context(|| format!("stream fps {value}"))?,
            "output" => stream.output = value.to_string(),
            STREAM_CONNECTOR => {
                anyhow::ensure!(!value.is_empty(), EMPTY_CONNECTOR);
                stream.connector = Some(value.to_owned());
            }
            STREAM_LOGICAL => {
                let (width, height) = parse_stream_size(value)?;
                anyhow::ensure!(width > 0 && height > 0, EMPTY_LOGICAL_SIZE);
                stream.logical = Some(value.to_owned());
            }
            "paused" => stream.paused = paused || matches!(value, "1" | "true"),
            other => anyhow::bail!("unknown stream field {other}"),
        }
    }
    anyhow::ensure!(stream.fd >= 0, "stream {spec} needs fd=N");
    anyhow::ensure!(stream.size.contains('x'), "stream {spec} needs size=WxH");
    Ok(stream)
}

pub(crate) fn plasma_streams(args: &crate::cli::PresentPlasmaArgs) -> Result<Vec<PlasmaStream>> {
    let mut streams = Vec::new();
    if let (Some(size), Some(fps)) = (&args.stream_size, args.stream_fps) {
        streams.push(PlasmaStream {
            fd: args.stream_fd.unwrap_or(-1),
            frame_fd: -1,
            size: size.clone(),
            fps,
            output: String::new(),
            connector: args.stream_output.clone(),
            logical: args.stream_logical.clone(),
            paused: args.paused,
        });
    }
    for spec in &args.streams {
        streams.push(parse_plasma_stream(spec, args.paused)?);
    }
    anyhow::ensure!(
        !streams.is_empty(),
        "present-plasma needs --stream or --stream-size/--stream-fps/--stream-fd"
    );
    anyhow::ensure!(
        streams.len() == 1 || streams.iter().all(|stream| stream.fd >= 0),
        "shared Plasma streams each need a frame socket"
    );
    Ok(streams)
}

fn stream_fd_list(streams: &[PlasmaStream]) -> String {
    streams
        .iter()
        .filter(|stream| stream.fd >= 0)
        .map(|stream| stream.fd.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlasmaRoute {
    pub(crate) prelude_gpu: bool,
    pub(crate) presenter_gpu: bool,
}

impl PlasmaRoute {
    pub(crate) fn new(still: bool, gpu_import: bool) -> Self {
        Self { prelude_gpu: gpu_import, presenter_gpu: gpu_import && !still }
    }

    pub(crate) fn presenter_writes_header(self, prefaced: bool) -> bool {
        !prefaced || self.prelude_gpu
    }

    pub(crate) fn presenter_rebases_stream(self, prefaced: bool) -> bool {
        self.presenter_gpu || (self.prelude_gpu && prefaced)
    }
}

pub(crate) fn present_plasma(assignment: &Assignment, streams: &[PlasmaStream]) -> Result<()> {
    let backends = BackendPaths::discover();
    let still = assignment.source.kind == SourceKind::Static;
    let route = PlasmaRoute::new(
        still,
        std::env::var("SKWD_PAPER_PLASMA_GPU_STREAM").as_deref() == Ok("1"),
    );
    let gpu_stream = route.presenter_gpu;
    if streams.len() > 1 {
        if still {
            anyhow::ensure!(
                streams.iter().all(|stream| stream.frame_fd >= 0),
                "shared still streams each need a frame pipe"
            );
        } else {
            anyhow::ensure!(gpu_stream, "a shared Plasma presenter needs the GPU frame stream");
        }
    }
    let mut preludes = Vec::new();
    let mut prefaced = false;
    let drawing =
        assignment.transition.as_ref().is_some_and(|transition| transition.drawing.is_some());
    for stream in streams {
        if route.prelude_gpu && stream.paused {
            continue;
        }
        let hidden = drawing && stream.paused;
        let transition = plasma_transition_command(&backends, assignment, &stream.size, stream.fps)
            .inspect_err(|error| tracing::warn!(%error, "Plasma transition prelude unavailable"))
            .ok()
            .flatten();
        let Some(mut transition) = transition else { continue };
        paper_runtime::die_with_parent(&mut transition);
        if route.prelude_gpu {
            paper_runtime::plasma::begin_stream(stream.fd, 1)?;
            transition.arg("--stream-fd").arg(stream.fd.to_string());
            transition.env("SKWD_PAPER_STREAM_EPOCH", "1");
        } else if stream.frame_fd >= 0 {
            write_stream_header_to(
                &mut paper_runtime::plasma::frame_pipe(stream.frame_fd)?,
                &stream.size,
            )?;
            transition.stdout(Stdio::from(paper_runtime::plasma::frame_pipe(stream.frame_fd)?));
        } else {
            write_stream_header(&stream.size)?;
        }
        if hidden {
            prefaced = true;
            continue;
        }
        match transition.spawn() {
            Ok(child) => {
                prefaced = true;
                preludes.push(child);
            }
            Err(error) => tracing::warn!(%error, "Plasma transition prelude failed to start"),
        }
    }
    for mut child in preludes {
        match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::warn!(%status, "Plasma transition prelude exited early"),
            Err(error) => tracing::warn!(%error, "Plasma transition prelude did not finish"),
        }
    }
    let mut command =
        plasma_command(&backends, assignment, streams, route.presenter_writes_header(prefaced))?;
    if route.presenter_rebases_stream(prefaced) {
        for stream in streams {
            paper_runtime::plasma::begin_stream(stream.fd, 2)?;
        }
        command.env("SKWD_PAPER_STREAM_EPOCH", "2");
    }
    if still {
        command.env("SKWD_PAPER_PLASMA_GPU_STREAM", "0");
    }
    command.env("SKWD_PAPER_PLASMA_FD", stream_fd_list(streams));
    let executable = command.get_program().to_string_lossy().into_owned();
    let error = command.exec();
    Err(error).with_context(|| format!("start Plasma presenter {executable}"))
}

fn plasma_command(
    backends: &BackendPaths,
    assignment: &Assignment,
    streams: &[PlasmaStream],
    write_header: bool,
) -> Result<StdCommand> {
    let source = &assignment.source;
    if streams.is_empty() {
        return Err(anyhow!("Plasma presenter needs a stream"));
    }
    let all_paused = streams.iter().all(|stream| stream.paused);
    let (executable, uses_vk) = match source.kind {
        SourceKind::Static => {
            (backends.still.require_headless("Plasma static image presentation")?, false)
        }
        SourceKind::Video => (backends.vk.require_headless("Plasma video presentation")?, true),
        SourceKind::WallpaperEngine => {
            (backends.vk.require_headless("Plasma Wallpaper Engine presentation")?, true)
        }
    };
    let mut command = StdCommand::new(executable);
    command.env("SKWD_PAPER_BACKGROUND", serde_json::to_string(&assignment.background)?);
    match source.kind {
        SourceKind::Static => {
            command.arg("*").arg(&source.path);
            for stream in streams {
                command.arg("--frame-stream").arg(&stream.size);
                let connector = stream.connector.as_deref().unwrap_or(&stream.output);
                command.arg(STILL_STREAM_OUTPUT).arg(if connector.is_empty() {
                    ALL_STREAM_OUTPUTS
                } else {
                    connector
                });
                command
                    .arg(paper_control::stream::LOGICAL)
                    .arg(stream.logical.as_ref().unwrap_or(&stream.size));
                if stream.frame_fd >= 0 {
                    command.arg("--frame-fd").arg(stream.frame_fd.to_string());
                }
            }
            command.arg("--fill-mode").arg(assignment.fill_mode.as_str());
            if !write_header {
                command.arg("--stream-no-header");
            }
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
        let gpu_stream = std::env::var("SKWD_PAPER_PLASMA_GPU_STREAM").as_deref() != Ok("0");
        if !gpu_stream && command.get_args().any(|arg| arg == "--scene") {
            anyhow::bail!("Plasma graphics backend cannot import GPU scene frames");
        }
        for stream in streams {
            command
                .arg("--stream-size")
                .arg(&stream.size)
                .arg("--stream-fps")
                .arg(stream.fps.clamp(1, 240).to_string())
                .arg(paper_control::stream::CONNECTOR)
                .arg(stream.connector.as_ref().unwrap_or(&stream.output))
                .arg(paper_control::stream::LOGICAL)
                .arg(stream.logical.as_ref().unwrap_or(&stream.size));
            if gpu_stream {
                command.arg("--stream-fd").arg(stream.fd.to_string());
            }
            if !stream.output.is_empty() {
                command.arg("--stream-output").arg(&stream.output);
                if stream.paused && !all_paused {
                    command.arg("--stream-paused").arg(&stream.output);
                }
            }
        }
        command
            .arg("--fill-mode")
            .arg(assignment.fill_mode.as_str())
            .arg("--mute")
            .arg(assignment.mute.to_string())
            .arg("--volume")
            .arg(assignment.volume.min(100).to_string());
        if all_paused {
            command.arg("--paused");
        }
        if !write_header {
            command.arg("--stream-no-header");
        }
    }
    command.env(SPECTRUM_MODE_ENV, spectrum_mode(assignment));
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
    let to = match &transition.drawing {
        Some(_) => drawing_source(transition, assignment).display().to_string(),
        None => transition_source(&assignment.source)?,
    };
    if from == to && transition.drawing.is_none() {
        return Ok(None);
    }
    let executable = backends.vk.require_headless("Plasma transition presentation")?;
    let mut command = StdCommand::new(executable);
    command.env("SKWD_PAPER_BACKGROUND", serde_json::to_string(&assignment.background)?);
    command.env_remove(paper_control::DRAWING_ENV).env_remove(paper_control::DRAWING_SOURCE_ENV);
    if let Some(drawing) = &transition.drawing {
        command
            .env(paper_control::DRAWING_ENV, serde_json::to_string(drawing)?)
            .env(paper_control::DRAWING_SOURCE_ENV, &to);
    }
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
        .arg("--preview-fps")
        .arg(transition.fps.unwrap_or(stream_fps).clamp(1, 1000).to_string())
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
    write_stream_header_to(&mut std::io::stdout().lock(), size)
}

fn parse_stream_size(size: &str) -> Result<(u32, u32)> {
    let (width, height) = size
        .split_once('x')
        .and_then(|(width, height)| Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?)))
        .ok_or_else(|| anyhow!("Plasma stream size must be WIDTHxHEIGHT"))?;
    Ok((width, height))
}

fn write_stream_header_to(output: &mut dyn Write, size: &str) -> Result<()> {
    let (width, height) = parse_stream_size(size)?;
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
        "SKWD_PAPER_DYNAMIC_EFFECTS",
        "SKWD_PAPER_BLUR",
        "SKWD_PAPER_DIM",
        "SKWD_PAPER_IDLE_SEC",
        "SKWD_PAPER_TRANSITIONS",
        "SKWD_PAPER_LOAD_TIMEOUT_MS",
        "SKWD_PAPER_SAND_QUALITY",
        "SKWD_PAPER_SAND_SCOPE",
        "SKWD_PAPER_SAND_PRIMARY",
        "SKWD_PAPER_SAND_SHARP",
        "SKWD_PAPER_SAND_FPS",
        "SKWD_PAPER_TRANSITION_FPS",
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
    if let Some(timeout) = policy.load_timeout_ms {
        command.env("SKWD_PAPER_LOAD_TIMEOUT_MS", timeout.to_string());
    }
    if let Some(surface) = &policy.surface {
        command.env("SKWD_VK_INPUT", "passthrough");
        command.env_remove("SKWD_VK_LAYER");
        command.env("SKWD_PAPER_NAMESPACE", &surface.namespace);
        command.env("SKWD_PAPER_DYNAMIC_EFFECTS", "1");
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
    if let Some(fps) = policy.transition_fps {
        command.env("SKWD_PAPER_TRANSITION_FPS", fps.to_string());
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
        if let Some(clamp) = scene.clamp {
            let mode = match clamp {
                paper_control::SceneClamp::Clamp => "clamp",
                paper_control::SceneClamp::Border => "border",
                paper_control::SceneClamp::Repeat => "repeat",
            };
            command.env("SKWD_VK_SCENE_CLAMP", mode);
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
    dependencies: Vec<RuntimeDependencyStatus>,
) -> BackendExecutable {
    let configured = std::env::var_os(env_key).map(|path| (env_key.to_string(), path.into()));
    select_backend(name, source_kinds, configured, discovery_candidates(name), dependencies)
}

fn select_backend(
    name: &'static str,
    source_kinds: &[SourceKind],
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
            dependencies,
            Some(&key),
        );
    }
    let mut invalid = None;
    for (path, discovery) in candidates {
        let backend =
            inspect_backend(name, path, discovery, source_kinds, dependencies.clone(), None);
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
            format!("{name} was not found next to skwd-paper or on PATH"),
        )
        .with_dependencies(dependencies)
    })
}

fn discovery_candidates(name: &str) -> Vec<(PathBuf, RendererDiscovery)> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push((parent.join(name), RendererDiscovery::Sibling));
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
) -> BackendExecutable {
    inspect_backend(name, path, RendererDiscovery::Injected, source_kinds, Vec::new(), None)
}

fn inspect_backend(
    name: &'static str,
    path: PathBuf,
    discovery: RendererDiscovery,
    source_kinds: &[SourceKind],
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
    diagnostic: String,
) -> BackendExecutable {
    BackendExecutable {
        path: None,
        capability: RendererCapability {
            executable: name.into(),
            source_kinds: source_kinds.to_vec(),
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

pub(crate) fn drawing_source<'a>(
    transition: &'a paper_control::TransitionPolicy,
    logical: &'a Assignment,
) -> &'a Path {
    transition.drawing_source.as_deref().unwrap_or(Path::new(&logical.source.path))
}

pub(crate) fn static_transition(assignment: &Assignment) -> bool {
    (assignment.source.kind == SourceKind::Static
        || assignment.transition.as_ref().is_some_and(|transition| transition.drawing.is_some()))
        && assignment.transition.as_ref().is_some_and(|transition| {
            transition
                .from
                .as_ref()
                .is_some_and(|from| from != &assignment.source.path || transition.drawing.is_some())
        })
}
