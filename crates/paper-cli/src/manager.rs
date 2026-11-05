#[path = "handoff.rs"]
mod handoff;
#[path = "playback.rs"]
pub(crate) mod playback;
#[path = "surface.rs"]
mod surface;

use crate::backend::{
    BackendPaths, OverlaySpec, Worker, drawing_source, static_transition, transition_source,
};
use anyhow::{Context, Result, anyhow};
use paper_control::ALL_OUTPUT;
use paper_control::{
    ApplyRequest, Assignment, AudioSetRequest, FillMode, PaperCommand, RendererFailed,
    RendererPolicy, RendererReady, SandScope, StopRequest,
};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DRAWING_DIRECTORY: &str = "drawing-";
const DRAWING_SNAPSHOT: &str = "target.ppm";
const OUTPUT_SEPARATOR: &str = ",";

async fn any_worker_exit(workers: &mut [Worker]) {
    if workers.is_empty() {
        std::future::pending::<()>().await;
    }
    let mut exits: Vec<_> =
        workers.iter_mut().map(|worker| Box::pin(worker.child.wait())).collect();
    std::future::poll_fn(|context| {
        for exit in &mut exits {
            if exit.as_mut().poll(context).is_ready() {
                return std::task::Poll::Ready(());
            }
        }
        std::task::Poll::Pending
    })
    .await;
}

pub(crate) struct WorkerStatus {
    pub(crate) assignment: Assignment,
    pub(crate) pid: u32,
    pub(crate) generation: u64,
}

pub(crate) struct ApplyTransaction {
    candidates: Vec<Worker>,
    overlays: Vec<Worker>,
    next: Option<ApplyStage>,
    reveal_steady: bool,
    ready: BTreeSet<u32>,
    touched: BTreeSet<String>,
    replace_all: bool,
    generation: u64,
    policy: Option<RendererPolicy>,
    surface_overrides: paper_control::SurfaceOverridesRequest,
    reported_failure: Option<String>,
    handoff: Option<handoff::Handoff>,
}

struct ApplyStage {
    freeze_parent: Option<PathBuf>,
    assignments: Vec<(Assignment, String)>,
    backends: BackendPaths,
    socket: PathBuf,
}

impl ApplyTransaction {
    pub(crate) fn load_timeout(&self) -> Duration {
        Duration::from_millis(self.policy.as_ref().and_then(|p| p.load_timeout_ms).unwrap_or(0))
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn note_ready(&mut self, ready: &RendererReady) -> bool {
        if ready.generation != self.generation {
            return false;
        }
        if self.candidates.iter().any(|worker| worker.pid() == ready.pid) {
            self.ready.insert(ready.pid);
            true
        } else {
            false
        }
    }

    pub(crate) fn all_ready(&self) -> bool {
        self.ready.len() == self.candidates.len()
    }

    pub(crate) async fn prepare_next(&mut self) -> Result<bool> {
        if let Some(stage) = self.next.take() {
            self.overlays = std::mem::take(&mut self.candidates);
            self.ready.clear();
            for (assignment, output) in stage.assignments {
                let mut snapshot_directory = None;
                let mut target_snapshot = None;
                if assignment.source.kind != paper_control::SourceKind::Static
                    || assignment
                        .transition
                        .as_ref()
                        .is_some_and(|transition| transition.drawing.is_some())
                {
                    let index = self
                        .overlays
                        .iter()
                        .position(|worker| worker.output == output)
                        .ok_or_else(|| anyhow!("drawing candidate missing"))?;
                    let directory =
                        Arc::new(tempfile::Builder::new().prefix(DRAWING_DIRECTORY).tempdir_in(
                            stage.freeze_parent.clone().map_or_else(freeze_parent, Ok)?,
                        )?);
                    let snapshot = directory.path().join(DRAWING_SNAPSHOT);
                    self.overlays[index].snapshot(&snapshot, &output, true).await?;
                    wait_snapshots(&mut self.overlays, &[index], std::slice::from_ref(&snapshot))
                        .await?;
                    target_snapshot = Some(snapshot);
                    self.overlays[index].drawing_hold = Some(Arc::clone(&directory));
                    snapshot_directory = Some(directory);
                }

                let mut worker = stage.backends.spawn_overlay(
                    OverlaySpec { assignment, snapshot: target_snapshot },
                    output,
                    &stage.socket,
                    self.generation,
                    self.policy.as_ref(),
                )?;
                if let Some(directory) = snapshot_directory {
                    worker.retain_snapshot(directory);
                }
                if let Some(handoff) = &self.handoff {
                    worker.retain_snapshot(Arc::clone(&handoff.directory));
                }
                self.candidates.push(worker);
            }
            self.reveal_steady = true;
            return Ok(true);
        }
        if !std::mem::take(&mut self.reveal_steady) {
            return Ok(false);
        }
        std::mem::swap(&mut self.candidates, &mut self.overlays);
        self.ready.clear();
        for candidate in &mut self.candidates {
            if !candidate.reveal_still().await? {
                self.ready.insert(candidate.pid());
            }
        }
        Ok(true)
    }

    pub(crate) fn note_failed(&mut self, failed: &RendererFailed) -> bool {
        if failed.generation != self.generation
            || !self
                .candidates
                .iter()
                .chain(&self.overlays)
                .any(|worker| worker.pid() == failed.pid)
        {
            return false;
        }
        self.reported_failure = Some(format!("{}: {}", failed.code, failed.message));
        true
    }

    pub(crate) fn failed(&mut self) -> Result<Option<String>> {
        if self.reported_failure.is_some() {
            return Ok(self.reported_failure.take());
        }
        for worker in self.candidates.iter_mut().chain(&mut self.overlays) {
            if let Some(status) = worker.exited()? {
                return Ok(Some(format!(
                    "Paper worker {} for {} exited before readiness: {status}",
                    worker.pid(),
                    worker.output
                )));
            }
        }
        Ok(None)
    }

    pub(crate) async fn rollback(self) {
        for worker in self.candidates.into_iter().chain(self.overlays) {
            worker.stop().await;
        }
    }

    pub(crate) async fn candidate_exit(&mut self) {
        tokio::select! {
            () = any_worker_exit(&mut self.candidates) => {},
            () = any_worker_exit(&mut self.overlays) => {},
        }
    }

    #[cfg(test)]
    fn candidate_readiness(&self) -> Vec<RendererReady> {
        self.candidates
            .iter()
            .map(|worker| RendererReady { pid: worker.pid(), generation: self.generation })
            .collect()
    }
}

pub(crate) struct Manager {
    mirrors: BTreeMap<String, (String, u32)>,
    surface_override: Option<paper_control::SurfacePolicy>,
    surface_overrides: paper_control::SurfaceOverridesRequest,
    widgets: Option<paper_control::WidgetScene>,
    surface_retry: BTreeSet<u32>,
    backends: Option<BackendPaths>,
    workers: Vec<Worker>,
    overlays: Vec<Worker>,
    next_generation: u64,
    paused: bool,
    playback_pause: playback::Control,
    duck: playback::Control,
    policy: Option<RendererPolicy>,
    freeze_parent: Option<PathBuf>,
    #[cfg(test)]
    refresh_sweeps: Arc<std::sync::atomic::AtomicUsize>,
}

impl Manager {
    pub(crate) fn new() -> Self {
        Self {
            mirrors: BTreeMap::new(),
            surface_override: None,
            surface_overrides: paper_control::SurfaceOverridesRequest::default(),
            widgets: None,
            surface_retry: BTreeSet::new(),
            backends: None,
            workers: Vec::new(),
            overlays: Vec::new(),
            next_generation: 1,
            paused: false,
            playback_pause: playback::Control::default(),
            duck: playback::Control::default(),
            policy: None,
            freeze_parent: None,
            #[cfg(test)]
            refresh_sweeps: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_backends(backends: BackendPaths, freeze_parent: PathBuf) -> Self {
        Self {
            mirrors: BTreeMap::new(),
            surface_override: None,
            surface_overrides: paper_control::SurfaceOverridesRequest::default(),
            widgets: None,
            surface_retry: BTreeSet::new(),
            backends: Some(backends),
            workers: Vec::new(),
            overlays: Vec::new(),
            next_generation: 1,
            paused: false,
            playback_pause: playback::Control::default(),
            duck: playback::Control::default(),
            policy: None,
            freeze_parent: Some(freeze_parent),
            refresh_sweeps: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn refresh_sweeps(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        Arc::clone(&self.refresh_sweeps)
    }

    pub(crate) async fn set_surface(
        &mut self,
        surface: paper_control::SurfacePolicy,
    ) -> Result<()> {
        surface.validate()?;
        if !self.is_empty() {
            surface::validate_namespace(self.policy.as_ref(), &surface.namespace)?;
            self.apply_surfaces(&paper_control::SurfaceOverridesRequest::default(), Some(&surface))
                .await?;
            self.policy.as_mut().unwrap().surface = Some(Box::new(surface.clone()));
        }
        self.surface_override = Some(surface);
        self.surface_overrides.surfaces.clear();
        Ok(())
    }

    pub(crate) async fn set_widgets(&mut self, scene: paper_control::WidgetScene) -> Result<()> {
        scene.validate().map_err(|error| anyhow!(error.to_string()))?;
        self.widgets = Some(scene);
        self.send_widgets().await;
        self.widgets.take_if(|scene| scene.widgets.is_empty());
        Ok(())
    }

    async fn send_widgets(&mut self) {
        let Some(scene) = &self.widgets else { return };
        for worker in &mut self.workers {
            if let Err(error) = worker.set_widgets(scene).await {
                tracing::warn!(error = %format!("{error:#}"), "widgets: worker did not take the scene");
            }
        }
    }

    pub(crate) async fn mirror(&mut self, target: &paper_control::MirrorTarget) -> Result<()> {
        target.validate()?;
        if target.socket.is_empty() {
            self.mirrors.remove(&target.output);
        }
        let worker = self.workers.iter_mut().find(|worker| {
            worker.assignment.outputs.contains(&target.output)
                || worker.assignment.outputs == [ALL_OUTPUT]
        });
        let Some(worker) = worker else {
            return if target.socket.is_empty() {
                Ok(())
            } else {
                Err(anyhow!("no renderer for {}", target.output))
            };
        };
        worker.mirror(target).await?;
        if !target.socket.is_empty() {
            self.mirrors.insert(target.output.clone(), (target.socket.clone(), worker.pid()));
        }
        Ok(())
    }

    async fn reconnect_mirrors(&mut self) {
        for (output, (socket, pid)) in std::mem::take(&mut self.mirrors) {
            if !paper_runtime::mirror::Link::peer_exists(Path::new(&socket)) {
                continue;
            }
            let Some(worker) = self.workers.iter_mut().find(|worker| {
                worker.assignment.outputs.contains(&output)
                    || worker.assignment.outputs == [ALL_OUTPUT]
            }) else {
                continue;
            };
            let target = paper_control::MirrorTarget { output, socket };
            if worker.pid() != pid
                && let Err(error) = worker.mirror(&target).await
            {
                tracing::warn!(output = %target.output, %error, "Paper mirror reconnect failed");
                continue;
            }
            self.mirrors.insert(target.output, (target.socket, worker.pid()));
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.workers.is_empty() && self.overlays.is_empty()
    }

    pub(crate) fn paused(&self) -> bool {
        self.paused || self.playback_pause.all
    }

    pub(crate) fn policy(&self) -> Option<RendererPolicy> {
        self.policy.clone()
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        let mut overlays = Vec::new();
        for mut worker in self.overlays.drain(..) {
            if worker.exited()?.is_none() {
                overlays.push(worker);
            } else {
                paper_runtime::scratch::release(worker.pid());
            }
        }
        self.overlays = overlays;
        for worker in &mut self.workers {
            if worker.exited()?.is_none()
                && worker.drawing_hold.is_some()
                && !self.overlays.iter().any(|overlay| {
                    overlay.generation == worker.generation && overlay.output == worker.output
                })
            {
                if worker.dynamic() {
                    use std::io::Write;
                    let mut control = worker.control_file()?;
                    control.write_all(
                        PaperCommand::duck(self.duck.effective(worker)).line().as_bytes(),
                    )?;
                    let mut audio = PaperCommand::audio(
                        Some(worker.assignment.mute),
                        Some(worker.assignment.volume),
                    );
                    audio.system_audio = Some(worker.assignment.system_audio.unwrap_or(true));
                    control.write_all(audio.line().as_bytes())?;
                    control.write_all(
                        PaperCommand::pause(self.paused || self.playback_pause.effective(worker))
                            .line()
                            .as_bytes(),
                    )?;
                }
                worker.drawing_hold = None;
                worker.pending_photos.clear();
            }
        }
        #[cfg(test)]
        self.refresh_sweeps.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut live = Vec::with_capacity(self.workers.len());
        for mut worker in self.workers.drain(..) {
            match worker.exited()? {
                None => live.push(worker),
                Some(status) => {
                    tracing::warn!(
                        output = %worker.output,
                        pid = worker.pid(),
                        %status,
                        "paper worker exited, assignment dropped"
                    );
                    paper_runtime::scratch::release(worker.pid());
                }
            }
        }
        self.workers = live;
        Ok(())
    }

    pub(crate) async fn worker_exit(&mut self) {
        tokio::select! {
            () = any_worker_exit(&mut self.workers) => {},
            () = any_worker_exit(&mut self.overlays) => {},
        }
    }

    pub(crate) async fn begin_apply(
        &mut self,
        request: &ApplyRequest,
        socket: &Path,
    ) -> Result<ApplyTransaction> {
        request.validate().map_err(|error| anyhow!(error.to_string()))?;
        self.refresh()?;
        if self.paused {
            return Err(anyhow!("resume Paper before applying a new composition"));
        }
        let mut policy = request.policy.clone().or_else(|| self.policy.clone());
        if let Some(surface) = policy.as_mut().and_then(|policy| policy.surface.as_mut())
            && let Some(effects) = &self.surface_override
            && surface.namespace == effects.namespace
        {
            **surface = effects.clone();
        }
        self.launch_surfaces(policy.as_ref(), request.replace_all)?;
        if !self.workers.is_empty() && !request.replace_all && policy != self.policy {
            return Err(anyhow!("changing renderer policy requires replace_all"));
        }
        if policy.as_ref().and_then(|policy| policy.transitions_enabled) == Some(false)
            && request.assignments.iter().any(|assignment| {
                assignment
                    .transition
                    .as_ref()
                    .is_some_and(|transition| transition.drawing.is_none())
            })
        {
            return Err(anyhow!("assignment transitions conflict with the active renderer policy"));
        }
        let transition_primary =
            primary_sand_transition_output(policy.as_ref(), &request.assignments);
        let mut assignments = request.assignments.clone();
        let mut touched: BTreeSet<String> = request
            .assignments
            .iter()
            .flat_map(|assignment| assignment.outputs.iter().cloned())
            .collect();
        if !request.replace_all
            && self.workers.iter().any(|worker| worker.assignment.outputs == [ALL_OUTPUT])
            && !touched.contains(ALL_OUTPUT)
        {
            return Err(anyhow!("an active wildcard assignment requires replace_all"));
        }
        if !request.replace_all {
            for worker in &self.workers {
                if worker.assignment.outputs.len() < 2 {
                    continue;
                }
                let keep = worker
                    .assignment
                    .outputs
                    .iter()
                    .filter(|output| !touched.contains(*output))
                    .cloned()
                    .collect::<Vec<_>>();
                if keep.is_empty() || keep.len() == worker.assignment.outputs.len() {
                    continue;
                }
                let mut assignment = worker.assignment.clone();
                assignment.outputs = keep;
                assignment.transition = None;
                assignments.push(assignment);
                touched.extend(worker.assignment.outputs.iter().cloned());
            }
        }
        let mut expanded = expand(&assignments, transition_primary.as_deref());
        for (assignment, _) in &mut expanded {
            self.playback_pause.validate(&assignment.outputs)?;
            self.duck.validate(&assignment.outputs)?;
            if let Some(transition) = assignment.transition.as_ref()
                && let Some(drawing) = &transition.drawing
            {
                if assignment.outputs.len() != 1 || assignment.outputs[0] == ALL_OUTPUT {
                    return Err(anyhow!("drawing requires one named output per renderer"));
                }
                drawing.load(drawing_source(transition, assignment))?;
            }
        }
        let handoff = self.capture_overlays(&mut expanded).await?;
        for (assignment, _) in &mut expanded {
            resolve_transition_from(assignment, &self.workers)?;
        }
        let mut transaction =
            self.stage(expanded, touched, request.replace_all, policy, socket).await?;
        transaction.handoff = handoff;
        Ok(transaction)
    }

    async fn stage(
        &mut self,
        expanded: Vec<(Assignment, String)>,
        touched: BTreeSet<String>,
        replace_all: bool,
        policy: Option<RendererPolicy>,
        socket: &Path,
    ) -> Result<ApplyTransaction> {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let mut backends = self.backend_paths();
        backends.surface_overrides = self.launch_surfaces(policy.as_ref(), replace_all)?;
        let overlays = expanded
            .iter()
            .filter(|(assignment, _)| static_transition(assignment))
            .cloned()
            .collect::<Vec<_>>();
        let next = (!overlays.is_empty()).then(|| ApplyStage {
            freeze_parent: self.freeze_parent.clone(),
            assignments: overlays,
            backends: backends.clone(),
            socket: socket.to_path_buf(),
        });
        let mut candidates = Vec::with_capacity(expanded.len());
        for (assignment, _) in &expanded {
            self.playback_pause.validate(&assignment.outputs)?;
            self.duck.validate(&assignment.outputs)?;
        }
        for (assignment, output) in expanded {
            let spawned = backends.spawn(assignment, output, socket, generation, policy.as_ref());
            match spawned {
                Ok(worker) => candidates.push(worker),
                Err(error) => {
                    for worker in candidates {
                        worker.stop().await;
                    }
                    return Err(error);
                }
            }
        }
        Ok(ApplyTransaction {
            candidates,
            overlays: Vec::new(),
            next,
            reveal_steady: false,
            ready: BTreeSet::new(),
            touched,
            replace_all,
            generation,
            policy,
            surface_overrides: backends.surface_overrides,
            reported_failure: None,
            handoff: None,
        })
    }

    pub(crate) async fn commit(
        &mut self,
        mut transaction: ApplyTransaction,
    ) -> Result<Vec<WorkerStatus>> {
        if !transaction.all_ready() || transaction.next.is_some() || transaction.reveal_steady {
            transaction.rollback().await;
            return Err(anyhow!("Paper composition has not completed presentation readiness"));
        }
        for worker in transaction.candidates.iter_mut().chain(&mut transaction.overlays) {
            if worker.dynamic() && worker.drawing_hold.is_none() {
                let pause = self.paused || self.playback_pause.effective(worker);
                let duck = self.duck.effective(worker);
                if let Err(error) = async {
                    worker.send(&PaperCommand::duck(duck)).await?;
                    worker.send(&PaperCommand::pause(pause)).await
                }
                .await
                {
                    transaction.rollback().await;
                    return Err(error.context("control ready Paper candidate"));
                }
            }
        }
        for candidate in &mut transaction.candidates {
            candidate.assignment.transition = None;
        }
        if let Some(handoff) = &mut transaction.handoff {
            handoff.disarm();
        }
        let mut overlays = Vec::new();
        for overlay in self.overlays.drain(..) {
            if transaction.replace_all
                || transaction.touched.contains(ALL_OUTPUT)
                || overlay.output == ALL_OUTPUT
                || overlay
                    .assignment
                    .outputs
                    .iter()
                    .any(|output| transaction.touched.contains(output))
            {
                overlay.stop().await;
            } else {
                overlays.push(overlay);
            }
        }
        overlays.extend(transaction.overlays);
        self.overlays = overlays;
        let mut retained = Vec::new();
        let mut retired = Vec::new();
        let touches_all = transaction.touched.contains(ALL_OUTPUT);
        for worker in self.workers.drain(..) {
            if transaction.replace_all
                || touches_all
                || worker.assignment.outputs == [ALL_OUTPUT]
                || worker
                    .assignment
                    .outputs
                    .iter()
                    .all(|output| transaction.touched.contains(output))
            {
                retired.push(worker);
            } else {
                retained.push(worker);
            }
        }
        retained.extend(transaction.candidates);
        self.workers = retained;
        self.policy = transaction.policy;
        self.surface_overrides = transaction.surface_overrides;
        self.send_widgets().await;
        for worker in retired {
            worker.stop().await;
        }
        self.reconnect_mirrors().await;
        Ok(self.status())
    }

    pub(crate) async fn begin_pause(&mut self, socket: &Path) -> Result<Option<ApplyTransaction>> {
        self.refresh()?;
        if self.paused {
            return Ok(None);
        }
        if self.overlays.iter().any(|worker| {
            worker
                .assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.drawing.is_some())
        }) {
            self.set_dynamic_pause(true).await?;
            self.paused = true;
            return Ok(None);
        }
        let freeze_indices = self
            .workers
            .iter()
            .enumerate()
            .filter_map(|(index, worker)| worker.pause_freeze_capable().then_some(index))
            .collect::<Vec<_>>();
        if freeze_indices.is_empty() {
            self.set_dynamic_pause(true).await?;
            self.paused = true;
            return Ok(None);
        }
        let parent = self.freeze_parent.clone().map_or_else(freeze_parent, Ok)?;
        let directory = Arc::new(tempfile::Builder::new().prefix("freeze-").tempdir_in(parent)?);
        let snapshots = freeze_indices
            .iter()
            .map(|index| {
                directory.path().join(format!("{}-{}.ppm", self.workers[*index].pid(), index))
            })
            .collect::<Vec<_>>();
        let mut changed: Vec<usize> = Vec::new();
        for index in 0..self.workers.len() {
            if !self.workers[index].dynamic() || self.workers[index].drawing_hold.is_some() {
                continue;
            }
            let command =
                freeze_indices.iter().position(|candidate| *candidate == index).map_or_else(
                    || PaperCommand::pause(true),
                    |position| PaperCommand::freeze(&snapshots[position].display().to_string()),
                );
            if let Err(error) = self.workers[index].send(&command).await {
                for changed_index in changed {
                    let paused = self.playback_pause.effective(&self.workers[changed_index]);
                    let _ = self.workers[changed_index].send(&PaperCommand::pause(paused)).await;
                }
                return Err(error.context("prepare Paper freeze frame"));
            }
            changed.push(index);
        }
        if let Err(error) = wait_snapshots(&mut self.workers, &freeze_indices, &snapshots).await {
            for index in changed {
                let paused = self.playback_pause.effective(&self.workers[index]);
                let _ = self.workers[index].send(&PaperCommand::pause(paused)).await;
            }
            return Err(error);
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let backends = self.backend_paths();
        let mut candidates = Vec::with_capacity(freeze_indices.len());
        for (index, snapshot) in freeze_indices.iter().zip(&snapshots) {
            let worker = &self.workers[*index];
            match backends.spawn_frozen(
                worker.assignment.clone(),
                snapshot,
                worker.output.clone(),
                socket,
                generation,
                self.policy.as_ref(),
                Arc::clone(&directory),
            ) {
                Ok(candidate) => candidates.push(candidate),
                Err(error) => {
                    for candidate in candidates {
                        candidate.stop().await;
                    }
                    self.abort_pause().await;
                    return Err(error);
                }
            }
        }
        let touched = worker_outputs(&candidates);
        Ok(Some(ApplyTransaction {
            candidates,
            overlays: Vec::new(),
            next: None,
            reveal_steady: false,
            ready: BTreeSet::new(),
            touched,
            replace_all: false,
            generation,
            policy: self.policy.clone(),
            surface_overrides: self.surface_overrides.clone(),
            reported_failure: None,
            handoff: None,
        }))
    }

    pub(crate) async fn commit_pause(
        &mut self,
        transaction: ApplyTransaction,
    ) -> Result<Vec<WorkerStatus>> {
        self.paused = true;
        match self.commit(transaction).await {
            Ok(status) => Ok(status),
            Err(error) => {
                self.paused = false;
                self.abort_pause().await;
                Err(error)
            }
        }
    }

    pub(crate) async fn abort_pause(&mut self) {
        for worker in &mut self.workers {
            if worker.dynamic() && worker.drawing_hold.is_none() {
                let paused = self.playback_pause.effective(worker);
                let _ = worker.send(&PaperCommand::pause(paused)).await;
            }
        }
    }

    pub(crate) async fn begin_resume(&mut self, socket: &Path) -> Result<Option<ApplyTransaction>> {
        self.refresh()?;
        if !self.paused {
            return Ok(None);
        }
        let frozen = self
            .workers
            .iter()
            .filter(|worker| {
                !worker.dynamic()
                    && worker.assignment.source.kind != paper_control::SourceKind::Static
            })
            .map(|worker| (worker.assignment.clone(), worker.output.clone()))
            .collect::<Vec<_>>();
        if frozen.is_empty() {
            self.set_dynamic_pause(false).await?;
            self.paused = false;
            return Ok(None);
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let backends = self.backend_paths();
        let mut candidates = Vec::with_capacity(frozen.len());
        for (mut assignment, output) in frozen {
            assignment.transition = None;
            match backends.spawn(assignment, output, socket, generation, self.policy.as_ref()) {
                Ok(candidate) => candidates.push(candidate),
                Err(error) => {
                    for candidate in candidates {
                        candidate.stop().await;
                    }
                    return Err(error);
                }
            }
        }
        let touched = worker_outputs(&candidates);
        Ok(Some(ApplyTransaction {
            candidates,
            overlays: Vec::new(),
            next: None,
            reveal_steady: false,
            ready: BTreeSet::new(),
            touched,
            replace_all: false,
            generation,
            policy: self.policy.clone(),
            surface_overrides: self.surface_overrides.clone(),
            reported_failure: None,
            handoff: None,
        }))
    }

    pub(crate) async fn commit_resume(
        &mut self,
        transaction: ApplyTransaction,
    ) -> Result<Vec<WorkerStatus>> {
        self.set_dynamic_pause(false).await?;
        self.paused = false;
        match self.commit(transaction).await {
            Ok(status) => Ok(status),
            Err(error) => {
                self.paused = true;
                let _ = self.set_dynamic_pause(true).await;
                Err(error)
            }
        }
    }

    async fn set_dynamic_pause(&mut self, paused: bool) -> Result<()> {
        let mut workers = self
            .workers
            .iter_mut()
            .chain(&mut self.overlays)
            .filter(|worker| worker.dynamic() && worker.drawing_hold.is_none())
            .collect::<Vec<_>>();
        for index in 0..workers.len() {
            let effective = paused || self.playback_pause.effective(workers[index]);
            if let Err(error) = workers[index].send(&PaperCommand::pause(effective)).await {
                for worker in &mut workers[..index] {
                    let previous = !paused || self.playback_pause.effective(worker);
                    let _ = worker.send(&PaperCommand::pause(previous)).await;
                }
                return Err(error.context("set Paper pause state"));
            }
        }
        Ok(())
    }

    pub(crate) async fn audio(
        &mut self,
        request: &AudioSetRequest,
    ) -> Result<(usize, Vec<WorkerStatus>)> {
        request.validate().map_err(|error| anyhow!(error.to_string()))?;
        self.refresh()?;
        playback::validate_scope(&request.outputs, self.workers.iter().chain(&self.overlays))?;
        let all = request.outputs.is_empty() || request.outputs.as_slice() == [ALL_OUTPUT];
        if !all && self.workers.iter().any(|worker| worker.output == ALL_OUTPUT) {
            return Err(anyhow!(
                "targeted audio is unavailable while a wildcard assignment is active"
            ));
        }
        let wanted: BTreeSet<&str> = request.outputs.iter().map(String::as_str).collect();
        let targets = self
            .workers
            .iter()
            .enumerate()
            .filter_map(|(index, worker)| {
                (all || worker
                    .assignment
                    .outputs
                    .iter()
                    .any(|output| wanted.contains(output.as_str())))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        let mut duck = self.duck.clone();
        if let Some(value) = request.duck {
            duck.set(&request.outputs, value);
        }
        for index in &targets {
            if !self.workers[*index].dynamic() || self.workers[*index].drawing_hold.is_some() {
                continue;
            }
            if let Err(error) = self.workers[*index].send_audio(request).await {
                self.restore_audio(&request.outputs).await;
                return Err(error.context("set Paper audio state"));
            }
        }
        if let Some(ducked) = request.duck {
            for index in 0..self.overlays.len() {
                if playback::selected(&request.outputs, &self.overlays[index])
                    && let Err(error) = self.overlays[index].send(&PaperCommand::duck(ducked)).await
                {
                    self.restore_audio(&request.outputs).await;
                    return Err(error.context("set Paper overlay audio state"));
                }
            }
        }
        for index in &targets {
            if let Some(mute) = request.mute {
                self.workers[*index].assignment.mute = mute;
            }
            if let Some(source) = request.system_audio {
                self.workers[*index].assignment.system_audio = Some(source);
            }
            if let Some(volume) = request.volume {
                self.workers[*index].assignment.volume = volume;
            }
        }
        self.duck = duck;
        let statuses = targets.iter().map(|index| worker_status(&self.workers[*index])).collect();
        Ok((targets.len(), statuses))
    }

    pub(crate) async fn stop(&mut self, request: &StopRequest) -> Result<usize> {
        let all =
            request.outputs.is_empty() || request.outputs.iter().any(|output| output == ALL_OUTPUT);
        let wanted: BTreeSet<&str> = request.outputs.iter().map(String::as_str).collect();
        let mut count = self
            .workers
            .iter()
            .flat_map(|worker| &worker.assignment.outputs)
            .filter(|output| wanted.contains(output.as_str()))
            .count();
        let mut retained = Vec::new();
        let mut stopped = Vec::new();
        for worker in self.workers.drain(..) {
            if all
                || worker.assignment.outputs == [ALL_OUTPUT]
                || worker.assignment.outputs.iter().all(|output| wanted.contains(output.as_str()))
            {
                stopped.push(worker);
            } else {
                retained.push(worker);
            }
        }
        self.workers = retained;
        if all {
            count = stopped.iter().map(|worker| worker.assignment.outputs.len()).sum();
        }
        for worker in stopped {
            worker.stop().await;
        }
        let mut overlays = Vec::new();
        for overlay in self.overlays.drain(..) {
            if all
                || overlay.output == ALL_OUTPUT
                || overlay.assignment.outputs.iter().any(|output| wanted.contains(output.as_str()))
            {
                overlay.stop().await;
            } else {
                overlays.push(overlay);
            }
        }
        self.overlays = overlays;
        self.reconnect_mirrors().await;
        Ok(count)
    }

    pub(crate) async fn begin_stop(
        &mut self,
        request: &StopRequest,
        socket: &Path,
    ) -> Result<Option<(ApplyTransaction, usize)>> {
        if request.outputs.is_empty() || request.outputs.iter().any(|output| output == ALL_OUTPUT) {
            return Ok(None);
        }
        self.refresh()?;
        let wanted: BTreeSet<String> = request.outputs.iter().cloned().collect();
        let mut touched = wanted.clone();
        let mut expanded = Vec::new();
        let mut count = 0;
        for worker in &self.workers {
            let keep = worker
                .assignment
                .outputs
                .iter()
                .filter(|output| !wanted.contains(*output))
                .cloned()
                .collect::<Vec<_>>();
            count += worker.assignment.outputs.len() - keep.len();
            if keep.is_empty() || keep.len() == worker.assignment.outputs.len() {
                continue;
            }
            let mut assignment = worker.assignment.clone();
            let output = keep.join(OUTPUT_SEPARATOR);
            assignment.outputs = keep;
            assignment.transition = None;
            expanded.push((assignment, output));
            touched.extend(worker.assignment.outputs.iter().cloned());
        }
        if expanded.is_empty() {
            return Ok(None);
        }
        let transaction = self.stage(expanded, touched, false, self.policy.clone(), socket).await?;
        Ok(Some((transaction, count)))
    }

    pub(crate) fn status(&self) -> Vec<WorkerStatus> {
        self.workers.iter().map(worker_status).collect()
    }

    pub(crate) fn renderer_capabilities(&self) -> Vec<paper_control::RendererCapability> {
        self.backend_paths().capabilities()
    }

    fn backend_paths(&self) -> BackendPaths {
        let mut backends = self.backends.clone().unwrap_or_else(BackendPaths::discover);
        backends.pause = self.playback_pause.clone();
        if self.paused {
            backends.pause.set(&[], true);
        }
        backends.duck = self.duck.clone();
        backends.surface_overrides = self.surface_overrides.clone();
        backends
    }
}

fn worker_status(worker: &Worker) -> WorkerStatus {
    WorkerStatus {
        assignment: worker.assignment.clone(),
        pid: worker.pid(),
        generation: worker.generation,
    }
}

fn worker_outputs(workers: &[Worker]) -> BTreeSet<String> {
    workers.iter().flat_map(|worker| worker.assignment.outputs.iter().cloned()).collect()
}

fn resolve_transition_from(assignment: &mut Assignment, workers: &[Worker]) -> Result<()> {
    if assignment.transition.as_ref().is_none_or(|transition| transition.from.is_some()) {
        return Ok(());
    }
    let incumbents = workers
        .iter()
        .filter(|worker| {
            assignment.outputs == [ALL_OUTPUT]
                || worker.assignment.outputs == [ALL_OUTPUT]
                || worker
                    .assignment
                    .outputs
                    .iter()
                    .any(|candidate| assignment.outputs.contains(candidate))
        })
        .collect::<Vec<_>>();
    if incumbents.is_empty() {
        return Ok(());
    }
    let mut paths = BTreeSet::new();
    for incumbent in incumbents {
        match transition_source(&incumbent.assignment.source) {
            Ok(path) => {
                paths.insert(path);
            }
            Err(_) if assignment.fill_mode == FillMode::Span => {
                assignment.transition = None;
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
    if paths.len() != 1 {
        if assignment.fill_mode == FillMode::Span {
            assignment.transition = None;
            return Ok(());
        }
        return Err(anyhow!("transition requires an explicit from hint when incumbents differ"));
    }
    if let Some(transition) = &mut assignment.transition {
        transition.from = paths.into_iter().next();
    }
    Ok(())
}

fn freeze_parent() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| anyhow!("Paper freeze cache has no XDG_CACHE_HOME or HOME"))?;
    let path = base.join("skwd-paper-v2").join("freeze");
    std::fs::create_dir_all(&path)
        .with_context(|| format!("create Paper freeze cache {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(anyhow!("Paper freeze cache must be a private owned directory"));
    }
    Ok(path)
}

async fn wait_snapshots(
    workers: &mut [Worker],
    indices: &[usize],
    paths: &[PathBuf],
) -> Result<()> {
    let timeout = std::env::var("SKWD_PAPER_FREEZE_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(Duration::from_secs(5), Duration::from_millis);
    let deadline = Instant::now() + timeout;
    loop {
        let mut complete = true;
        for (index, path) in indices.iter().zip(paths) {
            let error_path = PathBuf::from(format!("{}.error", path.display()));
            if let Ok(metadata) = std::fs::symlink_metadata(&error_path) {
                if metadata.nlink() != 1 {
                    complete = false;
                    continue;
                }
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.mode() & 0o777 != 0o600
                {
                    return Err(anyhow!("Paper freeze error failed file validation"));
                }
                let message = std::fs::read_to_string(&error_path)
                    .unwrap_or_else(|_| "Paper worker could not capture a freeze frame".into());
                return Err(anyhow!(message));
            }
            match std::fs::symlink_metadata(path) {
                Ok(metadata) => {
                    if metadata.nlink() != 1 {
                        complete = false;
                        continue;
                    }
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || metadata.uid() != unsafe { libc::geteuid() }
                        || metadata.mode() & 0o777 != 0o600
                        || metadata.len() <= 12
                    {
                        return Err(anyhow!("Paper freeze frame failed file validation"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    complete = false;
                    if let Some(status) = workers[*index].exited()? {
                        return Err(anyhow!(
                            "Paper worker {} exited during freeze: {status}",
                            workers[*index].pid()
                        ));
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        if complete {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("Paper freeze-frame capture timed out"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        for worker in self.workers.drain(..).chain(self.overlays.drain(..)) {
            worker.stop_blocking();
        }
    }
}

fn primary_sand_transition_output(
    policy: Option<&RendererPolicy>,
    assignments: &[Assignment],
) -> Option<String> {
    let sand = policy.and_then(|policy| policy.sand.as_ref())?;
    if sand.scope != Some(SandScope::Primary) {
        return None;
    }
    let outputs = assignments
        .iter()
        .filter(|assignment| {
            assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.effect().starts_with("sand-"))
        })
        .flat_map(|assignment| assignment.outputs.iter());
    let configured = sand.primary.as_deref().map(str::trim).filter(|output| !output.is_empty());
    outputs
        .clone()
        .find(|output| configured == Some(output.as_str()))
        .or_else(|| outputs.into_iter().next())
        .cloned()
}

fn expand(
    assignments: &[Assignment],
    transition_primary: Option<&str>,
) -> Vec<(Assignment, String)> {
    let mut expanded: Vec<(Assignment, String)> = Vec::new();
    for assignment in assignments {
        for output in &assignment.outputs {
            let mut assignment = assignment.clone();
            assignment.outputs = vec![output.clone()];
            if assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.effect().starts_with("sand-"))
                && transition_primary.is_some_and(|primary| output != primary)
            {
                assignment.transition = None;
            }
            if assignment.fill_mode == FillMode::Span
                && output != "*"
                && let Some((group, target)) =
                    expanded.iter_mut().find(|(candidate, _)| same_renderer(candidate, &assignment))
            {
                if group.transition != assignment.transition {
                    group.transition = None;
                }
                if !assignment.mute {
                    group.mute = false;
                    group.volume = assignment.volume;
                }
                group.outputs.push(output.clone());
                *target = group.outputs.join(OUTPUT_SEPARATOR);
                continue;
            }
            expanded.push((assignment, output.clone()));
        }
    }
    expanded
}

fn same_renderer(left: &Assignment, right: &Assignment) -> bool {
    left.source == right.source
        && left.fill_mode == right.fill_mode
        && left.background == right.background
        && (if left.fill_mode == FillMode::Span {
            left.mute || right.mute || left.volume == right.volume
        } else {
            left.mute == right.mute && left.volume == right.volume
        })
        && left.layer == right.layer
        && (left.fill_mode == FillMode::Span || left.transition == right.transition)
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
